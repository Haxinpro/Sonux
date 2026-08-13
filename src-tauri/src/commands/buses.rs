use tauri::State;

use crate::audio::backend::AudioBackend;
use crate::commands::routing::MAX_VOLUME;
use crate::persistence::buses::BusDef;
use crate::state::AppState;

pub(crate) fn set_bus_level(
    backend: &dyn AudioBackend,
    def: &BusDef,
) -> Result<(), crate::error::SinkError> {
    // Existing nodes may carry values from another profile. Defaults are
    // values too: omitting 100% or false would leave an old level/mute live.
    backend.set_sink_volume(&def.name, def.volume_percent)?;
    backend.set_sink_mute(&def.name, def.muted)?;
    Ok(())
}

fn configure_existing_bus(
    backend: &dyn AudioBackend,
    def: &BusDef,
    all_channels: &[String],
) -> Result<(), crate::error::SinkError> {
    backend.set_bus_members(&def.name, &def.effective_members(all_channels))?;
    set_bus_level(backend, def)
}

fn restore_bus(
    backend: &dyn AudioBackend,
    def: &BusDef,
    label: &str,
    all_channels: &[String],
) -> Result<(), crate::error::SinkError> {
    backend.set_bus_label(&def.name, label)?;
    configure_existing_bus(backend, def, all_channels)
}

fn rename_failure(
    error: impl std::fmt::Display,
    rollback: Result<(), crate::error::SinkError>,
) -> String {
    match rollback {
        Ok(()) => error.to_string(),
        Err(rollback_error) => {
            format!("{error}; restoring the previous mix also failed: {rollback_error}")
        }
    }
}

fn mutation_failure(
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

fn persist_bus_edit(
    mixer: &crate::mixer::state::MixerState,
    old_defs: &crate::persistence::buses::Buses,
    defs: &crate::persistence::buses::Buses,
    rollback_action: &str,
    rollback_live: impl Fn() -> Result<(), crate::error::SinkError>,
) -> Result<(), String> {
    if let Err(error) = defs.save() {
        return Err(mutation_failure(
            error,
            &[
                ("restoring the previous mix definitions", old_defs.save()),
                (rollback_action, rollback_live()),
            ],
        ));
    }
    if let Err(error) = crate::commands::profiles::save_active_with_buses(mixer, defs) {
        return Err(mutation_failure(
            error,
            &[
                (
                    "restoring the previous active profile",
                    crate::commands::profiles::save_active_with_buses(mixer, old_defs),
                ),
                ("restoring the previous mix definitions", old_defs.save()),
                (rollback_action, rollback_live()),
            ],
        ));
    }
    Ok(())
}

fn restore_removed_bus(
    backend: &dyn AudioBackend,
    def: &BusDef,
    label: &str,
    all_channels: &[String],
) -> Result<(), crate::error::SinkError> {
    // PipeWire removes globals asynchronously. A failed delete transaction
    // can reach rollback while the old global is still disappearing, so use
    // the bounded destroy/recreate path rather than a one-shot create.
    backend.set_bus_label(&def.name, label)?;
    if let Err(error) = configure_existing_bus(backend, def, all_channels) {
        return match backend.destroy_bus(&def.name) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(crate::error::SinkError::Config(format!(
                "{error}; removing the incomplete restored mix also failed: {cleanup_error}"
            ))),
        };
    }
    Ok(())
}

fn validate_member_channels(channels: &[String], all_channels: &[String]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for channel in channels {
        if !all_channels.contains(channel) {
            return Err(format!("unknown channel in mix membership: {channel}"));
        }
        if !seen.insert(channel) {
            return Err(format!("duplicate channel in mix membership: {channel}"));
        }
    }
    Ok(())
}

/// The user's mixes (buses) with their member channels.
#[tauri::command]
pub fn list_buses(state: State<'_, AppState>) -> Result<Vec<BusDef>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.buses.buses.clone())
}

/// Create a new mix. Recorders see it under `label`. New mixes carry
/// every channel (auto-include) until the user unchecks some.
#[tauri::command]
pub fn add_bus(
    state: State<'_, AppState>,
    label: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (def, old_defs, defs, prefs, all) = {
        let mixer = state.lock_mixer()?;
        let old_defs = mixer.buses.clone();
        let mut defs = old_defs.clone();
        let def = defs.add(&label).map_err(|e| e.to_string())?;
        (
            def,
            old_defs,
            defs,
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };
    if let Err(e) = state
        .backend
        .create_bus(&def.name, &prefs.decorate(&def.label))
    {
        return Err(e.to_string());
    }
    if let Err(error) = configure_existing_bus(state.backend.as_ref(), &def, &all) {
        return Err(mutation_failure(
            error,
            &[(
                "removing the incomplete mix",
                state.backend.destroy_bus(&def.name),
            )],
        ));
    }
    if let Err(error) = defs.save() {
        return Err(mutation_failure(
            error,
            &[
                ("restoring the previous mix definitions", old_defs.save()),
                (
                    "removing the unpersisted mix",
                    state.backend.destroy_bus(&def.name),
                ),
            ],
        ));
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_buses(&mixer, &defs) {
            return Err(mutation_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_buses(&mixer, &old_defs),
                    ),
                    ("restoring the previous mix definitions", old_defs.save()),
                    (
                        "removing the unpersisted mix",
                        state.backend.destroy_bus(&def.name),
                    ),
                ],
            ));
        }
    }
    let mut mixer = state.lock_mixer()?;
    mixer.buses = defs;
    Ok(())
}

