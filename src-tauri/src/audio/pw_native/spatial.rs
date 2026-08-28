//! Real 7.1 headphone virtualization.
//!
//! The Game and Media sinks expose FL/FR/FC/LFE/RL/RR/SL/SR. This processor
//! uses libmysofa to obtain a binaural impulse response for every virtual
//! speaker and FFTW overlap-save convolution to turn the eight inputs into
//! two headphone channels. When spatial audio is off (or the system HRTF is
//! unavailable), the same stable 7.1 device is standards-style downmixed so
//! centre/rear content is never silently dropped.

use std::ffi::{c_char, c_int, c_uint, c_void, CString};
use std::path::Path;

pub const SURROUND_CHANNELS: usize = 8;
const EARS: usize = 2;
const BLOCK: usize = 128;
const MAX_TAPS: usize = 512;
const FFT_SIZE: usize = 1024;
const BINS: usize = FFT_SIZE / 2 + 1;
const LFE_CHANNEL: usize = 3;
const ROOM_COMBS: usize = 4;
const DELAY_BUFFER: usize = 4096;
const _: () = assert!(FFT_SIZE >= MAX_TAPS + BLOCK - 1);
const FFTW_UNALIGNED: c_uint = 1 << 1;
const FFTW_ESTIMATE: c_uint = 1 << 6;
const AALTO_HRTF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../third_party/spatial/runtime/NF_LIB_HRTF_LFE.sofa"
));
const AALTO_RADIUS_METRES: f32 = 0.2;
// Calibrated against Sonux's ordinary 7.1-to-stereo fold-down with both the
// local music and game references. The Aalto FIRs preserve their measured
// tonal shape; this fixed broadband gain only restores comparable level.
const AALTO_PRODUCTION_GAIN: f32 = 2.818_383; // +9.0 dB

/// Explicit inputs to the offline/live spatial renderer. Keeping this small
/// value type separate from `EqParams` lets the comparison tool exercise the
/// exact production DSP without starting PipeWire or Tauri.
#[derive(Debug, Clone, Copy)]
pub struct SpatialRenderParams {
    pub enabled: bool,
    pub tuning: f32,
    pub distance: f32,
    pub headphones: bool,
}

impl SpatialRenderParams {
    pub fn new(enabled: bool, tuning: f32, distance: f32, headphones: bool) -> Self {
        Self {
            enabled,
            tuning: tuning.clamp(0.0, 1.0),
            distance: distance.clamp(0.0, 1.0),
            headphones,
        }
    }
}

/// Prefix of libmysofa's public `MYSOFA_HRTF` structure. The first six fields
/// are the AES69 dimensions; the remaining arrays stay opaque to this module.
#[repr(C)]
struct MySofaHrtf {
    i: c_uint,
    c: c_uint,
    r: c_uint,
    e: c_uint,
    n: c_uint,
    m: c_uint,
    _rest: [u8; 0],
}

#[repr(C)]
struct MySofaLookup {
    _private: [u8; 0],
}

