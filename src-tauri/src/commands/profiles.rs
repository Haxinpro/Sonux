use tauri::State;

use crate::persistence::channels::ChannelDef;
use crate::persistence::profiles::{self, Profile, ProfileInfo};
use crate::persistence::wireplumber;
use crate::state::AppState;

#[derive(Clone)]
struct LiveConfigSnapshot {
    defs: crate::persistence::channels::Channels,
    channels: Vec<crate::audio::types::VirtualSink>,
    assignments: crate::persistence::assignments::Assignments,
    outputs: crate::persistence::outputs::ChannelOutputs,
    eq: crate::persistence::eq::ChannelEq,
    mic: crate::audio::types::MicConfig,
    secondary_mics: Vec<crate::audio::types::MicConfig>,
    buses: crate::persistence::buses::Buses,
    stream_routes: Vec<(u32, String)>,
}

struct RollbackTarget<'a> {
    channels: &'a [crate::audio::types::VirtualSink],
    secondary_mics: &'a [crate::audio::types::MicConfig],
    buses: &'a crate::persistence::buses::Buses,
}

impl LiveConfigSnapshot {
    fn save(&self) -> Result<(), String> {
        self.defs.save().map_err(|error| error.to_string())?;
        self.assignments.save().map_err(|error| error.to_string())?;
        self.outputs.save().map_err(|error| error.to_string())?;
        self.eq.save().map_err(|error| error.to_string())?;
        crate::persistence::mic::save(&self.mic).map_err(|error| error.to_string())?;
        self.buses.save().map_err(|error| error.to_string())?;
        wireplumber::write(&self.assignments).map_err(|error| error.to_string())
    }

    fn restore(&self, active_profile: Option<&str>) -> Vec<String> {
        let mut errors = Vec::new();
        if let Err(error) = self.defs.save() {
            errors.push(error.to_string());
        }
        if let Err(error) = self.assignments.save() {
            errors.push(error.to_string());
        }
        if let Err(error) = self.outputs.save() {
            errors.push(error.to_string());
        }
        if let Err(error) = self.eq.save() {
            errors.push(error.to_string());
        }
        if let Err(error) = crate::persistence::mic::save(&self.mic) {
            errors.push(error.to_string());
        }
        if let Err(error) = self.buses.save() {
            errors.push(error.to_string());
        }
        if let Err(error) = wireplumber::write(&self.assignments) {
            errors.push(error.to_string());
        }
        if let Err(error) = crate::persistence::active::save(active_profile) {
            errors.push(error.to_string());
        }
        errors
    }

