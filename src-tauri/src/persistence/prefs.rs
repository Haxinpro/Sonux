use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// How Sink's devices are labeled in other apps' device lists.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceLabelStyle {
    /// "Game"
    #[default]
    Plain,
    /// "Game (Sonux)"
    Suffix,
    /// "Sonux · Game"
    Prefix,
}

/// Visual refresh policy for the mixer VU meters. This never changes audio
/// processing or routing; it only controls frontend rendering work.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MeterMode {
    Monitor,
    #[serde(rename = "fps_144")]
    Fps144,
    #[serde(rename = "fps_120")]
    Fps120,
    #[serde(rename = "fps_100")]
    Fps100,
    #[default]
    #[serde(
        rename = "fps_60",
        alias = "high",
        alias = "balanced",
        alias = "low_power"
    )]
    Fps60,
    Off,
}

/// App preferences, stored at `$XDG_CONFIG_HOME/sonux/prefs.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub device_label_style: DeviceLabelStyle,
    /// Live meter animation rate. Every enabled rate sleeps at silence.
    #[serde(default)]
    pub meter_mode: MeterMode,
    /// First-run tutorial completed (false = show it on launch).
    #[serde(default)]
    pub onboarded: bool,
    /// ChatMix-style balance: the two channel sink names being balanced
    /// (None = auto: Game/Chat when present, else the first two channels).
    #[serde(default)]
    pub balance_a: Option<String>,
    #[serde(default)]
    pub balance_b: Option<String>,
    /// Show the balance slider in the title bar.
    #[serde(default = "default_true")]
    pub show_balance: bool,
    /// When autostarting on login, boot straight to the tray instead of
    /// showing the window (only meaningful with autostart enabled).
    #[serde(default)]
    pub start_minimized: bool,
    /// Advanced opt-in: allow profiles to publish secondary processed mics.
    #[serde(default)]
    pub multiple_mics: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            device_label_style: DeviceLabelStyle::default(),
            meter_mode: MeterMode::default(),
            onboarded: false,
            balance_a: None,
            balance_b: None,
            show_balance: true,
            start_minimized: false,
            multiple_mics: false,
        }
    }
}

impl Prefs {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = dirs::config_dir()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sonux").join("prefs.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        fs::read_to_string(&path)
            .map(|raw| Self::parse(&raw))
            .unwrap_or_default()
    }

    /// Parse stored prefs; malformed input degrades to defaults rather
    /// than blocking launch.
    fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_else(|e| {
            eprintln!("sonux: ignoring malformed prefs: {e}");
            Self::default()
        })
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize prefs: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    /// Decorate a device label per the chosen style (applied at node
    /// creation; stored labels stay raw).
    pub fn decorate(&self, label: &str) -> String {
        match self.device_label_style {
            DeviceLabelStyle::Plain => label.to_string(),
            DeviceLabelStyle::Suffix => format!("{label} (Sonux)"),
            DeviceLabelStyle::Prefix => format!("Sonux · {label}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorate_styles() {
        let mut p = Prefs::default();
        assert_eq!(p.decorate("Game"), "Game");
        p.device_label_style = DeviceLabelStyle::Suffix;
        assert_eq!(p.decorate("Game"), "Game (Sonux)");
        p.device_label_style = DeviceLabelStyle::Prefix;
        assert_eq!(p.decorate("Game"), "Sonux · Game");
    }

    #[test]
    fn malformed_prefs_degrade_to_defaults() {
        // Corrupt / partially-written files must never panic or block
        // launch - they fall back to defaults.
        assert_eq!(Prefs::parse(""), Prefs::default());
        assert_eq!(Prefs::parse("{not json"), Prefs::default());
        assert_eq!(Prefs::parse("[]"), Prefs::default());
        assert_eq!(
            Prefs::parse(r#"{"device_label_style":"bogus_style"}"#),
            Prefs::default()
        );
        // Unknown fields are tolerated; known fields still apply.
        let p = Prefs::parse(r#"{"device_label_style":"suffix","future_field":1}"#);
        assert_eq!(p.device_label_style, DeviceLabelStyle::Suffix);
        assert_eq!(p.meter_mode, MeterMode::Fps60);
    }

    #[test]
    fn meter_mode_round_trips_and_old_values_migrate_to_60_fps() {
        let old = Prefs::parse(r#"{"onboarded":true}"#);
        assert_eq!(old.meter_mode, MeterMode::Fps60);

        let low = Prefs::parse(r#"{"meter_mode":"low_power"}"#);
        assert_eq!(low.meter_mode, MeterMode::Fps60);
        let high = Prefs::parse(r#"{"meter_mode":"high"}"#);
        assert_eq!(high.meter_mode, MeterMode::Fps60);
        let balanced = Prefs::parse(r#"{"meter_mode":"balanced"}"#);
        assert_eq!(balanced.meter_mode, MeterMode::Fps60);

        let monitor = Prefs::parse(r#"{"meter_mode":"monitor"}"#);
        assert_eq!(monitor.meter_mode, MeterMode::Monitor);
        let capped = Prefs::parse(r#"{"meter_mode":"fps_144"}"#);
        assert_eq!(capped.meter_mode, MeterMode::Fps144);

        let serialized = [
            (MeterMode::Monitor, r#""monitor""#),
            (MeterMode::Fps144, r#""fps_144""#),
            (MeterMode::Fps120, r#""fps_120""#),
            (MeterMode::Fps100, r#""fps_100""#),
            (MeterMode::Fps60, r#""fps_60""#),
            (MeterMode::Off, r#""off""#),
        ];
        for (mode, json) in serialized {
            assert_eq!(serde_json::to_string(&mode).unwrap(), json);
            assert_eq!(serde_json::from_str::<MeterMode>(json).unwrap(), mode);
        }
    }
}