#[repr(C)]
struct MySofaNeighborhood {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

type FftPlanRaw = *mut c_void;

#[link(name = "mysofa")]
unsafe extern "C" {
    fn mysofa_s2c(values: *mut f32);
    fn mysofa_load(filename: *const c_char, err: *mut c_int) -> *mut MySofaHrtf;
    fn mysofa_load_data(data: *const c_char, size: usize, err: *mut c_int) -> *mut MySofaHrtf;
    fn mysofa_check(hrtf: *mut MySofaHrtf) -> c_int;
    fn mysofa_tocartesian(hrtf: *mut MySofaHrtf);
    fn mysofa_free(hrtf: *mut MySofaHrtf);
    fn mysofa_lookup_init(hrtf: *mut MySofaHrtf) -> *mut MySofaLookup;
    fn mysofa_lookup(lookup: *mut MySofaLookup, coordinate: *mut f32) -> c_int;
    fn mysofa_lookup_free(lookup: *mut MySofaLookup);
    fn mysofa_neighborhood_init(
        hrtf: *mut MySofaHrtf,
        lookup: *mut MySofaLookup,
    ) -> *mut MySofaNeighborhood;
    fn mysofa_neighborhood(neighborhood: *mut MySofaNeighborhood, position: c_int) -> *mut c_int;
    fn mysofa_neighborhood_free(neighborhood: *mut MySofaNeighborhood);
    fn mysofa_interpolate(
        hrtf: *mut MySofaHrtf,
        coordinate: *mut f32,
        nearest: c_int,
        neighborhood: *mut c_int,
        fir: *mut f32,
        delays: *mut f32,
    ) -> *mut f32;
}

#[link(name = "fftw3f")]
unsafe extern "C" {
    fn fftwf_plan_dft_r2c_1d(
        n: c_int,
        input: *mut f32,
        output: *mut Complex,
        flags: c_uint,
    ) -> FftPlanRaw;
    fn fftwf_plan_dft_c2r_1d(
        n: c_int,
        input: *mut Complex,
        output: *mut f32,
        flags: c_uint,
    ) -> FftPlanRaw;
    fn fftwf_execute_dft_r2c(plan: FftPlanRaw, input: *mut f32, output: *mut Complex);
    fn fftwf_execute_dft_c2r(plan: FftPlanRaw, input: *mut Complex, output: *mut f32);
    fn fftwf_destroy_plan(plan: FftPlanRaw);
}

struct Plans {
    forward: FftPlanRaw,
    inverse: FftPlanRaw,
}

impl Plans {
    fn new() -> Result<Self, String> {
        let mut time = vec![0.0f32; FFT_SIZE];
        let mut freq = vec![Complex::default(); BINS];
        // SAFETY: FFTW_ESTIMATE does not execute the transform while planning;
        // both buffers have the advertised lengths. UNALIGNED lets subsequent
        // execute calls safely use ordinary Rust Vec allocations.
        let forward = unsafe {
            fftwf_plan_dft_r2c_1d(
                FFT_SIZE as c_int,
                time.as_mut_ptr(),
                freq.as_mut_ptr(),
                FFTW_ESTIMATE | FFTW_UNALIGNED,
            )
        };
        // SAFETY: same buffer sizing/lifetime argument as the forward plan.
        let inverse = unsafe {
            fftwf_plan_dft_c2r_1d(
                FFT_SIZE as c_int,
                freq.as_mut_ptr(),
                time.as_mut_ptr(),
                FFTW_ESTIMATE | FFTW_UNALIGNED,
            )
        };
        if forward.is_null() || inverse.is_null() {
            if !forward.is_null() {
                // SAFETY: plan was returned by FFTW and has not been freed.
                unsafe { fftwf_destroy_plan(forward) };
            }
            if !inverse.is_null() {
                // SAFETY: plan was returned by FFTW and has not been freed.
                unsafe { fftwf_destroy_plan(inverse) };
            }
            return Err("FFTW could not create spatial convolution plans".into());
        }
        Ok(Self { forward, inverse })
    }
}

impl Drop for Plans {
    fn drop(&mut self) {
        // SAFETY: these plans are uniquely owned and destroyed once.
        unsafe {
            fftwf_destroy_plan(self.forward);
            fftwf_destroy_plan(self.inverse);
        }
    }
}

struct VariableComb {
    buffer: Vec<f32>,
    position: usize,
    damped: f32,
}

impl VariableComb {
    fn new() -> Self {
        Self {
            buffer: vec![0.0; DELAY_BUFFER],
            position: 0,
            damped: 0.0,
        }
    }

    fn process(&mut self, input: f32, delay: f32, feedback: f32, damping: f32) -> f32 {
        let output = read_fractional_delay(&self.buffer, self.position, delay);
        self.damped = output * (1.0 - damping) + self.damped * damping;
        self.buffer[self.position] = input + self.damped * feedback;
        self.position = (self.position + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.position = 0;
        self.damped = 0.0;
    }
}

fn read_fractional_delay(buffer: &[f32], write_position: usize, delay: f32) -> f32 {
    let delay = delay.clamp(1.0, (buffer.len() - 2) as f32);
    let whole = delay.floor() as usize;
    let fraction = delay - whole as f32;
    let first = (write_position + buffer.len() - whole) % buffer.len();
    let second = (first + buffer.len() - 1) % buffer.len();
    buffer[first] * (1.0 - fraction) + buffer[second] * fraction
}

#[derive(Clone, Copy)]
struct AcousticValues {
    air_mix: f32,
    air_pole: f32,
    clarity_gain: f32,
    width: f32,
    direct: f32,
    early: f32,
    late: f32,
    feedback: f32,
    damping: f32,
    output_gain: f32,
    early_delays: [f32; ROOM_COMBS],
    comb_delays: [[f32; ROOM_COMBS]; EARS],
}

impl AcousticValues {
    fn neutral(sample_rate: f32) -> Self {
        Self::target(SpatialRenderParams::new(true, 0.5, 0.5, true), sample_rate)
    }

