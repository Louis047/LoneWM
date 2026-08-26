use tracing::info;
use wm_common::{
  try_warn, FullscreenStateConfig, HideMethod, WindowState,
};
use wm_platform::{NativeWindow, NativeWindowWindowsExt};

use crate::{
  commands::window::update_window_state,
  traits::{CommonGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn handle_window_minimize_ended(
  native_window: &NativeWindow,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let found_window = state.window_from_native(native_window);

  // Update the window's state to not be minimized.
  if let Some(window) = found_window {
    let is_minimized = try_warn!(window.native().is_minimized());

    window.update_native_properties(|properties| {
      properties.is_minimized = is_minimized;
    });

    if !is_minimized && window.state() == WindowState::Minimized {
      info!("Window minimize ended: {window}");

      // Re-assert the taskbar button. `AddTab` can fail silently for
      // windows that were minimized while cloaked, leaving them
      // unreachable from the taskbar.
      //
      // See: <https://github.com/glzr-io/glazewm/issues/1358>
      if config.value.general.hide_method == HideMethod::Cloak
        && !config.value.general.show_all_in_taskbar
        && window
          .workspace()
          .is_some_and(|workspace| workspace.is_displayed())
      {
        let _ = window.native().set_taskbar_visibility(true);
      }

      let mut target_state = window
        .prev_state()
        .unwrap_or(WindowState::default_from_config(&config.value));

      // A window that reopens with a saved maximized placement shouldn't
      // re-enter the maximized state — otherwise it gets stuck in a
      // maximize/minimize cycle. Restore the default state instead so
      // that the window gets tiled.
      //
      // See: <https://github.com/glzr-io/glazewm/issues/1165>
      let is_recently_managed = state
        .managed_timestamps
        .get(&window.id())
        .is_some_and(|time| time.elapsed().as_millis() < 1000);

      if is_recently_managed
        && matches!(
          target_state,
          WindowState::Fullscreen(FullscreenStateConfig {
            maximized: true,
            ..
          })
        )
      {
        target_state = WindowState::default_from_config(&config.value);
      }

      update_window_state(window.clone(), target_state, state, config)?;
    }
  }

  Ok(())
}
