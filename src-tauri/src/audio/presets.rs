//! Community EQ presets: a versioned JSON schema, with the "community
//! approved" set living in the repo's `presets/eq/` directory and embedded
//! into the binary at build time (see build.rs).

use serde::{Deserialize, Serialize};

use crate::audio::types::{EqBand, EqConfig, PlaybackMode};

fn default_gate_threshold_db() -> f32 {
    -48.0
}

fn default_comp_threshold_db() -> f32 {
    -18.0
}

fn default_comp_ratio() -> f32 {
    3.0
}

fn default_limiter_ceiling_db() -> f32 {
    -1.0
}

fn default_spatial_tuning() -> f32 {
    0.5
}

fn default_spatial_distance() -> f32 {
    0.5
}

fn default_enabled() -> bool {
    true
}

/// The shareable channel-processing preset format. Older schema-1 files did
/// not contain `enabled`; they retain the historical apply-and-enable
/// behaviour through `default_enabled`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    pub schema: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub preamp_db: f32,
    pub bands: Vec<EqBand>,
    #[serde(default)]
    pub tone_bass_db: f32,
    #[serde(default)]
    pub tone_voice_db: f32,
    #[serde(default)]
    pub tone_treble_db: f32,
    #[serde(default)]
    pub boost_db: f32,
    #[serde(default)]
    pub gate_enabled: bool,
    #[serde(default = "default_gate_threshold_db")]
    pub gate_threshold_db: f32,
    #[serde(default)]
    pub comp_enabled: bool,
    #[serde(default = "default_comp_threshold_db")]
    pub comp_threshold_db: f32,
    #[serde(default = "default_comp_ratio")]
    pub comp_ratio: f32,
    #[serde(default)]
    pub limiter_enabled: bool,
    #[serde(default = "default_limiter_ceiling_db")]
    pub limiter_ceiling_db: f32,
    #[serde(default)]
    pub playback_mode: PlaybackMode,
    #[serde(default)]
    pub spatial_enabled: bool,
    #[serde(default = "default_spatial_tuning")]
    pub spatial_tuning: f32,
    #[serde(default = "default_spatial_distance")]
    pub spatial_distance: f32,
}

pub const PRESET_SCHEMA: u32 = 1;

impl EqPreset {
    /// The channel config this preset applies (enabled, clamped).
    pub fn to_config(&self) -> EqConfig {
        let mut config = EqConfig {
            enabled: self.enabled,
            preamp_db: self.preamp_db,
            bands: self.bands.clone(),
            tone_bass_db: self.tone_bass_db,
            tone_voice_db: self.tone_voice_db,
            tone_treble_db: self.tone_treble_db,
            boost_db: self.boost_db,
            gate_enabled: self.gate_enabled,
            gate_threshold_db: self.gate_threshold_db,
            comp_enabled: self.comp_enabled,
            comp_threshold_db: self.comp_threshold_db,
            comp_ratio: self.comp_ratio,
            limiter_enabled: self.limiter_enabled,
            limiter_ceiling_db: self.limiter_ceiling_db,
            playback_mode: self.playback_mode,
            spatial_enabled: self.spatial_enabled,
            spatial_tuning: self.spatial_tuning,
            spatial_distance: self.spatial_distance,
        };
        config.clamp_ranges();
        config
    }

