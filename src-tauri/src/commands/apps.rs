use serde::{Deserialize, Serialize};
use tauri::State;

use crate::persistence::wireplumber;
use crate::state::AppState;

pub(crate) fn app_mutation_failure(
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

pub(crate) fn restore_assignments(
    state: &AppState,
    mixer: &crate::mixer::state::MixerState,
    assignments: &crate::persistence::assignments::Assignments,
) -> Result<(), crate::error::SinkError> {
    let mut errors = Vec::new();
    if let Err(error) = assignments.save() {
        errors.push(format!("assignments file: {error}"));
    }
    if let Err(error) = wireplumber::write(assignments) {
        errors.push(format!("WirePlumber rules: {error}"));
    }
    if let Err(error) = crate::commands::profiles::save_active_with_assignments(mixer, assignments)
    {
        errors.push(format!("active profile: {error}"));
    }
    if let Err(error) = state.publish_app_routes(Some(assignments), Some(&mixer.seen)) {
        errors.push(format!("live pre-link routes: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(crate::error::SinkError::Config(errors.join("; ")))
    }
}

pub(crate) fn persist_assignments(
    state: &AppState,
    mixer: &crate::mixer::state::MixerState,
    previous: &crate::persistence::assignments::Assignments,
    next: &crate::persistence::assignments::Assignments,
) -> Result<(), String> {
    if let Err(error) = next
        .save()
        .and_then(|()| wireplumber::write(next))
        .and_then(|()| crate::commands::profiles::save_active_with_assignments(mixer, next))
    {
        return Err(app_mutation_failure(
            error,
            &[(
                "restoring the previous assignment files and active profile",
                restore_assignments(state, mixer, previous),
            )],
        ));
    }
    if let Err(error) = state.publish_app_routes(Some(next), Some(&mixer.seen)) {
        return Err(app_mutation_failure(
            error,
            &[(
                "restoring the previous assignment files, active profile, and live routes",
                restore_assignments(state, mixer, previous),
            )],
        ));
    }
    Ok(())
}

/// A seen-app entry enriched with its current routing, alias and icon.
#[derive(Debug, Clone, Serialize)]
pub struct SeenApp {
    pub match_prop: String,
    pub match_value: String,
    pub display_name: String,
    pub icon_name: Option<String>,
    pub icon_path: Option<String>,
    pub desktop_id: Option<String>,
    pub last_seen: u64,
    pub ignored: bool,
    pub assigned_sink: Option<String>,
    pub alias: Option<String>,
}

/// Full app history (live and gone, including ignored entries - the
/// frontend decides what to show where).
#[tauri::command]
pub fn get_seen_apps(state: State<'_, AppState>) -> Result<Vec<SeenApp>, String> {
    let mixer = state.lock_mixer()?;
    Ok(snapshot_seen_apps(&mixer))
}

pub(crate) fn snapshot_seen_apps(mixer: &crate::mixer::state::MixerState) -> Vec<SeenApp> {
    mixer
        .seen
        .apps
        .iter()
        .map(|entry| {
            let binary = (entry.match_prop == "application.process.binary")
                .then_some(entry.match_value.as_str());
            // History entries have no live process - name-based lookup only.
            let resolved = crate::audio::icons::resolve(
                &entry.display_name,
                binary,
                entry.icon_name.as_deref(),
                None,
                entry.desktop_id.as_deref(),
            );
            SeenApp {
                match_prop: entry.match_prop.clone(),
                match_value: entry.match_value.clone(),
                display_name: resolved
                    .display_name
                    .unwrap_or_else(|| entry.display_name.clone()),
                icon_name: entry.icon_name.clone(),
                icon_path: resolved.icon_path,
                desktop_id: resolved.desktop_id.or_else(|| entry.desktop_id.clone()),
                last_seen: entry.last_seen,
                ignored: entry.ignored,
                assigned_sink: mixer
                    .assignments
                    .sink_for(&entry.match_prop, &entry.match_value)
                    .map(str::to_string),
                alias: mixer
                    .aliases
                    .get(&entry.match_prop, &entry.match_value)
                    .map(str::to_string),
            }
        })
        .collect()
}

/// One raw PipeWire identity inside a canonical desktop application group.
/// Group mutations remain identity-based so WirePlumber matching stays exact.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppIdentity {
    pub match_prop: String,
    pub match_value: String,
}

pub(crate) fn checked_identities(identities: Vec<AppIdentity>) -> Result<Vec<AppIdentity>, String> {
    if identities.is_empty() || identities.len() > 64 {
        return Err("application group must contain between 1 and 64 identities".into());
    }
    let mut unique = Vec::with_capacity(identities.len());
    for identity in identities {
        if identity.match_prop.trim().is_empty()
            || identity.match_value.trim().is_empty()
            || identity.match_prop.len() > 128
            || identity.match_value.len() > 512
        {
            return Err("invalid application identity".into());
        }
        if !unique.contains(&identity) {
            unique.push(identity);
        }
    }
    Ok(unique)
}

/// Atomically hide or restore every raw identity in one canonical app group.
#[tauri::command]
pub fn set_app_group_ignored(
    state: State<'_, AppState>,
    identities: Vec<AppIdentity>,
    ignored: bool,
) -> Result<(), String> {
    let identities = checked_identities(identities)?;
    let _profile_operation = state.lock_profile_operation()?;
    let (previous, next, assignments) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.seen.clone();
        let mut next = previous.clone();
        for identity in &identities {
            if !next.set_ignored(&identity.match_prop, &identity.match_value, ignored) {
                return Err("unknown app identity".into());
            }
        }
        (previous, next, mixer.assignments.clone())
    };
    if let Err(error) = next.save() {
        return Err(app_mutation_failure(
            error,
            &[("restoring the previous app history", previous.save())],
        ));
    }
    if let Err(error) = state.publish_app_routes(Some(&assignments), Some(&next)) {
        return Err(app_mutation_failure(
            error,
            &[
                ("restoring the previous app history", previous.save()),
                (
                    "restoring the previous live pre-link routes",
                    state
                        .publish_app_routes(Some(&assignments), Some(&previous))
                        .map_err(crate::error::SinkError::Config),
                ),
            ],
        ));
    }
    state.lock_mixer()?.seen = next;
    Ok(())
}

/// Hide (or un-hide) an app from the list and from auto-routing.
#[tauri::command]
pub fn set_app_ignored(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    ignored: bool,
) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    let (previous, next, assignments) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.seen.clone();
        let mut next = previous.clone();
        if !next.set_ignored(&match_prop, &match_value, ignored) {
            return Err("unknown app".to_string());
        }
        (previous, next, mixer.assignments.clone())
    };
    if let Err(error) = next.save() {
        return Err(app_mutation_failure(
            error,
            &[("restoring the previous app history", previous.save())],
        ));
    }
    if let Err(error) = state.publish_app_routes(Some(&assignments), Some(&next)) {
        return Err(app_mutation_failure(
            error,
            &[
                ("restoring the previous app history", previous.save()),
                (
                    "restoring the previous live pre-link routes",
                    state
                        .publish_app_routes(Some(&assignments), Some(&previous))
                        .map_err(crate::error::SinkError::Config),
                ),
            ],
        ));
    }
    state.lock_mixer()?.seen = next;
    Ok(())
}

