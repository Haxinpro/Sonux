use tauri::State;

use crate::audio::types::{MicClient, MicConfig, MicTestAction, MicTestStatus, OutputDevice};
use crate::persistence::mic;
use crate::persistence::mic_presets::{self, MicPreset};
use crate::state::AppState;

fn mic_edit_failure(
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

#[tauri::command]
pub fn get_mic_config(state: State<'_, AppState>) -> Result<MicConfig, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.mic.clone())
}

#[tauri::command]
pub fn get_mic_configs(state: State<'_, AppState>) -> Result<Vec<MicConfig>, String> {
    let mixer = state.lock_mixer()?;
    let mut configs = vec![mixer.mic.clone()];
    configs.extend(mixer.secondary_mics.clone());
    Ok(configs)
}

/// Apply and persist the mic chain configuration. The published label is
/// decorated per the device-naming preference at the backend boundary;
/// the stored config stays raw.
#[tauri::command]
pub fn set_mic_config(
    state: State<'_, AppState>,
    mut config: MicConfig,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    config.clamp_ranges();
    config.input_device = config.input_device.filter(|name| !name.is_empty());
    let primary = config.node_name == "sink_mic";
    let (previous, old_primary, old_secondary, next_primary, next_secondary, prefs) = {
        let mixer = state.lock_mixer()?;
        let previous = if primary {
            mixer.mic.clone()
        } else {
            if !mixer.prefs.multiple_mics {
                return Err("enable multiple microphone channels in Settings first".into());
            }
            mixer
                .secondary_mics
                .iter()
                .find(|mic| mic.node_name == config.node_name)
                .ok_or_else(|| "unknown microphone channel".to_string())?
                .clone()
        };
        let old_primary = mixer.mic.clone();
        let old_secondary = mixer.secondary_mics.clone();
        let mut next_primary = old_primary.clone();
        let mut next_secondary = old_secondary.clone();
        if primary {
            next_primary = config.clone();
        } else if let Some(target) = next_secondary
            .iter_mut()
            .find(|mic| mic.node_name == config.node_name)
        {
            *target = config.clone();
        }
        (
            previous,
            old_primary,
            old_secondary,
            next_primary,
            next_secondary,
            mixer.prefs.clone(),
        )
    };
    // An already pinned device may currently be unplugged; permit unrelated
    // edits in that state. A newly selected explicit input must be one the
    // backend exposes as routable right now.
    if config.input_device != previous.input_device {
        if let Some(input) = config.input_device.as_deref() {
            state.ensure_input_device(input)?;
        }
    }
    let mut applied = config.clone();
    applied.output_label = prefs.decorate(&config.output_label);
    let mut rollback_applied = previous.clone();
    rollback_applied.output_label = prefs.decorate(&previous.output_label);
    state
        .backend
        .set_mic_config(&applied)
        .map_err(|e| e.to_string())?;
    if primary {
        if let Err(error) = mic::save(&config) {
            return Err(mic_edit_failure(
                error,
                &[
                    (
                        "restoring the previous microphone file",
                        mic::save(&old_primary),
                    ),
                    (
                        "restoring the previous live microphone",
                        state.backend.set_mic_config(&rollback_applied),
                    ),
                ],
            ));
        }
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) =
            crate::commands::profiles::save_active_with_mics(&mixer, &next_primary, &next_secondary)
        {
            let mut rollbacks = vec![
                (
                    "restoring the previous active profile",
                    crate::commands::profiles::save_active_with_mics(
                        &mixer,
                        &old_primary,
                        &old_secondary,
                    ),
                ),
                (
                    "restoring the previous live microphone",
                    state.backend.set_mic_config(&rollback_applied),
                ),
            ];
            if primary {
                rollbacks.insert(
                    1,
                    (
                        "restoring the previous microphone file",
                        mic::save(&old_primary),
                    ),
                );
            }
            return Err(mic_edit_failure(error, &rollbacks));
        }
    }
    let mut mixer = state.lock_mixer()?;
    mixer.mic = next_primary;
    mixer.secondary_mics = next_secondary;
    Ok(())
}

const MAX_MIC_CHANNELS: usize = crate::persistence::profiles::MAX_MIC_CHANNELS;

fn mic_slug(label: &str) -> String {
    let slug = label
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        "mic".into()
    } else {
        slug
    }
}

