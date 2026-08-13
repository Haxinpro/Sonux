//! Named microphone-processing presets. Device selection, published name,
//! gain, mute and the chain's master enable state deliberately stay outside
//! the preset: presets describe the reusable gate/compressor/limiter sound.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::audio::types::{EqBand, MicConfig};
use crate::error::SinkError;

pub const MIC_PRESET_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicPreset {
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub eq_enabled: bool,
    #[serde(default)]
    pub eq_preamp_db: f32,
    #[serde(default = "default_mic_eq_bands")]
    pub eq_bands: Vec<EqBand>,
    pub gate_enabled: bool,
    pub gate_threshold_db: f32,
    pub comp_enabled: bool,
    pub comp_threshold_db: f32,
    pub comp_ratio: f32,
    pub limiter_enabled: bool,
    pub limiter_ceiling_db: f32,
}

impl MicPreset {
    pub fn from_config(name: String, config: &MicConfig) -> Self {
        Self {
            schema: MIC_PRESET_SCHEMA,
            name,
            eq_enabled: config.eq_enabled,
            eq_preamp_db: config.eq_preamp_db,
            eq_bands: config.eq_bands.clone(),
            gate_enabled: config.gate_enabled,
            gate_threshold_db: config.gate_threshold_db,
            comp_enabled: config.comp_enabled,
            comp_threshold_db: config.comp_threshold_db,
            comp_ratio: config.comp_ratio,
            limiter_enabled: config.limiter_enabled,
            limiter_ceiling_db: config.limiter_ceiling_db,
        }
    }
}

fn default_mic_eq_bands() -> Vec<EqBand> {
    MicConfig::default().eq_bands
}

fn presets_dir() -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("sonux").join("mic_presets"))
}

pub fn list() -> Result<Vec<MicPreset>, SinkError> {
    let dir = presets_dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut presets = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            match fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<MicPreset>(&raw).ok())
            {
                Some(preset) if preset.schema == MIC_PRESET_SCHEMA => presets.push(preset),
                _ => eprintln!("sonux: skipping malformed mic preset {}", path.display()),
            }
        }
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(presets)
}

pub fn save(name: &str, config: &MicConfig) -> Result<(), SinkError> {
    let name = super::profiles::sanitize_name(name)?;
    let dir = presets_dir()?;
    super::ensure_private_dir(&dir)?;
    let preset = MicPreset::from_config(name.clone(), config);
    let json = serde_json::to_string_pretty(&preset)
        .map_err(|error| SinkError::Config(format!("serialize mic preset: {error}")))?;
    super::write_atomic(&dir.join(format!("{name}.json")), json).map_err(SinkError::from)
}

pub fn delete(name: &str) -> Result<(), SinkError> {
    let name = super::profiles::sanitize_name(name)?;
    let path = presets_dir()?.join(format!("{name}.json"));
    fs::remove_file(path).map_err(SinkError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_excludes_hardware_and_live_controls() {
        let config = MicConfig {
            node_name: "sink_mic".into(),
            enabled: true,
            input_device: Some("hardware".into()),
            output_label: "Voice".into(),
            gain_percent: 175,
            muted: true,
            eq_enabled: true,
            eq_preamp_db: -2.0,
            eq_bands: MicConfig::default().eq_bands,
            gate_enabled: true,
            gate_threshold_db: -35.0,
            comp_enabled: true,
            comp_threshold_db: -20.0,
            comp_ratio: 4.0,
            limiter_enabled: true,
            limiter_ceiling_db: -2.0,
        };
        let preset = MicPreset::from_config("Speech".into(), &config);
        assert_eq!(preset.name, "Speech");
        assert!(preset.eq_enabled);
        assert_eq!(preset.eq_preamp_db, -2.0);
        assert_eq!(preset.gate_threshold_db, -35.0);
        assert_eq!(preset.comp_ratio, 4.0);
        assert_eq!(preset.limiter_ceiling_db, -2.0);
    }
}
