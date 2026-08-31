use tauri::State;

use crate::commands::apps::AppIdentity;

use crate::state::AppState;

pub(crate) const MAX_VOLUME: u8 = 150;

fn validate_requested_group(
    streams: &[crate::audio::types::AppStream],
    submitted_identities: &[AppIdentity],
    requested_indices: &[u32],
    desktop_id: Option<&str>,
) -> Result<(), String> {
    if requested_indices.is_empty() {
        return Err("application group has no requested live streams".into());
    }
    for stream_index in requested_indices {
        let Some(stream) = streams.iter().find(|stream| stream.index == *stream_index) else {
            return Err(format!(
                "application stream {stream_index} is no longer available"
            ));
        };
        if !submitted_identities.iter().any(|identity| {
            identity.match_prop == stream.match_prop && identity.match_value == stream.match_value
        }) {
            return Err(format!(
                "application stream {stream_index} does not belong to the group"
            ));
        }
        if desktop_id.is_some_and(|candidate| stream.desktop_id.as_deref() != Some(candidate)) {
            return Err(format!(
                "application stream {stream_index} does not match the canonical desktop application"
            ));
        }
    }
    Ok(())
}

fn plan_group_routes(
    streams: &[crate::audio::types::AppStream],
    identities: &[AppIdentity],
    desktop_id: Option<&str>,
) -> Vec<(u32, String)> {
    streams
        .iter()
        .filter(|stream| {
            desktop_id.is_some_and(|candidate| stream.desktop_id.as_deref() == Some(candidate))
                || identities.iter().any(|identity| {
                    identity.match_prop == stream.match_prop
                        && identity.match_value == stream.match_value
                })
        })
        .map(|stream| {
            (
                stream.index,
                stream.assigned_sink.clone().unwrap_or_default(),
            )
        })
        .collect()
}

