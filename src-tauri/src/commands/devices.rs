use tauri::State;

use crate::audio::types::{AppStream, OutputDevice, VirtualSink};
use crate::state::AppState;

/// How often the poll force-saves app history to refresh `last_seen` on disk.
const SEEN_FLUSH_SECS: u64 = 15 * 60;

/// Collapse the live snapshot to externally selected managed routes that are
/// unambiguous for an exact PipeWire identity. Two simultaneous streams can
/// share an identity; if they disagree, hash-map iteration order must not
/// decide which route Sonux persists.
fn unambiguous_managed_targets(
    streams: &[AppStream],
) -> std::collections::HashMap<(String, String), String> {
    let mut observed: std::collections::HashMap<(String, String), Option<String>> =
        std::collections::HashMap::new();
    for stream in streams {
        let Some(target) = stream.assigned_sink.as_ref() else {
            continue;
        };
        let key = (stream.match_prop.clone(), stream.match_value.clone());
        observed
            .entry(key)
            .and_modify(|current| {
                if current.as_deref() != Some(target.as_str()) {
                    *current = None;
                }
            })
            .or_insert_with(|| Some(target.clone()));
    }
    observed
        .into_iter()
        .filter_map(|(identity, target)| target.map(|target| (identity, target)))
        .collect()
}

fn single_canonical_assignment(
    seen: &crate::persistence::seen::SeenApps,
    assignments: &crate::persistence::assignments::Assignments,
    desktop_id: &str,
) -> Option<String> {
    let mut target: Option<&str> = None;
    for entry in &seen.apps {
        if entry.desktop_id.as_deref() != Some(desktop_id) {
            continue;
        }
        let Some(candidate) = assignments.sink_for(&entry.match_prop, &entry.match_value) else {
            continue;
        };
        match target {
            None => target = Some(candidate),
            Some(current) if current == candidate => {}
            Some(_) => return None,
        }
    }
    target.map(str::to_string)
}

fn canonical_group_is_ignored(seen: &crate::persistence::seen::SeenApps, desktop_id: &str) -> bool {
    let mut members = seen
        .apps
        .iter()
        .filter(|entry| entry.desktop_id.as_deref() == Some(desktop_id));
    members.next().is_some_and(|first| first.ignored) && members.all(|entry| entry.ignored)
}

/// Current channel state (volume/mute as tracked by MixerState).
#[tauri::command]
pub fn get_virtual_devices(state: State<'_, AppState>) -> Result<Vec<VirtualSink>, String> {
    let _profile_operation = state.lock_profile_operation()?;
    let names = {
        let mixer = state.lock_mixer()?;
        mixer
            .channels
            .iter()
            .map(|channel| channel.name.clone())
            .collect::<Vec<_>>()
    };
    let controls = state
        .backend
        .list_sink_control_states(&names)
        .map_err(|error| error.to_string())?;
    let mut mixer = state.lock_mixer()?;
    if mixer.sync_controls(&controls) {
        crate::commands::profiles::autosave_active(&mixer);
    }
    Ok(mixer.channels.clone())
}

/// All running app audio streams.
///
/// Doubles as the auto-routing enforcement point (Phase 2): the visible UI and
/// the native tray-state worker poll this twice per second, and any stream seen
/// for the first time whose app has a saved assignment is moved onto its
/// channel. Each stream is enforced once, so manual re-routing (here or in
/// pavucontrol) isn't fought.
#[tauri::command]
pub fn get_app_streams(state: State<'_, AppState>) -> Result<Vec<AppStream>, String> {
    poll_app_streams(state.inner())
}

/// Take one application-stream snapshot and apply saved routing decisions.
///
/// Kept separate from the Tauri command wrapper so the native tray-state
/// worker can preserve routing while the webview is hidden.
pub fn poll_app_streams(state: &AppState) -> Result<Vec<AppStream>, String> {
    // The backend snapshot and its adoption into the active profile are one
    // transaction. Taking this after the snapshot lets a completed profile
    // switch turn old routes into assignments in the new profile.
    let _profile_operation = state.lock_profile_operation()?;
    poll_app_streams_locked(state)
}

