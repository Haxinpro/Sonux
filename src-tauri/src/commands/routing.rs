use tauri::State;

use crate::state::AppState;

pub(crate) const MAX_VOLUME: u8 = 150;

fn channel_control_failure(
    error: impl std::fmt::Display,
    profile_rollback: Result<(), crate::error::SinkError>,
    live_rollback: Result<(), crate::error::SinkError>,
) -> String {
    let mut message = error.to_string();
    if let Err(rollback) = profile_rollback {
        message.push_str(&format!(
            "; restoring the previous active profile failed: {rollback}"
        ));
    }
    if let Err(rollback) = live_rollback {
        message.push_str(&format!(
            "; restoring the previous live channel failed: {rollback}"
        ));
    }
    message
}

/// Move an app stream onto a channel. An empty `sink_name` unassigns the
/// stream (returns it to the system default sink).
///
/// The choice is also recorded as a persistent assignment (Phase 2): saved
/// to `$XDG_CONFIG_HOME/sonux/assignments.json`, mirrored to a WirePlumber
/// conf fragment, and re-applied by the stream poll when the app restarts.
#[tauri::command]
pub fn route_app_to_channel(
    state: State<'_, AppState>,
    stream_index: u32,
    sink_name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    if !sink_name.is_empty() {
        state.ensure_known_channel(&sink_name)?;
    }
    // Resolve identity and the previous live target before changing either
    // persistence or PipeWire, so every later step has a compensation path.
    let streams = state
        .backend
        .list_app_streams()
        .map_err(|e| e.to_string())?;
    let Some(stream) = streams.iter().find(|s| s.index == stream_index) else {
        return Err(format!(
            "application stream {stream_index} is no longer available"
        ));
    };
    let previous_sink = stream.assigned_sink.clone().unwrap_or_default();

    let (previous, assignments) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.assignments.clone();
        let mut assignments = previous.clone();
        if sink_name.is_empty() {
            assignments.remove(&stream.match_prop, &stream.match_value);
        } else {
            assignments.set(&stream.match_prop, &stream.match_value, &sink_name);
        }
        (previous, assignments)
    };
    {
        let mixer = state.lock_mixer()?;
        crate::commands::apps::persist_assignments(&mixer, &previous, &assignments)?;
    }
    if let Err(error) = state.backend.move_stream_to_sink(stream_index, &sink_name) {
        let mixer = state.lock_mixer()?;
        return Err(crate::commands::apps::app_mutation_failure(
            error,
            &[
                (
                    "restoring the previous assignments and active profile",
                    crate::commands::apps::restore_assignments(&mixer, &previous),
                ),
                (
                    "restoring the previous live application route",
                    state
                        .backend
                        .move_stream_to_sink(stream_index, &previous_sink),
                ),
            ],
        ));
    }
    let mut mixer = state.lock_mixer()?;
    mixer.assignments = assignments;
    // The user explicitly placed this stream; don't auto-route it again.
    mixer.auto_routed.insert(stream_index);
    Ok(())
}

/// Set a channel's volume (0-150%).
#[tauri::command]
pub fn set_channel_volume(
    state: State<'_, AppState>,
    sink_name: String,
    volume: u8,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    // Only our own channels, so a compromised webview can't touch arbitrary
    // session sinks (TD-050).
    state.ensure_known_channel(&sink_name)?;
    let volume = volume.min(MAX_VOLUME);
    let (old_volume, old_channels, next_channels, buses) = {
        let mixer = state.lock_mixer()?;
        let old_volume = mixer
            .channels
            .iter()
            .find(|channel| channel.name == sink_name)
            .map(|channel| channel.volume_percent)
            .ok_or_else(|| format!("unknown channel: {sink_name}"))?;
        let old_channels = mixer.channels.clone();
        let mut next_channels = old_channels.clone();
        if let Some(channel) = next_channels
            .iter_mut()
            .find(|channel| channel.name == sink_name)
        {
            channel.volume_percent = volume;
        }
        (old_volume, old_channels, next_channels, mixer.buses.clone())
    };
    state
        .backend
        .set_sink_volume(&sink_name, volume)
        .map_err(|e| e.to_string())?;
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_channels_and_buses(
            &mixer,
            &next_channels,
            &buses,
        ) {
            return Err(channel_control_failure(
                error,
                crate::commands::profiles::save_active_with_channels_and_buses(
                    &mixer,
                    &old_channels,
                    &buses,
                ),
                state.backend.set_sink_volume(&sink_name, old_volume),
            ));
        }
    }
    state.lock_mixer()?.channels = next_channels;
    Ok(())
}

/// Mute or unmute a channel.
#[tauri::command]
pub fn toggle_channel_mute(
    state: State<'_, AppState>,
    sink_name: String,
    muted: bool,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    state.ensure_known_channel(&sink_name)?;
    let (old_muted, old_channels, next_channels, buses) = {
        let mixer = state.lock_mixer()?;
        let old_muted = mixer
            .channels
            .iter()
            .find(|channel| channel.name == sink_name)
            .map(|channel| channel.muted)
            .ok_or_else(|| format!("unknown channel: {sink_name}"))?;
        let old_channels = mixer.channels.clone();
        let mut next_channels = old_channels.clone();
        if let Some(channel) = next_channels
            .iter_mut()
            .find(|channel| channel.name == sink_name)
        {
            channel.muted = muted;
        }
        (old_muted, old_channels, next_channels, mixer.buses.clone())
    };
    state
        .backend
        .set_sink_mute(&sink_name, muted)
        .map_err(|e| e.to_string())?;
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_channels_and_buses(
            &mixer,
            &next_channels,
            &buses,
        ) {
            return Err(channel_control_failure(
                error,
                crate::commands::profiles::save_active_with_channels_and_buses(
                    &mixer,
                    &old_channels,
                    &buses,
                ),
                state.backend.set_sink_mute(&sink_name, old_muted),
            ));
        }
    }
    state.lock_mixer()?.channels = next_channels;
    Ok(())
}

/// Listen to a channel/mix/mic on the default output (session scoped -
/// not persisted, cleared on restart).
#[tauri::command]
pub fn set_monitor(
    state: State<'_, AppState>,
    sink_name: String,
    enabled: bool,
) -> Result<(), String> {
    // Monitoring is scoped to our own nodes: a channel, a mix bus, or the mic
    // (TD-050) - not any arbitrary session sink.
    {
        let mixer = state.lock_mixer()?;
        let known = sink_name == "sink_mic"
            || mixer
                .channel_defs
                .channels
                .iter()
                .any(|c| c.name == sink_name)
            || mixer.buses.buses.iter().any(|b| b.name == sink_name);
        if !known {
            return Err(format!("unknown monitor target: {sink_name}"));
        }
    }
    state
        .backend
        .set_monitor(&sink_name, enabled)
        .map_err(|e| e.to_string())
}

/// Set or clear a persistent display name for an app, keyed by its stream
/// identity. An empty `alias` reverts to the discovered name.
#[tauri::command]
pub fn rename_app(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    alias: String,
) -> Result<(), String> {
    let aliases = {
        let mut mixer = state.lock_mixer()?;
        mixer.aliases.set(&match_prop, &match_value, &alias);
        mixer.aliases.clone()
    };
    aliases.save().map_err(|e| e.to_string())
}

/// Set the volume of a single app stream (0-150%).
#[tauri::command]
pub fn set_app_volume(
    state: State<'_, AppState>,
    stream_index: u32,
    volume: u8,
) -> Result<(), String> {
    state
        .backend
        .set_app_volume(stream_index, volume.min(MAX_VOLUME))
        .map_err(|e| e.to_string())
}