    fn target(params: SpatialRenderParams, sample_rate: f32) -> Self {
        let distance = params.distance.clamp(0.0, 1.0);
        let tuning = params.tuning.clamp(0.0, 1.0);
        let close = ((0.5 - distance) * 2.0).max(0.0);
        let far = ((distance - 0.5) * 2.0).max(0.0);
        let performance = ((0.5 - tuning) * 2.0).max(0.0);
        let immersion = ((tuning - 0.5) * 2.0).max(0.0);
        let interaction = far * immersion;

        let cutoff_hz = (18_000.0 - 6_000.0 * far - 3_000.0 * immersion - 3_500.0 * interaction)
            .clamp(4_000.0, 20_000.0);
        let air_mix = (far + immersion - interaction).clamp(0.0, 1.0);
        let air_pole = (-2.0 * std::f32::consts::PI * cutoff_hz / sample_rate).exp();
        let clarity_gain = 10.0f32.powf(1.5 * performance / 20.0) - 1.0;
        let width = 1.0 + 0.08 * close + 0.02 * far * performance;

        let direct = 1.0 - 0.46 * far - 0.38 * immersion + 0.12 * interaction;
        let early = 0.30 * far + 0.24 * immersion - 0.12 * interaction;
        let late = 0.08 * far + 0.42 * immersion + 0.10 * interaction;
        let feedback = 0.18 + 0.24 * far + 0.50 * immersion - 0.18 * interaction;
        let damping = 0.18 + 0.30 * far + 0.38 * immersion - 0.20 * interaction;
        let room_scale = 0.72 + 0.50 * far + 0.53 * immersion - 0.05 * interaction;

        let base_early = [4.0, 7.0, 11.0, 15.0];
        let far_early = [6.0, 10.0, 14.0, 19.0];
        let immersion_early = [3.0, 6.0, 10.0, 16.0];
        let interaction_early = [1.0, 2.0, 4.0, 8.0];
        let early_delays = std::array::from_fn(|tap| {
            (base_early[tap]
                + far_early[tap] * far
                + immersion_early[tap] * immersion
                + interaction_early[tap] * interaction)
                * sample_rate
                / 1_000.0
        });
        let comb_ms = [[29.7, 37.1, 41.1, 43.7], [30.9, 36.3, 40.3, 45.1]];
        let comb_delays = std::array::from_fn(|ear| {
            std::array::from_fn(|comb| comb_ms[ear][comb] * room_scale * sample_rate / 1_000.0)
        });

        // Fixed compensation measured through Sonux's production KEMAR HRTF
        // using the approved prototypes. This avoids content-reactive gain
        // riding; Distance then adds a symmetric +2.5 .. -2.5 dB offset.
        let compensation_db =
            5.14 * far + 3.28 * immersion - 1.56 * interaction - 0.65 * performance;
        let distance_db = distance_level_db(distance);
        let output_gain = 10.0f32.powf((compensation_db + distance_db) / 20.0);

        Self {
            air_mix,
            air_pole,
            clarity_gain,
            width,
            direct,
            early,
            late,
            feedback,
            damping,
            output_gain,
            early_delays,
            comb_delays,
        }
    }

    fn approach(&mut self, target: &Self, amount: f32) {
        fn move_toward(value: &mut f32, target: f32, amount: f32) {
            *value += (target - *value) * amount;
        }
        move_toward(&mut self.air_mix, target.air_mix, amount);
        move_toward(&mut self.air_pole, target.air_pole, amount);
        move_toward(&mut self.clarity_gain, target.clarity_gain, amount);
        move_toward(&mut self.width, target.width, amount);
        move_toward(&mut self.direct, target.direct, amount);
        move_toward(&mut self.early, target.early, amount);
        move_toward(&mut self.late, target.late, amount);
        move_toward(&mut self.feedback, target.feedback, amount);
        move_toward(&mut self.damping, target.damping, amount);
        move_toward(&mut self.output_gain, target.output_gain, amount);
        for tap in 0..ROOM_COMBS {
            move_toward(
                &mut self.early_delays[tap],
                target.early_delays[tap],
                amount,
            );
            for ear in 0..EARS {
                move_toward(
                    &mut self.comb_delays[ear][tap],
                    target.comb_delays[ear][tap],
                    amount,
                );
            }
        }
    }
}

fn distance_level_db(distance: f32) -> f32 {
    (0.5 - distance.clamp(0.0, 1.0)) * 5.0
}

struct AcousticStage {
    sample_rate: f32,
    smoothing: f32,
    clarity_pole: f32,
    current: AcousticValues,
    air_state: [f32; EARS],
    clarity_state: [f32; EARS],
    early_buffer: [Vec<f32>; EARS],
    early_position: usize,
    combs: [[VariableComb; ROOM_COMBS]; EARS],
    active: bool,
}

impl AcousticStage {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        Self {
            sample_rate,
            smoothing: 1.0 - (-1.0 / (0.015 * sample_rate)).exp(),
            clarity_pole: (-2.0 * std::f32::consts::PI * 2_500.0 / sample_rate).exp(),
            current: AcousticValues::neutral(sample_rate),
            air_state: [0.0; EARS],
            clarity_state: [0.0; EARS],
            early_buffer: std::array::from_fn(|_| vec![0.0; DELAY_BUFFER]),
            early_position: 0,
            combs: std::array::from_fn(|_| std::array::from_fn(|_| VariableComb::new())),
            active: false,
        }
    }