    fn restore_backend(
        &self,
        state: &AppState,
        prefs: &crate::persistence::prefs::Prefs,
        attempted_channels: &[crate::audio::types::VirtualSink],
        attempted_secondary_mics: &[crate::audio::types::MicConfig],
        attempted_buses: &crate::persistence::buses::Buses,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        let mut record = |context: String, result: Result<(), crate::error::SinkError>| {
            if let Err(error) = result {
                errors.push(format!("{context}: {error}"));
            }
        };

        // Recreate old channels first so both their controls and evacuated app
        // streams have a valid destination before target-only nodes disappear.
        for channel in &self.channels {
            record(
                format!("recreate channel {}", channel.name),
                state
                    .backend
                    .create_virtual_sink(&channel.name, &prefs.decorate(&channel.label)),
            );
            if attempted_channels
                .iter()
                .any(|attempted| attempted.name == channel.name && attempted.label != channel.label)
            {
                record(
                    format!("restore label for {}", channel.name),
                    state
                        .backend
                        .set_virtual_sink_label(&channel.name, &prefs.decorate(&channel.label)),
                );
            }
        }
        for channel in attempted_channels {
            if !self.channels.iter().any(|old| old.name == channel.name) {
                record(
                    format!("remove target channel {}", channel.name),
                    state.backend.destroy_virtual_sink(&channel.name),
                );
            }
        }

        if state.backend_native {
            let mut primary = self.mic.clone();
            primary.output_label = prefs.decorate(&primary.output_label);
            record(
                "restore primary microphone".into(),
                state.backend.set_mic_config(&primary),
            );

            for mic in attempted_secondary_mics {
                if !self
                    .secondary_mics
                    .iter()
                    .any(|old| old.node_name == mic.node_name)
                {
                    let mut disabled = mic.clone();
                    disabled.enabled = false;
                    record(
                        format!("remove target microphone {}", mic.node_name),
                        state.backend.set_mic_config(&disabled),
                    );
                }
            }
            for mic in &self.secondary_mics {
                let mut applied = mic.clone();
                applied.enabled &= prefs.multiple_mics;
                applied.output_label = prefs.decorate(&applied.output_label);
                record(
                    format!("restore microphone {}", mic.node_name),
                    state.backend.set_mic_config(&applied),
                );
            }
        }

        for channel in &self.channels {
            record(
                format!("restore volume for {}", channel.name),
                state
                    .backend
                    .set_sink_volume(&channel.name, channel.volume_percent),
            );
            record(
                format!("restore mute for {}", channel.name),
                state.backend.set_sink_mute(&channel.name, channel.muted),
            );
            record(
                format!("restore output for {}", channel.name),
                state
                    .backend
                    .set_channel_output(&channel.name, self.outputs.get(&channel.name)),
            );
            record(
                format!("restore failover for {}", channel.name),
                state
                    .backend
                    .set_channel_failover(&channel.name, self.outputs.failover(&channel.name)),
            );
            if state.backend_native {
                record(
                    format!("restore processor for {}", channel.name),
                    state
                        .backend
                        .set_channel_eq(&channel.name, &self.eq.get(&channel.name)),
                );
            }
        }

        if state.backend_native {
            for bus in &attempted_buses.buses {
                if self.buses.get(&bus.name).is_none() {
                    record(
                        format!("remove target mix {}", bus.name),
                        state.backend.destroy_bus(&bus.name),
                    );
                }
            }
            let channel_names = self
                .channels
                .iter()
                .map(|channel| channel.name.clone())
                .collect::<Vec<_>>();
            for bus in &self.buses.buses {
                record(
                    format!("recreate mix {}", bus.name),
                    state
                        .backend
                        .create_bus(&bus.name, &prefs.decorate(&bus.label)),
                );
                if attempted_buses
                    .get(&bus.name)
                    .is_some_and(|attempted| attempted.label != bus.label)
                {
                    record(
                        format!("restore label for mix {}", bus.name),
                        state
                            .backend
                            .set_bus_label(&bus.name, &prefs.decorate(&bus.label)),
                    );
                }
                record(
                    format!("restore members for mix {}", bus.name),
                    state
                        .backend
                        .set_bus_members(&bus.name, &bus.effective_members(&channel_names)),
                );
                record(
                    format!("restore level for mix {}", bus.name),
                    crate::commands::buses::set_bus_level(state.backend.as_ref(), bus),
                );
            }
        }

        for (index, sink_name) in &self.stream_routes {
            record(
                format!("restore application stream {index}"),
                state.backend.move_stream_to_sink(*index, sink_name),
            );
        }
        errors
    }
}

fn persistence_failure(
    error: String,
    previous: &LiveConfigSnapshot,
    previous_active: Option<&str>,
    state: &AppState,
    prefs: &crate::persistence::prefs::Prefs,
    target: &RollbackTarget<'_>,
) -> String {
    let mut rollback_errors = previous.restore_backend(
        state,
        prefs,
        target.channels,
        target.secondary_mics,
        target.buses,
    );
    rollback_errors.extend(previous.restore(previous_active));
    if rollback_errors.is_empty() {
        error
    } else {
        format!(
            "{error}; restoring the previous live configuration also failed: {}",
            rollback_errors.join("; ")
        )
    }
}

