use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::SinkError;

const BACKUP_SCHEMA: u32 = 1;
const BACKUP_EXTENSION: &str = "sonux-backup";
const MAX_BACKUP_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackupKind {
    Manual,
    AutomaticRecovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFile {
    schema: u32,
    product: String,
    pub kind: BackupKind,
    pub created_at: u64,
    config_files: BTreeMap<String, String>,
    #[serde(default)]
    pub frontend_state: BTreeMap<String, String>,
    pub autostart_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    pub count: usize,
    pub last_backup_at: Option<u64>,
}

#[derive(Deserialize)]
struct ShortcutBackupState {
    enabled: bool,
    bindings: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct PresetSelection {
    source: String,
    name: String,
}

#[derive(Deserialize)]
struct ProfileSectionVisibility {
    channels: bool,
    applications: bool,
}

fn config_root() -> Result<PathBuf, SinkError> {
    dirs::config_dir()
        .map(|dir| dir.join("sonux"))
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))
}

pub fn backups_dir() -> Result<PathBuf, SinkError> {
    dirs::data_local_dir()
        .map(|dir| dir.join("sonux").join("backups"))
        .ok_or_else(|| SinkError::Config("cannot resolve the user data directory".into()))
}

fn collect_files(root: &Path) -> Result<BTreeMap<String, String>, SinkError> {
    let mut files = BTreeMap::new();
    if root.exists() {
        collect_files_from(root, root, &mut files)?;
    }
    Ok(files)
}

fn collect_files_from(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), SinkError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files_from(root, &entry.path(), files)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| SinkError::Config(format!("invalid configuration path: {error}")))?
                .to_str()
                .ok_or_else(|| SinkError::Config("configuration path is not valid UTF-8".into()))?
                .to_string();
            let contents = fs::read_to_string(entry.path()).map_err(|error| {
                SinkError::Config(format!("could not read {relative} for backup: {error}"))
            })?;
            files.insert(relative, contents);
        }
    }
    Ok(())
}

fn filename(kind: BackupKind, created_at: u64) -> String {
    let label = match kind {
        BackupKind::Manual => "Sonux Manual Backup",
        BackupKind::AutomaticRecovery => "Sonux Automatic Recovery Backup",
    };
    format!("{label} - {created_at}.{BACKUP_EXTENSION}")
}