    fn process(&mut self, stereo: &mut [[f32; BLOCK]; EARS], params: SpatialRenderParams) {
        if !self.active
            && (params.tuning - 0.5).abs() < 1e-6
            && (params.distance - 0.5).abs() < 1e-6
        {
            return;
        }
        self.active = true;
        let target = AcousticValues::target(params, self.sample_rate);
        let (left, right) = stereo.split_at_mut(1);
        for (left, right) in left[0].iter_mut().zip(right[0].iter_mut()) {
            self.current.approach(&target, self.smoothing);
            let source = [*left, *right];
            let mut air = [0.0; EARS];
            for ear in 0..EARS {
                self.air_state[ear] = (1.0 - self.current.air_pole) * source[ear]
                    + self.current.air_pole * self.air_state[ear];
                let damped =
                    source[ear] + self.current.air_mix * (self.air_state[ear] - source[ear]);
                self.clarity_state[ear] = (1.0 - self.clarity_pole) * damped
                    + self.clarity_pole * self.clarity_state[ear];
                air[ear] = damped + self.current.clarity_gain * (damped - self.clarity_state[ear]);
            }
            let mid = 0.5 * (air[0] + air[1]);
            let side = 0.5 * (air[0] - air[1]) * self.current.width;
            air = [mid + side, mid - side];

            let early = [
                0.43 * read_fractional_delay(
                    &self.early_buffer[0],
                    self.early_position,
                    self.current.early_delays[0],
                ) + 0.27
                    * read_fractional_delay(
                        &self.early_buffer[1],
                        self.early_position,
                        self.current.early_delays[1],
                    )
                    + 0.18
                        * read_fractional_delay(
                            &self.early_buffer[0],
                            self.early_position,
                            self.current.early_delays[2],
                        )
                    + 0.12
                        * read_fractional_delay(
                            &self.early_buffer[1],
                            self.early_position,
                            self.current.early_delays[3],
                        ),
                0.43 * read_fractional_delay(
                    &self.early_buffer[1],
                    self.early_position,
                    self.current.early_delays[0],
                ) + 0.27
                    * read_fractional_delay(
                        &self.early_buffer[0],
                        self.early_position,
                        self.current.early_delays[1],
                    )
                    + 0.18
                        * read_fractional_delay(
                            &self.early_buffer[1],
                            self.early_position,
                            self.current.early_delays[2],
                        )
                    + 0.12
                        * read_fractional_delay(
                            &self.early_buffer[0],
                            self.early_position,
                            self.current.early_delays[3],
                        ),
            ];
            self.early_buffer[0][self.early_position] = air[0];
            self.early_buffer[1][self.early_position] = air[1];
            self.early_position = (self.early_position + 1) % DELAY_BUFFER;

            let mut late = [0.0; EARS];
            for ear in 0..EARS {
                let input = 0.78 * air[ear] + 0.22 * air[1 - ear] + 0.35 * early[ear];
                for comb in 0..ROOM_COMBS {
                    late[ear] += self.combs[ear][comb].process(
                        input,
                        self.current.comb_delays[ear][comb],
                        self.current.feedback,
                        self.current.damping,
                    );
                }
                late[ear] /= ROOM_COMBS as f32;
            }
            let diffuse_left = 0.86 * late[0] + 0.14 * late[1];
            let diffuse_right = 0.86 * late[1] + 0.14 * late[0];
            *left = self.current.output_gain
                * (self.current.direct * air[0]
                    + self.current.early * early[0]
                    + self.current.late * diffuse_left);
            *right = self.current.output_gain
                * (self.current.direct * air[1]
                    + self.current.early * early[1]
                    + self.current.late * diffuse_right);
        }
    }