fn profile_lifecycle_failure(
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

/// Persist the current mixer state into the active profile, if any.
/// Profiles are live-bound: every profile-relevant mutation calls this so
/// switching away and back never loses changes.
pub fn autosave_active(mixer: &crate::mixer::state::MixerState) {
    if let Err(e) = save_active_with_buses(mixer, &mixer.buses) {
        eprintln!("sonux: autosave of active profile failed: {e}");
    }
}

/// Persist a staged bus definition into the active profile without first
/// mutating the live mixer. Bus transactions use this so a profile write can
/// participate in their rollback instead of being a best-effort side effect.
pub(crate) fn save_active_with_buses(
    mixer: &crate::mixer::state::MixerState,
    buses: &crate::persistence::buses::Buses,
) -> Result<(), crate::error::SinkError> {
    let Some(mut profile) = profile_for_autosave(mixer) else {
        return Ok(());
    };
    profile.buses = buses.clone();
    profiles::save(&profile)
}

pub(crate) fn save_active_with_channels_and_buses(
    mixer: &crate::mixer::state::MixerState,
    channels: &[crate::audio::types::VirtualSink],
    buses: &crate::persistence::buses::Buses,
) -> Result<(), crate::error::SinkError> {
    let Some(mut profile) = profile_for_autosave(mixer) else {
        return Ok(());
    };
    profile.channels = channels.to_vec();
    profile.buses = buses.clone();
    profiles::save(&profile)
}

pub(crate) fn save_active_with_channel_state(
    mixer: &crate::mixer::state::MixerState,
    channels: &[crate::audio::types::VirtualSink],
    assignments: &crate::persistence::assignments::Assignments,
    outputs: &crate::persistence::outputs::ChannelOutputs,
    eq: &crate::persistence::eq::ChannelEq,
    buses: &crate::persistence::buses::Buses,
) -> Result<(), crate::error::SinkError> {
    let Some(mut profile) = profile_for_autosave(mixer) else {
        return Ok(());
    };
    profile.channels = channels.to_vec();
    profile.assignments = assignments.clone();
    profile.outputs = outputs.clone();
    profile.eq = eq.clone();
    profile.buses = buses.clone();
    profiles::save(&profile)
}

pub(crate) fn save_active_with_eq(
    mixer: &crate::mixer::state::MixerState,
    eq: &crate::persistence::eq::ChannelEq,
) -> Result<(), crate::error::SinkError> {
    let Some(mut profile) = profile_for_autosave(mixer) else {
        return Ok(());
    };
    profile.eq = eq.clone();
    profiles::save(&profile)
}

pub(crate) fn save_active_with_assignments(
    mixer: &crate::mixer::state::MixerState,
    assignments: &crate::persistence::assignments::Assignments,
) -> Result<(), crate::error::SinkError> {
    let Some(mut profile) = profile_for_autosave(mixer) else {
        return Ok(());
    };
    profile.assignments = assignments.clone();
    profiles::save(&profile)
}

pub(crate) fn save_active_with_outputs(
    mixer: &crate::mixer::state::MixerState,
    outputs: &crate::persistence::outputs::ChannelOutputs,
) -> Result<(), crate::error::SinkError> {
    let Some(mut profile) = profile_for_autosave(mixer) else {
        return Ok(());
    };
    profile.outputs = outputs.clone();
    profiles::save(&profile)
}

pub(crate) fn save_active_with_mics(
    mixer: &crate::mixer::state::MixerState,
    mic: &crate::audio::types::MicConfig,
    secondary_mics: &[crate::audio::types::MicConfig],
) -> Result<(), crate::error::SinkError> {
    let Some(mut profile) = profile_for_autosave(mixer) else {
        return Ok(());
    };
    profile.mic = Some(mic.clone());
    profile.secondary_mics = secondary_mics.to_vec();
    profiles::save(&profile)
}

fn profile_for_autosave(mixer: &crate::mixer::state::MixerState) -> Option<Profile> {
    Some(Profile {
        name: mixer.active_profile.clone()?,
        protected: mixer.active_protected,
        channels: mixer.channels.clone(),
        mic: Some(mixer.mic.clone()),
        secondary_mics: mixer.secondary_mics.clone(),
        assignments: mixer.assignments.clone(),
        outputs: mixer.outputs.clone(),
        eq: mixer.eq.clone(),
        // Preserved from the cache rather than re-read from disk each mutation.
        trigger_device: mixer.active_trigger.clone(),
        buses: mixer.buses.clone(),
    })
}

#[tauri::command]
pub fn list_profiles() -> Result<Vec<ProfileInfo>, String> {
    profiles::list().map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct ProfileContent {
    channels: Vec<crate::audio::types::VirtualSink>,
    mic: crate::audio::types::MicConfig,
    secondary_mics: Vec<crate::audio::types::MicConfig>,
}

#[tauri::command]
pub fn get_profile_content(name: String) -> Result<ProfileContent, String> {
    profiles::load(&name)
        .map(|profile| ProfileContent {
            channels: profile.channels,
            mic: profile.mic.unwrap_or_else(crate::persistence::mic::load),
            secondary_mics: profile.secondary_mics,
        })
        .map_err(|error| error.to_string())
}

/// The profile changes are currently autosaving into (restored at launch).
#[tauri::command]
pub fn get_active_profile(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.active_profile.clone())
}

/// Bind (or clear, with empty string) an output device that auto-loads
/// this profile when it appears.
#[tauri::command]
pub fn set_profile_trigger(
    state: State<'_, AppState>,
    name: String,
    device: String,
) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    let trigger = if device.is_empty() {
        None
    } else {
        Some(device)
    };
    profiles::set_trigger(&name, trigger.clone()).map_err(|e| e.to_string())?;
    // Keep the cache in step so a later autosave doesn't overwrite the trigger
    // we just set on the active profile with a stale value.
    let mut mixer = state.lock_mixer()?;
    if mixer.active_profile.as_deref() == Some(name.as_str()) {
        mixer.active_trigger = trigger;
    }
    Ok(())
}