/// Rename a mix. The node is recreated so recorders immediately see the
/// new name (the node name stays stable, so OBS configs keep working -
/// capture re-attaches automatically).
#[tauri::command]
pub fn rename_bus(
    state: State<'_, AppState>,
    name: String,
    label: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (old_def, def, old_defs, defs, prefs, all) = {
        let mixer = state.lock_mixer()?;
        let old_def = mixer
            .buses
            .get(&name)
            .cloned()
            .ok_or_else(|| "unknown mix".to_string())?;
        let old_defs = mixer.buses.clone();
        let mut defs = old_defs.clone();
        defs.rename(&name, &label).map_err(|e| e.to_string())?;
        let def = defs
            .get(&name)
            .cloned()
            .ok_or_else(|| "unknown mix".to_string())?;
        (
            old_def,
            def,
            old_defs,
            defs,
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };

    let old_label = prefs.decorate(&old_def.label);
    let new_label = prefs.decorate(&def.label);

    let rename_result = state
        .backend
        .set_bus_label(&name, &new_label)
        .and_then(|()| configure_existing_bus(state.backend.as_ref(), &def, &all));
    if let Err(error) = rename_result {
        return Err(rename_failure(
            error,
            restore_bus(state.backend.as_ref(), &old_def, &old_label, &all),
        ));
    }

    {
        let mixer = state.lock_mixer()?;
        persist_bus_edit(
            &mixer,
            &old_defs,
            &defs,
            "restoring the previous live mix",
            || restore_bus(state.backend.as_ref(), &old_def, &old_label, &all),
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.buses = defs;
    Ok(())
}

/// Delete a mix.
#[tauri::command]
pub fn remove_bus(
    state: State<'_, AppState>,
    name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    state.ensure_known_bus(&name)?;
    if crate::persistence::buses::is_master(&name) {
        return Err("the master mix can't be deleted".to_string());
    }
    let (old_def, old_label, old_defs, defs, all) = {
        let mixer = state.lock_mixer()?;
        let old_def = mixer
            .buses
            .get(&name)
            .cloned()
            .ok_or_else(|| "unknown mix".to_string())?;
        let old_label = mixer.prefs.decorate(&old_def.label);
        let old_defs = mixer.buses.clone();
        let mut defs = old_defs.clone();
        defs.remove(&name).map_err(|e| e.to_string())?;
        (old_def, old_label, old_defs, defs, channel_names(&mixer))
    };
    state
        .backend
        .destroy_bus(&name)
        .map_err(|e| e.to_string())?;
    if let Err(error) = defs.save() {
        return Err(mutation_failure(
            error,
            &[
                ("restoring the previous mix definitions", old_defs.save()),
                (
                    "restoring the deleted mix",
                    restore_removed_bus(state.backend.as_ref(), &old_def, &old_label, &all),
                ),
            ],
        ));
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_buses(&mixer, &defs) {
            return Err(mutation_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_buses(&mixer, &old_defs),
                    ),
                    ("restoring the previous mix definitions", old_defs.save()),
                    (
                        "restoring the deleted mix",
                        restore_removed_bus(state.backend.as_ref(), &old_def, &old_label, &all),
                    ),
                ],
            ));
        }
    }
    let mut mixer = state.lock_mixer()?;
    mixer.buses = defs;
    Ok(())
}