fn authoritative_group_identities(
    seen: &crate::persistence::seen::SeenApps,
    streams: &[crate::audio::types::AppStream],
    desktop_id: &str,
) -> Result<Vec<AppIdentity>, String> {
    let mut identities = Vec::new();
    for entry in &seen.apps {
        if entry.desktop_id.as_deref() == Some(desktop_id) {
            identities.push(AppIdentity {
                match_prop: entry.match_prop.clone(),
                match_value: entry.match_value.clone(),
            });
        }
    }
    for stream in streams {
        if stream.desktop_id.as_deref() == Some(desktop_id) {
            identities.push(AppIdentity {
                match_prop: stream.match_prop.clone(),
                match_value: stream.match_value.clone(),
            });
        }
    }
    crate::commands::apps::checked_identities(identities)
}

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
/// pre-link metadata policy, and re-applied by the stream poll when the app restarts.
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
        crate::commands::apps::persist_assignments(&state, &mixer, &previous, &assignments)?;
    }
    if let Err(error) = state.backend.move_stream_to_sink(stream_index, &sink_name) {
        let mixer = state.lock_mixer()?;
        return Err(crate::commands::apps::app_mutation_failure(
            error,
            &[
                (
                    "restoring the previous assignments and active profile",
                    crate::commands::apps::restore_assignments(&state, &mixer, &previous),
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

/// Route every live stream and persistent raw identity belonging to one
/// canonical desktop application as a single compensated transaction.
#[tauri::command]
pub fn route_app_group_to_channel(
    state: State<'_, AppState>,
    stream_indices: Vec<u32>,
    identities: Vec<AppIdentity>,
    desktop_id: Option<String>,
    sink_name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let mut identities = crate::commands::apps::checked_identities(identities)?;
    let desktop_id = desktop_id
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty());
    if desktop_id.as_ref().is_some_and(|value| value.len() > 256) {
        return Err("invalid canonical desktop application id".into());
    }
    if stream_indices.len() > 128 {
        return Err("application group has too many live streams".into());
    }
    let mut stream_indices = stream_indices;
    stream_indices.sort_unstable();
    stream_indices.dedup();

    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    if !sink_name.is_empty() {
        state.ensure_known_channel(&sink_name)?;
    }
    let mut streams = state
        .backend
        .list_app_streams()
        .map_err(|error| error.to_string())?;
    crate::commands::devices::enrich_app_streams(state.inner(), &mut streams)?;
    validate_requested_group(
        &streams,
        &identities,
        &stream_indices,
        desktop_id.as_deref(),
    )?;
    if let Some(desktop_id) = desktop_id.as_deref() {
        let mixer = state.lock_mixer()?;
        // Once the canonical ID is proven by requested live streams, discard
        // the client identity list and rebuild solely from authoritative
        // persisted/live members. Unrelated extra identities cannot ride on
        // an otherwise valid request.
        identities = authoritative_group_identities(&mixer.seen, &streams, desktop_id)?;
    }
    // Expand from the fresh backend snapshot. This includes matching streams
    // that appeared after the UI built its request, preventing a canonical
    // group from remaining split behind a now-updated persistent assignment.
    let planned = plan_group_routes(&streams, &identities, desktop_id.as_deref());

    let (previous, assignments) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.assignments.clone();
        let mut assignments = previous.clone();
        for identity in &identities {
            if sink_name.is_empty() {
                assignments.remove(&identity.match_prop, &identity.match_value);
            } else {
                assignments.set(&identity.match_prop, &identity.match_value, &sink_name);
            }
        }
        (previous, assignments)
    };
    {
        let mixer = state.lock_mixer()?;
        crate::commands::apps::persist_assignments(&state, &mixer, &previous, &assignments)?;
    }

    let mut moved: Vec<(u32, String)> = Vec::new();
    for (stream_index, previous_sink) in planned {
        if let Err(error) = state.backend.move_stream_to_sink(stream_index, &sink_name) {
            let mut message = error.to_string();
            match state.lock_mixer() {
                Ok(mixer) => {
                    if let Err(rollback) =
                        crate::commands::apps::restore_assignments(&state, &mixer, &previous)
                    {
                        message
                            .push_str(&format!("; restoring assignments also failed: {rollback}"));
                    }
                }
                Err(rollback) => message.push_str(&format!(
                    "; restoring assignments could not acquire application state: {rollback}"
                )),
            }
            for (moved_index, moved_previous_sink) in moved.into_iter().rev() {
                if let Err(rollback) = state
                    .backend
                    .move_stream_to_sink(moved_index, &moved_previous_sink)
                {
                    message.push_str(&format!(
                        "; restoring application stream {moved_index} also failed: {rollback}"
                    ));
                }
            }
            return Err(message);
        }
        moved.push((stream_index, previous_sink));
    }
    let mut mixer = state.lock_mixer()?;
    mixer.assignments = assignments;
    mixer
        .auto_routed
        .extend(moved.iter().map(|(stream_index, _)| *stream_index));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(index: u32, value: &str) -> crate::audio::types::AppStream {
        crate::audio::types::AppStream {
            index,
            app_name: value.to_string(),
            match_prop: "application.name".into(),
            match_value: value.to_string(),
            alias: None,
            icon_name: None,
            icon_path: None,
            desktop_id: Some("game".into()),
            pid: None,
            assigned_sink: None,
            volume_percent: 100,
            muted: false,
            active: true,
        }
    }

    #[test]
    fn group_route_expands_to_matching_streams_that_appeared_after_the_ui_snapshot() {
        let streams = vec![stream(1, "helper-a"), stream(2, "helper-b")];
        let identities = vec![AppIdentity {
            match_prop: "application.name".into(),
            match_value: "helper-a".into(),
        }];
        validate_requested_group(&streams, &identities, &[1], Some("game")).expect("valid group");
        let planned = plan_group_routes(&streams, &identities, Some("game"));
        assert_eq!(
            planned.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn group_route_rejects_a_requested_stream_outside_the_group() {
        let streams = vec![stream(1, "helper-a"), stream(2, "other")];
        let identities = vec![AppIdentity {
            match_prop: "application.name".into(),
            match_value: "helper-a".into(),
        }];
        assert!(validate_requested_group(&streams, &identities, &[2], None).is_err());
    }

    #[test]
    fn group_route_rejects_a_canonical_id_from_another_application() {
        let streams = vec![stream(1, "helper-a"), stream(2, "helper-b")];
        let identities = vec![AppIdentity {
            match_prop: "application.name".into(),
            match_value: "helper-a".into(),
        }];
        assert!(validate_requested_group(&streams, &identities, &[1], Some("other")).is_err());
    }

    #[test]
    fn authoritative_canonical_membership_drops_unrelated_submitted_identities() {
        let mut streams = vec![stream(1, "helper-a"), stream(2, "unrelated")];
        streams[1].desktop_id = Some("other".into());
        let identities = authoritative_group_identities(
            &crate::persistence::seen::SeenApps::default(),
            &streams,
            "game",
        )
        .expect("canonical members");
        assert_eq!(
            identities,
            vec![AppIdentity {
                match_prop: "application.name".into(),
                match_value: "helper-a".into(),
            }]
        );
    }
}