/// Add canonical desktop identity, display name and icon data to a raw
/// backend snapshot. Shared by polling and group routing so commands resolve
/// late-arriving helpers with the same rules as the UI snapshot.
pub(crate) fn enrich_app_streams(
    state: &AppState,
    streams: &mut [AppStream],
) -> Result<(), String> {
    let desktop_hints = {
        let mixer = state.lock_mixer()?;
        mixer
            .seen
            .apps
            .iter()
            .filter_map(|entry| {
                entry.desktop_id.as_ref().map(|desktop_id| {
                    (
                        (entry.match_prop.clone(), entry.match_value.clone()),
                        desktop_id.clone(),
                    )
                })
            })
            .collect::<std::collections::HashMap<_, _>>()
    };

    for stream in streams {
        let binary = (stream.match_prop == "application.process.binary")
            .then_some(stream.match_value.as_str());
        let stored_desktop_id = desktop_hints
            .get(&(stream.match_prop.clone(), stream.match_value.clone()))
            .map(String::as_str);
        let desktop_id_hint = stream.desktop_id.as_deref().or(stored_desktop_id);
        let resolved = crate::audio::icons::resolve(
            &stream.app_name,
            binary,
            stream.icon_name.as_deref(),
            stream.pid,
            desktop_id_hint,
        );
        stream.icon_path = resolved.icon_path;
        stream.desktop_id = resolved
            .desktop_id
            .or_else(|| desktop_id_hint.map(str::to_string));
        if let Some(name) = resolved.display_name {
            stream.app_name = name;
        }
    }
    Ok(())
}