/// Apply a saved profile: reconcile the channel **layout** (create missing
/// channels, remove extras - streams evacuate to the default first), then
/// apply volumes/mutes/outputs, replace the assignment set, and clear the
/// auto-route ledger so the new routing is enforced within the next poll.
#[tauri::command]
pub fn load_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    apply_profile(&app, &state, name)
}

pub(crate) fn apply_profile(
    app: &tauri::AppHandle,
    state: &AppState,
    name: String,
) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    apply_profile_locked(app, state, name)
}

fn apply_profile_locked(
    app: &tauri::AppHandle,
    state: &AppState,
    name: String,
) -> Result<(), String> {
    let profile = profiles::load(&name).map_err(|e| e.to_string())?;
    if profile.channels.is_empty() {
        return Err(format!("profile {name} has no channels"));
    }

    // ---- layout reconciliation ----
    let stream_routes = state
        .backend
        .list_app_streams()
        .map_err(|error| format!("snapshot application routes before applying profile: {error}"))?
        .into_iter()
        .filter_map(|stream| stream.assigned_sink.map(|sink| (stream.index, sink)))
        .collect();
    let (previous_live, previous_active, prefs) = {
        let mixer = state.lock_mixer()?;
        (
            LiveConfigSnapshot {
                defs: mixer.channel_defs.clone(),
                channels: mixer.channels.clone(),
                assignments: mixer.assignments.clone(),
                outputs: mixer.outputs.clone(),
                eq: mixer.eq.clone(),
                mic: mixer.mic.clone(),
                secondary_mics: mixer.secondary_mics.clone(),
                buses: mixer.buses.clone(),
                stream_routes,
            },
            mixer.active_profile.clone(),
            mixer.prefs.clone(),
        )
    };
    let current: Vec<ChannelDef> = previous_live.defs.channels.clone();
    let target_mic = profile.mic.clone().unwrap_or_else(|| {
        state
            .lock_mixer()
            .map_or_else(|_| Default::default(), |mixer| mixer.mic.clone())
    });
    let target_secondary_mics = profile.secondary_mics.clone();
    let mut target_buses = profile.buses.clone();
    // The master mix always exists and carries the profile's full channel
    // set (this also upgrades old profiles saved before the master model).
    let names: Vec<String> = profile.channels.iter().map(|c| c.name.clone()).collect();
    target_buses.sync_master(&names);
    let current_buses = previous_live.buses.clone();

    let backend_result = (|| -> Result<(), String> {
        for channel in &profile.channels {
            if !current.iter().any(|c| c.name == channel.name) {
                state
                    .backend
                    .create_virtual_sink(&channel.name, &prefs.decorate(&channel.label))
                    .map_err(|e| e.to_string())?;
            } else if current
                .iter()
                .any(|old| old.name == channel.name && old.label != channel.label)
            {
                state
                    .backend
                    .set_virtual_sink_label(&channel.name, &prefs.decorate(&channel.label))
                    .map_err(|error| format!("apply label for {}: {error}", channel.name))?;
            }
        }

        if state.backend_native {
            let mut applied_mic = target_mic.clone();
            applied_mic.output_label = prefs.decorate(&target_mic.output_label);
            state
                .backend
                .set_mic_config(&applied_mic)
                .map_err(|error| error.to_string())?;
            for old in &previous_live.secondary_mics {
                if !target_secondary_mics
                    .iter()
                    .any(|mic| mic.node_name == old.node_name)
                {
                    let mut disabled = old.clone();
                    disabled.enabled = false;
                    state.backend.set_mic_config(&disabled).map_err(|error| {
                        format!("disable microphone {}: {error}", old.node_name)
                    })?;
                }
            }
            for mic in &target_secondary_mics {
                let mut applied = mic.clone();
                applied.enabled &= prefs.multiple_mics;
                applied.output_label = prefs.decorate(&mic.output_label);
                state
                    .backend
                    .set_mic_config(&applied)
                    .map_err(|error| format!("apply microphone {}: {error}", mic.node_name))?;
            }
        }
        for old in &current {
            if !profile.channels.iter().any(|c| c.name == old.name) {
                let streams = state.backend.list_app_streams().map_err(|error| {
                    format!("list streams before removing {}: {error}", old.name)
                })?;
                for stream in streams {
                    if stream.assigned_sink.as_deref() == Some(old.name.as_str()) {
                        state
                            .backend
                            .move_stream_to_sink(stream.index, "")
                            .map_err(|error| {
                                format!(
                                    "evacuate stream {} from {}: {error}",
                                    stream.index, old.name
                                )
                            })?;
                    }
                }
                state
                    .backend
                    .destroy_virtual_sink(&old.name)
                    .map_err(|error| format!("remove channel {}: {error}", old.name))?;
            }
        }

        for channel in &profile.channels {
            state
                .backend
                .set_sink_volume(&channel.name, channel.volume_percent)
                .map_err(|e| e.to_string())?;
            state
                .backend
                .set_sink_mute(&channel.name, channel.muted)
                .map_err(|e| e.to_string())?;
            state
                .backend
                .set_channel_output(&channel.name, profile.outputs.get(&channel.name))
                .map_err(|error| format!("apply output for {}: {error}", channel.name))?;
            state
                .backend
                .set_channel_failover(&channel.name, profile.outputs.failover(&channel.name))
                .map_err(|error| format!("apply failover for {}: {error}", channel.name))?;
            if state.backend_native {
                state
                    .backend
                    .set_channel_eq(&channel.name, &profile.eq.get(&channel.name))
                    .map_err(|error| format!("apply processor for {}: {error}", channel.name))?;
            }
        }

        if state.backend_native {
            for old in &current_buses.buses {
                if target_buses.get(&old.name).is_none() {
                    state
                        .backend
                        .destroy_bus(&old.name)
                        .map_err(|error| format!("remove mix {}: {error}", old.name))?;
                }
            }
            for bus in &target_buses.buses {
                if current_buses.get(&bus.name).is_none() {
                    state
                        .backend
                        .create_bus(&bus.name, &prefs.decorate(&bus.label))
                        .map_err(|error| format!("create mix {}: {error}", bus.name))?;
                } else if current_buses
                    .get(&bus.name)
                    .is_some_and(|old| old.label != bus.label)
                {
                    state
                        .backend
                        .set_bus_label(&bus.name, &prefs.decorate(&bus.label))
                        .map_err(|error| format!("apply label for mix {}: {error}", bus.name))?;
                }
                state
                    .backend
                    .set_bus_members(&bus.name, &bus.effective_members(&names))
                    .map_err(|error| format!("apply members for mix {}: {error}", bus.name))?;
                crate::commands::buses::set_bus_level(state.backend.as_ref(), bus)
                    .map_err(|error| format!("apply level for mix {}: {error}", bus.name))?;
            }
        }
        // Recreating a stable-name channel to update its immutable node
        // description makes session managers evacuate its streams. Put every
        // still-valid route back before committing the profile.
        for (index, sink_name) in &previous_live.stream_routes {
            if profile
                .channels
                .iter()
                .any(|channel| channel.name == *sink_name)
            {
                state
                    .backend
                    .move_stream_to_sink(*index, sink_name)
                    .map_err(|error| format!("restore stream {index}: {error}"))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = backend_result {
        let rollback_target = RollbackTarget {
            channels: &profile.channels,
            secondary_mics: &target_secondary_mics,
            buses: &target_buses,
        };
        return Err(persistence_failure(
            error,
            &previous_live,
            previous_active.as_deref(),
            state,
            &prefs,
            &rollback_target,
        ));
    }

    let target_live = LiveConfigSnapshot {
        defs: crate::persistence::channels::Channels {
            channels: profile
                .channels
                .iter()
                .map(|channel| ChannelDef {
                    name: channel.name.clone(),
                    label: channel.label.clone(),
                    icon: channel.icon.clone(),
                    stream_mix: channel.stream_mix,
                })
                .collect(),
        },
        channels: profile.channels.clone(),
        assignments: profile.assignments.clone(),
        outputs: profile.outputs.clone(),
        eq: profile.eq.clone(),
        mic: target_mic.clone(),
        secondary_mics: target_secondary_mics.clone(),
        buses: target_buses.clone(),
        stream_routes: Vec::new(),
    };

    // Persist every live file before rebinding the in-memory mixer. The active
    // marker is the commit point: until it succeeds, startup still selects the
    // previous profile and a failed switch can never autosave target state into
    // that previous profile.
    let rollback_target = RollbackTarget {
        channels: &profile.channels,
        secondary_mics: &target_secondary_mics,
        buses: &target_buses,
    };
    if let Err(error) = target_live.save() {
        return Err(persistence_failure(
            error,
            &previous_live,
            previous_active.as_deref(),
            state,
            &prefs,
            &rollback_target,
        ));
    }
    if let Err(error) = crate::persistence::active::save(Some(&name)) {
        return Err(persistence_failure(
            error.to_string(),
            &previous_live,
            previous_active.as_deref(),
            state,
            &prefs,
            &rollback_target,
        ));
    }

    {
        let mut mixer = state.lock_mixer()?;
        mixer.buses = target_buses.clone();
        mixer.channel_defs = target_live.defs;
        mixer.channels = profile.channels.clone();
        mixer.assignments = target_live.assignments;
        mixer.outputs = target_live.outputs;
        mixer.eq = target_live.eq;
        mixer.mic = target_live.mic;
        mixer.secondary_mics = target_secondary_mics.clone();
        mixer.active_profile = Some(name);
        mixer.active_trigger = profile.trigger_device;
        mixer.active_protected = profile.protected;
        mixer.auto_routed.clear();
    }
    crate::refresh_tray(app);
    Ok(())
}

/// Create a profile with a clean slate: the classic four channels at
/// 100%/unmuted, no assignments, all outputs following the default. It is
/// saved but not applied - load it to start fresh.
#[tauri::command]
pub fn create_blank_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
    mic_enabled: bool,
) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    let name = profiles::sanitize_name(&name).map_err(|e| e.to_string())?;
    if profiles::load(&name).is_ok() {
        return Err(format!("profile \"{name}\" already exists"));
    }
    let channels = crate::persistence::channels::Channels::default()
        .channels
        .into_iter()
        .map(|def| crate::audio::types::VirtualSink {
            name: def.name,
            label: def.label,
            icon: def.icon,
            volume_percent: 100,
            muted: false,
            stream_mix: def.stream_mix,
        })
        .collect();
    let mic = crate::audio::types::MicConfig {
        enabled: mic_enabled,
        ..Default::default()
    };
    let profile = Profile {
        name,
        protected: false,
        channels,
        mic: Some(mic),
        secondary_mics: Vec::new(),
        assignments: Default::default(),
        outputs: Default::default(),
        eq: Default::default(),
        trigger_device: None,
        buses: Default::default(),
    };
    profiles::save(&profile).map_err(|e| e.to_string())?;
    crate::refresh_tray(&app);
    Ok(())
}

/// Create a profile from another profile's audio setup. Automatic triggers
/// intentionally stay with the source: copying them would make two profiles
/// compete for the same device or application.
#[tauri::command]
pub fn copy_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    source_name: String,
    name: String,
) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    let name = profiles::sanitize_name(&name).map_err(|e| e.to_string())?;
    if profiles::load(&name).is_ok() {
        return Err(format!("profile \"{name}\" already exists"));
    }
    let mut profile = profiles::load(&source_name).map_err(|e| e.to_string())?;
    profile.name = name;
    profile.protected = false;
    profile.trigger_device = None;
    profiles::save(&profile).map_err(|e| e.to_string())?;
    crate::refresh_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn rename_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
    new_name: String,
) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    let name = profiles::sanitize_name(&name).map_err(|e| e.to_string())?;
    let new_name = profiles::sanitize_name(&new_name).map_err(|e| e.to_string())?;
    if name == new_name {
        return Ok(());
    }
    if profiles::load(&new_name).is_ok() {
        return Err(format!("profile \"{new_name}\" already exists"));
    }

    let original_profile = profiles::load(&name).map_err(|e| e.to_string())?;
    let mut profile = original_profile.clone();
    profile.name.clone_from(&new_name);
    profiles::save(&profile).map_err(|e| e.to_string())?;

    let previous_automation = crate::persistence::profile_automation::load();
    let mut automation = previous_automation.clone();
    if automation.return_profile.as_deref() == Some(name.as_str()) {
        automation.return_profile = Some(new_name.clone());
    }
    for rule in &mut automation.rules {
        if rule.profile == name {
            rule.profile.clone_from(&new_name);
        }
    }
    if let Err(error) = crate::persistence::profile_automation::save(&automation) {
        let automation_restore = crate::persistence::profile_automation::save(&previous_automation);
        let restored_references = automation_restore.is_ok();
        let cleanup = if restored_references {
            profiles::delete(&new_name)
        } else {
            Ok(())
        };
        let mut message = profile_lifecycle_failure(
            error,
            &[
                (
                    "restoring the previous automation rules",
                    automation_restore,
                ),
                ("removing the new profile file", cleanup),
            ],
        );
        if !restored_references {
            message.push_str("; retained the new profile because automation may reference it");
        }
        return Err(message);
    }

    let is_active = {
        let mixer = state.lock_mixer()?;
        mixer.active_profile.as_deref() == Some(name.as_str())
    };
    if is_active {
        if let Err(error) = crate::persistence::active::save(Some(&new_name)) {
            let marker_restore = crate::persistence::active::save(Some(&name));
            let automation_restore =
                crate::persistence::profile_automation::save(&previous_automation);
            let restored_references = marker_restore.is_ok() && automation_restore.is_ok();
            let cleanup = if restored_references {
                profiles::delete(&new_name)
            } else {
                Ok(())
            };
            let mut message = profile_lifecycle_failure(
                error,
                &[
                    ("restoring the previous active marker", marker_restore),
                    (
                        "restoring the previous automation rules",
                        automation_restore,
                    ),
                    ("removing the new profile file", cleanup),
                ],
            );
            if !restored_references {
                message.push_str("; retained the new profile because a reference may remain");
            }
            return Err(message);
        }
    }
    if let Err(error) = profiles::delete(&name) {
        let marker_restore = if is_active {
            crate::persistence::active::save(Some(&name))
        } else {
            Ok(())
        };
        let automation_restore = crate::persistence::profile_automation::save(&previous_automation);
        let original_restore = profiles::save(&original_profile);
        let restored_references =
            marker_restore.is_ok() && automation_restore.is_ok() && original_restore.is_ok();
        let cleanup = if restored_references {
            profiles::delete(&new_name)
        } else {
            Ok(())
        };
        let mut message = profile_lifecycle_failure(
            error,
            &[
                ("restoring the previous active marker", marker_restore),
                (
                    "restoring the previous automation rules",
                    automation_restore,
                ),
                ("restoring the original profile file", original_restore),
                ("removing the new profile file", cleanup),
            ],
        );
        if !restored_references {
            message.push_str("; retained the new profile because a reference may remain");
        }
        return Err(message);
    }
    if is_active {
        let mut mixer = state.lock_mixer()?;
        mixer.active_profile = Some(new_name);
    }
    crate::refresh_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    let profile_list = profiles::list().map_err(|e| e.to_string())?;
    if profile_list.len() <= 1 {
        return Err("keep at least one profile".to_string());
    }
    if profiles::load(&name).map_err(|e| e.to_string())?.protected {
        return Err("the fallback profile cannot be deleted".to_string());
    }
    let fallback = profile_list
        .iter()
        .find(|profile| profile.name != name)
        .map(|profile| profile.name.clone())
        .ok_or_else(|| "keep at least one profile".to_string())?;
    let is_active = {
        let mixer = state.lock_mixer()?;
        mixer.active_profile.as_deref() == Some(name.as_str())
    };
    let previous_automation = crate::persistence::profile_automation::load();
    let mut next_automation = previous_automation.clone();
    next_automation.rules.retain(|rule| rule.profile != name);
    if next_automation.return_profile.as_deref() == Some(name.as_str()) {
        next_automation.return_profile = None;
    }
    if is_active {
        apply_profile_locked(&app, &state, fallback)?;
    }
    if let Err(error) = crate::persistence::profile_automation::save(&next_automation) {
        let live_rollback = if is_active {
            apply_profile_locked(&app, &state, name.clone())
                .map_err(crate::error::SinkError::Config)
        } else {
            Ok(())
        };
        return Err(profile_lifecycle_failure(
            error,
            &[
                (
                    "restoring the previous automation rules",
                    crate::persistence::profile_automation::save(&previous_automation),
                ),
                ("restoring the previously active profile", live_rollback),
            ],
        ));
    }
    if let Err(error) = profiles::delete(&name) {
        let live_rollback = if is_active {
            apply_profile_locked(&app, &state, name.clone())
                .map_err(crate::error::SinkError::Config)
        } else {
            Ok(())
        };
        return Err(profile_lifecycle_failure(
            error,
            &[
                (
                    "restoring the previous automation rules",
                    crate::persistence::profile_automation::save(&previous_automation),
                ),
                ("restoring the previously active profile", live_rollback),
            ],
        ));
    }
    crate::refresh_tray(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autosave_snapshot_preserves_profile_metadata() {
        let mixer = crate::mixer::state::MixerState {
            active_profile: Some("Default".into()),
            active_trigger: Some("alsa_output.deck".into()),
            active_protected: true,
            ..Default::default()
        };

        let profile = profile_for_autosave(&mixer).expect("active profile snapshot");
        assert_eq!(profile.name, "Default");
        assert_eq!(profile.trigger_device.as_deref(), Some("alsa_output.deck"));
        assert!(profile.protected);
    }
}