    fn deactivate(&mut self) {
        if !self.active {
            return;
        }
        self.air_state = [0.0; EARS];
        self.clarity_state = [0.0; EARS];
        for buffer in &mut self.early_buffer {
            buffer.fill(0.0);
        }
        self.early_position = 0;
        for ear in &mut self.combs {
            for comb in ear {
                comb.reset();
            }
        }
        self.current = AcousticValues::neutral(self.sample_rate);
        self.active = false;
    }
}

/// RT-owned spatializer. All vectors and FFT plans are allocated before the
/// PipeWire process callback starts; `process` performs no heap allocation.
pub struct SpatialEngine {
    plans: Plans,
    /// Full HRTF spectra: [input channel][ear][bin]. The clean directional
    /// baseline deliberately does not truncate or blend these filters.
    full: [[Vec<Complex>; EARS]; SURROUND_CHANNELS],
    hrtf_available: bool,
    hrtf_active: bool,
    history: [[f32; MAX_TAPS - 1]; SURROUND_CHANNELS],
    pending: [[f32; SURROUND_CHANNELS]; BLOCK],
    pending_len: usize,
    time: Vec<f32>,
    spectrum: Vec<Complex>,
    accum: [Vec<Complex>; EARS],
    rendered: Vec<f32>,
    acoustic: AcousticStage,
}

impl SpatialEngine {
    pub fn new(sample_rate: f32) -> Result<Self, String> {
        let sample_rate = sample_rate.max(1.0);
        Self::with_loader(sample_rate, |engine| {
            engine.load_embedded_aalto(sample_rate)
        })
    }

    /// Opens a SOFA dataset through libmysofa's lower-level API for offline
    /// comparison. Some scientifically valid older datasets encode receiver
    /// coordinates as spherical; converting all coordinates in memory before
    /// validation makes those inputs consumable without modifying the source
    /// file. This constructor is not used by the live PipeWire chain.
    pub fn new_with_custom_hrtf(
        sample_rate: f32,
        path: &str,
        radius_metres: f32,
    ) -> Result<Self, String> {
        let sample_rate = sample_rate.max(1.0);
        let radius_metres = radius_metres.clamp(0.01, 20.0);
        Self::with_loader(sample_rate, |engine| {
            engine.load_custom_hrtf(path, sample_rate, radius_metres)
        })
    }

    fn with_loader(
        sample_rate: f32,
        loader: impl FnOnce(&mut SpatialEngine) -> bool,
    ) -> Result<Self, String> {
        let plans = Plans::new()?;
        let mut engine = Self {
            plans,
            full: std::array::from_fn(|_| std::array::from_fn(|_| vec![Complex::default(); BINS])),
            hrtf_available: false,
            hrtf_active: false,
            history: [[0.0; MAX_TAPS - 1]; SURROUND_CHANNELS],
            pending: [[0.0; SURROUND_CHANNELS]; BLOCK],
            pending_len: 0,
            time: vec![0.0; FFT_SIZE],
            spectrum: vec![Complex::default(); BINS],
            accum: std::array::from_fn(|_| vec![Complex::default(); BINS]),
            rendered: vec![0.0; FFT_SIZE],
            acoustic: AcousticStage::new(sample_rate),
        };
        engine.hrtf_available = loader(&mut engine);
        if !engine.hrtf_available {
            eprintln!("sonux: requested HRTF unavailable; spatial renderer uses stereo downmix");
        }
        Ok(engine)
    }

    fn fft_filter(&mut self, ir: &[f32], destination: &mut [Complex]) {
        self.time.fill(0.0);
        let count = ir.len().min(FFT_SIZE);
        self.time[..count].copy_from_slice(&ir[..count]);
        // SAFETY: plan and buffers are valid for FFT_SIZE real / BINS complex.
        unsafe {
            fftwf_execute_dft_r2c(
                self.plans.forward,
                self.time.as_mut_ptr(),
                destination.as_mut_ptr(),
            )
        };
    }

