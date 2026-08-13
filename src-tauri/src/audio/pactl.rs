use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use serde::Deserialize;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{
    is_spatial_channel, is_virtual_sink, AppStream, OutputDevice, SinkControlState,
};
use crate::error::SinkError;

/// `owner_module` value pactl uses when a sink has no owning module.
const PA_INVALID_INDEX: u32 = u32::MAX;
/// Marker placed in every fallback module Sonux creates.  A matching module
/// type and sink name are not an ownership proof: another client can choose
/// the same name.  Reconciliation and teardown require this marker too.
const SONUX_MODULE_MARKER: &str = "sonux.owner=sonux";

/// Phase 1 backend: drives the audio system through the `pactl` CLI, which
/// works against both PulseAudio and PipeWire (via pipewire-pulse).
///
/// Uses `pactl --format=json` (available since PulseAudio 16) so parsing is
/// structural rather than scraping human-oriented text.
pub struct PactlBackend {
    /// sink name -> index of the `module-null-sink` module that owns it.
    /// `create_virtual_sink` returns `()` per the trait, so module indices
    /// are tracked here instead of in `MixerState`.
    modules: Mutex<HashMap<String, u32>>,
    /// channel sink name -> index of its `module-loopback` (Phase 4 output
    /// routing fallback; the native backend uses passive links instead).
    loopbacks: Mutex<HashMap<String, u32>>,
}

// ---- JSON shapes for `pactl --format=json` output ----

#[derive(Deserialize)]
struct PactlVolume {
    value_percent: String,
}

#[derive(Deserialize)]
struct PactlSink {
    index: u32,
    name: String,
    description: String,
    #[serde(default)]
    owner_module: Option<u32>,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    volume: HashMap<String, PactlVolume>,
}

#[derive(Deserialize)]
struct PactlSinkInput {
    index: u32,
    /// Index of the sink this stream is currently connected to.
    sink: u32,
    mute: bool,
    #[serde(default)]
    corked: bool,
    volume: HashMap<String, PactlVolume>,
    #[serde(default)]
    properties: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct PactlModule {
    index: u32,
    name: String,
    #[serde(default)]
    argument: Option<String>,
}

impl PactlBackend {
    pub fn new() -> Self {
        Self {
            modules: Mutex::new(HashMap::new()),
            loopbacks: Mutex::new(HashMap::new()),
        }
    }