/// Erase an app from history entirely: sighting, assignment and alias.
#[tauri::command]
pub fn forget_app(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (old_seen, old_assignments, old_aliases, next_seen, next_assignments, next_aliases) = {
        let mixer = state.lock_mixer()?;
        let old_seen = mixer.seen.clone();
        let old_assignments = mixer.assignments.clone();
        let old_aliases = mixer.aliases.clone();
        let mut next_seen = old_seen.clone();
        let mut next_assignments = old_assignments.clone();
        let mut next_aliases = old_aliases.clone();
        next_seen.forget(&match_prop, &match_value);
        next_assignments.remove(&match_prop, &match_value);
        next_aliases.set(&match_prop, &match_value, "");
        (
            old_seen,
            old_assignments,
            old_aliases,
            next_seen,
            next_assignments,
            next_aliases,
        )
    };
    let persist = next_seen
        .save()
        .and_then(|()| next_assignments.save())
        .and_then(|()| next_aliases.save())
        .and_then(|()| wireplumber::write(&next_assignments))
        .and_then(|()| {
            let mixer = state
                .lock_mixer()
                .map_err(crate::error::SinkError::Config)?;
            crate::commands::profiles::save_active_with_assignments(&mixer, &next_assignments)
        });
    if let Err(error) = persist {
        let profile_restore = state
            .lock_mixer()
            .map_err(crate::error::SinkError::Config)
            .and_then(|mixer| {
                crate::commands::profiles::save_active_with_assignments(&mixer, &old_assignments)
            });
        return Err(app_mutation_failure(
            error,
            &[
                ("restoring the previous app history", old_seen.save()),
                ("restoring the previous assignments", old_assignments.save()),
                ("restoring the previous aliases", old_aliases.save()),
                (
                    "restoring the previous WirePlumber rules",
                    wireplumber::write(&old_assignments),
                ),
                ("restoring the previous active profile", profile_restore),
            ],
        ));
    }
    if let Err(error) = state.publish_app_routes(Some(&next_assignments), Some(&next_seen)) {
        let profile_restore = state
            .lock_mixer()
            .map_err(crate::error::SinkError::Config)
            .and_then(|mixer| {
                crate::commands::profiles::save_active_with_assignments(&mixer, &old_assignments)
            });
        return Err(app_mutation_failure(
            error,
            &[
                ("restoring the previous app history", old_seen.save()),
                ("restoring the previous assignments", old_assignments.save()),
                ("restoring the previous aliases", old_aliases.save()),
                ("restoring the previous active profile", profile_restore),
                (
                    "restoring the previous live pre-link routes",
                    state
                        .publish_app_routes(Some(&old_assignments), Some(&old_seen))
                        .map_err(crate::error::SinkError::Config),
                ),
            ],
        ));
    }
    let mut mixer = state.lock_mixer()?;
    mixer.seen = next_seen;
    mixer.assignments = next_assignments;
    mixer.aliases = next_aliases;
    Ok(())
}

