use wm_common::WmEvent;
use wm_platform::{CornerStyle, NativeWindowWindowsExt, OpacityValue};

use crate::{traits::WindowGetters, wm_state::WmState};

/// Pauses or unpauses the WM.
pub fn toggle_pause(state: &mut WmState) {
  let is_paused = !state.is_paused;
  state.is_paused = is_paused;

  if is_paused {
    reset_window_effects(state);
  } else {
    // Redraw full container tree on unpause.
    state
      .pending_sync
      .queue_container_to_redraw(state.root_container.clone());

    state.pending_sync.queue_all_effects_update();
  }

  state.emit_event(WmEvent::PauseChanged { is_paused });
}

/// Resets any applied window effects (e.g. transparency, title bar,
/// corners) when pausing. Otherwise, effects would remain frozen on
/// windows for as long as the WM is paused, since `platform_sync` is
/// skipped while paused.
///
/// See: <https://github.com/glzr-io/glazewm/issues/958>
fn reset_window_effects(state: &mut WmState) {
  for window in state.windows() {
    let _ = window.native().set_title_bar_visibility(true);
    let _ = window.native().set_corner_style(&CornerStyle::Default);
    let _ = window
      .native()
      .set_transparency(&OpacityValue::from_alpha(u8::MAX));
  }

  if let Ok(mut cache) = state.corner_stamp_cache.lock() {
    cache.clear();
  }

  state.prev_effects_window = None;
}