    /// Run pactl with the given args, returning stdout on success.
    fn run(args: &[&str]) -> Result<String, SinkError> {
        let output = Command::new("pactl").args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SinkError::PactlNotFound
            } else {
                SinkError::Io(e)
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            if stderr.contains("Connection refused") || stderr.contains("Connection failure") {
                return Err(SinkError::ServerUnreachable);
            }
            return Err(SinkError::CommandFailed(format!(
                "pactl {}: {}",
                args.join(" "),
                stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Run a `pactl --format=json list <kind>` query and deserialize it.
    fn query<T: serde::de::DeserializeOwned>(kind: &str) -> Result<T, SinkError> {
        let stdout = Self::run(&["--format=json", "list", kind])?;
        serde_json::from_str(&stdout)
            .map_err(|e| SinkError::Parse(format!("`pactl list {kind}`: {e}")))
    }

    fn list_sinks() -> Result<Vec<PactlSink>, SinkError> {
        Self::query("sinks")
    }

    fn lock_modules(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, u32>>, SinkError> {
        self.modules
            .lock()
            .map_err(|_| SinkError::Parse("module index table lock poisoned".into()))
    }

    fn ensure_visible_app_stream(stream_index: u32) -> Result<(), SinkError> {
        let inputs: Vec<PactlSinkInput> = Self::query("sink-inputs")?;
        inputs
            .iter()
            .any(|input| input.index == stream_index && is_visible_app_input(input))
            .then_some(())
            .ok_or_else(|| SinkError::UnknownSink(format!("application stream {stream_index}")))
    }

    /// Find the module index of a `module-null-sink` owning `sink_name` by
    /// scanning the live module list. Fallback for when the in-memory table
    /// has no entry (e.g. sink left over from a previous crashed run).
    fn find_null_sink_module(sink_name: &str) -> Result<Option<u32>, SinkError> {
        let modules: Vec<PactlModule> = Self::query("modules")?;
        Ok(null_sink_module(&modules, sink_name))
    }

    fn remove_orphaned_loopbacks(sink_name: &str) -> Result<(), SinkError> {
        let modules: Vec<PactlModule> = Self::query("modules")?;
        for index in owned_loopback_modules(&modules, sink_name) {
            Self::run(&["unload-module", &index.to_string()])?;
        }
        Ok(())
    }
}

fn null_sink_module(modules: &[PactlModule], sink_name: &str) -> Option<u32> {
    let needle = format!("sink_name={sink_name}");
    modules
        .iter()
        .find(|module| {
            module.name == "module-null-sink"
                && module.argument.as_deref().is_some_and(|args| {
                    let args: Vec<_> = args.split_whitespace().collect();
                    args.iter().any(|arg| *arg == needle) && has_ownership_marker(&args)
                })
        })
        .map(|module| module.index)
}

fn has_ownership_marker(args: &[&str]) -> bool {
    let sink_marker = format!("sink_properties={SONUX_MODULE_MARKER}");
    let loopback_marker = format!("sink_input_properties={SONUX_MODULE_MARKER}");
    args.iter().any(|argument| {
        let argument = argument.trim_matches('"');
        argument == SONUX_MODULE_MARKER || argument == sink_marker || argument == loopback_marker
    })
}

fn owned_loopback_modules(modules: &[PactlModule], sink_name: &str) -> Vec<u32> {
    let source = format!("source={sink_name}.monitor");
    modules
        .iter()
        .filter(|module| {
            module.name == "module-loopback"
                && module.argument.as_deref().is_some_and(|args| {
                    let args: Vec<_> = args.split_whitespace().collect();
                    args.iter().any(|arg| *arg == source) && has_ownership_marker(&args)
                })
        })
        .map(|module| module.index)
        .collect()
}

/// Parse a pactl `value_percent` string like "87%" into a percentage.
/// Multi-channel volumes are collapsed to the loudest channel.
fn volume_percent(volume: &HashMap<String, PactlVolume>) -> u8 {
    volume
        .values()
        .filter_map(|v| v.value_percent.trim_end_matches('%').parse::<u32>().ok())
        .max()
        .unwrap_or(100)
        .min(u8::MAX as u32) as u8
}

/// Extract a string property from a pactl properties map.
fn prop<'a>(props: &'a HashMap<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    props.get(key).and_then(|v| v.as_str())
}

fn is_visible_app_input(input: &PactlSinkInput) -> bool {
    !crate::audio::types::should_hide_app(|key| prop(&input.properties, key).map(str::to_string))
}

impl AudioBackend for PactlBackend {
    fn create_virtual_sink(&self, name: &str, label: &str) -> Result<(), SinkError> {
        // Idempotency: if the sink already exists (e.g. previous run crashed
        // before teardown), adopt its module instead of loading a duplicate.
        if let Some(existing) = Self::list_sinks()?.iter().find(|s| s.name == name) {
            let owned_module = Self::find_null_sink_module(name)?;
            match (existing.owner_module, owned_module) {
                (Some(owner), Some(module)) if owner == module || owner == PA_INVALID_INDEX => {
                    // A crashed process can leave its loopback modules alive.
                    // Remove exactly the marked dependants before normal
                    // output setup creates one replacement.
                    Self::remove_orphaned_loopbacks(name)?;
                    self.lock_modules()?.insert(name.to_string(), module);
                    return Ok(());
                }
                _ => {
                    return Err(SinkError::Config(format!(
                        "audio node name is already owned by another module: {name}"
                    )));
                }
            }
        }

        // Quote the description and escape it so a label with whitespace
        // (or quotes/backslashes) can't split into extra module properties -
        // pactl parses `sink_properties` as a space-delimited proplist, and
        // the value is otherwise attacker-influenced (TD-048). Control chars
        // are dropped so a newline can't start a new property line.
        let desc: String = label
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let sink_name = format!("sink_name={name}");
        // Keep the description and ownership marker inside one quoted
        // `sink_properties` proplist value. Without the outer quotes the
        // server parses `sonux.owner=...` as an unsupported module argument.
        let sink_props =
            format!("sink_properties=\"device.description=\\\"{desc}\\\" {SONUX_MODULE_MARKER}\"");
        let stdout = if is_spatial_channel(name) {
            Self::run(&[
                "load-module",
                "module-null-sink",
                &sink_name,
                "channels=8",
                "channel_map=front-left,front-right,front-center,lfe,rear-left,rear-right,side-left,side-right",
                &sink_props,
            ])?
        } else {
            Self::run(&["load-module", "module-null-sink", &sink_name, &sink_props])?
        };
        let module_index: u32 = stdout
            .trim()
            .parse()
            .map_err(|_| SinkError::Parse(format!("load-module returned {stdout:?}")))?;

        self.lock_modules()?.insert(name.to_string(), module_index);
        Ok(())
    }

    fn destroy_virtual_sink(&self, name: &str) -> Result<(), SinkError> {
        let loopback = self
            .loopbacks
            .lock()
            .map_err(|_| SinkError::Parse("loopback table lock poisoned".into()))?
            .get(name)
            .copied();
        if let Some(index) = loopback {
            Self::run(&["unload-module", &index.to_string()])?;
            self.loopbacks
                .lock()
                .map_err(|_| SinkError::Parse("loopback table lock poisoned".into()))?
                .remove(name);
        }
        let tracked = self.lock_modules()?.remove(name);
        let module_index = match tracked {
            Some(idx) => Some(idx),
            None => Self::find_null_sink_module(name)?,
        };

        match module_index {
            Some(idx) => {
                Self::run(&["unload-module", &idx.to_string()])?;
                Ok(())
            }
            // Sink does not exist - nothing to destroy. Treat as success so
            // teardown is idempotent.
            None => Ok(()),
        }
    }

    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError> {
        // Map sink index -> sink name so we can resolve each stream's
        // current channel assignment.
        let sink_names: HashMap<u32, String> = Self::list_sinks()?
            .into_iter()
            .map(|s| (s.index, s.name))
            .collect();

        let inputs: Vec<PactlSinkInput> = Self::query("sink-inputs")?;
        Ok(inputs
            .into_iter()
            .filter(is_visible_app_input)
            .map(|input| {
                // Shared identity resolution: skips generic/wrapper names
                // (e.g. "WEBRTC VoiceEngine" → the Discord binary). The
                // winning property+value is the stream's persistent identity.
                let (app_name, match_prop, match_value) =
                    crate::audio::types::resolve_identity(|key| {
                        prop(&input.properties, key).map(str::to_string)
                    });
                let icon_name =
                    prop(&input.properties, "application.icon_name").map(str::to_string);
                let assigned_sink = sink_names
                    .get(&input.sink)
                    .filter(|name| is_virtual_sink(name))
                    .cloned();

                AppStream {
                    index: input.index,
                    app_name,
                    match_prop,
                    match_value,
                    // Filled in by the command layer from the saved aliases.
                    alias: None,
                    icon_name,
                    icon_path: None,
                    pid: prop(&input.properties, "application.process.id")
                        .and_then(|v| v.parse().ok()),
                    assigned_sink,
                    volume_percent: volume_percent(&input.volume),
                    muted: input.mute,
                    active: !input.corked,
                }
            })
            .collect())
    }

    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        Ok(Self::list_sinks()?
            .into_iter()
            .filter(|s| !is_virtual_sink(&s.name))
            .map(|s| OutputDevice {
                index: s.index,
                name: s.name,
                description: s.description,
            })
            .collect())
    }

    fn list_sink_control_states(
        &self,
        sink_names: &[String],
    ) -> Result<Vec<SinkControlState>, SinkError> {
        let names: std::collections::HashSet<&str> =
            sink_names.iter().map(String::as_str).collect();
        Ok(Self::list_sinks()?
            .into_iter()
            .filter(|sink| names.contains(sink.name.as_str()))
            .map(|sink| SinkControlState {
                name: sink.name,
                volume_percent: volume_percent(&sink.volume),
                muted: sink.mute,
            })
            .collect())
    }

    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError> {
        Self::run(&["set-sink-volume", sink_name, &format!("{volume_percent}%")])?;
        Ok(())
    }

    fn set_sink_mute(&self, sink_name: &str, muted: bool) -> Result<(), SinkError> {
        Self::run(&["set-sink-mute", sink_name, if muted { "1" } else { "0" }])?;
        Ok(())
    }

    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError> {
        Self::ensure_visible_app_stream(stream_index)?;
        if !sink_name.is_empty() && !self.lock_modules()?.contains_key(sink_name) {
            return Err(SinkError::UnknownSink(sink_name.to_string()));
        }
        // Empty sink name = unassign: hand the stream back to the default sink.
        let target = if sink_name.is_empty() {
            "@DEFAULT_SINK@"
        } else {
            sink_name
        };
        Self::run(&["move-sink-input", &stream_index.to_string(), target])?;
        Ok(())
    }

    fn set_app_volume(&self, stream_index: u32, volume_percent: u8) -> Result<(), SinkError> {
        Self::ensure_visible_app_stream(stream_index)?;
        Self::run(&[
            "set-sink-input-volume",
            &stream_index.to_string(),
            &format!("{volume_percent}%"),
        ])?;
        Ok(())
    }

    fn list_input_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        #[derive(Deserialize)]
        struct PactlSource {
            index: u32,
            name: String,
            description: String,
        }
        let sources: Vec<PactlSource> = Self::query("sources")?;
        Ok(sources
            .into_iter()
            .filter(|s| !s.name.ends_with(".monitor"))
            .map(|s| OutputDevice {
                index: s.index,
                name: s.name,
                description: s.description,
            })
            .collect())
    }

    fn set_mic_config(&self, _config: &crate::audio::types::MicConfig) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "the mic DSP chain requires the native PipeWire backend".into(),
        ))
    }