/// Erase all raw identities belonging to one canonical application as one
/// persistence transaction, including their routing rules and aliases.
#[tauri::command]
pub fn forget_app_group(
    state: State<'_, AppState>,
    identities: Vec<AppIdentity>,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let identities = checked_identities(identities)?;
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (old_seen, old_assignments, old_aliases, next_seen, next_assignments, next_aliases) = {
        let mixer = state.lock_mixer()?;
        let old_seen = mixer.seen.clone();
        let old_assignments = mixer.assignments.clone();
        let old_aliases = mixer.aliases.clone();
        let mut next_seen = old_seen.clone();
        let mut next_assignments = old_assignments.clone();
        let mut next_aliases = old_aliases.clone();
        for identity in &identities {
            next_seen.forget(&identity.match_prop, &identity.match_value);
            next_assignments.remove(&identity.match_prop, &identity.match_value);
            next_aliases.set(&identity.match_prop, &identity.match_value, "");
        }
        (
            old_seen,
            old_assignments,
            old_aliases,
            next_seen,
            next_assignments,
            next_aliases,
        )
    };
    let persist = next_seen
        .save()
        .and_then(|()| next_assignments.save())
        .and_then(|()| next_aliases.save())
        .and_then(|()| wireplumber::write(&next_assignments))
        .and_then(|()| {
            let mixer = state
                .lock_mixer()
                .map_err(crate::error::SinkError::Config)?;
            crate::commands::profiles::save_active_with_assignments(&mixer, &next_assignments)
        });
    if let Err(error) = persist {
        let profile_restore = state
            .lock_mixer()
            .map_err(crate::error::SinkError::Config)
            .and_then(|mixer| {
                crate::commands::profiles::save_active_with_assignments(&mixer, &old_assignments)
            });
        return Err(app_mutation_failure(
            error,
            &[
                ("restoring the previous app history", old_seen.save()),
                ("restoring the previous assignments", old_assignments.save()),
                ("restoring the previous aliases", old_aliases.save()),
                (
                    "restoring the previous WirePlumber rules",
                    wireplumber::write(&old_assignments),
                ),
                ("restoring the previous active profile", profile_restore),
            ],
        ));
    }
    if let Err(error) = state.publish_app_routes(Some(&next_assignments), Some(&next_seen)) {
        let profile_restore = state
            .lock_mixer()
            .map_err(crate::error::SinkError::Config)
            .and_then(|mixer| {
                crate::commands::profiles::save_active_with_assignments(&mixer, &old_assignments)
            });
        return Err(app_mutation_failure(
            error,
            &[
                ("restoring the previous app history", old_seen.save()),
                ("restoring the previous assignments", old_assignments.save()),
                ("restoring the previous aliases", old_aliases.save()),
                ("restoring the previous active profile", profile_restore),
                (
                    "restoring the previous live pre-link routes",
                    state
                        .publish_app_routes(Some(&old_assignments), Some(&old_seen))
                        .map_err(crate::error::SinkError::Config),
                ),
            ],
        ));
    }
    let mut mixer = state.lock_mixer()?;
    mixer.seen = next_seen;
    mixer.assignments = next_assignments;
    mixer.aliases = next_aliases;
    Ok(())
}

