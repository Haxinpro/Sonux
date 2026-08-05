use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

pub const MIN_WIDTH: u32 = 960;
pub const MIN_HEIGHT: u32 = 600;
const MAX_WIDTH: u32 = 7680;
const MAX_HEIGHT: u32 = 4320;

/// Last normal (non-maximized) window size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    pub fn sanitized(self) -> Self {
        Self {
            width: self.width.clamp(MIN_WIDTH, MAX_WIDTH),
            height: self.height.clamp(MIN_HEIGHT, MAX_HEIGHT),
        }
    }
}

fn path() -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("sonux").join("window.json"))
}

pub fn load() -> Option<WindowSize> {
    let path = path().ok()?;
    let raw = fs::read_to_string(&path).ok()?;
    serde_json::from_str::<WindowSize>(&raw)
        .map(WindowSize::sanitized)
        .map_err(|e| eprintln!("sonux: ignoring malformed {}: {e}", path.display()))
        .ok()
}

pub fn save(size: WindowSize) -> Result<(), SinkError> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        super::ensure_private_dir(parent)?;
    }
    let json = serde_json::to_string_pretty(&size.sanitized())
        .map_err(|e| SinkError::Config(format!("serialize window size: {e}")))?;
    super::write_atomic(&path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_corrupt_or_stale_extreme_sizes() {
        assert_eq!(
            WindowSize {
                width: 1,
                height: 99
            }
            .sanitized(),
            WindowSize {
                width: MIN_WIDTH,
                height: MIN_HEIGHT
            }
        );
        assert_eq!(
            WindowSize {
                width: u32::MAX,
                height: u32::MAX
            }
            .sanitized(),
            WindowSize {
                width: MAX_WIDTH,
                height: MAX_HEIGHT
            }
        );
    }
}