    fn set_channel_eq(
        &self,
        _sink_name: &str,
        _config: &crate::audio::types::EqConfig,
    ) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "parametric EQ requires the native PipeWire backend".into(),
        ))
    }

    fn create_bus(&self, _name: &str, _label: &str) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "mix buses require the native PipeWire backend".into(),
        ))
    }

    fn destroy_bus(&self, _name: &str) -> Result<(), SinkError> {
        Ok(()) // nothing to destroy in the fallback
    }

    fn set_bus_members(&self, _name: &str, _channels: &[String]) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "mix buses require the native PipeWire backend".into(),
        ))
    }

    fn set_monitor(&self, _name: &str, _enabled: bool) -> Result<(), SinkError> {
        Err(SinkError::Config(
            "monitoring requires the native PipeWire backend".into(),
        ))
    }

    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError> {
        let sink = Self::run(&["get-default-sink"])
            .ok()
            .map(|s| s.trim().to_string());
        let source = Self::run(&["get-default-source"])
            .ok()
            .map(|s| s.trim().to_string());
        Ok((
            sink.filter(|s| !s.is_empty()),
            source.filter(|s| !s.is_empty()),
        ))
    }

    fn set_default_output(&self, name: &str) -> Result<(), SinkError> {
        Self::run(&["set-default-sink", name])?;
        Ok(())
    }

    fn set_default_input(&self, name: &str) -> Result<(), SinkError> {
        Self::run(&["set-default-source", name])?;
        Ok(())
    }

    fn set_channel_output(
        &self,
        sink_name: &str,
        output_name: Option<&str>,
    ) -> Result<(), SinkError> {
        // Only unload a loopback created and tracked by this backend. Module
        // arguments are not an ownership boundary: another client may use
        // the same managed monitor source for its own recording or routing.
        let previous = self
            .loopbacks
            .lock()
            .map_err(|_| SinkError::Parse("loopback table lock poisoned".into()))?
            .remove(sink_name);
        if let Some(index) = previous {
            Self::run(&["unload-module", &index.to_string()])?;
        }

        let target = output_name.unwrap_or("@DEFAULT_SINK@");
        let stdout = Self::run(&[
            "load-module",
            "module-loopback",
            &format!("source={sink_name}.monitor"),
            &format!("sink={target}"),
            "source_dont_move=true",
            &format!("sink_input_properties={SONUX_MODULE_MARKER}"),
        ])?;
        let module_index: u32 = stdout
            .trim()
            .parse()
            .map_err(|_| SinkError::Parse(format!("load-module returned {stdout:?}")))?;
        self.loopbacks
            .lock()
            .map_err(|_| SinkError::Parse("loopback table lock poisoned".into()))?
            .insert(sink_name.to_string(), module_index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_volume_percent() {
        let mut vol = HashMap::new();
        vol.insert(
            "front-left".to_string(),
            PactlVolume {
                value_percent: "87%".to_string(),
            },
        );
        vol.insert(
            "front-right".to_string(),
            PactlVolume {
                value_percent: "92%".to_string(),
            },
        );
        assert_eq!(volume_percent(&vol), 92);
    }

    #[test]
    fn volume_percent_defaults_to_100_on_garbage() {
        let mut vol = HashMap::new();
        vol.insert(
            "mono".to_string(),
            PactlVolume {
                value_percent: "not-a-number".to_string(),
            },
        );
        assert_eq!(volume_percent(&vol), 100);
    }

    #[test]
    fn parses_real_sink_json() {
        let json = r#"[{"index":66,"state":"SUSPENDED","name":"alsa_output.usb-Arctis-00.analog-stereo","description":"Arctis Analog Stereo","mute":false,"owner_module":4294967295,"volume":{"front-left":{"value":57016,"value_percent":"87%","db":"-3.63 dB"}}}]"#;
        let sinks: Vec<PactlSink> = serde_json::from_str(json).expect("sink json should parse");
        assert_eq!(sinks[0].index, 66);
        assert_eq!(sinks[0].name, "alsa_output.usb-Arctis-00.analog-stereo");
        assert_eq!(sinks[0].owner_module, Some(PA_INVALID_INDEX));
    }

    #[test]
    fn parses_sink_input_json() {
        let json = r#"[{"index":12,"sink":66,"mute":false,"volume":{"front-left":{"value":65536,"value_percent":"100%","db":"0.00 dB"}},"properties":{"application.name":"Firefox","application.icon_name":"firefox"}}]"#;
        let inputs: Vec<PactlSinkInput> =
            serde_json::from_str(json).expect("sink-input json should parse");
        assert_eq!(inputs[0].index, 12);
        assert_eq!(
            prop(&inputs[0].properties, "application.name"),
            Some("Firefox")
        );
    }

    #[test]
    fn module_lookup_requires_exact_name_type_and_ownership_marker() {
        let modules = vec![
            PactlModule {
                index: 1,
                name: "module-alsa-card".to_string(),
                argument: Some("sink_name=sink_game".to_string()),
            },
            PactlModule {
                index: 2,
                name: "module-null-sink".to_string(),
                argument: Some("sink_name=sink_game_extra".to_string()),
            },
            PactlModule {
                index: 3,
                name: "module-null-sink".to_string(),
                argument: Some("sink_name=sink_game channels=2".to_string()),
            },
            PactlModule {
                index: 4,
                name: "module-null-sink".to_string(),
                argument: Some(format!(
                    "sink_name=sink_game sink_properties={SONUX_MODULE_MARKER}"
                )),
            },
        ];
        assert_eq!(null_sink_module(&modules, "sink_game"), Some(4));
        assert_eq!(null_sink_module(&modules, "sink_chat"), None);
        let lookalike = [PactlModule {
            index: 5,
            name: "module-null-sink".to_string(),
            argument: Some(format!(
                "sink_name=sink_game sink_properties=x{SONUX_MODULE_MARKER}y"
            )),
        }];
        assert_eq!(null_sink_module(&lookalike, "sink_game"), None);
    }

    #[test]
    fn orphan_loopback_lookup_requires_exact_source_and_marker() {
        let modules = vec![
            PactlModule {
                index: 1,
                name: "module-loopback".to_string(),
                argument: Some(format!(
                    "source=sink_game.monitor sink=@DEFAULT_SINK@ sink_input_properties={SONUX_MODULE_MARKER}"
                )),
            },
            PactlModule {
                index: 2,
                name: "module-loopback".to_string(),
                argument: Some("source=sink_game.monitor sink=x".to_string()),
            },
            PactlModule {
                index: 3,
                name: "module-loopback".to_string(),
                argument: Some(format!(
                    "source=sink_game.monitor.extra sink=x sink_input_properties={SONUX_MODULE_MARKER}"
                )),
            },
            PactlModule {
                index: 4,
                name: "module-loopback".to_string(),
                argument: Some(format!(
                    "source=sink_game.monitor sink=x unrelated=x{SONUX_MODULE_MARKER}y"
                )),
            },
        ];
        assert_eq!(owned_loopback_modules(&modules, "sink_game"), vec![1]);
    }

    #[test]
    fn visible_app_input_filter_rejects_hidden_streams() {
        let visible: PactlSinkInput = serde_json::from_str(
            r#"{"index":1,"sink":2,"mute":false,"volume":{},"properties":{"application.name":"Game"}}"#,
        )
        .unwrap();
        let hidden: PactlSinkInput = serde_json::from_str(
            r#"{"index":2,"sink":2,"mute":false,"volume":{},"properties":{"media.role":"Event"}}"#,
        )
        .unwrap();
        assert!(is_visible_app_input(&visible));
        assert!(!is_visible_app_input(&hidden));
    }
}