/// Edit an app's routing assignment while it isn't running (pre-routing):
/// the app lands on its channel the moment it next plays audio. Empty
/// `sink_name` clears the assignment.
#[tauri::command]
pub fn set_app_assignment(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    sink_name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    if !sink_name.is_empty() {
        state.ensure_known_channel(&sink_name)?;
    }
    let (previous, assignments) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.assignments.clone();
        let mut assignments = previous.clone();
        if sink_name.is_empty() {
            assignments.remove(&match_prop, &match_value);
        } else {
            assignments.set(&match_prop, &match_value, &sink_name);
        }
        (previous, assignments)
    };
    {
        let mixer = state.lock_mixer()?;
        persist_assignments(&state, &mixer, &previous, &assignments)?;
    }
    state.lock_mixer()?.assignments = assignments;
    Ok(())
}

/// Pre-route every raw identity in a canonical application group together.
#[tauri::command]
pub fn set_app_group_assignment(
    state: State<'_, AppState>,
    identities: Vec<AppIdentity>,
    sink_name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let identities = checked_identities(identities)?;
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    if !sink_name.is_empty() {
        state.ensure_known_channel(&sink_name)?;
    }
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
        persist_assignments(&state, &mixer, &previous, &assignments)?;
    }
    state.lock_mixer()?.assignments = assignments;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(prop: &str, value: &str) -> AppIdentity {
        AppIdentity {
            match_prop: prop.into(),
            match_value: value.into(),
        }
    }

    #[test]
    fn group_identity_validation_deduplicates_without_losing_order() {
        let identities = checked_identities(vec![
            identity("application.name", "helper-a"),
            identity("application.name", "helper-a"),
            identity("application.process.binary", "helper-b"),
        ])
        .expect("valid identities");
        assert_eq!(
            identities,
            vec![
                identity("application.name", "helper-a"),
                identity("application.process.binary", "helper-b"),
            ]
        );
    }

    #[test]
    fn group_identity_validation_rejects_empty_and_unbounded_inputs() {
        assert!(checked_identities(Vec::new()).is_err());
        assert!(checked_identities(vec![identity("", "value")]).is_err());
        assert!(checked_identities(vec![identity("application.name", ""); 65]).is_err());
    }
}
