use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::State;

use crate::persistence::autostart;
use crate::persistence::prefs::{DeviceLabelStyle, Prefs};
use crate::state::AppState;

const RESTART_PARENT_ARG: &str = "--sink-restart-parent";

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
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.device_label_style = style;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Toggle "start minimized" (boot to tray when autostarting). Rewrites
/// the systemd unit when autostart is already enabled so the flag tracks
/// the preference.
#[tauri::command]
pub fn set_start_minimized(state: State<'_, AppState>, minimized: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.start_minimized = minimized;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    if autostart::is_enabled() {
        autostart::enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show or hide the title-bar balance slider.
#[tauri::command]
pub fn set_balance_visible(state: State<'_, AppState>, visible: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.show_balance = visible;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Pick the two channels the balance slider blends.
#[tauri::command]
pub fn set_balance_channels(
    state: State<'_, AppState>,
    a: Option<String>,
    b: Option<String>,
) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.balance_a = a;
        mixer.prefs.balance_b = b;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Mark the first-run tutorial as completed (never shown again, until a
/// factory reset).
#[tauri::command]
pub fn set_onboarded(state: State<'_, AppState>) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.onboarded = true;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
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
    state
        .backend
        .set_default_output(&name)
        .map_err(|e| e.to_string())
}

/// Set the system default input device.
#[tauri::command]
pub fn set_default_input(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .backend
        .set_default_input(&name)
        .map_err(|e| e.to_string())
}
