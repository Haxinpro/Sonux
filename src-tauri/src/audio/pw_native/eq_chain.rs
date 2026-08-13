#![allow(clippy::items_after_test_module)]

//! Per-channel EQ insert: captures a channel sink's monitor (post-fader,
//! the same tap point as the meters), runs the biquad cascade, and plays
//! the processed signal back out through a stream whose ports the loop
//! links to the channel's real targets (device, buses) instead of the raw
//! monitor.
//!
//! Topology:  channel sink ──monitor──▶ capture ──EQ──▶ ring ──▶ playback ──▶ device/buses
//!
//! Same construction as the mic chain (`mic.rs`), minus metering: EQ taps
//! add no LevelStore slots, so the MAX_METERS budget is untouched.

use std::sync::atomic::{AtomicI16, AtomicU16, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;

use crate::audio::pw_native::eq::{EqEngine, EqParams};
use crate::audio::pw_native::ring::Ring;
use crate::audio::pw_native::spatial::{SpatialEngine, SURROUND_CHANNELS};
use crate::audio::types::{is_spatial_channel, ChannelTestAction, ChannelTestStatus, EqConfig};
use crate::error::SinkError;

/// node.name prefixes of the EQ helper streams (under INTERNAL_PREFIX, so
/// they never show up in app/stream listings).
pub const EQ_CAPTURE_PREFIX: &str = "sink-internal-eq-capture-";
pub const EQ_PLAYBACK_PREFIX: &str = "sink-internal-eq-playback-";
pub const TEST_DRIVER_PREFIX: &str = "sink-internal-channel-test-driver-";

const TEST_SECONDS: usize = 12;
const TEST_RATE: usize = 48_000;
const TEST_IDLE: u8 = 0;
const TEST_RECORDING: u8 = 1;
const TEST_LOOPING: u8 = 2;
const TEST_ONCE: u8 = 3;
/// Maximum interleaved samples decoded at once by the RT capture callback.
/// Divisible by both supported channel counts (stereo and 7.1).
const CAPTURE_CHUNK_SAMPLES: usize = 8192;

fn decode_f32_chunk(bytes: &[u8], output: &mut Vec<f32>) {
    debug_assert!(bytes.len() / 4 <= output.capacity());
    output.clear();
    output.extend(
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])),
    );
}

/// Lock-free channel recorder owned by an EQ insert. It taps `input` before
/// spatial/EQ processing and substitutes saved samples at the same point.
/// PCM16 storage keeps the fixed RT-safe buffers modest (~23 MiB total for
/// the four default channels at twelve seconds each).
struct ChannelTestBuffer {
    samples: Box<[AtomicI16]>,
    channels: usize,
    length: AtomicUsize,
    cursor: AtomicUsize,
    mode: AtomicU8,
    user_recording: AtomicU8,
    recorded_peak: AtomicU16,
}