fn unique_backup_path(dir: &Path, kind: BackupKind, created_at: u64) -> PathBuf {
    let first = dir.join(filename(kind, created_at));
    if !first.exists() {
        return first;
    }
    let stem = first
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Sonux Backup");
    for suffix in 2_u64.. {
        let candidate = dir.join(format!("{stem} ({suffix}).{BACKUP_EXTENSION}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("the unbounded backup-name search always finds a free path")
}

fn write_backup_to(
    config_root: &Path,
    backup_dir: &Path,
    kind: BackupKind,
    frontend_state: BTreeMap<String, String>,
    autostart_enabled: bool,
) -> Result<PathBuf, SinkError> {
    let created_at = super::unix_now();
    let config_files = collect_files(config_root)?;
    validate_managed_payload(&config_files, &frontend_state)?;
    let backup = BackupFile {
        schema: BACKUP_SCHEMA,
        product: "Sonux".into(),
        kind,
        created_at,
        config_files,
        frontend_state,
        autostart_enabled,
    };
    super::ensure_private_dir(backup_dir)?;
    let path = unique_backup_path(backup_dir, kind, created_at);
    let json = serde_json::to_string_pretty(&backup)
        .map_err(|error| SinkError::Config(format!("could not serialize backup: {error}")))?;
    super::write_atomic(&path, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

pub fn create(
    kind: BackupKind,
    frontend_state: BTreeMap<String, String>,
    autostart_enabled: bool,
) -> Result<PathBuf, SinkError> {
    write_backup_to(
        &config_root()?,
        &backups_dir()?,
        kind,
        frontend_state,
        autostart_enabled,
    )
}

fn validate_relative_path(path: &str) -> Result<(), SinkError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(SinkError::Config(
            "backup contains an unsafe configuration path".into(),
        ));
    }
    Ok(())
}

fn parse_json<T: DeserializeOwned>(relative: &str, contents: &str) -> Result<T, SinkError> {
    serde_json::from_str(contents).map_err(|error| {
        SinkError::Config(format!("backup contains malformed {relative}: {error}"))
    })
}

fn validate_channel_names<'a>(
    relative: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<HashSet<String>, SinkError> {
    let mut seen = HashSet::new();
    for name in names {
        if !name.starts_with("sink_")
            || crate::persistence::channels::is_reserved_sink_name(name)
            || !seen.insert(name.to_string())
        {
            return Err(SinkError::Config(format!(
                "backup contains an invalid or duplicate channel in {relative}: {name}"
            )));
        }
    }
    if seen.is_empty() || seen.len() > crate::persistence::channels::MAX_CHANNELS {
        return Err(SinkError::Config(format!(
            "backup contains an invalid channel count in {relative}"
        )));
    }
    Ok(seen)
}

fn profile_path_parts(relative: &str) -> Option<(&str, &str)> {
    let mut parts = relative.split('/');
    let directory = parts.next()?;
    let filename = parts.next()?;
    if directory != "profiles" || parts.next().is_some() || !filename.ends_with(".json") {
        return None;
    }
    Some((directory, filename.trim_end_matches(".json")))
}

fn preset_path(relative: &str, directory: &str, max_depth: usize) -> bool {
    let parts = relative.split('/').collect::<Vec<_>>();
    parts.first() == Some(&directory)
        && (2..=max_depth).contains(&parts.len())
        && parts.last().is_some_and(|name| name.ends_with(".json"))
}

fn validate_profiles(files: &BTreeMap<String, String>) -> Result<Vec<String>, SinkError> {
    let mut profile_names = Vec::new();
    let mut trigger_owners = BTreeMap::<String, String>::new();
    for (relative, contents) in files {
        let Some((_, stem)) = profile_path_parts(relative) else {
            continue;
        };
        let mut profile: crate::persistence::profiles::Profile = parse_json(relative, contents)?;
        let safe_name = crate::persistence::profiles::sanitize_name(stem)?;
        if safe_name != stem || profile.name != stem {
            return Err(SinkError::Config(format!(
                "backup profile name does not match its file: {relative}"
            )));
        }
        crate::persistence::profiles::normalize_and_validate(&mut profile).map_err(|error| {
            SinkError::Config(format!(
                "backup contains an invalid profile {relative}: {error}"
            ))
        })?;
        let channels = validate_channel_names(
            relative,
            profile.channels.iter().map(|channel| channel.name.as_str()),
        )?;
        if profile
            .assignments
            .assignments
            .iter()
            .any(|assignment| !channels.contains(&assignment.sink_name))
        {
            return Err(SinkError::Config(format!(
                "backup profile contains an assignment to a missing channel: {relative}"
            )));
        }
        if let Some(device) = profile.trigger_device.as_ref() {
            if let Some(owner) = trigger_owners.insert(device.clone(), profile.name.clone()) {
                return Err(SinkError::Config(format!(
                    "backup assigns device {device} to both {owner} and {}",
                    profile.name
                )));
            }
        }
        profile_names.push(profile.name);
    }
    if profile_names.is_empty() {
        return Err(SinkError::Config(
            "backup does not contain a valid audio profile".into(),
        ));
    }
    profile_names.sort();
    profile_names.dedup();
    Ok(profile_names)
}

fn validate_frontend_state(state: &BTreeMap<String, String>) -> Result<(), SinkError> {
    for (key, value) in state {
        match key.as_str() {
            "sonux-theme" => {
                if !matches!(value.as_str(), "original" | "tokyo-night") {
                    return Err(SinkError::Config("backup contains an invalid theme".into()));
                }
            }
            "sonux-global-shortcuts" => {
                let shortcuts: ShortcutBackupState = parse_json(key, value)?;
                let _ = shortcuts.enabled;
                for action in ["toggle_game", "toggle_chat", "toggle_mic", "restart_app"] {
                    if !shortcuts.bindings.contains_key(action) {
                        return Err(SinkError::Config(format!(
                            "backup is missing the {action} shortcut"
                        )));
                    }
                }
            }
            "sonux-active-eq-presets" => {
                let presets: BTreeMap<String, PresetSelection> = parse_json(key, value)?;
                if presets.values().any(|preset| {
                    !matches!(preset.source.as_str(), "bundled" | "user")
                        || preset.name.trim().is_empty()
                }) {
                    return Err(SinkError::Config(
                        "backup contains an invalid active EQ preset selection".into(),
                    ));
                }
            }
            "sonux-profile-section-visibility" => {
                let visibility: ProfileSectionVisibility = parse_json(key, value)?;
                let _ = (visibility.channels, visibility.applications);
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_managed_payload(
    files: &BTreeMap<String, String>,
    frontend_state: &BTreeMap<String, String>,
) -> Result<(), SinkError> {
    let profile_names = validate_profiles(files)?;
    for (relative, contents) in files {
        match relative.as_str() {
            "active_profile" => {
                let active = contents.trim();
                if !profile_names.iter().any(|name| name == active) {
                    return Err(SinkError::Config(format!(
                        "backup names a missing active profile: {active}"
                    )));
                }
            }
            "aliases.json" => {
                let _: crate::persistence::aliases::Aliases = parse_json(relative, contents)?;
            }
            "assignments.json" => {
                let _: crate::persistence::assignments::Assignments =
                    parse_json(relative, contents)?;
            }
            "buses.json" => {
                let _: crate::persistence::buses::Buses = parse_json(relative, contents)?;
            }
            "channels.json" => {
                let channels: crate::persistence::channels::Channels =
                    parse_json(relative, contents)?;
                validate_channel_names(
                    relative,
                    channels
                        .channels
                        .iter()
                        .map(|channel| channel.name.as_str()),
                )?;
            }
            "eq.json" => {
                let _: crate::persistence::eq::ChannelEq = parse_json(relative, contents)?;
            }
            "mic.json" => {
                let _: crate::audio::types::MicConfig = parse_json(relative, contents)?;
            }
            "outputs.json" => {
                let _: crate::persistence::outputs::ChannelOutputs =
                    parse_json(relative, contents)?;
            }
            "prefs.json" => {
                let _: crate::persistence::prefs::Prefs = parse_json(relative, contents)?;
            }
            "profile_automation.json" => {
                let config: crate::persistence::profile_automation::ProfileAutomationConfig =
                    parse_json(relative, contents)?;
                crate::persistence::profile_automation::validate_with_profiles(
                    &config,
                    &profile_names,
                )?;
            }
            "seen_apps.json" => {
                let _: crate::persistence::seen::SeenApps = parse_json(relative, contents)?;
            }
            "window.json" => {
                let _: crate::persistence::window::WindowSize = parse_json(relative, contents)?;
            }
            _ if profile_path_parts(relative).is_some() => {}
            _ if preset_path(relative, "eq_presets", 3) => {
                let preset: crate::audio::presets::EqPreset = parse_json(relative, contents)?;
                if preset.schema != crate::audio::presets::PRESET_SCHEMA || preset.bands.is_empty()
                {
                    return Err(SinkError::Config(format!(
                        "backup contains an invalid EQ preset: {relative}"
                    )));
                }
            }
            _ if preset_path(relative, "mic_presets", 2) => {
                let preset: crate::persistence::mic_presets::MicPreset =
                    parse_json(relative, contents)?;
                if preset.schema != crate::persistence::mic_presets::MIC_PRESET_SCHEMA
                    || preset.eq_bands.is_empty()
                {
                    return Err(SinkError::Config(format!(
                        "backup contains an invalid microphone preset: {relative}"
                    )));
                }
            }
            _ => {}
        }
    }
    validate_frontend_state(frontend_state)
}

pub fn read(path: &Path) -> Result<BackupFile, SinkError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_BACKUP_BYTES {
        return Err(SinkError::Config(
            "the selected backup is not a valid Sonux backup".into(),
        ));
    }
    let raw = fs::read_to_string(path)?;
    let backup: BackupFile = serde_json::from_str(&raw)
        .map_err(|error| SinkError::Config(format!("could not read backup: {error}")))?;
    if backup.schema != BACKUP_SCHEMA || backup.product != "Sonux" {
        return Err(SinkError::Config(
            "the selected file uses an unsupported backup format".into(),
        ));
    }
    for relative in backup.config_files.keys() {
        validate_relative_path(relative)?;
    }
    validate_managed_payload(&backup.config_files, &backup.frontend_state)?;
    Ok(backup)
}

impl BackupFile {
    pub fn assignments(&self) -> Result<crate::persistence::assignments::Assignments, SinkError> {
        self.config_files
            .get("assignments.json")
            .map(|contents| parse_json("assignments.json", contents))
            .transpose()
            .map(|assignments| assignments.unwrap_or_default())
    }
}

fn restore_files_to(root: &Path, files: &BTreeMap<String, String>) -> Result<(), SinkError> {
    let parent = root
        .parent()
        .ok_or_else(|| SinkError::Config("invalid configuration directory".into()))?;
    super::ensure_private_dir(parent)?;
    let token = format!("{}-{}", std::process::id(), super::unix_now());
    let staging = parent.join(format!(".sonux-restore-{token}"));
    let previous = parent.join(format!(".sonux-before-restore-{token}"));
    super::ensure_private_dir(&staging)?;

    let staged = (|| {
        for (relative, contents) in files {
            validate_relative_path(relative)?;
            let destination = staging.join(relative);
            if let Some(parent) = destination.parent() {
                super::ensure_private_dir(parent)?;
            }
            super::write_atomic(&destination, contents)?;
        }
        Ok::<(), SinkError>(())
    })();
    if let Err(error) = staged {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    if root.exists() {
        if let Err(error) = fs::rename(root, &previous) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error.into());
        }
    }
    if let Err(error) = fs::rename(&staging, root) {
        if previous.exists() {
            let _ = fs::rename(&previous, root);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(error.into());
    }
    if previous.exists() {
        let _ = fs::remove_dir_all(previous);
    }
    Ok(())
}

pub fn restore(backup: &BackupFile) -> Result<(), SinkError> {
    restore_files_to(&config_root()?, &backup.config_files)
}

fn status_from(dir: &Path) -> Result<BackupStatus, SinkError> {
    let mut created = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BackupStatus {
                count: 0,
                last_backup_at: None,
            });
        }
        Err(error) => return Err(error.into()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some(BACKUP_EXTENSION) {
            continue;
        }
        if let Ok(backup) = read(&path) {
            created.push(backup.created_at);
        }
    }
    Ok(BackupStatus {
        count: created.len(),
        last_backup_at: created.into_iter().max(),
    })
}

pub fn status() -> Result<BackupStatus, SinkError> {
    status_from(&backups_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sonux-backup-{label}-{}-{}",
            std::process::id(),
            super::super::unix_now()
        ))
    }

    fn valid_profile_json(name: &str) -> String {
        serde_json::to_string(&crate::persistence::profiles::Profile {
            name: name.into(),
            protected: true,
            channels: vec![crate::audio::types::VirtualSink {
                name: "sink_game".into(),
                label: "Game".into(),
                icon: Some("sports_esports".into()),
                volume_percent: 100,
                muted: false,
                stream_mix: true,
            }],
            mic: Some(crate::audio::types::MicConfig::default()),
            secondary_mics: Vec::new(),
            assignments: crate::persistence::assignments::Assignments::default(),
            outputs: crate::persistence::outputs::ChannelOutputs::default(),
            eq: crate::persistence::eq::ChannelEq::default(),
            trigger_device: None,
            buses: crate::persistence::buses::Buses::default(),
        })
        .unwrap()
    }

    fn valid_files() -> BTreeMap<String, String> {
        BTreeMap::from([("profiles/Main.json".into(), valid_profile_json("Main"))])
    }

    #[test]
    fn manual_and_recovery_backups_are_named_and_counted() {
        let root = test_dir("create");
        let config = root.join("config");
        let backups = root.join("backups");
        fs::create_dir_all(config.join("profiles")).unwrap();
        fs::write(config.join("prefs.json"), "{\"onboarded\":true}").unwrap();
        fs::write(
            config.join("profiles/Main.json"),
            valid_profile_json("Main"),
        )
        .unwrap();

        let manual = write_backup_to(
            &config,
            &backups,
            BackupKind::Manual,
            BTreeMap::from([("sonux-theme".into(), "original".into())]),
            true,
        )
        .unwrap();
        let recovery = write_backup_to(
            &config,
            &backups,
            BackupKind::AutomaticRecovery,
            BTreeMap::new(),
            false,
        )
        .unwrap();

        assert!(manual
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("Manual Backup"));
        assert!(recovery
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("Automatic Recovery Backup"));
        let status = status_from(&backups).unwrap();
        assert_eq!(status.count, 2);
        assert!(status.last_backup_at.is_some());
        let parsed = read(&manual).unwrap();
        assert!(parsed.autostart_enabled);
        assert_eq!(parsed.config_files.len(), 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restore_replaces_the_complete_config_tree() {
        let root = test_dir("restore");
        let config = root.join("sonux");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("old.json"), "old").unwrap();
        let files = BTreeMap::from([
            ("prefs.json".into(), "new".into()),
            ("profiles/Main.json".into(), "profile".into()),
        ]);

        restore_files_to(&config, &files).unwrap();

        assert!(!config.join("old.json").exists());
        assert_eq!(
            fs::read_to_string(config.join("prefs.json")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(config.join("profiles/Main.json")).unwrap(),
            "profile"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restore_rejects_parent_directory_paths() {
        assert!(validate_relative_path("../outside.json").is_err());
        assert!(validate_relative_path("profiles/Main.json").is_ok());
    }

    #[test]
    fn validation_rejects_malformed_managed_configuration() {
        let mut files = valid_files();
        files.insert("prefs.json".into(), "not json".into());
        let error = validate_managed_payload(&files, &BTreeMap::new()).unwrap_err();
        assert!(error.to_string().contains("malformed prefs.json"));
    }

    #[test]
    fn validation_rejects_profile_name_mismatch_and_bad_frontend_state() {
        let mut files =
            BTreeMap::from([("profiles/Gaming.json".into(), valid_profile_json("Main"))]);
        assert!(validate_managed_payload(&files, &BTreeMap::new()).is_err());

        files = valid_files();
        let frontend = BTreeMap::from([("sonux-theme".into(), "transparent".into())]);
        assert!(validate_managed_payload(&files, &frontend).is_err());
    }

    #[test]
    fn validation_rejects_duplicate_profile_device_triggers() {
        let mut main: crate::persistence::profiles::Profile =
            serde_json::from_str(&valid_profile_json("Main")).unwrap();
        main.trigger_device = Some("alsa_output.headset".into());
        let mut gaming = main.clone();
        gaming.name = "Gaming".into();
        let files = BTreeMap::from([
            (
                "profiles/Main.json".into(),
                serde_json::to_string(&main).unwrap(),
            ),
            (
                "profiles/Gaming.json".into(),
                serde_json::to_string(&gaming).unwrap(),
            ),
        ]);

        let error = validate_managed_payload(&files, &BTreeMap::new())
            .expect_err("duplicate trigger ownership must be rejected");
        assert!(error.to_string().contains("alsa_output.headset"));
    }

    #[test]
    fn validation_rejects_unsafe_profile_microphones() {
        let mut profile: crate::persistence::profiles::Profile =
            serde_json::from_str(&valid_profile_json("Main")).unwrap();
        let secondary = crate::audio::types::MicConfig {
            node_name: "alsa_input.not_owned".into(),
            ..Default::default()
        };
        profile.secondary_mics.push(secondary);
        let files = BTreeMap::from([(
            "profiles/Main.json".into(),
            serde_json::to_string(&profile).unwrap(),
        )]);

        let error = validate_managed_payload(&files, &BTreeMap::new())
            .expect_err("foreign microphone nodes must be rejected");
        assert!(error.to_string().contains("invalid profile"));
    }

    #[test]
    fn restored_assignments_come_from_the_backup_payload() {
        let mut files = valid_files();
        let mut assignments = crate::persistence::assignments::Assignments::default();
        assignments.set("application.name", "game", "sink_game");
        files.insert(
            "assignments.json".into(),
            serde_json::to_string(&assignments).unwrap(),
        );
        let backup = BackupFile {
            schema: BACKUP_SCHEMA,
            product: "Sonux".into(),
            kind: BackupKind::Manual,
            created_at: 1,
            config_files: files,
            frontend_state: BTreeMap::new(),
            autostart_enabled: false,
        };

        assert_eq!(
            backup
                .assignments()
                .unwrap()
                .sink_for("application.name", "game"),
            Some("sink_game")
        );
    }
}
