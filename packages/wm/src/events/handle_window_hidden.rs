use tracing::info;
use wm_common::{DisplayState, HideMethod};
use wm_platform::{NativeWindow, NativeWindowWindowsExt};

use crate::{
  commands::window::unmanage_window, traits::WindowGetters,
  user_config::UserConfig, wm_state::WmState,
};

pub fn handle_window_hidden(
  native_window: &NativeWindow,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let found_window = state.window_from_native(native_window);

  if let Some(window) = found_window {
    info!("Window hidden: {window}");

    // Update the display state.
    if config.value.general.hide_method != HideMethod::PlaceInCorner
      && window.display_state() == DisplayState::Hiding
    {
      window.set_display_state(DisplayState::Hidden);
      return Ok(());
    }

    // Unmanage the window if it's not in a display state transition.
    //
    // The visibility check is based on the `WS_VISIBLE` style rather than
    // `NativeWindow::is_visible`, which also treats DWM-cloaked windows
    // as hidden. The shell cloaks UWP and Electron apps when they're
    // minimized or suspended — unmanaging a window in that state removes
    // it from the tree (leaking it through workspaces) and can leave it
    // permanently cloaked. Minimized windows also keep `WS_VISIBLE`, so
    // they're likewise skipped here.
    //
    // See: <https://github.com/glzr-io/glazewm/issues/1350>
    // See: <https://github.com/glzr-io/glazewm/issues/992>
    if config.value.general.hide_method == HideMethod::PlaceInCorner
      || window.display_state() == DisplayState::Shown
    {
      let is_actually_hidden = !window.native().is_shown().unwrap_or(true);

      if is_actually_hidden {
        unmanage_window(window, state)?;
      }
    }
  }

  Ok(())
}
