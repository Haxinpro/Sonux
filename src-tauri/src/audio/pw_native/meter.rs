//! Per-sink level metering: a small capture stream on each virtual sink's
//! monitor computes per-channel peaks in the process callback and raises
//! them into the shared `LevelStore`.

use std::io::Cursor;
use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Value};

use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::spatial::SURROUND_CHANNELS;
use crate::audio::pw_native::thread::METER_PREFIX;
use crate::audio::types::is_spatial_channel;
use crate::error::SinkError;

pub struct MeterHandle {
    _stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<MeterCtx>,
}

struct MeterCtx {
    slot: usize,
    levels: Arc<LevelStore>,
    channels: usize,
}

impl MeterHandle {
    /// `capture_sink` = true to tap a sink's monitor; false to capture a
    /// source node directly (e.g. the Stream Mix virtual source).
    pub fn new(
        core: &pw::core::CoreRc,
        sink_name: &str,
        sink_id: u32,
        levels: Arc<LevelStore>,
        capture_sink: bool,
    ) -> Result<Self, SinkError> {
        let slot = levels
            .slot_for(sink_name)
            .ok_or_else(|| SinkError::Config(format!("meter budget exhausted for {sink_name}")))?;

        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("meter {stage}: {e}"));
        let channels = if capture_sink && is_spatial_channel(sink_name) {
            SURROUND_CHANNELS
        } else {
            2
        };

        let props = pw::properties::properties! {
            "media.type" => "Audio",
            "media.category" => "Capture",
            "node.name" => format!("{METER_PREFIX}{sink_name}"),
            // For sinks: capture the monitor, don't keep the sink busy.
            "stream.capture.sink" => if capture_sink { "true" } else { "false" },
            "node.passive" => "true",
        };
        let stream = pw::stream::StreamRc::new(core.clone(), "sink-meter", props)
            .map_err(|e| err("stream", e))?;

        let listener = stream
            .add_local_listener_with_user_data(MeterCtx {
                slot,
                levels,
                channels,
            })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let Some(bytes) = super::capture_chunk_bytes(data) else {
                    return;
                };
                let mut peaks = [0.0f32; 2];
                // f32 interleaved. The 7.1 channel groups are folded into
                // the two UI bars; centre and LFE raise both sides.
                for (i, raw) in bytes.chunks_exact(4).enumerate() {
                    let v = f32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]).abs();
                    let ch = i % ctx.channels;
                    if ctx.channels == SURROUND_CHANNELS {
                        if matches!(ch, 0 | 2 | 3 | 4 | 6) {
                            peaks[0] = peaks[0].max(v);
                        }
                        if matches!(ch, 1 | 2 | 3 | 5 | 7) {
                            peaks[1] = peaks[1].max(v);
                        }
                    } else {
                        peaks[ch & 1] = peaks[ch & 1].max(v);
                    }
                }
                ctx.levels.raise(ctx.slot, 0, peaks[0]);
                ctx.levels.raise(ctx.slot, 1, peaks[1]);
            })
            .register()
            .map_err(|e| err("listener", e))?;

        // Negotiate the source's real shape so surround-only content is not
        // invisible to the meter.
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
        let object = Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        };
        let bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
            .map_err(|e| SinkError::Config(format!("meter format pod: {e:?}")))?
            .0
            .into_inner();
        let mut params = [Pod::from_bytes(&bytes)
            .ok_or_else(|| SinkError::Config("meter format pod invalid".into()))?];

        stream
            .connect(
                spa::utils::Direction::Input,
                Some(sink_id),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .map_err(|e| err("connect", e))?;

        Ok(Self {
            _stream: stream,
            _listener: listener,
        })
    }
}
