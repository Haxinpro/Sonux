#![allow(clippy::items_after_test_module)]

//! Phase 3 mic engine: captures the selected (or default) microphone,
//! runs the native DSP chain (EQ → gate → gain → compressor → limiter), and
//! plays the processed signal into a `Audio/Source/Virtual` node - a
//! virtual microphone that Discord/OBS can capture.
//!
//! Topology:  hw mic ──capture stream──▶ DSP ──ring──▶ playback stream ──▶ sink_mic (virtual source)

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;

use crate::audio::pw_native::dsp::{DspChain, DspSettings};
use crate::audio::pw_native::eq::{EqEngine, EqParams};
use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::ring::Ring;
use crate::audio::types::{EqConfig, MicConfig, MicTestAction, MicTestStatus};
use crate::error::SinkError;

/// node.name of the permanent primary virtual microphone.
pub const MIC_NODE: &str = "sink_mic";

pub fn is_mic_node(name: &str) -> bool {
    name == MIC_NODE || name.starts_with("source_mic_")
}

const MIC_TEST_SECONDS: usize = 30;
const MIC_TEST_RATE: usize = 48_000;
const TEST_IDLE: u8 = 0;
const TEST_RECORDING: u8 = 1;
const TEST_LOOPING: u8 = 2;
const CAPTURE_CHUNK_SAMPLES: usize = 4096;

fn decode_f32_chunk(bytes: &[u8], output: &mut Vec<f32>) {
    debug_assert!(bytes.len() / 4 <= output.capacity());
    output.clear();
    output.extend(
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])),
    );
}

/// A fixed-size lock-free raw microphone recorder. The capture callback
/// records before EQ/dynamics and substitutes the saved raw samples while
/// looping, so every live processing edit is audible immediately.
pub struct MicTestBuffer {
    samples: Box<[AtomicU32]>,
    length: AtomicUsize,
    cursor: AtomicUsize,
    mode: AtomicU8,
}

