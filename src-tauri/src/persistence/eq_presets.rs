//! Per-channel user processing presets: named JSON files (the same schema
//! as the bundled presets) under
//! `$XDG_CONFIG_HOME/sonux/eq_presets/<channel>/`. Both the channel and preset
//! name pass through the profiles store's sanitizer, so neither can traverse
//! outside the library.

use std::fs;
use std::path::PathBuf;

use crate::audio::presets::{EqPreset, PRESET_SCHEMA};
use crate::error::SinkError;
use crate::persistence::profiles::sanitize_name;

fn presets_dir() -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("sonux").join("eq_presets"))
}

fn channel_presets_dir(sink_name: &str) -> Result<PathBuf, SinkError> {
    let channel = sanitize_name(sink_name)?;
    Ok(presets_dir()?.join(channel))
}

/// All user presets, sorted by name. Unreadable files are skipped (one
/// corrupt preset must not hide the rest).
pub fn list(sink_name: &str) -> Result<Vec<EqPreset>, SinkError> {
    let dir = channel_presets_dir(sink_name)?;
    let mut presets = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(presets), // no library yet
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|raw| serde_json::from_str::<EqPreset>(&raw).map_err(|e| e.to_string()))
        {
            Ok(preset) if preset.schema == PRESET_SCHEMA && !preset.bands.is_empty() => {
                presets.push(preset);
            }
            Ok(_) => eprintln!("sonux: skipping eq preset {}: bad schema", path.display()),
            Err(e) => eprintln!("sonux: skipping eq preset {}: {e}", path.display()),
        }
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(presets)
}

pub fn save(sink_name: &str, preset: &EqPreset) -> Result<(), SinkError> {
    let name = sanitize_name(&preset.name)?;
    if preset.bands.is_empty() {
        return Err(SinkError::Config("a preset needs at least one band".into()));
    }
    let dir = channel_presets_dir(sink_name)?;
    crate::persistence::ensure_private_dir(&dir)?;
    let json = serde_json::to_string_pretty(preset)
        .map_err(|e| SinkError::Config(format!("serialize eq preset: {e}")))?;
    super::write_atomic(&dir.join(format!("{name}.json")), &json)?;
    Ok(())
}

pub fn delete(sink_name: &str, name: &str) -> Result<(), SinkError> {
    let name = sanitize_name(name)?;
    let path = channel_presets_dir(sink_name)?.join(format!("{name}.json"));
    match super::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // idempotent
        Err(e) => Err(SinkError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_names_are_rejected() {
        // sanitize_name (shared with profiles) is the barrier; pin that it
        // holds for the preset entry points too.
        assert!(delete("sink_game", "../etc/passwd").is_err());
        assert!(delete("../sink_game", "Safe").is_err());
        let preset = EqPreset {
            schema: PRESET_SCHEMA,
            name: "../escape".into(),
            author: None,
            description: None,
            preamp_db: 0.0,
            bands: crate::audio::types::default_eq_bands(),
            ..EqPreset::from_config("defaults".into(), crate::audio::types::EqConfig::default())
        };
        assert!(save("sink_game", &preset).is_err());
    }

    #[test]
    fn empty_band_presets_are_rejected() {
        let preset = EqPreset {
            schema: PRESET_SCHEMA,
            name: "Empty".into(),
            author: None,
            description: None,
            preamp_db: 0.0,
            bands: Vec::new(),
            ..EqPreset::from_config("defaults".into(), crate::audio::types::EqConfig::default())
        };
        assert!(save("sink_game", &preset).is_err());
    }
}