    fn load_embedded_aalto(&mut self, sample_rate: f32) -> bool {
        if (sample_rate - 48_000.0).abs() > 0.5 {
            return false;
        }
        let mut error = 0;
        // SAFETY: include_bytes! has static storage and remains valid for the
        // complete load. libmysofa creates an independently owned HRTF.
        let hrtf = unsafe {
            mysofa_load_data(
                AALTO_HRTF.as_ptr().cast::<c_char>(),
                AALTO_HRTF.len(),
                &mut error,
            )
        };
        if hrtf.is_null() || error != 0 {
            return false;
        }
        self.load_aalto_hrtf(hrtf, AALTO_RADIUS_METRES, AALTO_PRODUCTION_GAIN)
    }

    fn load_custom_hrtf(&mut self, path: &str, sample_rate: f32, radius_metres: f32) -> bool {
        if !Path::new(path).is_file() || (sample_rate - 48_000.0).abs() > 0.5 {
            return false;
        }
        let Ok(filename) = CString::new(path) else {
            return false;
        };
        let mut error = 0;
        // SAFETY: filename and error pointer remain valid for the call.
        let hrtf = unsafe { mysofa_load(filename.as_ptr(), &mut error) };
        if hrtf.is_null() || error != 0 {
            return false;
        }

        self.load_aalto_hrtf(hrtf, radius_metres, 1.0)
    }

    fn load_aalto_hrtf(
        &mut self,
        hrtf: *mut MySofaHrtf,
        radius_metres: f32,
        calibration_gain: f32,
    ) -> bool {
        // Aalto's published 2023 file stores ReceiverPosition as spherical.
        // libmysofa's easy API requires Cartesian receivers, but its public
        // lower-level API supports an in-memory canonical conversion first.
        // SAFETY: hrtf is uniquely owned until the cleanup block below.
        unsafe { mysofa_tocartesian(hrtf) };
        if unsafe { mysofa_check(hrtf) } != 0 {
            unsafe { mysofa_free(hrtf) };
            return false;
        }

        // SAFETY: the six dimension fields are the documented prefix of the
        // public MYSOFA_HRTF structure and hrtf is non-null.
        let (receivers, source_len) = unsafe { ((*hrtf).r as usize, (*hrtf).n as usize) };
        if receivers != EARS || source_len == 0 || source_len > MAX_TAPS {
            unsafe { mysofa_free(hrtf) };
            return false;
        }
        // SAFETY: both constructors borrow the live HRTF; they are released
        // before the HRTF itself.
        let lookup = unsafe { mysofa_lookup_init(hrtf) };
        if lookup.is_null() {
            unsafe { mysofa_free(hrtf) };
            return false;
        }
        let neighborhood = unsafe { mysofa_neighborhood_init(hrtf, lookup) };
        if neighborhood.is_null() {
            unsafe {
                mysofa_lookup_free(lookup);
                mysofa_free(hrtf);
            }
            return false;
        }

        let directions = [
            (30.0, 0.0),
            (330.0, 0.0),
            (0.0, 0.0),
            (0.0, 0.0),
            (150.0, 0.0),
            (210.0, 0.0),
            (90.0, 0.0),
            (270.0, 0.0),
        ];
        let gains = [0.34, 0.34, 0.27, 0.10, 0.23, 0.23, 0.23, 0.23];
        let mut fir = vec![0.0f32; source_len * receivers];
        let mut delays = vec![0.0f32; receivers];
        let mut loaded = true;

        for channel in 0..SURROUND_CHANNELS {
            let (azimuth, elevation) = directions[channel];
            let mut coordinate = [azimuth, elevation, radius_metres];
            unsafe { mysofa_s2c(coordinate.as_mut_ptr()) };
            let nearest = unsafe { mysofa_lookup(lookup, coordinate.as_mut_ptr()) };
            let neighbors = unsafe { mysofa_neighborhood(neighborhood, nearest) };
            fir.fill(0.0);
            delays.fill(0.0);
            let interpolated = unsafe {
                mysofa_interpolate(
                    hrtf,
                    coordinate.as_mut_ptr(),
                    nearest,
                    neighbors,
                    fir.as_mut_ptr(),
                    delays.as_mut_ptr(),
                )
            };
            if interpolated.is_null() {
                loaded = false;
                break;
            }
            // The returned buffer is receiver-major: one N-sample FIR per ear.
            let filters =
                unsafe { std::slice::from_raw_parts(interpolated, source_len * receivers) };
            for ear in 0..EARS {
                let source = &filters[ear * source_len..(ear + 1) * source_len];
                let mut full_ir = vec![0.0f32; MAX_TAPS];
                for (destination, sample) in full_ir.iter_mut().zip(source.iter()) {
                    *destination = *sample * gains[channel] * calibration_gain;
                }
                let mut spectrum = vec![Complex::default(); BINS];
                self.fft_filter(&full_ir, &mut spectrum);
                self.full[channel][ear] = spectrum;
            }
        }

        unsafe {
            mysofa_neighborhood_free(neighborhood);
            mysofa_lookup_free(lookup);
            mysofa_free(hrtf);
        }
        loaded
    }

    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>, params: SpatialRenderParams) {
        output.clear();
        for frame in input.as_chunks::<SURROUND_CHANNELS>().0 {
            self.pending[self.pending_len].copy_from_slice(frame);
            self.pending_len += 1;
            if self.pending_len == BLOCK {
                self.render_block(output, params);
                self.pending_len = 0;
            }
        }
    }