/// The application-stream polling transaction, for callers that already hold
/// `profile_operations`. Keeping lock acquisition outside this helper lets the
/// coherent profile snapshot reuse the exact routing/history transaction
/// without recursively locking the non-reentrant profile mutex.
pub(crate) fn poll_app_streams_locked(state: &AppState) -> Result<Vec<AppStream>, String> {
    // Keep the session-manager policy synchronized even after profile/channel
    // transactions that replace the complete assignment set. The native
    // backend suppresses identical metadata writes.
    let routes = {
        let mixer = state.lock_mixer()?;
        mixer
            .initialized
            .then(|| (mixer.assignments.clone(), mixer.seen.clone()))
    };
    if let Some((assignments, seen)) = routes.as_ref() {
        state.publish_app_routes(Some(assignments), Some(seen))?;
    }
    let mut streams = state
        .backend
        .list_app_streams()
        .map_err(|e| e.to_string())?;

    // Desktop-entry resolution: real icon files and polished display names
    // ("spotify" binary → Spotify with its actual icon). Cached per identity.
    enrich_app_streams(state, &mut streams)?;
    let observed_targets = unambiguous_managed_targets(&streams);

    let now = crate::persistence::unix_now();
    // Phase 1: under the lock, update history and *plan* auto-routing - but do
    // no blocking work. Holding the mixer mutex across the disk save or the
    // backend move calls (each up to the native backend's 3s request timeout)
    // would stall every other command - including tray-menu building - behind
    // this fast poll, and slow-loop polls would stack up (TD-004). So we snapshot
    // the decisions here and release the guard before touching disk or PipeWire.
    let (seen_to_save, assignment_change, planned) = {
        let mut mixer = state.lock_mixer()?;
        let mut structural_change = false;
        let mut adopted_identities = std::collections::HashSet::new();
        let previous_assignments = mixer.assignments.clone();
        let mut next_assignments = previous_assignments.clone();
        for stream in &streams {
            let was_known = mixer
                .seen
                .get(&stream.match_prop, &stream.match_value)
                .is_some();
            let inherit_ignored = !was_known
                && stream
                    .desktop_id
                    .as_deref()
                    .is_some_and(|desktop_id| canonical_group_is_ignored(&mixer.seen, desktop_id));
            structural_change |= mixer.seen.upsert(
                &stream.match_prop,
                &stream.match_value,
                &stream.app_name,
                stream.icon_name.as_deref(),
                stream.desktop_id.as_deref(),
                now,
            );
            if inherit_ignored {
                mixer
                    .seen
                    .set_ignored(&stream.match_prop, &stream.match_value, true);
                structural_change = true;
            }

            // An application may choose one of our virtual devices directly
            // in its own settings (Discord selecting Chat, for example).
            // Adopt that real PipeWire route so the inactive/history view and
            // future launches do not incorrectly call the app "unrouted".
            let identity = (stream.match_prop.clone(), stream.match_value.clone());
            if let Some(target) = observed_targets.get(&identity) {
                if next_assignments.sink_for(&stream.match_prop, &stream.match_value)
                    != Some(target.as_str())
                {
                    next_assignments.set(&stream.match_prop, &stream.match_value, target);
                    adopted_identities.insert(identity);
                }
            } else if stream.assigned_sink.is_none()
                && !was_known
                && next_assignments
                    .sink_for(&stream.match_prop, &stream.match_value)
                    .is_none()
            {
                // A canonical app can reveal helper identities only after it
                // starts playing. Inherit a route only when every already
                // assigned member agrees; mixed groups require an explicit
                // user choice and are never guessed.
                if let Some(target) = stream.desktop_id.as_deref().and_then(|desktop_id| {
                    single_canonical_assignment(&mixer.seen, &next_assignments, desktop_id)
                }) {
                    next_assignments.set(&stream.match_prop, &stream.match_value, &target);
                }
            }
        }
        let adopted_indices = streams
            .iter()
            .filter(|stream| {
                adopted_identities
                    .contains(&(stream.match_prop.clone(), stream.match_value.clone()))
            })
            .map(|stream| stream.index)
            .collect::<Vec<_>>();
        let assignments_changed = next_assignments.assignments != previous_assignments.assignments;
        // A pure last_seen bump never reports a structural change, so without
        // this the freshest timestamps only reach disk on a clean tray-quit -
        // and an unclean exit would leave a daily-used app looking stale
        // enough for the prune to forget it. Flushing on a slow cadence
        // bounds that drift, and re-runs the prune for sessions that outlive
        // the window.
        if now.saturating_sub(mixer.seen_saved_at) >= SEEN_FLUSH_SECS {
            mixer.prune_stale_apps(now);
            mixer.seen_saved_at = now;
            structural_change = true;
        }

        // Hide ignored identities (also exempts them from auto-routing).
        streams.retain(|s| !mixer.seen.is_ignored(&s.match_prop, &s.match_value));

        // Only enforce once the virtual sinks exist; otherwise streams would be
        // marked handled while their target sink can't be moved to yet.
        let mut planned: Vec<(u32, String, String)> = Vec::new();
        if mixer.initialized {
            for stream in &streams {
                if mixer.auto_routed.contains(&stream.index) {
                    continue;
                }
                if let Some(target) =
                    next_assignments.sink_for(&stream.match_prop, &stream.match_value)
                {
                    if stream.assigned_sink.as_deref() != Some(target) {
                        planned.push((stream.index, target.to_string(), stream.app_name.clone()));
                    }
                }
                // Marked handled once (before the move, so a concurrent poll
                // can't re-plan it); manual re-routing then isn't fought.
                mixer.auto_routed.insert(stream.index);
            }
            // Forget streams that have gone away, so the ledger can't grow
            // without bound and a recycled PipeWire index isn't mistaken for one
            // we already handled (which would skip auto-routing a new stream).
            let live: std::collections::HashSet<u32> = streams.iter().map(|s| s.index).collect();
            mixer.auto_routed.retain(|i| live.contains(i));
        }

        // User-chosen display names (in-memory read, cheap enough to keep here).
        for stream in &mut streams {
            stream.alias = mixer
                .aliases
                .get(&stream.match_prop, &stream.match_value)
                .map(str::to_string);
        }

        // Snapshot the history for an out-of-lock save, only when it changed.
        (
            structural_change.then(|| mixer.seen.clone()),
            assignments_changed.then_some((
                previous_assignments,
                next_assignments,
                adopted_indices,
            )),
            planned,
        )
    };

    // Phase 2: the blocking work, with the lock released.
    if let Some(seen) = seen_to_save {
        if let Err(e) = seen.save() {
            eprintln!("sonux: saving app history failed: {e}");
        }
    }
    if let Some((previous, next, adopted_indices)) = assignment_change {
        let persist_result = {
            let mixer = state.lock_mixer()?;
            crate::commands::apps::persist_assignments(state, &mixer, &previous, &next)
        };
        if let Err(error) = persist_result {
            // Let the next poll retry planned routing instead of retaining a
            // handled ledger entry from a failed adoption transaction.
            let mut mixer = state.lock_mixer()?;
            for (index, _, _) in &planned {
                mixer.auto_routed.remove(index);
            }
            for index in adopted_indices {
                mixer.auto_routed.remove(&index);
            }
            return Err(format!(
                "saving externally selected application routes failed: {error}"
            ));
        }
        state.lock_mixer()?.assignments = next;
    }
    for (index, target, app_name) in planned {
        match state.backend.move_stream_to_sink(index, &target) {
            // Reflect the successful move in the snapshot returned to the UI.
            Ok(()) => {
                if let Some(s) = streams.iter_mut().find(|s| s.index == index) {
                    s.assigned_sink = Some(target);
                }
            }
            Err(e) => eprintln!("sonux: auto-route of {app_name} (#{index}) failed: {e}"),
        }
    }

    Ok(streams)
}

