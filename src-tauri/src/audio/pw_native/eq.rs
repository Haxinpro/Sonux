//! Per-channel parametric EQ core: RBJ Audio EQ Cookbook biquads in a
//! cascade of up to MAX_EQ_BANDS, preceded by a preamp trim.
//!
//! Pure Rust, no external DSP crates (same stance as the mic chain in
//! `dsp.rs`). The frequency-response math is hand-mirrored in
//! `src/lib/eqMath.ts` for the UI curve - keep both in sync.
//!
//! Threading model: the command thread writes band parameters into
//! `EqParams` (plain atomics) and bumps a generation counter with Release
//! ordering; the RT capture callback owns an `EqEngine` and redesigns its
//! coefficients only when an Acquire load of the generation sees a change.
//! Coefficient design (a few sin/cos) off the hot path per *change*, not
//! per buffer, and never a lock on the RT thread.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use crate::audio::types::{EqBand, EqBandKind, EqConfig, PlaybackMode, MAX_EQ_BANDS};

/// Biquad transfer-function coefficients, normalized so a0 == 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl BiquadCoeffs {
    /// Pass-through (unity) filter.
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// RBJ Audio EQ Cookbook design. For shelves, `q` is the shelf slope S
    /// (not a resonance Q) - the schema shares one field for both, see
    /// `EqBand::q`. LowPass/HighPass ignore `gain_db`.
    pub fn design(kind: EqBandKind, freq_hz: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        // Guard the math: freq must sit below Nyquist and q must be
        // positive. Config-level clamps enforce this for real input; this
        // is the last line of defense against a divide-by-zero.
        let freq = freq_hz.clamp(1.0, sample_rate * 0.49);
        let q = q.max(0.01);
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let a = 10.0f32.powf(gain_db / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            EqBandKind::Peaking => {
                let alpha = sin_w0 / (2.0 * q);
                (
                    1.0 + alpha * a,
                    -2.0 * cos_w0,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cos_w0,
                    1.0 - alpha / a,
                )
            }
            EqBandKind::LowShelf | EqBandKind::HighShelf => {
                // Shelf slope form: alpha from S, the cookbook's
                // "shelf slope" parameterization.
                let s = q;
                let alpha = sin_w0 / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).max(0.0).sqrt();
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                let (ap1, am1) = (a + 1.0, a - 1.0);
                if kind == EqBandKind::LowShelf {
                    (
                        a * (ap1 - am1 * cos_w0 + two_sqrt_a_alpha),
                        2.0 * a * (am1 - ap1 * cos_w0),
                        a * (ap1 - am1 * cos_w0 - two_sqrt_a_alpha),
                        ap1 + am1 * cos_w0 + two_sqrt_a_alpha,
                        -2.0 * (am1 + ap1 * cos_w0),
                        ap1 + am1 * cos_w0 - two_sqrt_a_alpha,
                    )
                } else {
                    (
                        a * (ap1 + am1 * cos_w0 + two_sqrt_a_alpha),
                        -2.0 * a * (am1 + ap1 * cos_w0),
                        a * (ap1 + am1 * cos_w0 - two_sqrt_a_alpha),
                        ap1 - am1 * cos_w0 + two_sqrt_a_alpha,
                        2.0 * (am1 - ap1 * cos_w0),
                        ap1 - am1 * cos_w0 - two_sqrt_a_alpha,
                    )
                }
            }
            EqBandKind::LowPass => {
                let alpha = sin_w0 / (2.0 * q);
                let b1 = 1.0 - cos_w0;
                (
                    b1 / 2.0,
                    b1,
                    b1 / 2.0,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
            EqBandKind::HighPass => {
                let alpha = sin_w0 / (2.0 * q);
                let b1 = 1.0 + cos_w0;
                (
                    b1 / 2.0,
                    -b1,
                    b1 / 2.0,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
        };

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

/// Per-channel filter memory (transposed direct form II: two states, good
/// numerical behavior at f32).
#[derive(Debug, Default, Clone, Copy)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, x: f32, c: &BiquadCoeffs) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

const KIND_PEAKING: u8 = 0;
const KIND_LOW_SHELF: u8 = 1;
const KIND_HIGH_SHELF: u8 = 2;
const KIND_LOW_PASS: u8 = 3;
const KIND_HIGH_PASS: u8 = 4;

fn kind_to_u8(kind: EqBandKind) -> u8 {
    match kind {
        EqBandKind::Peaking => KIND_PEAKING,
        EqBandKind::LowShelf => KIND_LOW_SHELF,
        EqBandKind::HighShelf => KIND_HIGH_SHELF,
        EqBandKind::LowPass => KIND_LOW_PASS,
        EqBandKind::HighPass => KIND_HIGH_PASS,
    }
}

fn kind_from_u8(v: u8) -> EqBandKind {
    match v {
        KIND_LOW_SHELF => EqBandKind::LowShelf,
        KIND_HIGH_SHELF => EqBandKind::HighShelf,
        KIND_LOW_PASS => EqBandKind::LowPass,
        KIND_HIGH_PASS => EqBandKind::HighPass,
        _ => EqBandKind::Peaking,
    }
}

struct AtomicBand {
    kind: AtomicU8,
    freq_bits: AtomicU32,
    gain_bits: AtomicU32,
    q_bits: AtomicU32,
}

impl AtomicBand {
    fn flat() -> Self {
        Self {
            kind: AtomicU8::new(KIND_PEAKING),
            freq_bits: AtomicU32::new(1000.0f32.to_bits()),
            gain_bits: AtomicU32::new(0.0f32.to_bits()),
            q_bits: AtomicU32::new(1.0f32.to_bits()),
        }
    }

    fn store(&self, band: &EqBand) {
        self.kind.store(kind_to_u8(band.kind), Ordering::Relaxed);
        self.freq_bits
            .store(band.freq_hz.to_bits(), Ordering::Relaxed);
        self.gain_bits
            .store(band.gain_db.to_bits(), Ordering::Relaxed);
        self.q_bits.store(band.q.to_bits(), Ordering::Relaxed);
    }

    fn load(&self) -> EqBand {
        EqBand {
            kind: kind_from_u8(self.kind.load(Ordering::Relaxed)),
            freq_hz: f32::from_bits(self.freq_bits.load(Ordering::Relaxed)),
            gain_db: f32::from_bits(self.gain_bits.load(Ordering::Relaxed)),
            q: f32::from_bits(self.q_bits.load(Ordering::Relaxed)),
        }
    }
}

/// Live-tunable EQ parameters shared with the RT capture callback.
///
/// Single writer (the loop thread handling commands), single reader (the RT
/// callback). Field writes are Relaxed; the trailing Release bump of
/// `generation` publishes them all to the reader's Acquire load - no locks,
/// no retries, and torn *intermediate* states are impossible because the
/// reader only redesigns after seeing a new generation.
pub struct EqParams {
    enabled: AtomicBool,
    preamp_bits: AtomicU32,
    band_count: AtomicUsize,
    bands: [AtomicBand; MAX_EQ_BANDS],
    tone_bass_bits: AtomicU32,
    tone_voice_bits: AtomicU32,
    tone_treble_bits: AtomicU32,
    boost_bits: AtomicU32,
    gate: AtomicBool,
    gate_threshold_bits: AtomicU32,
    comp: AtomicBool,
    comp_threshold_bits: AtomicU32,
    comp_ratio_bits: AtomicU32,
    limiter: AtomicBool,
    limiter_ceiling_bits: AtomicU32,
    playback_mode: AtomicU8,
    spatial: AtomicBool,
    spatial_tuning_bits: AtomicU32,
    spatial_distance_bits: AtomicU32,
    generation: AtomicU64,
}

impl EqParams {
    pub fn from_config(config: &EqConfig) -> Self {
        let p = Self {
            enabled: AtomicBool::new(false),
            preamp_bits: AtomicU32::new(0.0f32.to_bits()),
            band_count: AtomicUsize::new(0),
            bands: std::array::from_fn(|_| AtomicBand::flat()),
            tone_bass_bits: AtomicU32::new(0.0f32.to_bits()),
            tone_voice_bits: AtomicU32::new(0.0f32.to_bits()),
            tone_treble_bits: AtomicU32::new(0.0f32.to_bits()),
            boost_bits: AtomicU32::new(0.0f32.to_bits()),
            gate: AtomicBool::new(false),
            gate_threshold_bits: AtomicU32::new((-48.0f32).to_bits()),
            comp: AtomicBool::new(false),
            comp_threshold_bits: AtomicU32::new((-18.0f32).to_bits()),
            comp_ratio_bits: AtomicU32::new(3.0f32.to_bits()),
            limiter: AtomicBool::new(false),
            limiter_ceiling_bits: AtomicU32::new((-1.0f32).to_bits()),
            playback_mode: AtomicU8::new(0),
            spatial: AtomicBool::new(false),
            spatial_tuning_bits: AtomicU32::new(0.5f32.to_bits()),
            spatial_distance_bits: AtomicU32::new(0.5f32.to_bits()),
            generation: AtomicU64::new(0),
        };
        p.apply(config);
        p
    }

    /// Publish a new config to the RT reader (command thread only).
    pub fn apply(&self, config: &EqConfig) {
        let count = config.bands.len().min(MAX_EQ_BANDS);
        for (slot, band) in self.bands.iter().zip(config.bands.iter()) {
            slot.store(band);
        }
        self.band_count.store(count, Ordering::Relaxed);
        self.enabled.store(config.enabled, Ordering::Relaxed);
        self.preamp_bits
            .store(config.preamp_db.to_bits(), Ordering::Relaxed);
        self.tone_bass_bits
            .store(config.tone_bass_db.to_bits(), Ordering::Relaxed);
        self.tone_voice_bits
            .store(config.tone_voice_db.to_bits(), Ordering::Relaxed);
        self.tone_treble_bits
            .store(config.tone_treble_db.to_bits(), Ordering::Relaxed);
        self.boost_bits
            .store(config.boost_db.to_bits(), Ordering::Relaxed);
        self.gate.store(config.gate_enabled, Ordering::Relaxed);
        self.gate_threshold_bits
            .store(config.gate_threshold_db.to_bits(), Ordering::Relaxed);
        self.comp.store(config.comp_enabled, Ordering::Relaxed);
        self.comp_threshold_bits
            .store(config.comp_threshold_db.to_bits(), Ordering::Relaxed);
        self.comp_ratio_bits
            .store(config.comp_ratio.to_bits(), Ordering::Relaxed);
        self.limiter
            .store(config.limiter_enabled, Ordering::Relaxed);
        self.limiter_ceiling_bits
            .store(config.limiter_ceiling_db.to_bits(), Ordering::Relaxed);
        self.playback_mode.store(
            u8::from(config.playback_mode == PlaybackMode::Headphones),
            Ordering::Relaxed,
        );
        self.spatial
            .store(config.spatial_enabled, Ordering::Relaxed);
        self.spatial_tuning_bits
            .store(config.spatial_tuning.to_bits(), Ordering::Relaxed);
        self.spatial_distance_bits
            .store(config.spatial_distance.to_bits(), Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn spatial_snapshot(&self) -> (bool, f32, f32, bool) {
        (
            self.spatial.load(Ordering::Relaxed),
            f32::from_bits(self.spatial_tuning_bits.load(Ordering::Relaxed)),
            f32::from_bits(self.spatial_distance_bits.load(Ordering::Relaxed)),
            self.playback_mode.load(Ordering::Relaxed) == 1,
        )
    }
}

/// The RT-side processor: owns coefficient + filter state, refreshed from
/// `EqParams` when the generation changes or the sample rate renegotiates.
pub struct EqEngine {
    sample_rate: f32,
    /// u64::MAX = "must refresh" sentinel (set on rate change / creation).
    seen_generation: u64,
    enabled: bool,
    preamp_linear: f32,
    coeffs: [BiquadCoeffs; MAX_EQ_BANDS],
    count: usize,
    /// Per band, per stereo channel. A rate change resets filter memory -
    /// accepted, same as the mic chain rebuilding DspChain on rate change.
    state: [[BiquadState; 2]; MAX_EQ_BANDS],
    /// Independent broad Bass/Voice/Treble stage. It is deliberately not
    /// represented by the draggable parametric-EQ points in the UI.
    tone_coeffs: [BiquadCoeffs; 3],
    tone_state: [[BiquadState; 2]; 3],
    boost_linear: f32,
    gate_enabled: bool,
    gate_threshold_db: f32,
    gate_env: f32,
    gate_gain: f32,
    gate_hold: u32,
    comp_enabled: bool,
    comp_threshold_db: f32,
    comp_ratio: f32,
    comp_env: f32,
    limiter_enabled: bool,
    limiter_ceiling_db: f32,
    limiter_gain: f32,
    headphones: bool,
    crossfeed_lp: [f32; 2],
}

const GATE_ATTACK_MS: f32 = 5.0;
const GATE_RELEASE_MS: f32 = 150.0;
const GATE_HOLD_MS: f32 = 200.0;
const COMP_ATTACK_MS: f32 = 8.0;
const COMP_RELEASE_MS: f32 = 120.0;
const COMP_MAKEUP_DB: f32 = 4.0;
const LIMIT_RELEASE_MS: f32 = 60.0;
const CROSSFEED_HZ: f32 = 700.0;
const CROSSFEED_AMOUNT: f32 = 0.12;

fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

fn time_coeff(ms: f32, sample_rate: f32) -> f32 {
    (-1.0 / (ms * 0.001 * sample_rate.max(1.0))).exp()
}

impl EqEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            seen_generation: u64::MAX,
            enabled: false,
            preamp_linear: 1.0,
            coeffs: [BiquadCoeffs::identity(); MAX_EQ_BANDS],
            count: 0,
            state: [[BiquadState::default(); 2]; MAX_EQ_BANDS],
            tone_coeffs: [BiquadCoeffs::identity(); 3],
            tone_state: [[BiquadState::default(); 2]; 3],
            boost_linear: 1.0,
            gate_enabled: false,
            gate_threshold_db: -48.0,
            gate_env: 0.0,
            gate_gain: 0.0,
            gate_hold: 0,
            comp_enabled: false,
            comp_threshold_db: -18.0,
            comp_ratio: 3.0,
            comp_env: 0.0,
            limiter_enabled: false,
            limiter_ceiling_db: -1.0,
            limiter_gain: 1.0,
            headphones: false,
            crossfeed_lp: [0.0; 2],
        }
    }

    /// Coefficients are frequency-relative, so a renegotiated rate forces a
    /// redesign on the next process() even if the params are unchanged.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 && sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.seen_generation = u64::MAX;
            self.state = [[BiquadState::default(); 2]; MAX_EQ_BANDS];
            self.tone_state = [[BiquadState::default(); 2]; 3];
            self.gate_env = 0.0;
            self.gate_gain = 0.0;
            self.gate_hold = 0;
            self.comp_env = 0.0;
            self.limiter_gain = 1.0;
            self.crossfeed_lp = [0.0; 2];
        }
    }

    fn refresh(&mut self, params: &EqParams) {
        let generation = params.generation();
        if generation == self.seen_generation {
            return;
        }
        self.seen_generation = generation;
        self.enabled = params.enabled.load(Ordering::Relaxed);
        let preamp_db = f32::from_bits(params.preamp_bits.load(Ordering::Relaxed));
        self.preamp_linear = 10.0f32.powf(preamp_db / 20.0);
        self.boost_linear = db_to_linear(f32::from_bits(params.boost_bits.load(Ordering::Relaxed)));
        self.gate_enabled = params.gate.load(Ordering::Relaxed);
        self.gate_threshold_db = f32::from_bits(params.gate_threshold_bits.load(Ordering::Relaxed));
        self.comp_enabled = params.comp.load(Ordering::Relaxed);
        self.comp_threshold_db = f32::from_bits(params.comp_threshold_bits.load(Ordering::Relaxed));
        self.comp_ratio = f32::from_bits(params.comp_ratio_bits.load(Ordering::Relaxed));
        self.limiter_enabled = params.limiter.load(Ordering::Relaxed);
        self.limiter_ceiling_db =
            f32::from_bits(params.limiter_ceiling_bits.load(Ordering::Relaxed));
        // The HRTF already contains the interaural filtering and delay. The
        // gentle stereo crossfeed remains useful for ordinary stereo
        // channels, but applying it after binauralization would blur cues.
        self.headphones = params.playback_mode.load(Ordering::Relaxed) == 1
            && !params.spatial.load(Ordering::Relaxed);
        self.count = params.band_count.load(Ordering::Relaxed).min(MAX_EQ_BANDS);
        for i in 0..self.count {
            let band = params.bands[i].load();
            self.coeffs[i] = BiquadCoeffs::design(
                band.kind,
                band.freq_hz,
                band.gain_db,
                band.q,
                self.sample_rate,
            );
        }
        self.tone_coeffs[0] = BiquadCoeffs::design(
            EqBandKind::LowShelf,
            120.0,
            f32::from_bits(params.tone_bass_bits.load(Ordering::Relaxed)),
            0.71,
            self.sample_rate,
        );
        self.tone_coeffs[1] = BiquadCoeffs::design(
            EqBandKind::Peaking,
            1200.0,
            f32::from_bits(params.tone_voice_bits.load(Ordering::Relaxed)),
            0.8,
            self.sample_rate,
        );
        self.tone_coeffs[2] = BiquadCoeffs::design(
            EqBandKind::HighShelf,
            8000.0,
            f32::from_bits(params.tone_treble_bits.load(Ordering::Relaxed)),
            0.71,
            self.sample_rate,
        );
    }

    /// Process interleaved stereo in place: parametric EQ → tone stage →
    /// headphone crossfeed → gate → boost → linked compressor → linked
    /// limiter. Stereo-linked dynamics preserve image position rather than pulling
    /// one side around alone.
    pub fn process_interleaved(&mut self, buf: &mut [f32], params: &EqParams) {
        self.refresh(params);
        let gate_threshold = db_to_linear(self.gate_threshold_db);
        let gate_attack = time_coeff(GATE_ATTACK_MS, self.sample_rate);
        let gate_release = time_coeff(GATE_RELEASE_MS, self.sample_rate);
        let gate_hold_samples = (GATE_HOLD_MS * 0.001 * self.sample_rate) as u32;
        let comp_attack = time_coeff(COMP_ATTACK_MS, self.sample_rate);
        let comp_release = time_coeff(COMP_RELEASE_MS, self.sample_rate);
        // Strength 1:1 must be a true no-op. Scale the gentle automatic
        // makeup up to the 3:1 reference point, then cap it.
        let comp_makeup_db = (COMP_MAKEUP_DB * (self.comp_ratio - 1.0) / 2.0).clamp(0.0, 6.0);
        let comp_makeup = db_to_linear(comp_makeup_db);
        let limiter_ceiling = db_to_linear(self.limiter_ceiling_db);
        let limiter_release = time_coeff(LIMIT_RELEASE_MS, self.sample_rate);
        let crossfeed_alpha =
            1.0 - (-2.0 * std::f32::consts::PI * CROSSFEED_HZ / self.sample_rate).exp();

        for frame in buf.chunks_exact_mut(2) {
            let mut stereo = [frame[0], frame[1]];
            if self.enabled {
                for (ch, sample) in stereo.iter_mut().enumerate() {
                    let mut x = *sample * self.preamp_linear;
                    for i in 0..self.count {
                        x = self.state[i][ch].process(x, &self.coeffs[i]);
                    }
                    for i in 0..self.tone_coeffs.len() {
                        x = self.tone_state[i][ch].process(x, &self.tone_coeffs[i]);
                    }
                    *sample = x;
                }
            }

            if self.headphones {
                self.crossfeed_lp[0] += crossfeed_alpha * (stereo[0] - self.crossfeed_lp[0]);
                self.crossfeed_lp[1] += crossfeed_alpha * (stereo[1] - self.crossfeed_lp[1]);
                stereo = [
                    stereo[0] * (1.0 - CROSSFEED_AMOUNT) + self.crossfeed_lp[1] * CROSSFEED_AMOUNT,
                    stereo[1] * (1.0 - CROSSFEED_AMOUNT) + self.crossfeed_lp[0] * CROSSFEED_AMOUNT,
                ];
            }

            if self.gate_enabled {
                let mag = stereo[0].abs().max(stereo[1].abs());
                self.gate_env = if mag > self.gate_env {
                    mag + gate_attack * (self.gate_env - mag)
                } else {
                    mag + gate_release * (self.gate_env - mag)
                };
                let open = self.gate_env > gate_threshold;
                if open {
                    self.gate_hold = gate_hold_samples;
                } else if self.gate_hold > 0 {
                    self.gate_hold -= 1;
                }
                let target = if open || self.gate_hold > 0 { 1.0 } else { 0.0 };
                let coefficient = if target > self.gate_gain {
                    gate_attack
                } else {
                    gate_release
                };
                self.gate_gain = target + coefficient * (self.gate_gain - target);
                stereo[0] *= self.gate_gain;
                stereo[1] *= self.gate_gain;
            }

            stereo[0] *= self.boost_linear;
            stereo[1] *= self.boost_linear;

            if self.comp_enabled {
                let mag = stereo[0].abs().max(stereo[1].abs()).max(1e-9);
                self.comp_env = if mag > self.comp_env {
                    mag + comp_attack * (self.comp_env - mag)
                } else {
                    mag + comp_release * (self.comp_env - mag)
                };
                let over_db = 20.0 * self.comp_env.log10() - self.comp_threshold_db;
                let reduction = if over_db > 0.0 {
                    db_to_linear(-over_db * (1.0 - 1.0 / self.comp_ratio.max(1.0)))
                } else {
                    1.0
                } * comp_makeup;
                stereo[0] *= reduction;
                stereo[1] *= reduction;
            }

            if self.limiter_enabled {
                let mag = stereo[0].abs().max(stereo[1].abs());
                let needed = if mag * self.limiter_gain > limiter_ceiling && mag > 0.0 {
                    limiter_ceiling / mag
                } else {
                    1.0
                };
                if needed < self.limiter_gain {
                    self.limiter_gain = needed;
                } else {
                    self.limiter_gain = needed + limiter_release * (self.limiter_gain - needed);
                }
                stereo[0] =
                    (stereo[0] * self.limiter_gain).clamp(-limiter_ceiling, limiter_ceiling);
                stereo[1] =
                    (stereo[1] * self.limiter_gain).clamp(-limiter_ceiling, limiter_ceiling);
            }

            frame.copy_from_slice(&stereo);
        }
    }

    /// Process a mono microphone buffer through only the parametric EQ
    /// portion of this engine. Microphone dynamics are handled by
    /// `DspChain`, so applying the channel dynamics here as well would
    /// compress/gate the signal twice.
    pub fn process_mono_eq(&mut self, buf: &mut [f32], params: &EqParams) {
        self.refresh(params);
        if !self.enabled {
            return;
        }
        for sample in buf {
            let mut value = *sample * self.preamp_linear;
            for band in 0..self.count {
                value = self.state[band][0].process(value, &self.coeffs[band]);
            }
            for tone in 0..self.tone_coeffs.len() {
                value = self.tone_state[tone][0].process(value, &self.tone_coeffs[tone]);
            }
            *sample = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Analytic magnitude response |H(e^jw)| in dB - exact, no time-domain
    /// sampling artifacts. This is the same formula the UI curve uses
    /// (src/lib/eqMath.ts), so these tests also pin the shared math.
    fn measured_gain_db(c: &BiquadCoeffs, freq: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sample_rate);
        let (b0, b1, b2) = (f64::from(c.b0), f64::from(c.b1), f64::from(c.b2));
        let (a1, a2) = (f64::from(c.a1), f64::from(c.a2));
        let num_re = b0 + b1 * w.cos() + b2 * (2.0 * w).cos();
        let num_im = -(b1 * w.sin() + b2 * (2.0 * w).sin());
        let den_re = 1.0 + a1 * w.cos() + a2 * (2.0 * w).cos();
        let den_im = -(a1 * w.sin() + a2 * (2.0 * w).sin());
        let mag =
            ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im)).sqrt();
        (20.0 * mag.log10()) as f32
    }

    fn config(bands: Vec<EqBand>, preamp_db: f32) -> EqConfig {
        EqConfig {
            enabled: true,
            preamp_db,
            bands,
            ..EqConfig::default()
        }
    }

    fn band(kind: EqBandKind, freq_hz: f32, gain_db: f32, q: f32) -> EqBand {
        EqBand {
            kind,
            freq_hz,
            gain_db,
            q,
        }
    }

    const SR: f32 = 48000.0;

    #[test]
    fn identity_passes_signal_unchanged() {
        let c = BiquadCoeffs::identity();
        let mut state = BiquadState::default();
        for x in [0.0f32, 1.0, -0.5, 0.25] {
            assert_eq!(state.process(x, &c), x);
        }
    }

    #[test]
    fn mono_eq_uses_the_same_preamp_and_bands() {
        let params = EqParams::from_config(&config(Vec::new(), -6.0));
        let mut engine = EqEngine::new(SR);
        let mut mono = [1.0f32, -0.5];
        engine.process_mono_eq(&mut mono, &params);
        let gain = db_to_linear(-6.0);
        assert!((mono[0] - gain).abs() < 1e-5);
        assert!((mono[1] + 0.5 * gain).abs() < 1e-5);
    }

    #[test]
    fn peaking_matches_gain_at_center_freq() {
        let c = BiquadCoeffs::design(EqBandKind::Peaking, 1000.0, 6.0, 1.0, SR);
        let g = measured_gain_db(&c, 1000.0, SR);
        assert!((g - 6.0).abs() < 0.3, "expected ~+6 dB at center, got {g}");
    }

    #[test]
    fn peaking_is_flat_far_from_center() {
        let c = BiquadCoeffs::design(EqBandKind::Peaking, 1000.0, 12.0, 2.0, SR);
        let g = measured_gain_db(&c, 60.0, SR);
        assert!(g.abs() < 0.5, "expected ~0 dB two+ octaves away, got {g}");
    }

    #[test]
    fn low_shelf_boosts_below_corner_flat_above() {
        let c = BiquadCoeffs::design(EqBandKind::LowShelf, 200.0, 6.0, 0.71, SR);
        let low = measured_gain_db(&c, 40.0, SR);
        let high = measured_gain_db(&c, 4000.0, SR);
        assert!(
            (low - 6.0).abs() < 0.5,
            "low end should be ~+6 dB, got {low}"
        );
        assert!(high.abs() < 0.5, "high end should be flat, got {high}");
    }

    #[test]
    fn high_shelf_boosts_above_corner_flat_below() {
        let c = BiquadCoeffs::design(EqBandKind::HighShelf, 5000.0, -6.0, 0.71, SR);
        let low = measured_gain_db(&c, 200.0, SR);
        let high = measured_gain_db(&c, 15000.0, SR);
        assert!(low.abs() < 0.5, "low end should be flat, got {low}");
        assert!(
            (high + 6.0).abs() < 0.6,
            "high end should be ~-6 dB, got {high}"
        );
    }

    #[test]
    fn low_pass_attenuates_above_cutoff() {
        let c = BiquadCoeffs::design(EqBandKind::LowPass, 1000.0, 0.0, 0.71, SR);
        let pass = measured_gain_db(&c, 100.0, SR);
        let stop = measured_gain_db(&c, 8000.0, SR);
        assert!(pass.abs() < 0.5, "passband should be flat, got {pass}");
        assert!(
            stop < -30.0,
            "stopband should be strongly attenuated, got {stop}"
        );
    }

    #[test]
    fn high_pass_attenuates_below_cutoff() {
        let c = BiquadCoeffs::design(EqBandKind::HighPass, 1000.0, 0.0, 0.71, SR);
        let stop = measured_gain_db(&c, 100.0, SR);
        let pass = measured_gain_db(&c, 8000.0, SR);
        assert!(
            stop < -30.0,
            "stopband should be strongly attenuated, got {stop}"
        );
        assert!(pass.abs() < 0.5, "passband should be flat, got {pass}");
    }

    #[test]
    fn params_apply_then_engine_refresh_picks_up_bands() {
        let params = EqParams::from_config(&config(
            vec![band(EqBandKind::Peaking, 2000.0, 4.0, 1.5)],
            -3.0,
        ));
        let mut engine = EqEngine::new(SR);
        engine.refresh(&params);
        assert!(engine.enabled);
        assert_eq!(engine.count, 1);
        assert!((engine.preamp_linear - 10.0f32.powf(-3.0 / 20.0)).abs() < 1e-6);
        let expected = BiquadCoeffs::design(EqBandKind::Peaking, 2000.0, 4.0, 1.5, SR);
        assert_eq!(engine.coeffs[0], expected);

        // A second apply with different bands is picked up on next refresh.
        params.apply(&config(
            vec![band(EqBandKind::HighPass, 80.0, 0.0, 0.71)],
            0.0,
        ));
        engine.refresh(&params);
        assert_eq!(engine.count, 1);
        let expected = BiquadCoeffs::design(EqBandKind::HighPass, 80.0, 0.0, 0.71, SR);
        assert_eq!(engine.coeffs[0], expected);
    }

    #[test]
    fn set_sample_rate_forces_recompute_at_new_rate() {
        let params = EqParams::from_config(&config(
            vec![band(EqBandKind::Peaking, 2000.0, 4.0, 1.5)],
            0.0,
        ));
        let mut engine = EqEngine::new(48000.0);
        engine.refresh(&params);
        let at_48k = engine.coeffs[0];
        engine.set_sample_rate(96000.0);
        engine.refresh(&params);
        assert_ne!(engine.coeffs[0], at_48k);
        assert_eq!(
            engine.coeffs[0],
            BiquadCoeffs::design(EqBandKind::Peaking, 2000.0, 4.0, 1.5, 96000.0)
        );
    }

    #[test]
    fn preamp_is_linear_multiply_before_cascade() {
        let params = EqParams::from_config(&config(vec![], 6.0));
        let mut engine = EqEngine::new(SR);
        let mut buf = vec![0.5f32, -0.5, 0.25, -0.25];
        engine.process_interleaved(&mut buf, &params);
        let expected = 10.0f32.powf(6.0 / 20.0);
        assert!((buf[0] - 0.5 * expected).abs() < 1e-6);
        assert!((buf[1] + 0.5 * expected).abs() < 1e-6);
    }

    #[test]
    fn tone_stage_is_independent_of_parametric_bands() {
        let mut cfg = config(Vec::new(), 0.0);
        cfg.tone_bass_db = 6.0;
        cfg.tone_voice_db = -4.0;
        cfg.tone_treble_db = 3.0;
        let params = EqParams::from_config(&cfg);
        let mut engine = EqEngine::new(SR);
        engine.refresh(&params);

        assert_eq!(engine.count, 0, "tone controls must not create graph bands");
        let bass = measured_gain_db(&engine.tone_coeffs[0], 40.0, SR);
        let voice = measured_gain_db(&engine.tone_coeffs[1], 1200.0, SR);
        let treble = measured_gain_db(&engine.tone_coeffs[2], 15000.0, SR);
        assert!(
            (bass - 6.0).abs() < 0.6,
            "bass stage should add ~6 dB, got {bass}"
        );
        assert!(
            (voice + 4.0).abs() < 0.3,
            "voice stage should cut ~4 dB, got {voice}"
        );
        assert!(
            (treble - 3.0).abs() < 0.6,
            "treble stage should add ~3 dB, got {treble}"
        );
    }

    #[test]
    fn disabled_config_passes_through_untouched() {
        let mut cfg = config(vec![band(EqBandKind::Peaking, 1000.0, 12.0, 1.0)], 12.0);
        cfg.enabled = false;
        let params = EqParams::from_config(&cfg);
        let mut engine = EqEngine::new(SR);
        let original = vec![0.5f32, -0.5, 0.25, -0.25];
        let mut buf = original.clone();
        engine.process_interleaved(&mut buf, &params);
        assert_eq!(buf, original);
    }

    #[test]
    fn cascade_of_ten_flat_bands_is_still_flat() {
        let bands: Vec<EqBand> = (0..MAX_EQ_BANDS)
            .map(|i| {
                band(
                    EqBandKind::Peaking,
                    100.0 * (i as f32 + 1.0) * 2.0,
                    0.0,
                    1.0,
                )
            })
            .collect();
        let params = EqParams::from_config(&config(bands, 0.0));
        let mut engine = EqEngine::new(SR);
        // A 1 kHz sine through ten 0 dB bands should come out at unity.
        let mut buf: Vec<f32> = (0..2000)
            .flat_map(|n| {
                let s = (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / SR).sin() * 0.5;
                [s, s]
            })
            .collect();
        let peak_in = 0.5f32;
        engine.process_interleaved(&mut buf, &params);
        let peak_out = buf[2000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            (peak_out - peak_in).abs() < 0.01,
            "flat cascade drifted: in {peak_in}, out {peak_out}"
        );
    }

    #[test]
    fn stereo_channels_are_processed_independently() {
        // A DC-blocking high-pass fed L=DC, R=silence must not bleed.
        let params = EqParams::from_config(&config(
            vec![band(EqBandKind::HighPass, 500.0, 0.0, 0.71)],
            0.0,
        ));
        let mut engine = EqEngine::new(SR);
        let mut buf: Vec<f32> = (0..2000).flat_map(|_| [1.0f32, 0.0]).collect();
        engine.process_interleaved(&mut buf, &params);
        let right_energy: f32 = buf.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
        assert_eq!(right_energy, 0.0, "silent right channel picked up energy");
        let left_tail = buf[3000..]
            .iter()
            .step_by(2)
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            left_tail < 0.01,
            "DC should be blocked on the left, got {left_tail}"
        );
    }

    #[test]
    fn boost_and_limiter_process_with_eq_disabled() {
        let cfg = EqConfig {
            boost_db: 12.0,
            limiter_enabled: true,
            limiter_ceiling_db: -3.0,
            ..EqConfig::default()
        };
        assert!(cfg.needs_chain());
        let params = EqParams::from_config(&cfg);
        let mut engine = EqEngine::new(SR);
        let mut buf = vec![0.8f32; 4800];
        engine.process_interleaved(&mut buf, &params);
        let ceiling = db_to_linear(-3.0);
        assert!(buf.iter().all(|x| x.abs() <= ceiling + 1e-5));
    }

    #[test]
    fn linked_compressor_preserves_stereo_ratio() {
        let cfg = EqConfig {
            comp_enabled: true,
            comp_threshold_db: -24.0,
            comp_ratio: 4.0,
            ..EqConfig::default()
        };
        let params = EqParams::from_config(&cfg);
        let mut engine = EqEngine::new(SR);
        let mut buf: Vec<f32> = (0..48000).flat_map(|_| [0.5f32, 0.25]).collect();
        engine.process_interleaved(&mut buf, &params);
        let tail = &buf[buf.len() - 2000..];
        for frame in tail.chunks_exact(2) {
            assert!((frame[0] / frame[1] - 2.0).abs() < 1e-4);
        }
    }

    #[test]
    fn headphone_mode_crossfeeds_left_into_right() {
        let cfg = EqConfig {
            playback_mode: PlaybackMode::Headphones,
            ..EqConfig::default()
        };
        let params = EqParams::from_config(&cfg);
        let mut engine = EqEngine::new(SR);
        let mut buf: Vec<f32> = (0..4800).flat_map(|_| [0.5f32, 0.0]).collect();
        engine.process_interleaved(&mut buf, &params);
        let right_peak = buf
            .iter()
            .skip(1)
            .step_by(2)
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(right_peak > 0.01, "crossfeed should reach the silent side");
        assert!(right_peak < 0.1, "crossfeed must stay subtle");
    }
}
