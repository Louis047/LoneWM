use anyhow::Context;
use wm_common::{
  try_warn, ActiveDragOperation, FullscreenStateConfig, WindowState,
};
use wm_platform::LengthValue;

use crate::{
  commands::{
    container::{move_container_within_tree, set_focused_descendant},
    window::{set_window_size, update_window_state},
  },
  events::update_floating_window_position,
  models::{NonTilingWindow, WindowContainer},
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

/// Handles the event for when a window is finished being moved or resized
/// by the user (e.g. via the window's drag handles or titlebar).
pub fn handle_window_moved_or_resized_end(
  window: &WindowContainer,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let Some(active_drag) = window.active_drag() else {
    return Ok(());
  };

  match &window {
    WindowContainer::NonTilingWindow(window) => {
      let is_maximized = try_warn!(window.native().is_maximized());

      window.update_native_properties(|properties| {
        properties.is_maximized = is_maximized;
      });

      let nearest_monitor = state
        .nearest_monitor(&window.native())
        .context("Failed to get workspace of nearest monitor.")?;

      let should_fullscreen = window.should_fullscreen(
        &nearest_monitor
          .displayed_workspace()
          .context("No workspace.")?,
      )?;

      if is_maximized || should_fullscreen {
        let fullscreen_state = if let WindowState::Fullscreen(
          fullscreen_state,
        ) = window.state()
        {
          fullscreen_state
        } else {
          config.value.window_behavior.state_defaults.fullscreen
        };

        let window = update_window_state(
          window.clone().into(),
          WindowState::Fullscreen(FullscreenStateConfig {
            maximized: is_maximized,
            ..fullscreen_state
          }),
          state,
          config,
        )?;

        window.set_active_drag(None);

        if is_maximized {
          // Dequeue the window from redraw if it's maximized, since the
          // window is already in the correct state.
          state
            .pending_sync
            .dequeue_container_from_redraw(window.clone());
        } else {
          // Force a redraw to snap the window to the monitor edges.
          state.pending_sync.queue_container_to_redraw(window.clone());
        }

        return Ok(());
      }

      if active_drag.is_from_floating {
        update_floating_window_position(
          window,
          window.native_properties().frame,
          &nearest_monitor,
          state,
        )?;
        window.set_active_drag(None);
      } else {
        // Window is a temporary floating window that should be
        // reverted back to tiling in the dwindle layout.
        let window = drop_as_tiling_window(window, state, config)?;
        window.set_active_drag(None);
      }
    }
    WindowContainer::TilingWindow(window) => {
      tracing::info!(
        "Tiling window move/resize ended: {}",
        window.as_window_container()?
      );

      let frame = window.native_properties().frame;

      // Update the window's size based on the new frame position on
      // resize.
      if active_drag.operation == Some(ActiveDragOperation::Resize) {
        set_window_size(
          window.clone().into(),
          Some(LengthValue::from_px(frame.width())),
          Some(LengthValue::from_px(frame.height())),
          state,
        )?;
      }

      window.set_active_drag(None);
      state.wm_set_frames.remove(&window.id());

      // Force a redraw of the window to snap it back to its original
      // position.
      state.pending_sync.queue_container_to_redraw(window.clone());
    }
  }

  Ok(())
}

/// Handles transition from temporary floating window to tiling window on
/// drag end in a pure Dwindle architecture.
fn drop_as_tiling_window(
  moved_window: &NonTilingWindow,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WindowContainer> {
  tracing::info!(
    "Tiling window drag ended: {}",
    moved_window.as_window_container()?
  );

  state.wm_set_frames.remove(&moved_window.id());

  let mouse_pos = state.dispatcher.cursor_position()?;
  let mouse_workspace = state
    .monitor_at_point(&mouse_pos)
    .and_then(|monitor| monitor.displayed_workspace())
    .or_else(|| moved_window.workspace())
    .context("Couldn't find workspace for window drop.")?;

  let current_workspace = moved_window
    .workspace()
    .context("Window has no workspace.")?;

  // 1. If dragged across workspaces/monitors, transfer the floating window
  //    first.
  if mouse_workspace.id() != current_workspace.id() {
    let current_monitor =
      current_workspace.monitor().context("No monitor.")?;
    let target_monitor =
      mouse_workspace.monitor().context("No monitor.")?;

    if current_monitor
      .has_dpi_difference(&target_monitor.clone().into())?
    {
      moved_window.set_has_pending_dpi_adjustment(true);
    }

    moved_window.set_insertion_target(None);

    move_container_within_tree(
      &moved_window.clone().into(),
      &mouse_workspace.clone().into(),
      mouse_workspace.child_count(),
      state,
    )?;
  }

  // 2. Find target tiling sibling on the target workspace under or nearest
  //    to cursor.
  let tiling_siblings = mouse_workspace
    .descendants()
    .filter_map(|c| c.as_tiling_window().cloned())
    .filter(|w| w.id() != moved_window.id())
    .collect::<Vec<_>>();

  if let Some(target_sibling) = tiling_siblings
    .iter()
    .find(|w| w.to_rect().is_ok_and(|r| r.contains_point(&mouse_pos)))
    .or_else(|| {
      tiling_siblings.iter().min_by(|a, b| {
        let dist_a = a
          .to_rect()
          .map_or(f32::MAX, |r| r.distance_to_point(&mouse_pos));
        let dist_b = b
          .to_rect()
          .map_or(f32::MAX, |r| r.distance_to_point(&mouse_pos));
        dist_a.total_cmp(&dist_b)
      })
    })
  {
    // Set target sibling as the focused descendant so
    // dwindle_insertion_target automatically selects it for spiral
    // split placement.
    set_focused_descendant(&target_sibling.clone().into(), None);
    moved_window.set_insertion_target(None);
  }

  // 3. Re-tile the window cleanly via update_window_state.
  let moved_window = update_window_state(
    moved_window.clone().into(),
    WindowState::Tiling,
    state,
    config,
  )?;

  state
    .pending_sync
    .queue_containers_to_redraw(mouse_workspace.tiling_children())
    .queue_focus_change()
    .queue_workspace_to_reorder(mouse_workspace);

  Ok(moved_window)
}
