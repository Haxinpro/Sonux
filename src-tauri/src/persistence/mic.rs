use std::fs;
use std::path::PathBuf;

use crate::audio::types::MicConfig;
use crate::error::SinkError;

/// Mic chain configuration, stored as JSON at
/// `$XDG_CONFIG_HOME/sonux/mic.json`.
pub fn config_path() -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("sonux").join("mic.json"))
}

pub fn load() -> MicConfig {
    let Ok(path) = config_path() else {
        return MicConfig::default();
    };
    match fs::read_to_string(&path) {
        Ok(raw) => parse(&raw).unwrap_or_else(|e| {
            eprintln!("sonux: ignoring invalid {}: {e}", path.display());
            MicConfig::default()
        }),
        Err(_) => MicConfig::default(),
    }
}

pub fn save(config: &MicConfig) -> Result<(), SinkError> {
    let mut config = config.clone();
    normalize_and_validate(&mut config)?;
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        crate::persistence::ensure_private_dir(parent)?;
    }
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| SinkError::Config(format!("serialize mic config: {e}")))?;
    super::write_atomic(&path, &json)?;
    Ok(())
}

pub(crate) fn parse(raw: &str) -> Result<MicConfig, SinkError> {
    let mut config: MicConfig = serde_json::from_str(raw)
        .map_err(|error| SinkError::Config(format!("malformed microphone config: {error}")))?;
    normalize_and_validate(&mut config)?;
    Ok(config)
}

pub(crate) fn normalize_and_validate(config: &mut MicConfig) -> Result<(), SinkError> {
    if config.node_name != "sink_mic" {
        return Err(SinkError::Config(
            "the global microphone must use the sink_mic node".into(),
        ));
    }
    let label = config.output_label.trim();
    if label.is_empty() || label.len() > 64 {
        return Err(SinkError::Config(
            "microphone label must be 1-64 characters".into(),
        ));
    }
    config.output_label = label.to_string();
    config.input_device = config.input_device.take().filter(|name| !name.is_empty());
    config.clamp_ranges();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_mic_rejects_foreign_nodes_and_clamps_dsp() {
        let mut config = MicConfig {
            node_name: "source_mic_other".into(),
            ..Default::default()
        };
        assert!(normalize_and_validate(&mut config).is_err());
        config.node_name = "sink_mic".into();
        config.gain_percent = u8::MAX;
        config.comp_ratio = f32::INFINITY;
        normalize_and_validate(&mut config).unwrap();
        assert_eq!(config.gain_percent, 200);
        assert_eq!(config.comp_ratio, 3.0);
    }
}