/// Physical output devices (everything that isn't one of our virtual sinks).
#[tauri::command]
pub fn get_output_devices(state: State<'_, AppState>) -> Result<Vec<OutputDevice>, String> {
    state
        .backend
        .list_output_devices()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::audio::backend::AudioBackend;
    use crate::audio::types::{EqConfig, MicConfig, SinkControlState};
    use crate::error::SinkError;

    struct RoutingBackend {
        stream: AppStream,
        moves: Mutex<Vec<(u32, String)>>,
    }

    impl AudioBackend for RoutingBackend {
        fn create_virtual_sink(&self, _: &str, _: &str) -> Result<(), SinkError> {
            Ok(())
        }
        fn destroy_virtual_sink(&self, _: &str) -> Result<(), SinkError> {
            Ok(())
        }
        fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError> {
            Ok(vec![self.stream.clone()])
        }
        fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
            Ok(Vec::new())
        }
        fn list_sink_control_states(
            &self,
            _: &[String],
        ) -> Result<Vec<SinkControlState>, SinkError> {
            Ok(Vec::new())
        }
        fn set_sink_volume(&self, _: &str, _: u8) -> Result<(), SinkError> {
            Ok(())
        }
        fn set_sink_mute(&self, _: &str, _: bool) -> Result<(), SinkError> {
            Ok(())
        }
        fn move_stream_to_sink(&self, index: u32, sink: &str) -> Result<(), SinkError> {
            self.moves.lock().unwrap().push((index, sink.to_string()));
            Ok(())
        }
        fn set_app_volume(&self, _: u32, _: u8) -> Result<(), SinkError> {
            Ok(())
        }
        fn set_channel_output(&self, _: &str, _: Option<&str>) -> Result<(), SinkError> {
            Ok(())
        }
        fn set_channel_eq(&self, _: &str, _: &EqConfig) -> Result<(), SinkError> {
            Ok(())
        }
        fn create_bus(&self, _: &str, _: &str) -> Result<(), SinkError> {
            Ok(())
        }
        fn destroy_bus(&self, _: &str) -> Result<(), SinkError> {
            Ok(())
        }
        fn set_bus_members(&self, _: &str, _: &[String]) -> Result<(), SinkError> {
            Ok(())
        }
        fn set_monitor(&self, _: &str, _: bool) -> Result<(), SinkError> {
            Ok(())
        }
        fn list_input_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
            Ok(Vec::new())
        }
        fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError> {
            Ok((None, None))
        }
        fn set_default_output(&self, _: &str) -> Result<(), SinkError> {
            Ok(())
        }
        fn set_default_input(&self, _: &str) -> Result<(), SinkError> {
            Ok(())
        }
        fn set_mic_config(&self, _: &MicConfig) -> Result<(), SinkError> {
            Ok(())
        }
    }

    #[test]
    fn canonical_assignment_is_inherited_only_when_existing_members_agree() {
        let mut seen = crate::persistence::seen::SeenApps::default();
        let mut assignments = crate::persistence::assignments::Assignments::default();
        seen.upsert(
            "application.name",
            "helper-a",
            "Game",
            None,
            Some("game"),
            1,
        );
        seen.upsert(
            "application.name",
            "helper-b",
            "Game",
            None,
            Some("game"),
            1,
        );
        assignments.set("application.name", "helper-a", "sink_game");
        assignments.set("application.name", "helper-b", "sink_game");
        assert_eq!(
            single_canonical_assignment(&seen, &assignments, "game").as_deref(),
            Some("sink_game")
        );

        assignments.set("application.name", "helper-b", "sink_media");
        assert_eq!(
            single_canonical_assignment(&seen, &assignments, "game"),
            None
        );
    }

    #[test]
    fn canonical_ignore_is_inherited_only_when_existing_members_are_all_ignored() {
        let mut seen = crate::persistence::seen::SeenApps::default();
        seen.upsert(
            "application.name",
            "helper-a",
            "Game",
            None,
            Some("game"),
            1,
        );
        seen.set_ignored("application.name", "helper-a", true);
        assert!(canonical_group_is_ignored(&seen, "game"));

        seen.upsert(
            "application.name",
            "helper-b",
            "Game",
            None,
            Some("game"),
            1,
        );
        assert!(!canonical_group_is_ignored(&seen, "game"));
    }

    fn routed_stream(index: u32, identity: &str, sink: &str) -> AppStream {
        AppStream {
            index,
            app_name: "Shared identity".into(),
            match_prop: "application.name".into(),
            match_value: identity.into(),
            alias: None,
            icon_name: None,
            icon_path: None,
            desktop_id: None,
            pid: None,
            assigned_sink: Some(sink.into()),
            volume_percent: 100,
            muted: false,
            active: true,
        }
    }

    #[test]
    fn exact_identity_route_is_adopted_only_when_all_live_streams_agree() {
        let agreed = vec![
            routed_stream(1, "browser", "sink_game"),
            routed_stream(2, "browser", "sink_game"),
        ];
        assert_eq!(
            unambiguous_managed_targets(&agreed)
                .get(&("application.name".into(), "browser".into()))
                .map(String::as_str),
            Some("sink_game")
        );

        let conflicting = vec![
            routed_stream(2, "browser", "sink_media"),
            routed_stream(1, "browser", "sink_game"),
        ];
        assert!(!unambiguous_managed_targets(&conflicting)
            .contains_key(&("application.name".into(), "browser".into())));

        let reversed = [conflicting[1].clone(), conflicting[0].clone()];
        assert!(!unambiguous_managed_targets(&reversed)
            .contains_key(&("application.name".into(), "browser".into())));
    }

    #[test]
    fn locked_poll_enforces_changed_profile_assignment_and_snapshot_fields_agree() {
        let identity_prop = "application.name";
        let identity_value = "sonux-snapshot-route-test";
        let backend = Arc::new(RoutingBackend {
            stream: AppStream {
                index: 41,
                app_name: "Sonux Snapshot Route Test".into(),
                match_prop: identity_prop.into(),
                match_value: identity_value.into(),
                alias: None,
                icon_name: None,
                icon_path: None,
                desktop_id: None,
                pid: None,
                assigned_sink: None,
                volume_percent: 100,
                muted: false,
                active: true,
            },
            moves: Mutex::new(Vec::new()),
        });
        let state = AppState::for_test(backend.clone());
        let now = crate::persistence::unix_now();
        {
            let mut mixer = state.lock_mixer().unwrap();
            mixer.initialized = true;
            mixer.seen.upsert(
                identity_prop,
                identity_value,
                "Sonux Snapshot Route Test",
                None,
                None,
                now,
            );
            mixer.seen_saved_at = now;
            mixer
                .assignments
                .set(identity_prop, identity_value, "sink_a");
            mixer
                .assignments
                .set(identity_prop, identity_value, "sink_b");
        }

        let _profile_operation = state.lock_profile_operation().unwrap();
        let streams = poll_app_streams_locked(&state).unwrap();
        let seen = crate::commands::apps::snapshot_seen_apps(&state.lock_mixer().unwrap());

        assert_eq!(
            backend.moves.lock().unwrap().as_slice(),
            &[(41, "sink_b".into())]
        );
        assert_eq!(streams[0].assigned_sink.as_deref(), Some("sink_b"));
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].assigned_sink.as_deref(), Some("sink_b"));
    }
}

