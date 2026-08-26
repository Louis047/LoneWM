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

/// Resets any applied window effects (e.g. focused border, transparency)
/// when pausing. Otherwise, effects would remain frozen on windows for as
/// long as the WM is paused, since `platform_sync` is skipped while
/// paused.
///
/// See: <https://github.com/glzr-io/glazewm/issues/958>
fn reset_window_effects(state: &mut WmState) {
  for window in state.windows() {
    // Only call `set_border_color` when the cached color differs from
    // `None` — re-applying the same `DWMWA_BORDER_COLOR` forces DWM
    // to re-evaluate the frame and visibly flickers.
    let handle = window.native().id().0;
    let needs_reset = state
      .border_stamp_cache
      .lock()
      .ok()
      .and_then(|cache| cache.get(&handle).cloned())
      .is_some_and(|stamp| stamp.color.is_some());

    if needs_reset {
      let _ = window.native().set_border_color(None);
    }

    if let Ok(mut cache) = state.border_stamp_cache.lock() {
      cache.remove(&handle);
    }
    let _ = window.native().set_title_bar_visibility(true);
    let _ = window.native().set_corner_style(&CornerStyle::Default);
    let _ = window
      .native()
      .set_transparency(&OpacityValue::from_alpha(u8::MAX));
  }

  state.prev_effects_window = None;
}
