use serde::{Deserialize, Serialize};

use crate::{
  parsed_config::{
    FloatingStateConfig, FullscreenStateConfig, InitialWindowState,
  },
  ParsedConfig,
};

/// Represents the possible states a window can have.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WindowState {
  Floating(FloatingStateConfig),
  Fullscreen(FullscreenStateConfig),
  Minimized,
  Tiling,
}

impl WindowState {
  #[must_use]
  pub fn default_from_config(config: &ParsedConfig) -> Self {
    match config.window_behavior.initial_state {
      InitialWindowState::Tiling => Self::Tiling,
      InitialWindowState::Floating => Self::Floating(
        config.window_behavior.state_defaults.floating.clone(),
      ),
    }
  }

  #[must_use]
  pub fn is_same_state(&self, other: &Self) -> bool {
    match (self, other) {
      (Self::Fullscreen(a), Self::Fullscreen(b)) => {
        a.effective_mode() == b.effective_mode()
      }
      (Self::Floating(_), Self::Floating(_)) => true,
      _ => std::mem::discriminant(self) == std::mem::discriminant(other),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::fullscreen_mode::FullscreenMode;

  #[test]
  fn test_window_state_is_same_state() {
    let full = WindowState::Fullscreen(FullscreenStateConfig {
      mode: FullscreenMode::Full,
      maximized: false,
      shown_on_top: false,
      respect_gaps: true,
    });

    let monocle = WindowState::Fullscreen(FullscreenStateConfig {
      mode: FullscreenMode::Monocle,
      maximized: false,
      shown_on_top: false,
      respect_gaps: true,
    });

    assert!(full.is_same_state(&full));
    assert!(monocle.is_same_state(&monocle));
    assert!(!full.is_same_state(&monocle));

    let float1 = WindowState::Floating(FloatingStateConfig {
      centered: false,
      shown_on_top: false,
    });
    let float2 = WindowState::Floating(FloatingStateConfig {
      centered: true,
      shown_on_top: true,
    });
    assert!(float1.is_same_state(&float2));

    assert!(!full.is_same_state(&WindowState::Tiling));
    assert!(WindowState::Tiling.is_same_state(&WindowState::Tiling));
  }
}