#[tauri::command]
pub fn add_mic_channel(
    state: State<'_, AppState>,
    label: String,
    input_device: Option<String>,
    copy_from: Option<String>,
    expected_profile: Option<String>,
) -> Result<MicConfig, String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let label = label.trim();
    if label.is_empty() || label.len() > 32 {
        return Err("microphone name must be 1-32 characters".into());
    }
    let (mut config, prefs) = {
        let mixer = state.lock_mixer()?;
        if !mixer.prefs.multiple_mics {
            return Err("enable multiple microphone channels in Settings first".into());
        }
        if mixer.secondary_mics.len() + 1 >= MAX_MIC_CHANNELS {
            return Err(format!(
                "at most {MAX_MIC_CHANNELS} microphone channels are supported"
            ));
        }
        let source = copy_from.as_deref().and_then(|name| {
            std::iter::once(&mixer.mic)
                .chain(&mixer.secondary_mics)
                .find(|mic| mic.node_name == name)
        });
        (source.cloned().unwrap_or_default(), mixer.prefs.clone())
    };
    let base = format!("source_mic_{}", mic_slug(label));
    let mut node_name = base.clone();
    let mut suffix = 2;
    {
        let mixer = state.lock_mixer()?;
        while mixer
            .secondary_mics
            .iter()
            .any(|mic| mic.node_name == node_name)
        {
            node_name = format!("{base}_{suffix}");
            suffix += 1;
        }
    }
    config.node_name = node_name;
    config.output_label = label.to_string();
    config.input_device = input_device.filter(|name| !name.is_empty());
    if let Some(input) = config.input_device.as_deref() {
        state.ensure_input_device(input)?;
    }
    config.enabled = true;
    let mut applied = config.clone();
    applied.output_label = prefs.decorate(&config.output_label);
    let (primary_mic, old_secondary, next_secondary) = {
        let mixer = state.lock_mixer()?;
        let mut next_secondary = mixer.secondary_mics.clone();
        next_secondary.push(config.clone());
        (
            mixer.mic.clone(),
            mixer.secondary_mics.clone(),
            next_secondary,
        )
    };
    state
        .backend
        .set_mic_config(&applied)
        .map_err(|error| error.to_string())?;
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) =
            crate::commands::profiles::save_active_with_mics(&mixer, &primary_mic, &next_secondary)
        {
            let mut disabled = applied.clone();
            disabled.enabled = false;
            return Err(mic_edit_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_mics(
                            &mixer,
                            &primary_mic,
                            &old_secondary,
                        ),
                    ),
                    (
                        "removing the unpersisted microphone",
                        state.backend.set_mic_config(&disabled),
                    ),
                ],
            ));
        }
    }
    state.lock_mixer()?.secondary_mics = next_secondary;
    Ok(config)
}

#[tauri::command]
pub fn remove_mic_channel(
    state: State<'_, AppState>,
    node_name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    if node_name == "sink_mic" {
        return Err("the primary microphone cannot be deleted".into());
    }
    let (previous, primary_mic, old_secondary, next_secondary, prefs) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer
            .secondary_mics
            .iter()
            .find(|mic| mic.node_name == node_name)
            .cloned()
            .ok_or_else(|| "unknown microphone channel".to_string())?;
        let next_secondary = mixer
            .secondary_mics
            .iter()
            .filter(|mic| mic.node_name != node_name)
            .cloned()
            .collect::<Vec<_>>();
        (
            previous,
            mixer.mic.clone(),
            mixer.secondary_mics.clone(),
            next_secondary,
            mixer.prefs.clone(),
        )
    };
    let mut disabled = previous.clone();
    disabled.enabled = false;
    state
        .backend
        .set_mic_config(&disabled)
        .map_err(|error| error.to_string())?;
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) =
            crate::commands::profiles::save_active_with_mics(&mixer, &primary_mic, &next_secondary)
        {
            let mut rollback_applied = previous.clone();
            rollback_applied.output_label = prefs.decorate(&previous.output_label);
            return Err(mic_edit_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_mics(
                            &mixer,
                            &primary_mic,
                            &old_secondary,
                        ),
                    ),
                    (
                        "restoring the previous live microphone",
                        state.backend.set_mic_config(&rollback_applied),
                    ),
                ],
            ));
        }
    }
    state.lock_mixer()?.secondary_mics = next_secondary;
    Ok(())
}

#[tauri::command]
pub fn reorder_mic_channels(
    state: State<'_, AppState>,
    order: Vec<String>,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (primary_mic, old_secondary, next_secondary) = {
        let mixer = state.lock_mixer()?;
        let unique: std::collections::HashSet<&String> = order.iter().collect();
        if order.len() != mixer.secondary_mics.len()
            || unique.len() != order.len()
            || !order.iter().all(|name| {
                mixer
                    .secondary_mics
                    .iter()
                    .any(|mic| &mic.node_name == name)
            })
        {
            return Err(
                "microphone order must list every secondary microphone exactly once".into(),
            );
        }
        let old_secondary = mixer.secondary_mics.clone();
        let mut next_secondary = old_secondary.clone();
        next_secondary.sort_by_key(|mic| {
            order
                .iter()
                .position(|name| name == &mic.node_name)
                .unwrap_or(usize::MAX)
        });
        (mixer.mic.clone(), old_secondary, next_secondary)
    };
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) =
            crate::commands::profiles::save_active_with_mics(&mixer, &primary_mic, &next_secondary)
        {
            return Err(mic_edit_failure(
                error,
                &[(
                    "restoring the previous active profile",
                    crate::commands::profiles::save_active_with_mics(
                        &mixer,
                        &primary_mic,
                        &old_secondary,
                    ),
                )],
            ));
        }
    }
    state.lock_mixer()?.secondary_mics = next_secondary;
    Ok(())
}

