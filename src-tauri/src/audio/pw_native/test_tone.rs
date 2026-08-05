//! Persistent eight-channel speaker-test stream used by the spatial map.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;

use crate::audio::pw_native::spatial::SURROUND_CHANNELS;
use crate::error::SinkError;

const TEST_FRAMES: u32 = 28_800; // 600 ms at 48 kHz
const TEST_RATE: f32 = 48_000.0;

struct TestParams {
    channel: AtomicUsize,
    remaining: AtomicU32,
}

struct TestCtx {
    params: Arc<TestParams>,
}

pub struct SpatialTestHandle {
    _stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<TestCtx>,
    params: Arc<TestParams>,
}

impl SpatialTestHandle {
    pub fn trigger(&self, channel: usize) {
        self.params
            .channel
            .store(channel.min(SURROUND_CHANNELS - 1), Ordering::Relaxed);
        self.params.remaining.store(TEST_FRAMES, Ordering::Release);
    }

    pub fn new(core: &pw::core::CoreRc, sink_name: &str, sink_id: u32) -> Result<Self, SinkError> {
        let err =
            |stage: &str, e: pw::Error| SinkError::Config(format!("spatial test {stage}: {e}"));
        let params = Arc::new(TestParams {
            channel: AtomicUsize::new(0),
            remaining: AtomicU32::new(0),
        });
        let stream_name = format!("sink-internal-spatial-test-{sink_name}");
        let stream = pw::stream::StreamRc::new(
            core.clone(),
            &stream_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Playback",
                "node.name" => stream_name.as_str(),
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("stream", e))?;

        let listener = stream
            .add_local_listener_with_user_data(TestCtx {
                params: params.clone(),
            })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let requested = buffer.requested() as usize;
                let Some(data) = buffer.datas_mut().first_mut() else {
                    return;
                };
                let Some(bytes) = data.data() else { return };
                let frame_bytes = SURROUND_CHANNELS * 4;
                let max_frames = bytes.len() / frame_bytes;
                let frames = if requested > 0 {
                    requested.min(max_frames)
                } else {
                    max_frames.min(1024)
                };
                bytes[..frames * frame_bytes].fill(0);

                let channel = ctx.params.channel.load(Ordering::Relaxed);
                let mut remaining = ctx.params.remaining.load(Ordering::Acquire);
                for frame in 0..frames {
                    if remaining == 0 {
                        break;
                    }
                    let elapsed = TEST_FRAMES - remaining;
                    let time = elapsed as f32 / TEST_RATE;
                    let frequency = if channel == 3 { 92.5 } else { 523.25 };
                    let attack = (elapsed as f32 / 720.0).min(1.0);
                    let release = (remaining as f32 / 2_400.0).min(1.0);
                    let decay = (-4.2 * time).exp();
                    let fundamental = (std::f32::consts::TAU * frequency * time).sin();
                    let overtone = (std::f32::consts::TAU * frequency * 1.5 * time).sin();
                    let sample = (fundamental + overtone * 0.22) * 0.13 * attack * release * decay;
                    let offset = (frame * SURROUND_CHANNELS + channel) * 4;
                    bytes[offset..offset + 4].copy_from_slice(&sample.to_ne_bytes());
                    remaining -= 1;
                }
                ctx.params.remaining.store(remaining, Ordering::Release);
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = frame_bytes as i32;
                *chunk.size_mut() = (frames * frame_bytes) as u32;
            })
            .register()
            .map_err(|e| err("listener", e))?;

        let mut info = spa::param::audio::AudioInfoRaw::new();
        info.set_format(spa::param::audio::AudioFormat::F32LE);
        info.set_rate(48_000);
        info.set_channels(SURROUND_CHANNELS as u32);
        let mut position = [0; spa::param::audio::MAX_CHANNELS];
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
        info.set_position(position);
        let format = spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(spa::pod::Object {
                type_: spa::sys::SPA_TYPE_OBJECT_Format,
                id: spa::sys::SPA_PARAM_EnumFormat,
                properties: info.into(),
            }),
        )
        .map(|(c, _)| c.into_inner())
        .map_err(|e| SinkError::Config(format!("spatial test format: {e:?}")))?;
        let mut stream_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("spatial test format pod invalid".into()))?];
        stream
            .connect(
                spa::utils::Direction::Output,
                Some(sink_id),
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut stream_params,
            )
            .map_err(|e| err("connect", e))?;
        // PipeWire/Pulse compatibility can seed an 8-channel stream with
        // only the first stereo pair at unity and all remaining positions at
        // zero. The speaker map needs every discrete channel audible.
        stream
            .set_control(spa::sys::SPA_PROP_channelVolumes, &[1.0; SURROUND_CHANNELS])
            .map_err(|e| err("channel volumes", e))?;

        Ok(Self {
            _stream: stream,
            _listener: listener,
            params,
        })
    }
}