impl ChannelTestBuffer {
    fn new(channels: usize) -> Self {
        Self {
            samples: (0..TEST_SECONDS * TEST_RATE * channels)
                .map(|_| AtomicI16::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            channels,
            length: AtomicUsize::new(0),
            cursor: AtomicUsize::new(0),
            mode: AtomicU8::new(TEST_IDLE),
            user_recording: AtomicU8::new(0),
            recorded_peak: AtomicU16::new(0),
        }
    }

    fn status(&self) -> ChannelTestStatus {
        let mode = self.mode.load(Ordering::Acquire);
        let peak = f32::from(self.recorded_peak.load(Ordering::Acquire)) / f32::from(i16::MAX);
        ChannelTestStatus {
            recording: mode == TEST_RECORDING,
            playing: mode == TEST_LOOPING || mode == TEST_ONCE,
            has_recording: self.user_recording.load(Ordering::Acquire) != 0,
            recorded_peak_db: if peak > 0.0 {
                (20.0 * peak.log10()).max(-96.0)
            } else {
                -96.0
            },
        }
    }

    fn start_recording(&self) {
        self.mode.store(TEST_IDLE, Ordering::Release);
        self.length.store(0, Ordering::Release);
        self.cursor.store(0, Ordering::Release);
        self.user_recording.store(0, Ordering::Release);
        self.recorded_peak.store(0, Ordering::Release);
        self.mode.store(TEST_RECORDING, Ordering::Release);
    }

    fn stop_recording(&self) {
        if self.mode.load(Ordering::Acquire) == TEST_RECORDING {
            self.mode.store(TEST_IDLE, Ordering::Release);
        }
    }

    fn start_playback(&self) -> Result<(), SinkError> {
        if self.user_recording.load(Ordering::Acquire) == 0
            || self.length.load(Ordering::Acquire) == 0
        {
            return Err(SinkError::Config("record a channel test first".into()));
        }
        self.cursor.store(0, Ordering::Release);
        self.mode.store(TEST_LOOPING, Ordering::Release);
        Ok(())
    }

    fn stop_playback(&self) {
        let mode = self.mode.load(Ordering::Acquire);
        if mode == TEST_LOOPING || mode == TEST_ONCE {
            self.mode.store(TEST_IDLE, Ordering::Release);
        }
    }

    fn load_and_play(&self, source: &[i16], source_channels: usize) -> Result<(), SinkError> {
        if source_channels == 0 || source.len() < source_channels {
            return Err(SinkError::Config("test sample has no audio frames".into()));
        }
        self.mode.store(TEST_IDLE, Ordering::Release);
        self.user_recording.store(0, Ordering::Release);
        let frames = (source.len() / source_channels).min(self.samples.len() / self.channels);
        for frame in 0..frames {
            for channel in 0..self.channels {
                let value = match (source_channels, self.channels, channel) {
                    (1, _, _) => source[frame],
                    (2, 2, ch) => source[frame * 2 + ch],
                    (2, _, 0) => source[frame * 2],
                    (2, _, 1) => source[frame * 2 + 1],
                    (src, dst, ch) if src == dst => source[frame * src + ch],
                    _ => 0,
                };
                self.samples[frame * self.channels + channel].store(value, Ordering::Relaxed);
            }
        }
        let length = frames * self.channels;
        self.length.store(length, Ordering::Release);
        self.cursor.store(0, Ordering::Release);
        self.mode.store(TEST_ONCE, Ordering::Release);
        Ok(())
    }

    fn process_raw(&self, input: &mut [f32]) {
        match self.mode.load(Ordering::Acquire) {
            TEST_RECORDING => {
                let start = self.cursor.load(Ordering::Relaxed);
                let count = self.samples.len().saturating_sub(start).min(input.len());
                for (slot, sample) in self.samples[start..start + count].iter().zip(input.iter()) {
                    let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                    slot.store(pcm, Ordering::Relaxed);
                    self.recorded_peak
                        .fetch_max(pcm.unsigned_abs(), Ordering::Relaxed);
                }
                let end = start + count;
                self.cursor.store(end, Ordering::Relaxed);
                self.length.store(end, Ordering::Release);
                if end > 0 {
                    self.user_recording.store(1, Ordering::Release);
                }
                if count < input.len() || end == self.samples.len() {
                    self.mode.store(TEST_IDLE, Ordering::Release);
                }
            }
            TEST_LOOPING | TEST_ONCE => {
                let mode = self.mode.load(Ordering::Relaxed);
                let length = self.length.load(Ordering::Acquire);
                if length == 0 {
                    self.mode.store(TEST_IDLE, Ordering::Release);
                    input.fill(0.0);
                    return;
                }
                let mut cursor = self.cursor.load(Ordering::Relaxed).min(length - 1);
                let mut ended = false;
                for sample in input {
                    if ended {
                        *sample = 0.0;
                        continue;
                    }
                    *sample = f32::from(self.samples[cursor].load(Ordering::Relaxed))
                        / f32::from(i16::MAX);
                    cursor += 1;
                    if cursor == length {
                        if mode == TEST_LOOPING {
                            cursor = 0;
                        } else {
                            self.mode.store(TEST_IDLE, Ordering::Release);
                            cursor = 0;
                            ended = true;
                        }
                    }
                }
                self.cursor.store(cursor, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

struct EqCaptureCtx {
    engine: EqEngine,
    params: Arc<EqParams>,
    ring: Arc<Ring>,
    input: Vec<f32>,
    scratch: Vec<f32>,
    spatial: Option<SpatialEngine>,
    test: Arc<ChannelTestBuffer>,
}

struct EqPlaybackCtx {
    ring: Arc<Ring>,
}

struct TestDriverCtx {
    channels: usize,
}

pub struct EqChainHandle {
    _capture: pw::stream::StreamRc,
    _capture_listener: pw::stream::StreamListener<EqCaptureCtx>,
    playback: pw::stream::StreamRc,
    _playback_listener: pw::stream::StreamListener<EqPlaybackCtx>,
    pub params: Arc<EqParams>,
    driver: pw::stream::StreamRc,
    _driver_listener: pw::stream::StreamListener<TestDriverCtx>,
    test: Arc<ChannelTestBuffer>,
}

impl EqChainHandle {
    /// Node id of the playback stream - the loop links its output ports to
    /// the channel's targets. Only valid once the server has created the
    /// stream's node (callers filter u32::MAX, like `mic_playback_node`).
    pub fn playback_node_id(&self) -> u32 {
        self.playback.node_id()
    }

    pub fn test(&self, action: ChannelTestAction) -> Result<ChannelTestStatus, SinkError> {
        match action {
            ChannelTestAction::StartRecording => {
                self.test.start_recording();
                // Keep the passive monitor capture scheduled even before an
                // application produces its first frame. The driver writes
                // only zeros, so it cannot alter the recorded channel.
                self.driver.set_active(true).map_err(|error| {
                    SinkError::Config(format!("start channel recording: {error}"))
                })?;
            }
            ChannelTestAction::StopRecording => {
                self.test.stop_recording();
                let _ = self.driver.set_active(false);
            }
            ChannelTestAction::StartPlayback => {
                self.test.start_playback()?;
                self.driver
                    .set_active(true)
                    .map_err(|error| SinkError::Config(format!("start channel test: {error}")))?;
            }
            ChannelTestAction::StopPlayback => {
                self.test.stop_playback();
                let _ = self.driver.set_active(false);
            }
            ChannelTestAction::LoadAndPlay { samples, channels } => {
                self.test.load_and_play(&samples, channels)?;
                self.driver
                    .set_active(true)
                    .map_err(|error| SinkError::Config(format!("start channel sample: {error}")))?;
            }
            ChannelTestAction::Status => {
                let status = self.test.status();
                if !status.recording && !status.playing {
                    let _ = self.driver.set_active(false);
                }
            }
        }
        Ok(self.test.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }

    #[test]
    fn records_and_repeats_raw_audio_before_processing() {
        let test = ChannelTestBuffer::new(2);
        let original = [0.25, -0.5, 0.75, -0.125];
        test.start_recording();
        test.process_raw(&mut original.clone());
        test.stop_recording();
        assert!(test.status().has_recording);

        test.start_playback().unwrap();
        let mut playback = [0.0; 8];
        test.process_raw(&mut playback);
        for (actual, expected) in playback.iter().zip(original.iter().cycle()) {
            near(*actual, *expected);
        }
    }

    #[test]
    fn bundled_sample_plays_once_then_fills_the_quantum_with_silence() {
        let test = ChannelTestBuffer::new(2);
        test.load_and_play(&[8192, -8192], 2).unwrap();
        let mut playback = [1.0; 6];
        test.process_raw(&mut playback);
        near(playback[0], 8192.0 / f32::from(i16::MAX));
        near(playback[1], -8192.0 / f32::from(i16::MAX));
        assert_eq!(&playback[2..], &[0.0; 4]);
        assert!(!test.status().playing);
        assert!(!test.status().has_recording);
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

/// Interleaved F32 format pod for stream negotiation. Game/Media use the
/// stable 7.1 order FL FR FC LFE RL RR SL SR; all other inserts are stereo.
fn f32_format(channels: usize) -> Result<Vec<u8>, SinkError> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(48_000);
    info.set_channels(channels as u32);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    if channels == SURROUND_CHANNELS {
        position[..SURROUND_CHANNELS].copy_from_slice(&[
            spa::sys::SPA_AUDIO_CHANNEL_FL,
            spa::sys::SPA_AUDIO_CHANNEL_FR,
            spa::sys::SPA_AUDIO_CHANNEL_FC,
            spa::sys::SPA_AUDIO_CHANNEL_LFE,
            spa::sys::SPA_AUDIO_CHANNEL_RL,
            spa::sys::SPA_AUDIO_CHANNEL_RR,
            spa::sys::SPA_AUDIO_CHANNEL_SL,
            spa::sys::SPA_AUDIO_CHANNEL_SR,
        ]);
    } else {
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    }
    info.set_position(position);
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
    .map_err(|e| SinkError::Config(format!("eq format pod: {e:?}")))
}

impl EqChainHandle {
    /// Build both streams against a live channel sink node.
    pub fn new(
        core: &pw::core::CoreRc,
        sink_name: &str,
        sink_id: u32,
        config: &EqConfig,
    ) -> Result<Self, SinkError> {
        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("eq {stage}: {e}"));
        let params = Arc::new(EqParams::from_config(config));
        let surround = is_spatial_channel(sink_name);
        // Interleaved stereo: 8192 samples = the same ~85 ms of headroom at
        // 48 kHz as the mic's 4096 mono; real added latency is one quantum.
        let ring = Arc::new(Ring::new(16384));
        let input_channels = if surround { SURROUND_CHANNELS } else { 2 };
        let test = Arc::new(ChannelTestBuffer::new(input_channels));
        let spatial =
            if surround {
                Some(SpatialEngine::new(48_000.0).map_err(|e| {
                    SinkError::Config(format!("spatial processor initialization: {e}"))
                })?)
            } else {
                None
            };

        // ---- capture: channel monitor -> EQ -> ring ----
        // Passive like the meters: the channel's own app streams drive the
        // sink; the EQ tap must not keep an idle channel running.
        let capture_name = format!("{EQ_CAPTURE_PREFIX}{sink_name}");
        let capture = pw::stream::StreamRc::new(
            core.clone(),
            &capture_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Capture",
                "node.name" => capture_name.as_str(),
                "stream.capture.sink" => "true",
                "node.passive" => "true",
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("capture stream", e))?;

        let capture_listener = capture
            .add_local_listener_with_user_data(EqCaptureCtx {
                engine: EqEngine::new(48000.0),
                params: params.clone(),
                ring: ring.clone(),
                input: Vec::with_capacity(CAPTURE_CHUNK_SAMPLES),
                scratch: Vec::with_capacity(CAPTURE_CHUNK_SAMPLES),
                spatial,
                test: test.clone(),
            })
            .param_changed(|_, ctx, id, param| {
                // Coefficients are rate-relative: redesign on renegotiation.
                // Filter state resets - same tradeoff the mic chain accepts.
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let Some(param) = param else { return };
                let mut info = spa::param::audio::AudioInfoRaw::new();
                if info.parse(param).is_ok() && info.rate() > 0 {
                    ctx.engine.set_sample_rate(info.rate() as f32);
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

                let input_channels = if ctx.spatial.is_some() {
                    SURROUND_CHANNELS
                } else {
                    2
                };
                let samples = (valid.min(bytes.len()) / 4) / input_channels * input_channels;
                for chunk in bytes[..samples * 4].chunks(CAPTURE_CHUNK_SAMPLES * 4) {
                    decode_f32_chunk(chunk, &mut ctx.input);
                    ctx.test.process_raw(&mut ctx.input);

                    if let Some(spatial) = &mut ctx.spatial {
                        let (enabled, tuning, distance, headphones) = ctx.params.spatial_snapshot();
                        spatial.process(
                            &ctx.input,
                            &mut ctx.scratch,
                            super::spatial::SpatialRenderParams::new(
                                enabled, tuning, distance, headphones,
                            ),
                        );
                    } else {
                        ctx.scratch.clear();
                        ctx.scratch.extend_from_slice(&ctx.input);
                    }
                    ctx.engine
                        .process_interleaved(&mut ctx.scratch, &ctx.params);
                    ctx.ring.push(&ctx.scratch);
                }
            })
            .register()
            .map_err(|e| err("capture listener", e))?;

        let capture_format = f32_format(if surround { SURROUND_CHANNELS } else { 2 })?;
        let mut capture_params = [Pod::from_bytes(&capture_format)
            .ok_or_else(|| SinkError::Config("eq capture format pod invalid".into()))?];
        capture
            .connect(
                spa::utils::Direction::Input,
                Some(sink_id),
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut capture_params,
            )
            .map_err(|e| err("capture connect", e))?;

        // ---- playback: ring -> device/buses ----
        // node.autoconnect=false keeps WirePlumber's hands off this stream
        // (it routes playback streams to the default sink - the link police
        // in thread.rs destroys anything that slips through anyway); the
        // loop links it to the channel's resolved targets itself.
        let playback_name = format!("{EQ_PLAYBACK_PREFIX}{sink_name}");
        let playback = pw::stream::StreamRc::new(
            core.clone(),
            &playback_name,
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
            .add_local_listener_with_user_data(EqPlaybackCtx { ring })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                // Fill only what the graph asked for this cycle (frames);
                // interleaved stereo = 2 samples, 8 bytes per frame.
                let requested = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let max_bytes = data.data().map(|d| d.len()).unwrap_or(0);
                let max_frames = max_bytes / 8;
                let frames = if requested > 0 {
                    requested.min(max_frames)
                } else {
                    max_frames.min(1024)
                };
                if frames == 0 {
                    return;
                }
                if let Some(bytes) = data.data() {
                    let mut chunk_samples = [0.0f32; 1024];
                    let total_samples = frames * 2;
                    let mut written = 0;
                    while written < total_samples {
                        let take = (total_samples - written).min(chunk_samples.len());
                        ctx.ring.pop(&mut chunk_samples[..take]);
                        for (i, s) in chunk_samples[..take].iter().enumerate() {
                            let off = (written + i) * 4;
                            bytes[off..off + 4].copy_from_slice(&s.to_ne_bytes());
                        }
                        written += take;
                    }
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = 8;
                *chunk.size_mut() = (frames * 8) as u32;
            })
            .register()
            .map_err(|e| err("playback listener", e))?;

        let playback_format = f32_format(2)?;
        let mut playback_params = [Pod::from_bytes(&playback_format)
            .ok_or_else(|| SinkError::Config("eq playback format pod invalid".into()))?];
        playback
            .connect(
                spa::utils::Direction::Output,
                None,
                // No AUTOCONNECT: the loop creates the links itself.
                pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                &mut playback_params,
            )
            .map_err(|e| err("playback connect", e))?;

        // ---- test driver: activated only while a saved/bundled sample is
        // playing. It feeds silence into the channel sink, keeping the
        // passive capture callback running; ChannelTestBuffer substitutes
        // the saved raw signal before spatial/EQ processing.
        let driver_name = format!("{TEST_DRIVER_PREFIX}{sink_name}");
        let driver = pw::stream::StreamRc::new(
            core.clone(),
            &driver_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Playback",
                "node.name" => driver_name.as_str(),
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("test driver stream", e))?;
        let driver_listener = driver
            .add_local_listener_with_user_data(TestDriverCtx {
                channels: input_channels,
            })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let requested = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let bytes_per_frame = ctx.channels * 4;
                let max_frames =
                    data.data().map(|bytes| bytes.len()).unwrap_or(0) / bytes_per_frame;
                let frames = if requested > 0 {
                    requested.min(max_frames)
                } else {
                    max_frames.min(1024)
                };
                if let Some(bytes) = data.data() {
                    bytes[..frames * bytes_per_frame].fill(0);
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = bytes_per_frame as i32;
                *chunk.size_mut() = (frames * bytes_per_frame) as u32;
            })
            .register()
            .map_err(|e| err("test driver listener", e))?;
        let driver_format = f32_format(input_channels)?;
        let mut driver_params = [Pod::from_bytes(&driver_format)
            .ok_or_else(|| SinkError::Config("test driver format pod invalid".into()))?];
        driver
            .connect(
                spa::utils::Direction::Output,
                Some(sink_id),
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::INACTIVE
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut driver_params,
            )
            .map_err(|e| err("test driver connect", e))?;

        Ok(Self {
            _capture: capture,
            _capture_listener: capture_listener,
            playback,
            _playback_listener: playback_listener,
            params,
            driver,
            _driver_listener: driver_listener,
            test,
        })
    }
}