/// Create the user's virtual sinks and restore volume/mute from the active
/// profile. Idempotent: safe to call again if the sinks already exist.
#[tauri::command]
pub fn init_virtual_devices(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Startup reconciliation is one graph-wide transaction. Automation also
    // applies profiles from its monitor thread, so use the shared boundary to
    // prevent the two sequences from interleaving backend mutations.
    let _profile_operation = state.lock_profile_operation()?;
    let (defs, prefs, active_profile) = {
        let mixer = state.lock_mixer()?;
        (
            mixer.channel_defs.clone(),
            mixer.prefs.clone(),
            mixer.active_profile.clone(),
        )
    };
    let restored_channels = active_profile
        .as_deref()
        .and_then(|name| crate::persistence::profiles::load(name).ok())
        .map(|profile| profile.channels)
        .unwrap_or_default();

    for def in &defs.channels {
        state
            .backend
            .create_virtual_sink(&def.name, &prefs.decorate(&def.label))
            .map_err(|e| e.to_string())?;
        let restored = restored_channels
            .iter()
            .find(|channel| channel.name == def.name);
        state
            .backend
            .set_sink_volume(
                &def.name,
                restored.map_or(100, |channel| channel.volume_percent),
            )
            .map_err(|e| e.to_string())?;
        state
            .backend
            .set_sink_mute(&def.name, restored.is_some_and(|channel| channel.muted))
            .map_err(|e| e.to_string())?;
    }

    let (outputs, eq, mic, secondary_mics, buses) = {
        let mut mixer = state.lock_mixer()?;
        mixer.init_defaults();
        for channel in &mut mixer.channels {
            if let Some(restored) = restored_channels
                .iter()
                .find(|saved| saved.name == channel.name)
            {
                channel.volume_percent = restored.volume_percent;
                channel.muted = restored.muted;
            }
        }
        // The master mix always carries the full channel set.
        let names: Vec<String> = defs.channels.iter().map(|c| c.name.clone()).collect();
        mixer.buses.sync_master(&names);
        (
            mixer.outputs.clone(),
            mixer.eq.clone(),
            mixer.mic.clone(),
            mixer.secondary_mics.clone(),
            mixer.buses.clone(),
        )
    };
    let mut graph_errors = Vec::new();
    if let Err(e) = buses.save() {
        graph_errors.push(format!("save synchronized mixes: {e}"));
    }

    // Wire every channel to its saved output (or the system default) so
    // channels are audible from the start.
    for def in &defs.channels {
        if let Err(e) = state
            .backend
            .set_channel_output(&def.name, outputs.get(&def.name))
        {
            graph_errors.push(format!("route channel {}: {e}", def.name));
        }
        // Restore per-channel failover (default on, so only push the ones off).
        if !outputs.failover(&def.name) {
            if let Err(e) = state.backend.set_channel_failover(&def.name, false) {
                graph_errors.push(format!("restore failover for {}: {e}", def.name));
            }
        }
        // Restore the whole channel processor. Game/Media always need their
        // insert because their stable 7.1 device must be either HRTF-rendered
        // or safely downmixed even before a user changes any setting.
        if state.backend_native {
            let config = eq.get(&def.name);
            if let Err(e) = state.backend.set_channel_eq(&def.name, &config) {
                graph_errors.push(format!("restore processor for {}: {e}", def.name));
            }
        }
    }

    // Bring up the user's mixes and their memberships.
    let names: Vec<String> = defs.channels.iter().map(|c| c.name.clone()).collect();
    if state.backend_native {
        for bus in &buses.buses {
            if let Err(e) = state
                .backend
                .create_bus(&bus.name, &prefs.decorate(&bus.label))
            {
                graph_errors.push(format!("create mix {}: {e}", bus.name));
                continue;
            }
            if let Err(e) = state
                .backend
                .set_bus_members(&bus.name, &bus.effective_members(&names))
            {
                graph_errors.push(format!("restore members for {}: {e}", bus.name));
            }
            if let Err(e) = crate::commands::buses::set_bus_level(state.backend.as_ref(), bus) {
                graph_errors.push(format!("restore level for {}: {e}", bus.name));
            }
        }
    }

    // Bring the mic chain up if it was enabled last session.
    if state.backend_native && mic.enabled {
        let mut applied = mic.clone();
        applied.output_label = prefs.decorate(&mic.output_label);
        if let Err(e) = state.backend.set_mic_config(&applied) {
            graph_errors.push(format!("restore primary microphone: {e}"));
        }
    }
    if state.backend_native && prefs.multiple_mics {
        for mic in secondary_mics.into_iter().filter(|mic| mic.enabled) {
            let mut applied = mic.clone();
            applied.output_label = prefs.decorate(&mic.output_label);
            if let Err(error) = state.backend.set_mic_config(&applied) {
                graph_errors.push(format!(
                    "restore secondary microphone {}: {error}",
                    mic.node_name
                ));
            }
        }
    }

    if !graph_errors.is_empty() {
        if let Ok(mut mixer) = state.lock_mixer() {
            mixer.initialized = false;
        }
        return Err(format!(
            "audio graph initialization incomplete: {}",
            graph_errors.join("; ")
        ));
    }

    // First run or recovery from an externally emptied profile directory:
    // capture the current layout as "Default" so there is always a known-good
    // state to return to. It also becomes the active (autosaving) profile.
    let profile_list = crate::persistence::profiles::list().map_err(|error| {
        if let Ok(mut mixer) = state.lock_mixer() {
            mixer.initialized = false;
        }
        format!("list profiles during startup: {error}")
    })?;
    if profile_list.is_empty() {
        if crate::persistence::profiles::has_any_profile_files().map_err(|error| {
            if let Ok(mut mixer) = state.lock_mixer() {
                mixer.initialized = false;
            }
            format!("inspect profile directory during startup: {error}")
        })? {
            state.lock_mixer()?.initialized = false;
            return Err(
                "no valid profiles remain; existing profile files were preserved for recovery"
                    .into(),
            );
        }
        let default = {
            let mixer = state.lock_mixer()?;
            crate::persistence::profiles::Profile {
                name: "Default".to_string(),
                protected: true,
                channels: mixer.channels.clone(),
                mic: Some(mixer.mic.clone()),
                secondary_mics: mixer.secondary_mics.clone(),
                assignments: mixer.assignments.clone(),
                outputs: mixer.outputs.clone(),
                eq: mixer.eq.clone(),
                trigger_device: None,
                buses: mixer.buses.clone(),
            }
        };
        if let Err(error) = crate::persistence::profiles::save(&default) {
            state.lock_mixer()?.initialized = false;
            return Err(format!("creating Default profile failed: {error}"));
        }
        if let Err(error) = crate::persistence::active::save(Some(&default.name)) {
            let marker_rollback = crate::persistence::active::save(active_profile.as_deref());
            let restored_marker = marker_rollback.is_ok();
            let profile_rollback = if restored_marker {
                crate::persistence::profiles::delete(&default.name)
            } else {
                Ok(())
            };
            state.lock_mixer()?.initialized = false;
            let mut message = format!("saving active Default profile failed: {error}");
            if let Err(rollback_error) = marker_rollback {
                message.push_str(&format!(
                    "; restoring the previous active marker also failed: {rollback_error}"
                ));
            }
            if !restored_marker {
                message.push_str(
                    "; retained Default because the active marker may still reference it",
                );
            }
            if let Err(rollback_error) = profile_rollback {
                message.push_str(&format!(
                    "; removing the unbound Default profile also failed: {rollback_error}"
                ));
            }
            return Err(message);
        }
        let mut mixer = state.lock_mixer()?;
        mixer.active_profile = Some(default.name);
        mixer.active_trigger = None; // the Default profile has no trigger
        mixer.active_protected = true;
    }
    // Upgrade older installations that predate the protected fallback flag.
    // Prefer the conventional Default profile; otherwise retain the first
    // profile in the stable alphabetical listing.
    if let Ok(profiles) = crate::persistence::profiles::list() {
        if !profiles.iter().any(|profile| profile.protected) {
            if let Some(fallback) = profiles
                .iter()
                .find(|profile| profile.name == "Default")
                .or_else(|| profiles.first())
            {
                if let Ok(mut profile) = crate::persistence::profiles::load(&fallback.name) {
                    profile.protected = true;
                    match crate::persistence::profiles::save(&profile) {
                        Ok(()) => {
                            if let Ok(mut mixer) = state.lock_mixer() {
                                if mixer.active_profile.as_deref() == Some(fallback.name.as_str()) {
                                    mixer.active_protected = true;
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("sonux: protecting fallback profile failed: {error}")
                        }
                    }
                }
            }
        }
    }
    // Profiles/active state may have changed since the tray was built.
    let (assignments, seen) = {
        let mixer = state.lock_mixer()?;
        (mixer.assignments.clone(), mixer.seen.clone())
    };
    if let Err(error) = state.publish_app_routes(Some(&assignments), Some(&seen)) {
        state.lock_mixer()?.initialized = false;
        return Err(format!("publish pre-link app routes: {error}"));
    }
    crate::refresh_tray(&app);
    Ok(())
}

/// Current per-channel output choices (None = follow system default).
#[tauri::command]
pub fn get_channel_outputs(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Option<String>>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer
        .channel_defs
        .channels
        .iter()
        .map(|def| {
            (
                def.name.clone(),
                mixer.outputs.get(&def.name).map(str::to_string),
            )
        })
        .collect())
}

/// Per-channel resolved output: the device node.name each channel is actually
/// routed to right now (after explicit/default/fallback resolution). The UI
/// shows this under "System default" so failover is visible. Empty on the
/// pactl fallback, which can't report it.
#[tauri::command]
pub fn get_resolved_outputs(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, Option<String>>, String> {
    state
        .backend
        .resolved_channel_outputs()
        .map_err(|e| e.to_string())
}

/// Whether each channel fails over to another device when its chosen device
/// (or the default) is gone. On unless explicitly turned off.
#[tauri::command]
pub fn get_channel_failover(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, bool>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer
        .channel_defs
        .channels
        .iter()
        .map(|def| (def.name.clone(), mixer.outputs.failover(&def.name)))
        .collect())
}

fn output_edit_failure(
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

/// Route a channel to an output device; empty `output_name` = follow the
/// system default. Persisted across restarts.
#[tauri::command]
pub fn set_channel_output(
    state: State<'_, AppState>,
    sink_name: String,
    output_name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    state.ensure_known_channel(&sink_name)?;
    let output = if output_name.is_empty() {
        None
    } else {
        Some(output_name)
    };
    if let Some(output) = output.as_deref() {
        state.ensure_output_device(output)?;
    }
    let (old_output, old_outputs, outputs) = {
        let mixer = state.lock_mixer()?;
        let old_output = mixer.outputs.get(&sink_name).map(str::to_string);
        let old_outputs = mixer.outputs.clone();
        let mut outputs = old_outputs.clone();
        outputs.set(&sink_name, output.clone());
        (old_output, old_outputs, outputs)
    };
    state
        .backend
        .set_channel_output(&sink_name, output.as_deref())
        .map_err(|e| e.to_string())?;

    if let Err(error) = outputs.save() {
        return Err(output_edit_failure(
            error,
            &[
                ("restoring the previous output file", old_outputs.save()),
                (
                    "restoring the previous live output",
                    state
                        .backend
                        .set_channel_output(&sink_name, old_output.as_deref()),
                ),
            ],
        ));
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_outputs(&mixer, &outputs) {
            return Err(output_edit_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_outputs(&mixer, &old_outputs),
                    ),
                    ("restoring the previous output file", old_outputs.save()),
                    (
                        "restoring the previous live output",
                        state
                            .backend
                            .set_channel_output(&sink_name, old_output.as_deref()),
                    ),
                ],
            ));
        }
    }
    state.lock_mixer()?.outputs = outputs;
    Ok(())
}