    pub fn from_config(name: String, config: EqConfig) -> Self {
        Self {
            schema: PRESET_SCHEMA,
            name,
            author: None,
            description: None,
            enabled: config.enabled,
            preamp_db: config.preamp_db,
            bands: config.bands,
            tone_bass_db: config.tone_bass_db,
            tone_voice_db: config.tone_voice_db,
            tone_treble_db: config.tone_treble_db,
            boost_db: config.boost_db,
            gate_enabled: config.gate_enabled,
            gate_threshold_db: config.gate_threshold_db,
            comp_enabled: config.comp_enabled,
            comp_threshold_db: config.comp_threshold_db,
            comp_ratio: config.comp_ratio,
            limiter_enabled: config.limiter_enabled,
            limiter_ceiling_db: config.limiter_ceiling_db,
            playback_mode: config.playback_mode,
            spatial_enabled: config.spatial_enabled,
            spatial_tuning: config.spatial_tuning,
            spatial_distance: config.spatial_distance,
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/eq_presets_generated.rs"));

/// Parse one bundled source, or explain why it's unusable.
fn parse_bundled(stem: &str, raw: &str) -> Result<EqPreset, String> {
    let preset: EqPreset = serde_json::from_str(raw).map_err(|e| format!("preset {stem}: {e}"))?;
    if preset.schema != PRESET_SCHEMA {
        return Err(format!(
            "preset {stem}: unsupported schema {}",
            preset.schema
        ));
    }
    if preset.bands.is_empty() {
        return Err(format!("preset {stem}: no bands"));
    }
    Ok(preset)
}

/// All bundled presets, sorted by name. Malformed entries are logged and
/// skipped - defense in depth even though these ship inside the binary
/// (a bad community PR must degrade one preset, not the whole menu).
pub fn bundled_presets() -> Vec<EqPreset> {
    let mut presets: Vec<EqPreset> = BUNDLED_EQ_PRESET_SOURCES
        .iter()
        .filter_map(|(stem, raw)| match parse_bundled(stem, raw) {
            Ok(preset) => Some(preset),
            Err(e) => {
                eprintln!("sonux: skipping bundled eq {e}");
                None
            }
        })
        .collect();
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    presets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::types::EqBandKind;

    #[test]
    fn bundled_presets_parse_and_sort_by_name() {
        let presets = bundled_presets();
        assert!(
            presets.iter().any(|p| p.name == "Flat"),
            "the Flat reference preset must ship"
        );
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        for p in &presets {
            assert_eq!(p.schema, PRESET_SCHEMA);
            assert!(!p.bands.is_empty());
        }
    }

    #[test]
    fn flat_preset_is_numerically_flat() {
        let presets = bundled_presets();
        let flat = presets.iter().find(|p| p.name == "Flat").expect("shipped");
        assert_eq!(flat.preamp_db, 0.0);
        assert!(flat.bands.iter().all(|b| b.gain_db == 0.0));
    }

    #[test]
    fn malformed_bundled_entry_is_skipped_not_panicked() {
        assert!(parse_bundled("bad", "{not json").is_err());
        assert!(parse_bundled("bad", r#"{"schema":2,"name":"x","bands":[]}"#).is_err());
        assert!(
            parse_bundled("bad", r#"{"schema":1,"name":"x","bands":[]}"#).is_err(),
            "zero bands is rejected"
        );
    }

    #[test]
    fn to_config_preserves_enabled_and_clamps() {
        let preset = EqPreset {
            schema: 1,
            name: "Hot".into(),
            author: None,
            description: None,
            enabled: true,
            preamp_db: -99.0,
            bands: vec![EqBand {
                kind: EqBandKind::Peaking,
                freq_hz: 90000.0,
                gain_db: 4.0,
                q: 1.0,
            }],
            ..EqPreset::from_config("defaults".into(), EqConfig::default())
        };
        let config = preset.to_config();
        assert!(config.enabled);
        assert_eq!(config.preamp_db, -24.0);
        assert_eq!(config.bands[0].freq_hz, 20000.0);
    }

    #[test]
    fn channel_preset_round_trips_every_processing_setting() {
        let config = EqConfig {
            enabled: true,
            preamp_db: -3.5,
            tone_bass_db: 1.5,
            tone_voice_db: -2.0,
            tone_treble_db: 2.5,
            boost_db: 3.0,
            gate_enabled: true,
            gate_threshold_db: -42.0,
            comp_enabled: true,
            comp_threshold_db: -20.0,
            comp_ratio: 4.0,
            limiter_enabled: true,
            limiter_ceiling_db: -2.0,
            playback_mode: PlaybackMode::Speakers,
            spatial_enabled: true,
            spatial_tuning: 0.72,
            spatial_distance: 0.31,
            ..EqConfig::default()
        };

        let restored = EqPreset::from_config("Complete".into(), config.clone()).to_config();
        assert_eq!(restored, config);
    }

    #[test]
    fn legacy_preset_without_enabled_still_enables_on_apply() {
        let raw = r#"{
            "schema": 1,
            "name": "Legacy",
            "preamp_db": 0.0,
            "bands": [{"kind":"peaking","freq_hz":1000.0,"gain_db":0.0,"q":1.0}]
        }"#;
        let preset: EqPreset = serde_json::from_str(raw).unwrap();
        assert!(preset.enabled);
        let config = preset.to_config();
        assert!(config.enabled);
        assert_eq!(config.tone_bass_db, 0.0);
        assert_eq!(config.tone_voice_db, 0.0);
        assert_eq!(config.tone_treble_db, 0.0);
    }
}
