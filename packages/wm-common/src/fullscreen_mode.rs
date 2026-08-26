use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Mode for fullscreening a window.
///
/// - `Full`: Takes over the entire monitor screen, covering taskbar and
///   gaps.
/// - `Monocle`: Expands to fill the workspace work area, keeping the
///   taskbar and outer gaps visible.
#[derive(
  Clone,
  Copy,
  Debug,
  Default,
  Deserialize,
  Eq,
  PartialEq,
  Serialize,
  ValueEnum,
)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum FullscreenMode {
  /// Takes over the entire monitor screen, covering taskbar and gaps.
  #[default]
  Full,

  /// Expands to fill the workspace work area, keeping the taskbar and
  /// outer gaps visible.
  Monocle,
}