/// Turn a channel's auto-failover on or off. Off = the channel plays only on
/// its chosen device (or exact default) and stays silent when that's gone.
/// Persisted across restarts.
#[tauri::command]
pub fn set_channel_failover(
    state: State<'_, AppState>,
    sink_name: String,
    enabled: bool,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    state.ensure_known_channel(&sink_name)?;
    let (old_enabled, old_outputs, outputs) = {
        let mixer = state.lock_mixer()?;
        let old_enabled = mixer.outputs.failover(&sink_name);
        let old_outputs = mixer.outputs.clone();
        let mut outputs = old_outputs.clone();
        outputs.set_failover(&sink_name, enabled);
        (old_enabled, old_outputs, outputs)
    };
    state
        .backend
        .set_channel_failover(&sink_name, enabled)
        .map_err(|e| e.to_string())?;

    if let Err(error) = outputs.save() {
        return Err(output_edit_failure(
            error,
            &[
                ("restoring the previous output file", old_outputs.save()),
                (
                    "restoring the previous live failover",
                    state.backend.set_channel_failover(&sink_name, old_enabled),
                ),
            ],
        ));
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_outputs(&mixer, &outputs) {
            return Err(output_edit_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_outputs(&mixer, &old_outputs),
                    ),
                    ("restoring the previous output file", old_outputs.save()),
                    (
                        "restoring the previous live failover",
                        state.backend.set_channel_failover(&sink_name, old_enabled),
                    ),
                ],
            ));
        }
    }
    state.lock_mixer()?.outputs = outputs;
    Ok(())
}

/// Destroy all virtual sinks. Called before the app exits.
#[tauri::command]
pub fn teardown_virtual_devices(state: State<'_, AppState>) -> Result<(), String> {
    let errors = state.teardown_virtual_sinks();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