impl MicTestBuffer {
    fn new() -> Self {
        Self {
            samples: (0..MIC_TEST_SECONDS * MIC_TEST_RATE)
                .map(|_| AtomicU32::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            length: AtomicUsize::new(0),
            cursor: AtomicUsize::new(0),
            mode: AtomicU8::new(TEST_IDLE),
        }
    }

    pub fn apply(&self, action: MicTestAction) -> Result<MicTestStatus, SinkError> {
        match action {
            MicTestAction::StartRecording => {
                self.mode.store(TEST_IDLE, Ordering::Release);
                self.length.store(0, Ordering::Release);
                self.cursor.store(0, Ordering::Release);
                self.mode.store(TEST_RECORDING, Ordering::Release);
            }
            MicTestAction::StopRecording => {
                if self.mode.load(Ordering::Acquire) == TEST_RECORDING {
                    self.mode.store(TEST_IDLE, Ordering::Release);
                }
            }
            MicTestAction::StartLoop => {
                if self.length.load(Ordering::Acquire) == 0 {
                    return Err(SinkError::Config("record a microphone test first".into()));
                }
                self.cursor.store(0, Ordering::Release);
                self.mode.store(TEST_LOOPING, Ordering::Release);
            }
            MicTestAction::StopLoop => {
                if self.mode.load(Ordering::Acquire) == TEST_LOOPING {
                    self.mode.store(TEST_IDLE, Ordering::Release);
                }
            }
            MicTestAction::Status => {}
        }
        Ok(self.status())
    }

    fn status(&self) -> MicTestStatus {
        let mode = self.mode.load(Ordering::Acquire);
        MicTestStatus {
            recording: mode == TEST_RECORDING,
            playing: mode == TEST_LOOPING,
            has_recording: self.length.load(Ordering::Acquire) > 0,
        }
    }

    fn process_raw(&self, buffer: &mut [f32]) {
        match self.mode.load(Ordering::Acquire) {
            TEST_RECORDING => {
                let start = self.cursor.load(Ordering::Relaxed);
                let available = self.samples.len().saturating_sub(start);
                let count = available.min(buffer.len());
                for (slot, sample) in self.samples[start..start + count].iter().zip(buffer.iter()) {
                    slot.store(sample.to_bits(), Ordering::Relaxed);
                }
                let end = start + count;
                self.cursor.store(end, Ordering::Relaxed);
                self.length.store(end, Ordering::Release);
                if count < buffer.len() || end == self.samples.len() {
                    self.mode.store(TEST_IDLE, Ordering::Release);
                }
            }
            TEST_LOOPING => {
                let length = self.length.load(Ordering::Acquire);
                if length == 0 {
                    self.mode.store(TEST_IDLE, Ordering::Release);
                    return;
                }
                let mut cursor = self.cursor.load(Ordering::Relaxed).min(length - 1);
                for sample in buffer {
                    *sample = f32::from_bits(self.samples[cursor].load(Ordering::Relaxed));
                    cursor += 1;
                    if cursor == length {
                        cursor = 0;
                    }
                }
                self.cursor.store(cursor, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

fn mic_eq_config(config: &MicConfig) -> EqConfig {
    EqConfig {
        enabled: config.eq_enabled,
        preamp_db: config.eq_preamp_db,
        bands: config.eq_bands.clone(),
        tone_bass_db: 0.0,
        tone_voice_db: 0.0,
        tone_treble_db: 0.0,
        ..EqConfig::default()
    }
}

/// Live-tunable DSP parameters, shared with the RT capture callback.
pub struct MicParams {
    pub eq: EqParams,
    gain_bits: AtomicU32,
    gate: AtomicBool,
    comp: AtomicBool,
    limiter: AtomicBool,
    muted: AtomicBool,
    gate_threshold_bits: AtomicU32,
    comp_threshold_bits: AtomicU32,
    comp_ratio_bits: AtomicU32,
    limiter_ceiling_bits: AtomicU32,
}

impl MicParams {
    pub fn from_config(config: &MicConfig) -> Self {
        let p = Self {
            eq: EqParams::from_config(&mic_eq_config(config)),
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
            gate: AtomicBool::new(true),
            comp: AtomicBool::new(true),
            limiter: AtomicBool::new(true),
            muted: AtomicBool::new(false),
            gate_threshold_bits: AtomicU32::new((-40.0f32).to_bits()),
            comp_threshold_bits: AtomicU32::new((-18.0f32).to_bits()),
            comp_ratio_bits: AtomicU32::new(3.0f32.to_bits()),
            limiter_ceiling_bits: AtomicU32::new((-1.0f32).to_bits()),
        };
        p.apply(config);
        p
    }

    pub fn apply(&self, config: &MicConfig) {
        self.eq.apply(&mic_eq_config(config));
        let gain = f32::from(config.gain_percent) / 100.0;
        self.gain_bits.store(gain.to_bits(), Ordering::Relaxed);
        self.gate.store(config.gate_enabled, Ordering::Relaxed);
        self.comp.store(config.comp_enabled, Ordering::Relaxed);
        self.limiter
            .store(config.limiter_enabled, Ordering::Relaxed);
        self.muted.store(config.muted, Ordering::Relaxed);
        self.gate_threshold_bits
            .store(config.gate_threshold_db.to_bits(), Ordering::Relaxed);
        self.comp_threshold_bits
            .store(config.comp_threshold_db.to_bits(), Ordering::Relaxed);
        self.comp_ratio_bits
            .store(config.comp_ratio.to_bits(), Ordering::Relaxed);
        self.limiter_ceiling_bits
            .store(config.limiter_ceiling_db.to_bits(), Ordering::Relaxed);
    }

    fn settings(&self) -> DspSettings {
        DspSettings {
            gate_enabled: self.gate.load(Ordering::Relaxed),
            comp_enabled: self.comp.load(Ordering::Relaxed),
            limiter_enabled: self.limiter.load(Ordering::Relaxed),
            gain: f32::from_bits(self.gain_bits.load(Ordering::Relaxed)),
            muted: self.muted.load(Ordering::Relaxed),
            gate_threshold_db: f32::from_bits(self.gate_threshold_bits.load(Ordering::Relaxed)),
            comp_threshold_db: f32::from_bits(self.comp_threshold_bits.load(Ordering::Relaxed)),
            comp_ratio: f32::from_bits(self.comp_ratio_bits.load(Ordering::Relaxed)),
            limiter_ceiling_db: f32::from_bits(self.limiter_ceiling_bits.load(Ordering::Relaxed)),
        }
    }
}

struct CaptureCtx {
    eq: EqEngine,
    chain: DspChain,
    params: Arc<MicParams>,
    test: Arc<MicTestBuffer>,
    ring: Arc<Ring>,
    levels: Arc<LevelStore>,
    level_slot: usize,
    scratch: Vec<f32>,
}

struct PlaybackCtx {
    ring: Arc<Ring>,
}

pub struct MicStreams {
    _capture: pw::stream::StreamRc,
    _capture_listener: pw::stream::StreamListener<CaptureCtx>,
    playback: pw::stream::StreamRc,
    _playback_listener: pw::stream::StreamListener<PlaybackCtx>,
    pub params: Arc<MicParams>,
    pub test: Arc<MicTestBuffer>,
}

impl MicStreams {
    /// Node id of the playback stream - the loop links its output ports to
    /// the virtual mic itself (WirePlumber 0.5 does not reliably honor
    /// target.object for playback→virtual-source routing).
    pub fn playback_node_id(&self) -> u32 {
        self.playback.node_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_test_recording_loops_before_processing() {
        let test = MicTestBuffer::new();
        test.apply(MicTestAction::StartRecording).unwrap();
        let mut original = [0.1f32, -0.2, 0.3, -0.4];
        test.process_raw(&mut original);
        test.apply(MicTestAction::StopRecording).unwrap();
        assert!(test.status().has_recording);

        test.apply(MicTestAction::StartLoop).unwrap();
        let mut looped = [0.0f32; 8];
        test.process_raw(&mut looped);
        assert_eq!(looped, [0.1, -0.2, 0.3, -0.4, 0.1, -0.2, 0.3, -0.4]);
    }

    #[test]
    fn oversized_capture_is_decoded_without_growing_rt_storage() {
        let bytes = vec![0u8; CAPTURE_CHUNK_SAMPLES * 4 * 3];
        let mut decoded = Vec::with_capacity(CAPTURE_CHUNK_SAMPLES);
        let original_capacity = decoded.capacity();
        let mut samples = 0;
        for chunk in bytes.chunks(CAPTURE_CHUNK_SAMPLES * 4) {
            decode_f32_chunk(chunk, &mut decoded);
            samples += decoded.len();
            assert_eq!(decoded.capacity(), original_capacity);
        }
        assert_eq!(samples, CAPTURE_CHUNK_SAMPLES * 3);
    }
}

/// Mono F32 format pod for stream negotiation.
fn mono_f32_format() -> Result<Vec<u8>, SinkError> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_channels(1);
    let object = spa::pod::Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Format,
        id: spa::sys::SPA_PARAM_EnumFormat,
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(c, _)| c.into_inner())
    .map_err(|e| SinkError::Config(format!("mic format pod: {e:?}")))
}

impl MicStreams {
    /// Build both streams. `mic_target` is the node.name of the hardware
    /// mic to capture (None = system default source). Targets are set via
    /// the `target.object` property - the connect-id parameter is
    /// deprecated and WirePlumber 0.5 ignores it.
    pub fn new(
        core: &pw::core::CoreRc,
        config: &MicConfig,
        node_name: &str,
        mic_target: Option<&str>,
        levels: Arc<LevelStore>,
    ) -> Result<Self, SinkError> {
        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("mic {stage}: {e}"));
        let params = Arc::new(MicParams::from_config(config));
        let test = Arc::new(MicTestBuffer::new());
        let level_slot = levels
            .slot_for(node_name)
            .ok_or_else(|| SinkError::Config("meter budget exhausted for mic".into()))?;
        // ~85 ms of headroom at 48 kHz; actual added latency is one quantum.
        let ring = Arc::new(Ring::new(4096));

        // ---- capture: hardware mic -> DSP -> ring ----
        // NOT passive: this stream must hold the hardware mic running for
        // as long as the chain is enabled. With passive links the source
        // suspends the moment its last real consumer leaves (e.g. Discord
        // switching from the raw mic to the virtual one) - and the chain
        // starves exactly when someone starts using it.
        let capture_name = format!("sink-internal-{node_name}-capture");
        let playback_name = format!("sink-internal-{node_name}-playback");
        let mut capture_props = pw::properties::properties! {
            "media.type" => "Audio",
            "media.category" => "Capture",
            "node.name" => capture_name.as_str(),
            // Never let the session manager migrate this stream (e.g. when
            // the default source changes - it could land on sink_mic and
            // feed the chain its own output). Default-follow is handled by
            // rebuilding with a resolved hardware target instead.
            "node.dont-reconnect" => "true",
        };
        if let Some(target) = mic_target {
            capture_props.insert("target.object", target);
        }
        let capture = pw::stream::StreamRc::new(core.clone(), &capture_name, capture_props)
            .map_err(|e| err("capture stream", e))?;

        let capture_listener = capture
            .add_local_listener_with_user_data(CaptureCtx {
                eq: EqEngine::new(48000.0),
                chain: DspChain::new(48000.0),
                params: params.clone(),
                test: test.clone(),
                ring: ring.clone(),
                levels,
                level_slot,
                scratch: Vec::with_capacity(CAPTURE_CHUNK_SAMPLES),
            })
            .param_changed(|_, ctx, id, param| {
                // Track the negotiated rate so DSP time constants are right.
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let Some(param) = param else { return };
                let mut info = spa::param::audio::AudioInfoRaw::new();
                if info.parse(param).is_ok() && info.rate() > 0 {
                    ctx.eq.set_sample_rate(info.rate() as f32);
                    ctx.chain = DspChain::new(info.rate() as f32);
                }
            })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let valid = data.chunk().size() as usize;
                let Some(bytes) = data.data() else { return };

                let samples = valid.min(bytes.len()) / 4;
                let settings = ctx.params.settings();
                let mut peak = 0.0f32;
                for chunk in bytes[..samples * 4].chunks(CAPTURE_CHUNK_SAMPLES * 4) {
                    decode_f32_chunk(chunk, &mut ctx.scratch);

                    // Record or substitute the raw hardware signal before any
                    // processing. A running loop therefore follows EQ/dynamics
                    // edits in real time.
                    ctx.test.process_raw(&mut ctx.scratch);
                    ctx.eq.process_mono_eq(&mut ctx.scratch, &ctx.params.eq);
                    ctx.chain.process(&mut ctx.scratch, &settings);

                    peak = ctx
                        .scratch
                        .iter()
                        .fold(peak, |maximum, sample| maximum.max(sample.abs()));
                    ctx.ring.push(&ctx.scratch);
                }

                // Post-DSP level for the UI (mono → both meter channels).
                ctx.levels.raise(ctx.level_slot, 0, peak);
                ctx.levels.raise(ctx.level_slot, 1, peak);
            })
            .register()
            .map_err(|e| err("capture listener", e))?;

        let format = mono_f32_format()?;
        let mut capture_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("mic capture format pod invalid".into()))?];
        capture
            .connect(
                spa::utils::Direction::Input,
                None,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut capture_params,
            )
            .map_err(|e| err("capture connect", e))?;

        // ---- playback: ring -> virtual source ----
        // node.autoconnect=false keeps WirePlumber's hands off this stream
        // (it routes playback streams to the default *sink*, i.e. the
        // speakers - observed live); the loop links it to sink_mic itself.
        let playback = pw::stream::StreamRc::new(
            core.clone(),
            &playback_name,
            // NOT passive (see capture): the processed signal must reach
            // sink_mic whenever the chain is up, regardless of who is -
            // or isn't - capturing at this instant.
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Playback",
                "node.name" => playback_name.as_str(),
                "node.autoconnect" => "false",
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("playback stream", e))?;

        let playback_listener = playback
            .add_local_listener_with_user_data(PlaybackCtx { ring })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                // Fill only what the graph asked for this cycle - filling
                // the whole mmap'd buffer (8k+ frames vs ~1k produced per
                // quantum) starves the ring and chops the audio.
                let requested = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let max_bytes = data.data().map(|d| d.len()).unwrap_or(0);
                let max_frames = max_bytes / 4;
                let n = if requested > 0 {
                    requested.min(max_frames)
                } else {
                    max_frames.min(1024)
                };
                if n == 0 {
                    return;
                }
                {
                    let bytes = data.data().expect("checked above");
                    // Pop straight into the buffer as f32 ne bytes.
                    let mut frame = [0.0f32; 1024];
                    let mut written = 0;
                    while written < n {
                        let take = (n - written).min(frame.len());
                        ctx.ring.pop(&mut frame[..take]);
                        for (i, s) in frame[..take].iter().enumerate() {
                            let off = (written + i) * 4;
                            bytes[off..off + 4].copy_from_slice(&s.to_ne_bytes());
                        }
                        written += take;
                    }
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = 4;
                *chunk.size_mut() = (n * 4) as u32;
            })
            .register()
            .map_err(|e| err("playback listener", e))?;

        let mut playback_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("mic playback format pod invalid".into()))?];
        playback
            .connect(
                spa::utils::Direction::Output,
                None,
                // No AUTOCONNECT: the loop creates the links to sink_mic.
                pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                &mut playback_params,
            )
            .map_err(|e| err("playback connect", e))?;

        Ok(Self {
            _capture: capture,
            _capture_listener: capture_listener,
            playback,
            _playback_listener: playback_listener,
            params,
            test,
        })
    }
}
