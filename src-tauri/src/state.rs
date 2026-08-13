use std::sync::{Arc, Mutex};

use crate::audio::backend::AudioBackend;
use crate::mixer::state::MixerState;

/// Application state managed by Tauri and shared across commands and the tray.
pub struct AppState {
    pub backend: Arc<dyn AudioBackend>,
    /// True when the native PipeWire backend is driving (vs pactl fallback).
    pub backend_native: bool,
    pub mixer: Mutex<MixerState>,
    /// Serializes profile application with delayed profile-scoped edits.
    /// Without this boundary, a fader IPC can mutate the graph halfway
    /// through a profile switch and autosave into the wrong profile.
    profile_operations: Mutex<()>,
    /// Single-use path selected by the trusted native backup dialog. The
    /// webview receives only a display name and cannot nominate a local file.
    backup_restore_grant: Mutex<Option<std::path::PathBuf>>,
}

impl AppState {
    /// Lock the mixer state, mapping poisoning to a command-friendly error.
    /// All command handlers go through this instead of hand-rolled map_errs.
    pub fn lock_mixer(&self) -> Result<std::sync::MutexGuard<'_, MixerState>, String> {
        self.mixer
            .lock()
            .map_err(|_| "mixer state lock poisoned".to_string())
    }

    pub fn lock_profile_operation(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.profile_operations
            .lock()
            .map_err(|_| "profile operation lock poisoned".to_string())
    }

    pub fn set_backup_restore_grant(&self, path: std::path::PathBuf) -> Result<(), String> {
        *self
            .backup_restore_grant
            .lock()
            .map_err(|_| "backup restore grant lock poisoned".to_string())? = Some(path);
        Ok(())
    }

    pub fn take_backup_restore_grant(&self) -> Result<std::path::PathBuf, String> {
        self.backup_restore_grant
            .lock()
            .map_err(|_| "backup restore grant lock poisoned".to_string())?
            .take()
            .ok_or_else(|| "choose a backup with the native file picker first".to_string())
    }

    pub fn clear_backup_restore_grant(&self) -> Result<(), String> {
        *self
            .backup_restore_grant
            .lock()
            .map_err(|_| "backup restore grant lock poisoned".to_string())? = None;
        Ok(())
    }

    pub fn lock_expected_profile_operation(
        &self,
        expected: Option<&str>,
    ) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        let operation = self.lock_profile_operation()?;
        self.ensure_expected_profile(expected)?;
        Ok(operation)
    }

    pub fn ensure_expected_profile(&self, expected: Option<&str>) -> Result<(), String> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let mixer = self.lock_mixer()?;
        if mixer.active_profile.as_deref() == Some(expected) {
            Ok(())
        } else {
            Err("profile changed before the pending edit could be applied".into())
        }
    }

    /// Reject anything that is not one of the channels in the active mixer
    /// definition. A `sink_` prefix is only a naming convention, not proof
    /// that the PipeWire node belongs to Sonux.
    pub fn ensure_known_channel(&self, sink_name: &str) -> Result<(), String> {
        let mixer = self.lock_mixer()?;
        mixer
            .channel_defs
            .channels
            .iter()
            .any(|channel| channel.name == sink_name)
            .then_some(())
            .ok_or_else(|| format!("unknown channel: {sink_name}"))
    }

    pub fn ensure_known_bus(&self, name: &str) -> Result<(), String> {
        let mixer = self.lock_mixer()?;
        mixer
            .buses
            .get(name)
            .is_some()
            .then_some(())
            .ok_or_else(|| format!("unknown mix: {name}"))
    }

    pub fn ensure_output_device(&self, name: &str) -> Result<(), String> {
        let devices = self
            .backend
            .list_output_devices()
            .map_err(|error| error.to_string())?;
        ensure_listed_device(&devices, name, "output")
    }

    pub fn ensure_input_device(&self, name: &str) -> Result<(), String> {
        let devices = self
            .backend
            .list_input_devices()
            .map_err(|error| error.to_string())?;
        ensure_listed_device(&devices, name, "input")
    }

    pub fn new(backend: Arc<dyn AudioBackend>, backend_native: bool) -> Self {
        // Saved assignments are loaded eagerly so auto-routing can enforce
        // them as soon as the sinks exist.
        let channel_defs = crate::persistence::channels::Channels::load();
        let buses = crate::persistence::buses::Buses::load(&channel_defs);
        let active_profile = crate::persistence::active::load();
        // Cache profile metadata once so autosave never has to re-read the
        // profile file merely to preserve fields it does not edit.
        let active_profile_data = active_profile
            .as_deref()
            .and_then(|name| crate::persistence::profiles::load(name).ok());
        let active_trigger = active_profile_data
            .as_ref()
            .and_then(|profile| profile.trigger_device.clone());
        let active_protected = active_profile_data
            .as_ref()
            .is_some_and(|profile| profile.protected);
        let secondary_mics = active_profile_data
            .as_ref()
            .map(|profile| profile.secondary_mics.clone())
            .unwrap_or_default();
        let now = crate::persistence::unix_now();
        let mut mixer = MixerState {
            assignments: crate::persistence::assignments::Assignments::load(),
            aliases: crate::persistence::aliases::Aliases::load(),
            outputs: crate::persistence::outputs::ChannelOutputs::load(),
            eq: crate::persistence::eq::ChannelEq::load(),
            mic: crate::persistence::mic::load(),
            secondary_mics,
            channel_defs,
            buses,
            seen: crate::persistence::seen::SeenApps::load(),
            active_profile,
            active_trigger,
            active_protected,
            prefs: crate::persistence::prefs::Prefs::load(),
            seen_saved_at: now,
            ..MixerState::default()
        };
        if mixer.prune_stale_apps(now) {
            if let Err(e) = mixer.seen.save() {
                eprintln!("sonux: pruning app history failed: {e}");
            }
        }
        Self {
            backend,
            backend_native,
            mixer: Mutex::new(mixer),
            profile_operations: Mutex::new(()),
            backup_restore_grant: Mutex::new(None),
        }
    }

    /// Best-effort teardown of all virtual sinks. Collects error messages
    /// instead of aborting on the first failure so a single bad unload
    /// doesn't leave the remaining sinks behind.
    pub fn teardown_virtual_sinks(&self) -> Vec<String> {
        let names: Vec<String> = self
            .mixer
            .lock()
            .map(|m| {
                m.channel_defs
                    .channels
                    .iter()
                    .map(|c| c.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut errors = Vec::new();
        for name in names {
            if let Err(e) = self.backend.destroy_virtual_sink(&name) {
                errors.push(format!("{name}: {e}"));
            }
        }
        if let Ok(mut mixer) = self.mixer.lock() {
            // Persist freshest last-seen timestamps on the way out (the
            // poll only writes on structural changes).
            let _ = mixer.seen.save();
            mixer.reset();
        }
        errors
    }
}

fn ensure_listed_device(
    devices: &[crate::audio::types::OutputDevice],
    name: &str,
    kind: &str,
) -> Result<(), String> {
    devices
        .iter()
        .any(|device| device.name == name)
        .then_some(())
        .ok_or_else(|| format!("unknown {kind} device: {name}"))
}

#[cfg(test)]
mod tests {
    use super::ensure_listed_device;
    use crate::audio::types::OutputDevice;

    #[test]
    fn explicit_devices_must_come_from_the_backend_listing() {
        let devices = vec![OutputDevice {
            index: 7,
            name: "alsa_output.deck".into(),
            description: "Deck speakers".into(),
        }];
        assert!(ensure_listed_device(&devices, "alsa_output.deck", "output").is_ok());
        assert!(ensure_listed_device(&devices, "sink_game", "output").is_err());
    }
}