/// Hardware microphones available as the chain's input.
#[tauri::command]
pub fn get_input_devices(state: State<'_, AppState>) -> Result<Vec<OutputDevice>, String> {
    state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())
}

/// Applications currently recording from the processed microphone.
#[tauri::command]
pub fn get_mic_clients(state: State<'_, AppState>) -> Result<Vec<MicClient>, String> {
    snapshot_mic_clients(state.inner())
}

pub(crate) fn snapshot_mic_clients(state: &AppState) -> Result<Vec<MicClient>, String> {
    let mut clients = state
        .backend
        .list_mic_clients()
        .map_err(|error| error.to_string())?;
    for client in &mut clients {
        let binary = (client.match_prop == "application.process.binary")
            .then_some(client.match_value.as_str());
        let resolved = crate::audio::icons::resolve(
            &client.app_name,
            binary,
            client.icon_name.as_deref(),
            client.pid,
            None,
        );
        client.icon_path = resolved.icon_path;
        if let Some(name) = resolved.display_name {
            client.app_name = name;
        }
    }
    deduplicate_mic_clients(&mut clients);
    Ok(clients)
}

fn deduplicate_mic_clients(clients: &mut Vec<MicClient>) {
    clients.sort_by(|left, right| {
        (
            &left.mic_node,
            &left.app_name,
            &left.match_prop,
            &left.match_value,
        )
            .cmp(&(
                &right.mic_node,
                &right.app_name,
                &right.match_prop,
                &right.match_value,
            ))
    });
    clients.dedup_by(|left, right| {
        left.mic_node == right.mic_node
            && left.match_prop == right.match_prop
            && left.match_value == right.match_value
    });
}

#[tauri::command]
pub fn get_mic_test_status(
    state: State<'_, AppState>,
    node_name: Option<String>,
) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(
            node_name.as_deref().unwrap_or("sink_mic"),
            MicTestAction::Status,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_mic_test_recording(
    state: State<'_, AppState>,
    node_name: Option<String>,
) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(
            node_name.as_deref().unwrap_or("sink_mic"),
            MicTestAction::StartRecording,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stop_mic_test_recording(
    state: State<'_, AppState>,
    node_name: Option<String>,
) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(
            node_name.as_deref().unwrap_or("sink_mic"),
            MicTestAction::StopRecording,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn play_mic_test_loop(
    state: State<'_, AppState>,
    node_name: Option<String>,
) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(
            node_name.as_deref().unwrap_or("sink_mic"),
            MicTestAction::StartLoop,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stop_mic_test_playback(
    state: State<'_, AppState>,
    node_name: Option<String>,
) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(
            node_name.as_deref().unwrap_or("sink_mic"),
            MicTestAction::StopLoop,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_mic_presets() -> Result<Vec<MicPreset>, String> {
    mic_presets::list().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_mic_preset(name: String, mut config: MicConfig) -> Result<(), String> {
    config.clamp_ranges();
    mic_presets::save(&name, &config).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_mic_preset(name: String) -> Result<(), String> {
    mic_presets::delete(&name).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::deduplicate_mic_clients;
    use crate::audio::types::MicClient;

    fn client(index: u32, mic_node: &str) -> MicClient {
        MicClient {
            index,
            mic_node: mic_node.into(),
            app_name: "Recorder".into(),
            match_prop: "application.name".into(),
            match_value: "recorder".into(),
            icon_name: None,
            icon_path: None,
            pid: None,
            active: true,
        }
    }

    #[test]
    fn microphone_clients_are_deduplicated_per_processed_node() {
        let mut clients = vec![
            client(1, "sink_mic"),
            client(2, "source_mic_chat"),
            client(3, "sink_mic"),
        ];

        deduplicate_mic_clients(&mut clients);

        assert_eq!(clients.len(), 2);
        assert!(clients.iter().any(|client| client.mic_node == "sink_mic"));
        assert!(clients
            .iter()
            .any(|client| client.mic_node == "source_mic_chat"));
    }
}