    fn render_block(&mut self, output: &mut Vec<f32>, params: SpatialRenderParams) {
        if !params.enabled || !params.headphones || !self.hrtf_available {
            if self.hrtf_active {
                for channel in &mut self.history {
                    channel.fill(0.0);
                }
                self.hrtf_active = false;
            }
            self.acoustic.deactivate();
            self.render_downmix(output);
            return;
        }
        self.hrtf_active = true;

        for ear in &mut self.accum {
            ear.fill(Complex::default());
        }
        for channel in 0..SURROUND_CHANNELS {
            self.time.fill(0.0);
            self.time[..MAX_TAPS - 1].copy_from_slice(&self.history[channel]);
            for frame in 0..BLOCK {
                self.time[MAX_TAPS - 1 + frame] = self.pending[frame][channel];
            }
            // SAFETY: plan and buffers have the exact planned sizes.
            unsafe {
                fftwf_execute_dft_r2c(
                    self.plans.forward,
                    self.time.as_mut_ptr(),
                    self.spectrum.as_mut_ptr(),
                )
            };

            for ear in 0..EARS {
                for bin in 0..BINS {
                    let x = self.spectrum[bin];
                    if channel == LFE_CHANNEL {
                        // Sub-bass carries little useful HRTF directionality.
                        // Established SOFA renderers bypass convolution for
                        // LFE; mixing it equally into both ears also avoids
                        // the KEMAR filter's steep low-frequency roll-off.
                        let gain = direct_downmix_gain(channel, ear);
                        self.accum[ear][bin].re += x.re * gain;
                        self.accum[ear][bin].im += x.im * gain;
                        continue;
                    }

                    let h = self.full[channel][ear][bin];
                    self.accum[ear][bin].re += x.re * h.re - x.im * h.im;
                    self.accum[ear][bin].im += x.re * h.im + x.im * h.re;
                }
            }

            self.history[channel].copy_within(BLOCK.., 0);
            let tail_start = MAX_TAPS - 1 - BLOCK;
            for frame in 0..BLOCK {
                self.history[channel][tail_start + frame] = self.pending[frame][channel];
            }
        }

        let normalization = 1.0 / FFT_SIZE as f32;
        let mut stereo = [[0.0f32; BLOCK]; EARS];
        for (ear, rendered) in stereo.iter_mut().enumerate() {
            // SAFETY: inverse plan and buffers have the planned sizes.
            unsafe {
                fftwf_execute_dft_c2r(
                    self.plans.inverse,
                    self.accum[ear].as_mut_ptr(),
                    self.rendered.as_mut_ptr(),
                )
            };
            for (frame, sample) in rendered.iter_mut().enumerate() {
                *sample = self.rendered[MAX_TAPS - 1 + frame] * normalization;
            }
        }
        self.acoustic.process(&mut stereo, params);
        for (left, right) in stereo[0].iter().zip(stereo[1].iter()) {
            output.push(*left);
            output.push(*right);
        }
    }

    fn render_downmix(&self, output: &mut Vec<f32>) {
        // ITU-like stereo fold-down with explicit LFE contribution and
        // conservative headroom. This is also Speaker mode's behaviour.
        const C: f32 = std::f32::consts::FRAC_1_SQRT_2;
        for frame in &self.pending {
            let left = frame[0] + C * frame[2] + 0.5 * frame[3] + C * frame[4] + C * frame[6];
            let right = frame[1] + C * frame[2] + 0.5 * frame[3] + C * frame[5] + C * frame[7];
            output.push(left * 0.42);
            output.push(right * 0.42);
        }
    }
}

