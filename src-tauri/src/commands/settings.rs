use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::persistence::autostart;
use crate::persistence::prefs::{DeviceLabelStyle, MeterMode, Prefs};
use crate::state::AppState;

const RESTART_PARENT_ARG: &str = "--sink-restart-parent";

fn settings_mutation_failure(
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

/// A detached replacement waits for the current process to disappear before
/// it initializes PipeWire or the single-instance plugin. This avoids both a
/// short-lived duplicate audio graph and terminal launchers killing the new
/// process together with the old process group.
pub(crate) fn wait_for_restart_parent() {
    #[cfg(target_os = "linux")]
    {
        let mut args = std::env::args_os();
        while let Some(arg) = args.next() {
            if arg != RESTART_PARENT_ARG {
                continue;
            }
            let Some(pid) = args
                .next()
                .and_then(|value| value.to_string_lossy().parse::<u32>().ok())
            else {
                return;
            };
            let parent = std::path::PathBuf::from(format!("/proc/{pid}"));
            for _ in 0..200 {
                if !parent.exists() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            eprintln!("sonux: restart parent {pid} did not exit within 5 seconds; continuing");
            return;
        }
    }
}

fn current_args_without_restart_marker() -> Vec<OsString> {
    let mut filtered = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == RESTART_PARENT_ARG {
            let _ = args.next();
        } else {
            filtered.push(arg);
        }
    }
    filtered
}

/// Cargo replaces a release binary by renaming over it. Linux then reports
/// the still-running executable as `/path/to/sink (deleted)`, even though the
/// freshly built `/path/to/sink` already exists. The kernel suffix is not part
/// of the real filename and must never be passed to systemd-run or Command.
fn normalize_restart_executable(executable: PathBuf) -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Some(path) = executable
        .to_str()
        .and_then(|path| path.strip_suffix(" (deleted)"))
    {
        return PathBuf::from(path);
    }
    executable
}

#[cfg(target_os = "linux")]
fn spawn_systemd_replacement(executable: &std::path::Path) -> Result<(), String> {
    // Plasma launches desktop applications in transient systemd scopes. A
    // normal child remains in that cgroup and is killed as soon as the old
    // application exits, even after creating a new Unix process group. Put
    // the replacement in its own user service so it survives that cleanup.
    let unit = format!("sonux-restart-{}", std::process::id());
    let output = Command::new("systemd-run")
        .args([
            "--user",
            "--quiet",
            "--collect",
            "--service-type=exec",
            "--unit",
        ])
        .arg(unit)
        .arg(executable)
        .args(current_args_without_restart_marker())
        .arg(RESTART_PARENT_ARG)
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Could not ask systemd to restart Sonux: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if detail.is_empty() {
            format!("systemd-run exited with {}", output.status)
        } else {
            format!("systemd-run exited with {}: {detail}", output.status)
        })
    }
}

fn spawn_direct_replacement(executable: &std::path::Path) -> Result<(), String> {
    let mut command = Command::new(executable);
    command
        .args(current_args_without_restart_marker())
        .arg(RESTART_PARENT_ARG)
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not launch the replacement Sonux process: {error}"))
}

fn spawn_detached_replacement() -> Result<(), String> {
    let executable = normalize_restart_executable(
        std::env::current_exe()
            .map_err(|error| format!("Could not locate the Sonux executable: {error}"))?,
    );

    #[cfg(target_os = "linux")]
    match spawn_systemd_replacement(&executable) {
        Ok(()) => return Ok(()),
        Err(error) => {
            eprintln!("sonux: systemd restart unavailable ({error}); using direct launch")
        }
    }

    spawn_direct_replacement(&executable)
}

#[cfg(test)]
mod restart_tests {
    use super::normalize_restart_executable;
    use std::path::PathBuf;

    #[test]
    fn strips_linux_deleted_suffix_from_rebuilt_executable() {
        assert_eq!(
            normalize_restart_executable(PathBuf::from("target/release/sonux (deleted)",)),
            PathBuf::from("target/release/sonux"),
        );
    }

