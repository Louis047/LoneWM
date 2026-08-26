use anyhow::Context;
use wm_common::{
  try_warn, ActiveDrag, ActiveDragOperation, DisplayState,
  FloatingStateConfig, FullscreenStateConfig, HideMethod, WindowState,
};
use wm_platform::{NativeWindow, NativeWindowWindowsExt, Rect};

use crate::{
  commands::{
    container::{flatten_split_container, move_container_within_tree},
    window::update_window_state,
  },
  events::handle_window_moved_or_resized_end,
  models::{Monitor, NonTilingWindow, WindowContainer},
  traits::{CommonGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

#[allow(clippy::too_many_lines)]
pub fn handle_window_moved_or_resized(
  native_window: &NativeWindow,
  is_interactive_start: bool,
  is_interactive_end: bool,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let found_window = state.window_from_native(native_window);

  if let Some(window) = found_window {
    let old_frame_position = window.native_properties().frame;
    let frame_position = try_warn!(window.native().frame());

    window.update_native_properties(|properties| {
      properties.frame = frame_position.clone();
    });

    // Handle windows that are actively being dragged.
    if !state.is_paused && window.active_drag().is_some() {
      // The drag operation has ended when `is_interactive_end` is `true`.
      // This corresponds to a `EVENT_SYSTEM_MOVESIZEEND` event.
      if is_interactive_end {
        return handle_window_moved_or_resized_end(&window, state, config);
      }

      return update_drag_state(&window, &frame_position, state, config);
    }

    let old_is_maximized = window.native_properties().is_maximized;
    let is_maximized = try_warn!(window.native().is_maximized());

    // Ignore duplicate move/resize events. Window position changes can
    // trigger multiple events. For example, restoring from maximized can
    // trigger as many as 4 identical events on Windows.
    if old_frame_position == frame_position
      && old_is_maximized == is_maximized
      && !is_interactive_start
    {
      return Ok(());
    }

    window.update_native_properties(|properties| {
      properties.is_maximized = is_maximized;
    });

    // If the window is not maximized, update its cached shadow borders.
    // Maximized windows temporarily have 0 shadow borders, in which case
    // we should use its previous value for redraws.
    {
      let shadow_borders = try_warn!(window.native().shadow_borders());
      if !is_maximized {
        window.update_native_properties(|properties| {
          properties.shadow_borders = shadow_borders;
        });
      }
    }

    let is_minimized = try_warn!(window.native().is_minimized());

    // Ignore events for minimized windows. Let them be handled by the
    // `PlatformEvent::WindowMinimized` event handler instead.
    if is_minimized {
      return Ok(());
    }

    // Detect whether the window is starting to be interactively moved or
    // resized by the user (e.g. via the window's drag handles).
    //
    let is_mouse_down = state
      .dispatcher
      .is_mouse_down(&wm_platform::MouseButton::Left);

    let is_drag_start = !state.is_paused
      && is_interactive_start
      && is_mouse_down
      && !matches!(window.state(), WindowState::Minimized);

    if is_drag_start {
      tracing::info!("Window started dragging: {window}");

      state.wm_set_frames.remove(&window.id());

      window.set_active_drag(Some(ActiveDrag {
        operation: None,
        is_from_floating: matches!(
          window.state(),
          WindowState::Floating(_)
        ),
        initial_position: old_frame_position.clone(),
      }));

      update_drag_state(&window, &frame_position, state, config)?;

      return Ok(());
    }

    let nearest_monitor = state
      .nearest_monitor(&window.native())
      .context("No nearest monitor.")?;

    // For `HideMethod::PlaceInCorner`, hiding/showing is implemented by
    // repositioning the window. Since the OS won't emit real
    // shown/hidden events in this mode, update `DisplayState` based on
    // whether the window has been moved to the monitor's bottom corner.
    if config.value.general.hide_method == HideMethod::PlaceInCorner {
      let is_in_corner = is_in_corner(
        &frame_position,
        &nearest_monitor.native_properties().working_area,
      );

      // TODO: Consider redrawing if hidden and should be shown, or if
      // shown and should be hidden.
      // TODO: It can be valid for a floating window to be in the corner,
      // in which case, it currently doesn't get updated to
      // `DisplayState::Shown`.
      let display_state = match (window.display_state(), is_in_corner) {
        (DisplayState::Hiding, true) => DisplayState::Hidden,
        (DisplayState::Showing, false) => DisplayState::Shown,
        _ => window.display_state(),
      };

      if display_state != window.display_state() {
        window.set_display_state(display_state);
        return Ok(());
      }
    }

    // Ignore echo events for rects that were set by the WM itself.
    // When the window frame matches what the WM commanded, no state
    // transition or restoration should take place.
    //
    // See: <https://github.com/glzr-io/glazewm/issues/1418>
    let is_wm_echo = state.wm_set_frames.get(&window.id()).is_some_and(
      |(set_rect, _)| rects_approx_equal(&frame_position, set_rect),
    );

    if is_wm_echo {
      return Ok(());
    }

    // If the window moved to a frame different from what the WM set (e.g.
    // user drag or app-initiated move), invalidate `wm_set_frames` so
    // subsequent redraws snap the window back to its assigned slot.
    state.wm_set_frames.remove(&window.id());

    let should_fullscreen = {
      let workspace = nearest_monitor
        .displayed_workspace()
        .context("No workspace.")?;

      let should_fullscreen = window.should_fullscreen(&workspace)?;

      match window.state() {
        // Override the fullscreen check for when an app self-exits
        // fullscreen (e.g. Chrome via F11) and restores its window to
        // a position that exactly covers the workspace rect.
        WindowState::Fullscreen(fullscreen)
          if !fullscreen.maximized && should_fullscreen =>
        {
          let workspace_rect = workspace.max_workspace_rect()?;

          let old_frame = old_frame_position
            .apply_delta(&window.border_delta().inverse(), None);
          let new_frame = frame_position
            .apply_delta(&window.border_delta().inverse(), None);

          let old_exceeded =
            old_frame.inset(1).contains_rect(&workspace_rect);
          let new_exceeds =
            new_frame.inset(1).contains_rect(&workspace_rect);

          // The window should no longer be fullscreen if the old frame
          // exceeded the workspace bounds (app was in OS fullscreen),
          // but the new frame no longer does. Configs with
          // 0px outer gaps always use the
          // `should_fullscreen` check, since the old frame
          // will never exceed the workspace bounds.
          if old_exceeded && !new_exceeds {
            false
          } else {
            should_fullscreen
          }
        }
        _ => should_fullscreen,
      }
    };

    // Handle a window being maximized or entering fullscreen.
    if is_maximized || should_fullscreen {
      // Apps can restore their saved window placement (e.g. maximized)
      // shortly after their window is opened. Treat that as a launch
      // placement rather than a user-initiated maximize, so the pending
      // redraw restores the window into its tiling slot instead of
      // keeping it maximized.
      //
      // See: <https://github.com/glzr-io/glazewm/issues/1365>
      let is_launch_maximize = is_maximized
        && !should_fullscreen
        && state
          .managed_timestamps
          .get(&window.id())
          .is_some_and(|time| time.elapsed().as_millis() < 1000);

      if !is_launch_maximize {
        let is_same_state = is_maximized
          && matches!(
            window.state(),
            WindowState::Fullscreen(FullscreenStateConfig {
              maximized: true,
              ..
            })
          )
          || should_fullscreen
            && matches!(
              window.state(),
              WindowState::Fullscreen(FullscreenStateConfig {
                maximized: false,
                ..
              })
            );

        // Ignore if there's no state change.
        if is_same_state {
          return Ok(());
        }

        let fullscreen_state = if let WindowState::Fullscreen(
          fullscreen_state,
        ) = window.state()
        {
          fullscreen_state
        } else {
          config.value.window_behavior.state_defaults.fullscreen
        };

        let window = update_window_state(
          window.clone(),
          WindowState::Fullscreen(FullscreenStateConfig {
            maximized: is_maximized,
            ..fullscreen_state
          }),
          state,
          config,
        )?;

        if is_maximized {
          // Dequeue the window from redraw if it's maximized, since the
          // window is already in the correct state.
          state
            .pending_sync
            .dequeue_container_from_redraw(window.clone());
        }

        // TODO: Handle a fullscreen window being moved from one monitor to
        // another.

        return Ok(());
      }
    }

    match window.state() {
      WindowState::Fullscreen(_) => {
        // Window is no longer maximized/fullscreen and should be restored.
        tracing::info!("Restoring window from fullscreen: {window}");

        update_window_state(
          window.clone(),
          window.toggled_state(window.state(), config),
          state,
          config,
        )?;
      }
      WindowState::Floating(_) => {
        if let WindowContainer::NonTilingWindow(window) = window {
          update_floating_window_position(
            &window,
            frame_position,
            &nearest_monitor,
            state,
          )?;
        }
      }
      _ => {}
    }
  }

  Ok(())
}

// TODO: Move to shared location. `handle_window_moved_or_resized_end.rs`
// also uses this.
pub fn update_floating_window_position(
  window: &NonTilingWindow,
  frame_position: Rect,
  nearest_monitor: &Monitor,
  state: &mut WmState,
) -> anyhow::Result<()> {
  tracing::info!(
    "Updating floating window position: {}",
    window.as_window_container()?
  );

  // Update state with the new location of the floating window.
  window.set_floating_placement(frame_position);
  window.set_has_custom_floating_placement(true);

  let monitor = window.monitor().context("No monitor.")?;

  // Update the window's workspace if it goes out of bounds of its
  // current workspace.
  if monitor.id() != nearest_monitor.id() {
    // Since the window is moving to a different monitor, adjustments
    // might need to be made because of DPI.
    window.set_has_pending_dpi_adjustment(true);

    let updated_workspace = nearest_monitor
      .displayed_workspace()
      .context("Failed to get workspace of nearest monitor.")?;

    tracing::info!(
      "Floating window moved to new workspace: {updated_workspace}",
    );

    window.set_insertion_target(None);

    move_container_within_tree(
      &window.clone().into(),
      &updated_workspace.clone().into(),
      updated_workspace.child_count(),
      state,
    )?;
  }

  Ok(())
}

/// Updates the window operation based on changes in frame position.
///
/// This function determines whether a window is being moved or resized and
/// updates its operation state accordingly. If the window is being moved,
/// it's set to floating mode.
fn update_drag_state(
  window: &WindowContainer,
  frame_position: &Rect,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let Some(active_drag) = window.active_drag() else {
    return Ok(());
  };

  // Ignore if the window position has not changed yet.
  if *frame_position == active_drag.initial_position {
    return Ok(());
  }

  // Determine the drag operation if not already set.
  let is_move = if let Some(operation) = active_drag.operation {
    matches!(operation, ActiveDragOperation::Move)
  } else {
    let is_move = *frame_position != active_drag.initial_position
      && frame_position.height() == active_drag.initial_position.height()
      && frame_position.width() == active_drag.initial_position.width();

    let operation = if is_move {
      ActiveDragOperation::Move
    } else {
      ActiveDragOperation::Resize
    };

    window.set_active_drag(Some(ActiveDrag {
      operation: Some(operation),
      ..active_drag.clone()
    }));

    is_move
  };

  // Transition window to be floating while it's being dragged, but only
  // for maximized/fullscreen windows being restored during drag.
  // Tiled windows remain tiled to prevent layout churn and flicker
  // on custom titlebar / WinUI 3 apps (e.g. WhatsApp).
  if is_move && matches!(window.state(), WindowState::Fullscreen(_)) {
    let move_distance = frame_position
      .center_point()
      .distance_between(&active_drag.initial_position.center_point());

    // Dragging operations on a maximized window can only occur on Windows.
    // The OS immediately restores it while it's being dragged, so we need
    // to update state accordingly without a redraw.
    let is_maximized = matches!(
      window.state(),
      WindowState::Fullscreen(FullscreenStateConfig {
        maximized: true,
        ..
      })
    );

    if move_distance >= 30.0 || is_maximized {
      let parent = window.parent().context("No parent")?;

      let is_fullscreen =
        matches!(window.state(), WindowState::Fullscreen(_))
          && !is_maximized;

      let window = update_window_state(
        window.clone(),
        WindowState::Floating(FloatingStateConfig {
          centered: false,
          ..config.value.window_behavior.state_defaults.floating
        }),
        state,
        config,
      )?;

      // `update_window_state` automatically adds the window for redraw,
      // which we don't want in this case. However, for fullscreen windows,
      // we do actually want it to be resized initially so that it's
      // easier to move around while dragging.
      if !is_fullscreen {
        state
          .pending_sync
          .dequeue_container_from_redraw(window.clone());
      }

      // Flatten the parent split container if it only contains the window.
      // TODO: Consider doing this to `move_container_within_tree`, so that
      // the behavior is consistent.
      if let Some(split_parent) = parent.as_split() {
        if split_parent.child_count() == 1 {
          flatten_split_container(split_parent.clone())?;

          // Hacky fix to redraw siblings after flattening. The parent is
          // queued for redraw from the state change, which gets detached
          // on flatten.
          // TODO: Change `queue_containers_to_redraw` to iterate over its
          // descendant windows and store those instead.
          state
            .pending_sync
            .queue_containers_to_redraw(window.tiling_siblings());
        }
      }
    }
  }

  Ok(())
}

/// Gets whether two rects are approximately equal, allowing for a small
/// tolerance in each edge (the OS can be off by a few px when applying a
/// position).
pub(crate) fn rects_approx_equal(a: &Rect, b: &Rect) -> bool {
  const TOLERANCE: i32 = 2;

  (a.left - b.left).abs() <= TOLERANCE
    && (a.top - b.top).abs() <= TOLERANCE
    && (a.right - b.right).abs() <= TOLERANCE
    && (a.bottom - b.bottom).abs() <= TOLERANCE
}

/// Gets whether the window is in the corner of the monitor.
fn is_in_corner(window_frame: &Rect, monitor_rect: &Rect) -> bool {
  // Visible portion of the window used when positioning windows in the
  // monitor's corner. See `platform_sync` for how hidden windows are
  // positioned.
  const VISIBLE_SLIVER_PX: i32 = 1;

  // Allow 1px of leeway.
  let is_left_corner =
    (window_frame.right - VISIBLE_SLIVER_PX - monitor_rect.left).abs()
      <= 1;

  // Allow 1px of leeway.
  let is_right_corner =
    (window_frame.x() + VISIBLE_SLIVER_PX - monitor_rect.right).abs() <= 1;

  // bar height.
  let is_bottom_of_monitor =
    (window_frame.y() - monitor_rect.bottom).abs() <= 55;

  (is_left_corner || is_right_corner) && is_bottom_of_monitor
}

#[cfg(test)]
mod tests {
  use wm_platform::Rect;

  use super::is_in_corner;

  #[test]
  fn matches_corner_positions() {
    let monitor = Rect::from_xy(0, 0, 1920, 1080);

    let frame_in_right_corner = Rect::from_xy(1919, 1050, 600, 600);
    assert!(is_in_corner(&frame_in_right_corner, &monitor));

    let frame_in_left_corner = Rect::from_xy(-599, 1050, 600, 600);
    assert!(is_in_corner(&frame_in_left_corner, &monitor));
  }

  #[test]
  fn does_not_match_non_corner_positions() {
    let monitor = Rect::from_xy(0, 0, 1920, 1080);
    let frame = Rect::from_xy(100, 100, 800, 600);

    assert!(!is_in_corner(&frame, &monitor));
  }
}