/// LFE bypass uses the same coefficient as Speaker mode's ordinary fold-down.
/// Directional channels stay wholly in the HRTF path so their phase is never
/// mixed with a second, time-misaligned direct path.
fn direct_downmix_gain(channel: usize, ear: usize) -> f32 {
    const C: f32 = std::f32::consts::FRAC_1_SQRT_2;
    const HEADROOM: f32 = 0.42;
    match (channel, ear) {
        (0, 0) | (1, 1) => HEADROOM,
        (2, _) => C * HEADROOM,
        (3, _) => 0.5 * HEADROOM,
        (4, 0) | (5, 1) | (6, 0) | (7, 1) => C * HEADROOM,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_surround_shape_is_eight_to_two() {
        assert_eq!(SURROUND_CHANNELS, 8);
        assert_eq!(EARS, 2);
    }

    #[test]
    fn lfe_bypasses_equally_to_both_ears() {
        assert_eq!(direct_downmix_gain(LFE_CHANNEL, 0), 0.21);
        assert_eq!(direct_downmix_gain(LFE_CHANNEL, 1), 0.21);
    }

    #[test]
    fn audited_aalto_hrtf_is_embedded() {
        assert!(AALTO_HRTF.len() > 1_000_000);
        assert_eq!(&AALTO_HRTF[..8], b"\x89HDF\r\n\x1a\n");
    }

    #[test]
    fn render_params_clamp_untrusted_persisted_values() {
        let params = SpatialRenderParams::new(true, -4.0, 9.0, true);
        assert_eq!(params.tuning, 0.0);
        assert_eq!(params.distance, 1.0);
    }

    #[test]
    fn distance_level_is_symmetric_around_neutral() {
        assert_eq!(distance_level_db(0.0), 2.5);
        assert_eq!(distance_level_db(0.5), 0.0);
        assert_eq!(distance_level_db(1.0), -2.5);
    }

    #[test]
    fn approved_acoustic_endpoints_map_to_live_coefficients() {
        let close_performance =
            AcousticValues::target(SpatialRenderParams::new(true, 0.0, 0.0, true), 48_000.0);
        assert!((close_performance.direct - 1.0).abs() < 1e-6);
        assert!(close_performance.early < 0.001);
        assert!(close_performance.late < 0.001);
        assert!((close_performance.width - 1.08).abs() < 1e-6);
        assert!(close_performance.clarity_gain > 0.18);

        let far_immersion =
            AcousticValues::target(SpatialRenderParams::new(true, 1.0, 1.0, true), 48_000.0);
        assert!((far_immersion.direct - 0.28).abs() < 1e-6);
        assert!((far_immersion.early - 0.42).abs() < 1e-6);
        assert!((far_immersion.late - 0.60).abs() < 1e-6);
        assert!(far_immersion
            .comb_delays
            .iter()
            .flatten()
            .all(|delay| { *delay >= 1.0 && *delay < (DELAY_BUFFER - 2) as f32 }));
    }

    #[test]
    fn acoustic_midpoints_are_a_transparent_pass_through() {
        let mut stage = AcousticStage::new(48_000.0);
        let mut stereo = [[0.0; BLOCK]; EARS];
        let (left, right) = stereo.split_at_mut(1);
        for (frame, (left, right)) in left[0].iter_mut().zip(right[0].iter_mut()).enumerate() {
            *left = frame as f32 / BLOCK as f32 - 0.5;
            *right = 0.25 - frame as f32 / (2.0 * BLOCK as f32);
        }
        let expected = stereo;
        stage.process(&mut stereo, SpatialRenderParams::new(true, 0.5, 0.5, true));
        for (actual, expected) in stereo.iter().flatten().zip(expected.iter().flatten()) {
            assert!((*actual - *expected).abs() < 1e-6);
        }
    }

    #[test]
    fn disabling_hrtf_clears_convolution_history_before_reenable() {
        let mut engine = SpatialEngine::new(48_000.0).expect("spatial engine");
        engine.hrtf_active = true;
        engine.history[0].fill(0.75);
        engine.acoustic.active = true;
        let input = vec![0.0; BLOCK * SURROUND_CHANNELS];
        let mut output = Vec::with_capacity(BLOCK * EARS);

        engine.process(
            &input,
            &mut output,
            SpatialRenderParams::new(false, 0.5, 0.5, true),
        );

        assert!(!engine.hrtf_active);
        assert!(!engine.acoustic.active);
        assert!(engine.history.iter().flatten().all(|sample| *sample == 0.0));
    }
}
