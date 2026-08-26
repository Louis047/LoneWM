use tracing::info;
use wm_common::{try_warn, WindowState};
use wm_platform::NativeWindow;

use crate::{
  commands::{
    container::set_focused_descendant, window::update_window_state,
  },
  traits::{CommonGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn handle_window_minimized(
  native_window: &NativeWindow,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let found_window = state.window_from_native(native_window);

  // Update the window's state to be minimized.
  if let Some(window) = found_window {
    // Ignore minimize events for windows that were just repositioned by
    // the WM. Restoring and moving a window across monitors can
    // transiently produce a minimize event, which would otherwise leave
    // the window stuck minimized.
    //
    // See: <https://github.com/glzr-io/glazewm/issues/1259>
    let is_recent_wm_reposition = state
      .wm_set_frames
      .get(&window.id())
      .is_some_and(|(_, time)| time.elapsed().as_millis() < 500);

    // The minimize event is treated as authoritative. For some windows
    // (e.g. Electron apps), a synchronous `IsIconic` check at the start
    // of the minimize can still return false, which would leave the
    // window as a focus target even though it's minimized.
    //
    // See: <https://github.com/glzr-io/glazewm/issues/1115>
    if is_recent_wm_reposition {
      let is_minimized = try_warn!(window.native().is_minimized());

      if !is_minimized {
        return Ok(());
      }
    }

    window.update_native_properties(|properties| {
      properties.is_minimized = true;
    });

    if window.state() != WindowState::Minimized {
      info!("Window minimized: {window}");

      let window = update_window_state(
        window.clone(),
        WindowState::Minimized,
        state,
        config,
      )?;

      // Clear the drag state, as a window can be minimized while
      // being dragged (e.g. via `toggle-minimized`).
      // TODO: Investigate other code paths where the drag state should be
      // cleared (e.g. most commands that call `update_window_state`).
      window.set_active_drag(None);

      // Focus should be reassigned after a window has been minimized.
      if let Some(focus_target) = state.focus_target_after_removal(&window)
      {
        set_focused_descendant(&focus_target, None);
        state.pending_sync.queue_focus_change().queue_cursor_jump();
        state.unmanaged_or_minimized_timestamp =
          Some(std::time::Instant::now());
      }
    }
  }

  Ok(())
}