/// Replace the channel set a mix carries. `channels` is what the user
/// sees checked; for auto-include mixes the complement (the unchecked
/// set) is what gets stored, so future channels keep flowing in.
#[tauri::command]
pub fn set_bus_members(
    state: State<'_, AppState>,
    name: String,
    channels: Vec<String>,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    // Validate against the definition set first, so a rejected request
    // (master mix, unknown name) never reaches the backend - otherwise
    // backend membership and the persisted definition could diverge.
    let (old_defs, defs, old_members) = {
        let mixer = state.lock_mixer()?;
        if crate::persistence::buses::is_master(&name) {
            return Err("the master mix always carries every channel".to_string());
        }
        let Some(def) = mixer.buses.get(&name) else {
            return Err("unknown mix".to_string());
        };
        let all = channel_names(&mixer);
        validate_member_channels(&channels, &all)?;
        let old_members = def.effective_members(&all);
        let stored = if def.exclude {
            all.into_iter().filter(|c| !channels.contains(c)).collect()
        } else {
            channels.clone()
        };
        let old_defs = mixer.buses.clone();
        let mut defs = old_defs.clone();
        defs.set_members(&name, stored).map_err(|e| e.to_string())?;
        (old_defs, defs, old_members)
    };
    state
        .backend
        .set_bus_members(&name, &channels)
        .map_err(|e| e.to_string())?;
    {
        let mixer = state.lock_mixer()?;
        persist_bus_edit(
            &mixer,
            &old_defs,
            &defs,
            "restoring the previous live mix membership",
            || state.backend.set_bus_members(&name, &old_members),
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.buses = defs;
    Ok(())
}

/// Switch a mix between manual selection and auto-include mode. The
/// carried set is preserved; only what happens to future channels changes.
#[tauri::command]
pub fn set_bus_exclude(
    state: State<'_, AppState>,
    name: String,
    exclude: bool,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (old_defs, defs) = {
        let mixer = state.lock_mixer()?;
        let all = channel_names(&mixer);
        let old_defs = mixer.buses.clone();
        let mut defs = old_defs.clone();
        defs.set_exclude(&name, exclude, &all)
            .map_err(|e| e.to_string())?;
        (old_defs, defs)
    };
    {
        let mixer = state.lock_mixer()?;
        persist_bus_edit(
            &mixer,
            &old_defs,
            &defs,
            "restoring the live mix mode",
            || Ok(()),
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.buses = defs;
    Ok(())
}

/// Set a mix's playback level (0-150%) - what recorders hear. Unlike
/// `set_channel_volume`, this accepts mix nodes (including the master mix,
/// whose reserved name `set_channel_volume` rejects) and persists the level.
#[tauri::command]
pub fn set_bus_volume(
    state: State<'_, AppState>,
    name: String,
    volume: u8,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    state.ensure_known_bus(&name)?;
    let volume = volume.min(MAX_VOLUME);
    let (old_defs, defs, old_volume) = {
        let mixer = state.lock_mixer()?;
        let old_volume = mixer
            .buses
            .get(&name)
            .map(|def| def.volume_percent)
            .ok_or_else(|| "unknown mix".to_string())?;
        let old_defs = mixer.buses.clone();
        let mut defs = old_defs.clone();
        defs.set_volume(&name, volume).map_err(|e| e.to_string())?;
        (old_defs, defs, old_volume)
    };
    state
        .backend
        .set_sink_volume(&name, volume)
        .map_err(|e| e.to_string())?;
    {
        let mixer = state.lock_mixer()?;
        persist_bus_edit(
            &mixer,
            &old_defs,
            &defs,
            "restoring the previous live mix volume",
            || state.backend.set_sink_volume(&name, old_volume),
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.buses = defs;
    Ok(())
}

/// Mute or unmute a mix for recorders. Persisted, and accepts the master mix.
#[tauri::command]
pub fn set_bus_mute(
    state: State<'_, AppState>,
    name: String,
    muted: bool,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    state.ensure_known_bus(&name)?;
    let (old_defs, defs, old_muted) = {
        let mixer = state.lock_mixer()?;
        let old_muted = mixer
            .buses
            .get(&name)
            .map(|def| def.muted)
            .ok_or_else(|| "unknown mix".to_string())?;
        let old_defs = mixer.buses.clone();
        let mut defs = old_defs.clone();
        defs.set_muted(&name, muted).map_err(|e| e.to_string())?;
        (old_defs, defs, old_muted)
    };
    state
        .backend
        .set_sink_mute(&name, muted)
        .map_err(|e| e.to_string())?;
    {
        let mixer = state.lock_mixer()?;
        persist_bus_edit(
            &mixer,
            &old_defs,
            &defs,
            "restoring the previous live mix mute",
            || state.backend.set_sink_mute(&name, old_muted),
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.buses = defs;
    Ok(())
}

/// The current channel sink names (the "all channels" set for mixes).
pub(crate) fn channel_names(mixer: &crate::mixer::state::MixerState) -> Vec<String> {
    mixer.channels.iter().map(|c| c.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::validate_member_channels;

    #[test]
    fn bus_members_must_be_known_channels() {
        let all = vec!["sink_game".to_string(), "sink_chat".to_string()];
        assert!(validate_member_channels(&["sink_game".to_string()], &all).is_ok());
        assert_eq!(
            validate_member_channels(&["sink_hardware".to_string()], &all),
            Err("unknown channel in mix membership: sink_hardware".to_string())
        );
        assert_eq!(
            validate_member_channels(&["sink_game".to_string(), "sink_game".to_string()], &all),
            Err("duplicate channel in mix membership: sink_game".to_string())
        );
    }
}
