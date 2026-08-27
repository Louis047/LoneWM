use serde::{Deserialize, Serialize};

/// Unified corner radius for windows (via DWM
/// `DWMWA_WINDOW_CORNER_PREFERENCE`).
///
/// `Auto` uses the OS default (rounded on Windows 11, square on 10).
/// Named presets map directly to DWM constants. The `Px` variant
/// picks the closest DWM preset for the window.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerRadius {
  /// Use Windows DWM default (rounded on Win11, square on Win10).
  #[default]
  Auto,
  /// Force square corners (`DWMWCP_DONOTROUND`).
  Square,
  /// Standard rounded (~8px, `DWMWCP_ROUND`).
  Round,
  /// Small rounded (~4px, `DWMWCP_ROUNDSMALL`).
  SmallRound,
  /// Specific pixel radius — DWM is set to the closest preset.
  #[serde(untagged)]
  Px(u32),
}
