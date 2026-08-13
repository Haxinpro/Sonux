use std::collections::HashMap;
use std::io::Read;

use serde::Serialize;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::audio::eq_import::parse_autoeq;
use crate::audio::presets::{bundled_presets, EqPreset, PRESET_SCHEMA};
use crate::audio::types::EqConfig;
use crate::persistence::eq_presets;
use crate::state::AppState;

const MAX_EQ_IMPORT_BYTES: u64 = 2 * 1024 * 1024;

fn eq_failure(
    error: impl std::fmt::Display,
    rollbacks: &[(&str, Result<(), crate::error::SinkError>)],
) -> String {
    let mut message = error.to_string();
    for (action, rollback) in rollbacks {
        if let Err(rollback_error) = rollback {
            message.push_str(&format!("; {action} also failed: {rollback_error}"));
        }
    }
    message
}

/// All channels' EQ configs in one round-trip (channels without an entry
/// have never been configured - the frontend treats them as default).
#[tauri::command]
pub fn get_channel_eq_configs(
    state: State<'_, AppState>,
) -> Result<HashMap<String, EqConfig>, String> {
    Ok(state.lock_mixer()?.eq.configs.clone())
}

/// Apply and persist one channel's parametric EQ.
#[tauri::command]
pub fn set_channel_eq(
    state: State<'_, AppState>,
    sink_name: String,
    mut config: EqConfig,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    state.ensure_known_channel(&sink_name)?;
    config.clamp_ranges();
    let (old_config, old_eq, eq) = {
        let mixer = state.lock_mixer()?;
        let old_config = mixer.eq.get(&sink_name);
        let old_eq = mixer.eq.clone();
        let mut eq = old_eq.clone();
        eq.set(&sink_name, config.clone());
        (old_config, old_eq, eq)
    };
    state
        .backend
        .set_channel_eq(&sink_name, &config)
        .map_err(|e| e.to_string())?;
    if let Err(error) = eq.save() {
        return Err(eq_failure(
            error,
            &[
                ("restoring the previous EQ file", old_eq.save()),
                (
                    "restoring the previous live EQ",
                    state.backend.set_channel_eq(&sink_name, &old_config),
                ),
            ],
        ));
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_eq(&mixer, &eq) {
            return Err(eq_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_eq(&mixer, &old_eq),
                    ),
                    ("restoring the previous EQ file", old_eq.save()),
                    (
                        "restoring the previous live EQ",
                        state.backend.set_channel_eq(&sink_name, &old_config),
                    ),
                ],
            ));
        }
    }
    state.lock_mixer()?.eq = eq;
    Ok(())
}

#[tauri::command]
pub fn test_spatial_channel(
    state: State<'_, AppState>,
    sink_name: String,
    channel: String,
) -> Result<(), String> {
    state.ensure_known_channel(&sink_name)?;
    state
        .backend
        .test_spatial_channel(&sink_name, &channel)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct EqPresetEntry {
    /// "bundled" (ships in the binary) or "user" (local library).
    pub source: String,
    pub preset: EqPreset,
}

/// Bundled EQ starting points first, then this channel's user presets.
#[tauri::command]
pub fn list_eq_presets(sink_name: String) -> Result<Vec<EqPresetEntry>, String> {
    let mut entries: Vec<EqPresetEntry> = bundled_presets()
        .into_iter()
        .map(|preset| EqPresetEntry {
            source: "bundled".into(),
            preset,
        })
        .collect();
    entries.extend(
        eq_presets::list(&sink_name)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|preset| EqPresetEntry {
                source: "user".into(),
                preset,
            }),
    );
    Ok(entries)
}

/// Save the given config into the user's local preset library.
#[tauri::command]
pub fn save_user_eq_preset(
    sink_name: String,
    name: String,
    mut config: EqConfig,
) -> Result<(), String> {
    config.clamp_ranges();
    let preset = EqPreset::from_config(name, config);
    eq_presets::save(&sink_name, &preset).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_user_eq_preset(sink_name: String, name: String) -> Result<(), String> {
    eq_presets::delete(&sink_name, &name).map_err(|e| e.to_string())
}

/// A channel's EQ as shareable preset JSON (pretty-printed, schema 1).
#[tauri::command]
pub fn export_channel_eq(state: State<'_, AppState>, sink_name: String) -> Result<String, String> {
    state.ensure_known_channel(&sink_name)?;
    let (config, label) = {
        let mixer = state.lock_mixer()?;
        let label = mixer
            .channels
            .iter()
            .find(|c| c.name == sink_name)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| sink_name.clone());
        (mixer.eq.get(&sink_name), label)
    };
    let preset = EqPreset::from_config(label, config);
    serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())
}

/// Ask for a destination and write the channel EQ there. The path never
/// crosses IPC, so an untrusted webview caller cannot nominate an arbitrary
/// file for truncation.
#[tauri::command]
pub async fn export_channel_eq_to_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    sink_name: String,
) -> Result<bool, String> {
    let default_name = format!("{}-channel.json", sink_name.trim_start_matches("sink_"));
    let Some(selected) = app
        .dialog()
        .file()
        .add_filter("Channel preset", &["json"])
        .set_file_name(default_name)
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let mut path = selected.into_path().map_err(|error| error.to_string())?;
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        path.set_extension("json");
    }
    let json = export_channel_eq(state, sink_name)?;
    crate::persistence::write_atomic(&path, json)
        .map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(true)
}

/// Parse pasted preset text: our JSON schema or an AutoEq result block.
/// Returns a disabled config for preview-then-apply in the modal.
#[tauri::command]
pub fn import_eq_config(text: String) -> Result<EqConfig, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        let preset: EqPreset = serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
        if preset.schema != PRESET_SCHEMA {
            return Err(format!("unsupported preset schema {}", preset.schema));
        }
        if preset.bands.is_empty() {
            return Err("preset has no bands".into());
        }
        let mut config = preset.to_config();
        config.enabled = false;
        Ok(config)
    } else {
        parse_autoeq(trimmed).map_err(|e| e.to_string())
    }
}

/// Ask for a preset file, then parse it. As with export, no caller-controlled
/// path is accepted over IPC. A size ceiling avoids reading a huge selected
/// file into memory before discovering that it is not an EQ preset.
#[tauri::command]
pub async fn import_eq_file(app: tauri::AppHandle) -> Result<Option<EqConfig>, String> {
    let Some(selected) = app
        .dialog()
        .file()
        .add_filter("EQ preset", &["json", "txt"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    let file =
        std::fs::File::open(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut text = String::new();
    file.take(MAX_EQ_IMPORT_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    if text.len() as u64 > MAX_EQ_IMPORT_BYTES {
        return Err("EQ preset file is larger than 2 MiB".into());
    }
    import_eq_config(text).map(Some)
}