    #[test]
    fn keeps_normal_executable_path_unchanged() {
        let path = PathBuf::from("target/release/sonux");
        assert_eq!(normalize_restart_executable(path.clone()), path);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BackendInfo {
    /// True = native PipeWire backend; false = pactl subprocess fallback.
    pub native: bool,
}

#[tauri::command]
pub fn get_backend_info(state: State<'_, AppState>) -> BackendInfo {
    BackendInfo {
        native: state.backend_native,
    }
}

#[tauri::command]
pub fn get_autostart() -> bool {
    autostart::is_enabled()
}

#[tauri::command]
pub fn get_backup_status() -> Result<crate::persistence::backup::BackupStatus, String> {
    crate::persistence::backup::status().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_backup(
    state: State<'_, AppState>,
    frontend_state: BTreeMap<String, String>,
) -> Result<crate::persistence::backup::BackupStatus, String> {
    let _profile_operation = state.lock_profile_operation()?;
    {
        let mixer = state.lock_mixer()?;
        crate::commands::profiles::autosave_active(&mixer);
    }
    crate::persistence::backup::create(
        crate::persistence::backup::BackupKind::Manual,
        frontend_state,
        autostart::is_enabled(),
    )
    .map_err(|error| error.to_string())?;
    crate::persistence::backup::status().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_backup_location() -> Result<(), String> {
    let directory = crate::persistence::backup::backups_dir().map_err(|error| error.to_string())?;
    crate::persistence::ensure_private_dir(&directory).map_err(|error| error.to_string())?;
    Command::new("xdg-open")
        .arg(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the backup location: {error}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreBackupResult {
    frontend_state: BTreeMap<String, String>,
    recovery_backup: String,
    warning: Option<String>,
}

/// Open the trusted native picker and retain a single-use restore grant in
/// Rust. Only the display name crosses into the webview.
#[tauri::command]
pub async fn choose_backup_for_restore(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    // Opening a new picker invalidates any selection the user previously
    // canceled in the confirmation UI.
    state.clear_backup_restore_grant()?;
    let selected = app
        .dialog()
        .file()
        .set_title("Restore Sonux backup")
        .add_filter("Sonux backup", &["sonux-backup"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| format!("Could not resolve the selected backup: {error}"))?;
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Sonux backup")
        .to_string();
    state.set_backup_restore_grant(path)?;
    Ok(Some(display_name))
}

#[tauri::command]
pub fn cancel_backup_restore(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_backup_restore_grant()
}

#[tauri::command]
pub fn restore_backup(
    state: State<'_, AppState>,
    frontend_state: BTreeMap<String, String>,
) -> Result<RestoreBackupResult, String> {
    let path = state.take_backup_restore_grant()?;
    let selected = crate::persistence::backup::read(&path).map_err(|error| error.to_string())?;
    let restored_assignments = selected.assignments().map_err(|error| error.to_string())?;
    let _profile_operation = state.lock_profile_operation()?;
    {
        let mixer = state.lock_mixer()?;
        crate::commands::profiles::autosave_active(&mixer);
    }
    let recovery = crate::persistence::backup::create(
        crate::persistence::backup::BackupKind::AutomaticRecovery,
        frontend_state,
        autostart::is_enabled(),
    )
    .map_err(|error| format!("Could not create the automatic recovery backup: {error}"))?;

    crate::persistence::backup::restore(&selected).map_err(|error| error.to_string())?;
    let recovery_backup = recovery
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Sonux Automatic Recovery Backup")
        .to_string();
    let mut warnings = Vec::new();
    if let Err(error) = crate::persistence::wireplumber::write(&restored_assignments) {
        warnings.push(format!(
            "Could not regenerate the restored WirePlumber routing rules: {error}"
        ));
    }
    let autostart_result = if selected.autostart_enabled {
        autostart::enable()
    } else {
        autostart::disable()
    };
    if let Err(error) = autostart_result {
        warnings.push(format!("Could not restore Start at login: {error}"));
    }

    Ok(RestoreBackupResult {
        frontend_state: selected.frontend_state,
        recovery_backup: recovery_backup.clone(),
        warning: (!warnings.is_empty()).then(|| {
            format!(
                "The configuration was restored, but Sonux was not restarted. {} Your previous setup remains available as \"{recovery_backup}\".",
                warnings.join(" ")
            )
        }),
    })
}

/// Enable/disable the systemd user unit for autostart on login.
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<bool, String> {
    let result = if enabled {
        autostart::enable()
    } else {
        autostart::disable()
    };
    result.map_err(|e| e.to_string())?;
    Ok(autostart::is_enabled())
}

#[tauri::command]
pub fn get_prefs(state: State<'_, AppState>) -> Result<Prefs, String> {
    Ok(state.lock_mixer()?.prefs.clone())
}

/// Set the device naming style. Existing nodes keep their labels until
/// they are recreated (restart or rename).
#[tauri::command]
pub fn set_device_label_style(
    state: State<'_, AppState>,
    style: DeviceLabelStyle,
) -> Result<(), String> {
    let prefs = {
        let mixer = state.lock_mixer()?;
        let mut prefs = mixer.prefs.clone();
        prefs.device_label_style = style;
        prefs
    };
    prefs.save().map_err(|e| e.to_string())?;
    state.lock_mixer()?.prefs = prefs;
    Ok(())
}

/// Set the visual VU meter refresh policy. Audio processing and routing are
/// deliberately unaffected.
#[tauri::command]
pub fn set_meter_mode(state: State<'_, AppState>, mode: MeterMode) -> Result<(), String> {
    let prefs = {
        let mixer = state.lock_mixer()?;
        let mut prefs = mixer.prefs.clone();
        prefs.meter_mode = mode;
        prefs
    };
    prefs.save().map_err(|e| e.to_string())?;
    state.lock_mixer()?.prefs = prefs;
    Ok(())
}

/// Toggle "start minimized" (boot to tray when autostarting). Rewrites
/// the systemd unit when autostart is already enabled so the flag tracks
/// the preference.
#[tauri::command]
pub fn set_start_minimized(state: State<'_, AppState>, minimized: bool) -> Result<(), String> {
    let (previous, prefs) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.prefs.clone();
        let mut prefs = previous.clone();
        prefs.start_minimized = minimized;
        (previous, prefs)
    };
    prefs.save().map_err(|e| e.to_string())?;
    if autostart::is_enabled() {
        if let Err(error) = autostart::enable() {
            let restore_file = previous.save();
            let restore_unit = match &restore_file {
                Ok(()) => autostart::enable(),
                Err(error) => Err(crate::error::SinkError::Config(error.to_string())),
            };
            return Err(settings_mutation_failure(
                error,
                &[
                    ("restoring the previous preferences", restore_file),
                    ("restoring the previous autostart unit", restore_unit),
                ],
            ));
        }
    }
    state.lock_mixer()?.prefs = prefs;
    Ok(())
}

/// Opt in to secondary processed microphones. Disabling preserves their
/// profile configurations but removes their live PipeWire nodes.
#[tauri::command]
pub fn set_multiple_mics(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let _profile_operation = state.lock_profile_operation()?;
    let (previous, prefs, secondary) = {
        let mixer = state.lock_mixer()?;
        let previous = mixer.prefs.clone();
        let mut prefs = previous.clone();
        prefs.multiple_mics = enabled;
        (previous, prefs, mixer.secondary_mics.clone())
    };
    prefs.save().map_err(|error| error.to_string())?;
    let mut applied_configs: Vec<&crate::audio::types::MicConfig> = Vec::new();
    for config in &secondary {
        let mut applied = config.clone();
        applied.enabled &= enabled;
        applied.output_label = prefs.decorate(&config.output_label);
        if let Err(error) = state.backend.set_mic_config(&applied) {
            let mut rollback_errors = Vec::new();
            for previous_config in applied_configs.iter().rev() {
                let mut rollback: crate::audio::types::MicConfig = (**previous_config).clone();
                rollback.enabled &= previous.multiple_mics;
                rollback.output_label = previous.decorate(&rollback.output_label);
                if let Err(rollback_error) = state.backend.set_mic_config(&rollback) {
                    rollback_errors.push(format!(
                        "restore microphone {}: {rollback_error}",
                        rollback.node_name
                    ));
                }
            }
            let prefs_restore = previous.save();
            if let Err(rollback_error) = &prefs_restore {
                rollback_errors.push(format!("restore preferences: {rollback_error}"));
            }
            let detail = if rollback_errors.is_empty() {
                String::new()
            } else {
                format!("; rollback also failed: {}", rollback_errors.join("; "))
            };
            return Err(format!(
                "toggling secondary mic {} failed: {error}{detail}",
                config.node_name
            ));
        }
        applied_configs.push(config);
    }
    state.lock_mixer()?.prefs = prefs;
    Ok(())
}

/// Show or hide the title-bar balance slider.
#[tauri::command]
pub fn set_balance_visible(state: State<'_, AppState>, visible: bool) -> Result<(), String> {
    let prefs = {
        let mixer = state.lock_mixer()?;
        let mut prefs = mixer.prefs.clone();
        prefs.show_balance = visible;
        prefs
    };
    prefs.save().map_err(|e| e.to_string())?;
    state.lock_mixer()?.prefs = prefs;
    Ok(())
}

/// Pick the two channels the balance slider blends.
#[tauri::command]
pub fn set_balance_channels(
    state: State<'_, AppState>,
    a: Option<String>,
    b: Option<String>,
) -> Result<(), String> {
    let prefs = {
        let mixer = state.lock_mixer()?;
        let mut prefs = mixer.prefs.clone();
        prefs.balance_a = a;
        prefs.balance_b = b;
        prefs
    };
    prefs.save().map_err(|e| e.to_string())?;
    state.lock_mixer()?.prefs = prefs;
    Ok(())
}

/// Mark the first-run tutorial as completed (never shown again, until a
/// factory reset).
#[tauri::command]
pub fn set_onboarded(state: State<'_, AppState>) -> Result<(), String> {
    let prefs = {
        let mixer = state.lock_mixer()?;
        let mut prefs = mixer.prefs.clone();
        prefs.onboarded = true;
        prefs
    };
    prefs.save().map_err(|e| e.to_string())?;
    state.lock_mixer()?.prefs = prefs;
    Ok(())
}

/// Factory reset: tear down our audio nodes, wipe every saved file, undo
/// autostart, and relaunch as if freshly installed.
#[tauri::command]
pub fn reset_app(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Best-effort teardown - the relaunch recreates everything anyway.
    for err in state.teardown_virtual_sinks() {
        eprintln!("sonux: reset teardown: {err}");
    }
    let _ = autostart::disable();
    crate::persistence::wipe_all().map_err(|e| e.to_string())?;
    restart_app(app)
}

/// Relaunch the current production executable without changing any saved
/// state. PipeWire drops this process's nodes during exit; startup recreates
/// them from the persisted mixer configuration.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) -> Result<(), String> {
    spawn_detached_replacement()?;
    app.exit(0);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct DefaultDevices {
    pub output: Option<String>,
    pub input: Option<String>,
}

/// Current system default output/input device node names.
#[tauri::command]
pub fn get_default_devices(state: State<'_, AppState>) -> Result<DefaultDevices, String> {
    let (output, input) = state
        .backend
        .get_default_devices()
        .map_err(|e| e.to_string())?;
    Ok(DefaultDevices { output, input })
}

/// Set the system default output device.
#[tauri::command]
pub fn set_default_output(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.ensure_output_device(&name)?;
    state
        .backend
        .set_default_output(&name)
        .map_err(|e| e.to_string())
}

/// Set the system default input device.
#[tauri::command]
pub fn set_default_input(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.ensure_input_device(&name)?;
    state
        .backend
        .set_default_input(&name)
        .map_err(|e| e.to_string())
}
