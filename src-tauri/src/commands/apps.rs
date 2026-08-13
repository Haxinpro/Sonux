use serde::Serialize;
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
    if errors.is_empty() {
        Ok(())
    } else {
        Err(crate::error::SinkError::Config(errors.join("; ")))
    }
}

pub(crate) fn persist_assignments(
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
                restore_assignments(mixer, previous),
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
    Ok(mixer
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
            );
            SeenApp {
                match_prop: entry.match_prop.clone(),
                match_value: entry.match_value.clone(),
                display_name: resolved
                    .display_name
                    .unwrap_or_else(|| entry.display_name.clone()),
                icon_name: entry.icon_name.clone(),
                icon_path: resolved.icon_path,
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
        .collect())
}

/// Hide (or un-hide) an app from the list and from auto-routing.
#[tauri::command]
pub fn set_app_ignored(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    ignored: bool,
) -> Result<(), String> {
    let (previous, next) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.seen.clone();
        let mut next = previous.clone();
        if !next.set_ignored(&match_prop, &match_value, ignored) {
            return Err("unknown app".to_string());
        }
        (previous, next)
    };
    if let Err(error) = next.save() {
        return Err(app_mutation_failure(
            error,
            &[("restoring the previous app history", previous.save())],
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
        persist_assignments(&mixer, &previous, &assignments)?;
    }
    state.lock_mixer()?.assignments = assignments;
    Ok(())
}
