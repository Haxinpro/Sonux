use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::persistence::profile_automation::{self, ProfileAutomationConfig};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunningApplication {
    pub executable: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct AutomationStatus {
    pub automatic_profile: Option<String>,
    pub matched_executable: Option<String>,
    pub manual_override: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct ProfileAutomationRuntime {
    status: Arc<Mutex<AutomationStatus>>,
    started: Mutex<bool>,
}

impl ProfileAutomationRuntime {
    pub fn start(&self, app: AppHandle) {
        let Ok(mut started) = self.started.lock() else {
            return;
        };
        if *started {
            return;
        }
        *started = true;
        let status = Arc::clone(&self.status);
        let _ = thread::Builder::new()
            .name("sonux-profile-automation".into())
            .spawn(move || monitor(app, status));
    }

    fn status(&self) -> AutomationStatus {
        self.status
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| AutomationStatus {
                error: Some("profile automation status lock poisoned".into()),
                ..AutomationStatus::default()
            })
    }
}

#[tauri::command]
pub fn get_profile_automation() -> ProfileAutomationConfig {
    profile_automation::load()
}

#[tauri::command]
pub fn save_profile_automation(
    state: State<'_, AppState>,
    config: ProfileAutomationConfig,
) -> Result<(), String> {
    // Validation reads the current profile list. Hold the same transaction
    // boundary as profile rename/delete so that list cannot become stale
    // before the automation file is committed.
    let _profile_operation = state.lock_profile_operation()?;
    profile_automation::save(&config).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_profile_automation_status(
    runtime: State<'_, ProfileAutomationRuntime>,
) -> AutomationStatus {
    runtime.status()
}

#[tauri::command]
pub fn list_running_applications() -> Vec<RunningApplication> {
    running_applications()
}

fn monitor(app: AppHandle, shared_status: Arc<Mutex<AutomationStatus>>) {
    let mut automatic_profile: Option<String> = None;
    let mut restore_profile: Option<String> = None;
    let mut matched_executable: Option<String> = None;
    let mut manual_override = false;
    let mut seen_output_devices: Option<HashSet<String>> = None;

    loop {
        let config = profile_automation::load();
        let applications = running_application_names();
        let state = app.state::<AppState>();
        let mut current_profile = state
            .lock_mixer()
            .ok()
            .and_then(|mixer| mixer.active_profile.clone());
        let mut error = None;

        // Device-triggered profiles must keep working while the webview is
        // hidden in the tray. Treat the first successful sample as a baseline,
        // then react only to devices that appear later.
        if let Ok(outputs) = state.backend.list_output_devices() {
            let current_devices = outputs
                .into_iter()
                .map(|device| device.name)
                .collect::<HashSet<_>>();
            if let Some(previous_devices) = &seen_output_devices {
                if let Ok(profiles) = crate::persistence::profiles::list() {
                    if let Some(profile) =
                        newly_connected_profile(&profiles, previous_devices, &current_devices)
                    {
                        if current_profile.as_deref() != Some(profile.as_str()) {
                            match crate::commands::profiles::apply_profile(
                                &app,
                                &state,
                                profile.clone(),
                            ) {
                                Ok(()) => {
                                    current_profile = Some(profile.clone());
                                    let _ = app.emit("profile-changed", &profile);
                                }
                                Err(message) => error = Some(message),
                            }
                        }
                    }
                }
            }
            seen_output_devices = Some(current_devices);
        }

        if automatic_profile.is_some() && current_profile != automatic_profile {
            manual_override = true;
            automatic_profile = None;
        }

        let matched = if config.enabled && !manual_override {
            newest_matching_rule(&config, &applications)
        } else {
            None
        };

        match matched {
            Some((profile, executable))
                if automatic_profile.as_deref() != Some(profile.as_str()) =>
            {
                if restore_profile.is_none() {
                    restore_profile = config.return_profile.clone().or(current_profile);
                }
                match crate::commands::profiles::apply_profile(&app, &state, profile.clone()) {
                    Ok(()) => {
                        automatic_profile = Some(profile.clone());
                        matched_executable = Some(executable);
                        let _ = app.emit("profile-changed", &profile);
                        if config.notifications {
                            show_profile_notification(&profile);
                        }
                    }
                    Err(message) => error = Some(message),
                }
            }
            None if automatic_profile.is_some() => {
                if let Some(profile) = restore_profile.clone() {
                    match crate::commands::profiles::apply_profile(&app, &state, profile.clone()) {
                        Ok(()) => {
                            restore_profile = None;
                            automatic_profile = None;
                            matched_executable = None;
                            let _ = app.emit("profile-changed", &profile);
                            if config.notifications {
                                show_profile_notification(&profile);
                            }
                        }
                        Err(message) => error = Some(message),
                    }
                } else {
                    automatic_profile = None;
                    matched_executable = None;
                }
            }
            _ => {}
        }

        if manual_override && newest_matching_rule(&config, &applications).is_none() {
            manual_override = false;
            restore_profile = None;
            matched_executable = None;
        }

        let next_status = AutomationStatus {
            automatic_profile: automatic_profile.clone(),
            matched_executable: matched_executable.clone(),
            manual_override,
            error,
        };
        if let Ok(mut status) = shared_status.lock() {
            if *status != next_status {
                *status = next_status.clone();
                let _ = app.emit("profile-automation-status", &next_status);
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn newly_connected_profile(
    profiles: &[crate::persistence::profiles::ProfileInfo],
    previous: &HashSet<String>,
    current: &HashSet<String>,
) -> Option<String> {
    profiles
        .iter()
        .find(|profile| {
            profile
                .trigger_device
                .as_ref()
                .is_some_and(|device| current.contains(device) && !previous.contains(device))
        })
        .map(|profile| profile.name.clone())
}

fn show_profile_notification(profile: &str) {
    let _ = Command::new("notify-send")
        .args([
            "--app-name=Sonux",
            "--icon=audio-card",
            "Profile activated",
            profile,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn newest_matching_rule(
    config: &ProfileAutomationConfig,
    applications: &HashMap<String, u64>,
) -> Option<(String, String)> {
    config
        .rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            applications
                .get(&rule.executable.to_ascii_lowercase())
                .map(|started| (*started, rule.profile.clone(), rule.executable.clone()))
        })
        .max_by_key(|(started, _, _)| *started)
        .map(|(_, profile, executable)| (profile, executable))
}

fn running_applications() -> Vec<RunningApplication> {
    let mut applications = HashMap::<String, RunningApplication>::new();
    for (application, _) in scan_processes() {
        applications
            .entry(application.executable.to_ascii_lowercase())
            .or_insert(application);
    }
    let mut applications = applications.into_values().collect::<Vec<_>>();
    applications.sort_by_key(|application| application.executable.to_ascii_lowercase());
    applications
}

fn running_application_names() -> HashMap<String, u64> {
    let mut result: HashMap<String, u64> = HashMap::new();
    for (application, started) in scan_processes() {
        result
            .entry(application.executable.to_ascii_lowercase())
            .and_modify(|existing| *existing = (*existing).max(started))
            .or_insert(started);
    }
    result
}

fn scan_processes() -> Vec<(RunningApplication, u64)> {
    let own_pid = std::process::id();
    let own_uid = process_uid(own_pid);
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| *pid != own_pid && process_uid(*pid) == own_uid)
        .filter_map(|pid| {
            Some((
                application_for_pid(pid)?,
                process_start_time(pid).unwrap_or(0),
            ))
        })
        .collect()
}

fn application_for_pid(pid: u32) -> Option<RunningApplication> {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let executable_path = fs::read_link(root.join("exe")).ok()?;
    let arguments = fs::read(root.join("cmdline"))
        .ok()?
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect::<Vec<_>>();
    let command_executable = command_executable(&executable_path, &arguments);
    let executable = command_executable
        .and_then(|(value, _)| portable_file_name(value))
        .map(str::to_owned)
        .or_else(|| {
            executable_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })?;
    if executable.is_empty() || ignored_executable(&executable) {
        return None;
    }
    Some(RunningApplication {
        executable,
        path: command_executable
            .filter(|(_, windows_guest)| *windows_guest)
            .map(|(value, _)| value.to_owned())
            .unwrap_or_else(|| executable_path.to_string_lossy().into_owned()),
    })
}

fn command_executable<'a>(
    executable_path: &Path,
    arguments: &'a [String],
) -> Option<(&'a str, bool)> {
    let compatibility_runtime = compatibility_runtime_name(&executable_path.to_string_lossy())
        || arguments
            .first()
            .is_some_and(|argument| compatibility_runtime_name(argument));
    if compatibility_runtime {
        if let Some(argument) = arguments
            .iter()
            .find(|argument| argument.to_ascii_lowercase().ends_with(".exe"))
        {
            return Some((argument, true));
        }
    }

    arguments
        .first()
        .filter(|argument| {
            Path::new(argument.as_str()).exists() || !argument.chars().any(char::is_whitespace)
        })
        .map(|argument| (argument.as_str(), false))
}

fn compatibility_runtime_name(value: &str) -> bool {
    portable_file_name(value).is_some_and(|name| {
        let name = name.to_ascii_lowercase();
        matches!(
            name.as_str(),
            "wine" | "wine64" | "wine-preloader" | "wine64-preloader"
        ) || name == "proton"
            || name.starts_with("proton-")
    })
}

fn ignored_executable(executable: &str) -> bool {
    let executable = executable.to_ascii_lowercase();
    [
        "bash",
        "sh",
        "zsh",
        "fish",
        "systemd",
        "dbus-daemon",
        "sonux",
        "at-spi-bus-launcher",
        "at-spi2-registryd",
        "explorer.exe",
        "plugplay.exe",
        "rpcss.exe",
        "services.exe",
        "svchost.exe",
        "winedevice.exe",
        "steamwebhelper.exe",
    ]
    .contains(&executable.as_str())
}

fn portable_file_name(value: &str) -> Option<&str> {
    value.rsplit(['/', '\\']).find(|part| !part.is_empty())
}

fn process_uid(pid: u32) -> Option<u32> {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn process_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = stat.rsplit_once(") ")?.1;
    after_name.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::profile_automation::ApplicationRule;

    #[test]
    fn newest_running_rule_wins() {
        let config = ProfileAutomationConfig {
            enabled: true,
            return_profile: Some("Main".into()),
            notifications: true,
            rules: vec![
                ApplicationRule {
                    executable: "old.exe".into(),
                    path: None,
                    profile: "Old".into(),
                    enabled: true,
                },
                ApplicationRule {
                    executable: "new.exe".into(),
                    path: None,
                    profile: "New".into(),
                    enabled: true,
                },
            ],
        };
        let running = HashMap::from([("old.exe".into(), 10), ("new.exe".into(), 20)]);
        assert_eq!(
            newest_matching_rule(&config, &running),
            Some(("New".into(), "new.exe".into()))
        );
    }

    #[test]
    fn disabled_rules_do_not_match() {
        let config = ProfileAutomationConfig {
            enabled: true,
            return_profile: None,
            notifications: true,
            rules: vec![ApplicationRule {
                executable: "game".into(),
                path: None,
                profile: "Gaming".into(),
                enabled: false,
            }],
        };
        assert_eq!(
            newest_matching_rule(&config, &HashMap::from([("game".into(), 1)])),
            None
        );
    }

    #[test]
    fn parses_process_start_time_after_names_with_spaces() {
        let sample = "123 (game process) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 99";
        let after_name = sample.rsplit_once(") ").unwrap().1;
        assert_eq!(after_name.split_whitespace().nth(19), Some("99"));
    }

    #[test]
    fn extracts_linux_and_windows_executable_names() {
        assert_eq!(
            portable_file_name("/usr/bin/native-game"),
            Some("native-game")
        );
        assert_eq!(
            portable_file_name(r"C:\games\Example\game.exe"),
            Some("game.exe")
        );
    }

    #[test]
    fn exe_arguments_are_only_programs_under_wine_or_proton() {
        let native_arguments = vec!["/usr/bin/editor".into(), "/tmp/Game.exe".into()];
        assert_eq!(
            command_executable(Path::new("/usr/bin/editor"), &native_arguments),
            Some(("/usr/bin/editor", false))
        );

        let wine_arguments = vec!["wine64".into(), r"C:\Games\Game.exe".into()];
        assert_eq!(
            command_executable(Path::new("/usr/bin/wine64-preloader"), &wine_arguments),
            Some((r"C:\Games\Game.exe", true))
        );

        let proton_arguments = vec![
            "/steam/compatibilitytools.d/Proton-GE/proton".into(),
            "run".into(),
            r"Z:\Games\Game.exe".into(),
        ];
        assert_eq!(
            command_executable(Path::new("/usr/bin/python3"), &proton_arguments),
            Some((r"Z:\Games\Game.exe", true))
        );

        let native_exe = vec!["/opt/tools/native.exe".into()];
        assert_eq!(
            command_executable(Path::new("/opt/tools/native.exe"), &native_exe),
            Some(("/opt/tools/native.exe", false))
        );
    }

    #[test]
    fn newly_connected_device_selects_its_bound_profile() {
        use crate::persistence::profiles::ProfileInfo;

        let profiles = vec![ProfileInfo {
            name: "Headset".into(),
            trigger_device: Some("usb-headset".into()),
            protected: false,
        }];
        let previous = HashSet::from(["speakers".into()]);
        let current = HashSet::from(["speakers".into(), "usb-headset".into()]);

        assert_eq!(
            newly_connected_profile(&profiles, &previous, &current),
            Some("Headset".into())
        );
        assert_eq!(newly_connected_profile(&profiles, &current, &current), None);
    }
}
