use anyhow::Context;
use wm_common::WindowState;
use wm_platform::{Direction, Rect};

use crate::{
  commands::{
    container::{
      attach_container, detach_container, set_focused_descendant,
      swap_tiling_windows, tiling_focus_target,
    },
    window::dwindle_insertion_target,
  },
  models::{
    Container, Monitor, NonTilingWindow, TilingWindow, WindowContainer,
  },
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

/// The distance in pixels to snap the window to the monitor's edge.
const SNAP_DISTANCE: i32 = 15;

#[allow(clippy::needless_pass_by_value)]
pub fn move_window_in_direction(
  window: WindowContainer,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  match &window {
    WindowContainer::TilingWindow(tiling_window) => {
      match tiling_window.state() {
        WindowState::Fullscreen(_) => {
          move_to_workspace_in_direction(&window, direction, state, config)
        }
        _ => move_tiling_window(
          tiling_window.clone(),
          direction,
          state,
          config,
        ),
      }
    }
    WindowContainer::NonTilingWindow(non_tiling_window) => {
      match non_tiling_window.state() {
        WindowState::Floating(_) => {
          move_floating_window(non_tiling_window.clone(), direction, state)
        }
        WindowState::Fullscreen(_) => {
          move_to_workspace_in_direction(&window, direction, state, config)
        }
        _ => Ok(()),
      }
    }
  }
}

/// Moves a tiling window in the given direction.
///
/// In a binary dwindle layout, moving within the same workspace swaps leaf
/// positions with the 2D spatial neighbor window, preserving binary split
/// structure, split ratios, and container geometry. If no neighbor exists
/// in that direction, the window crosses to the adjacent
/// monitor/workspace.
fn move_tiling_window(
  window_to_move: TilingWindow,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  // Check for a spatial neighbor tiling window within the same workspace.
  if let Some(target) =
    tiling_focus_target(&window_to_move.clone().into(), direction)?
  {
    if let Some(neighbor_window) = target.as_tiling_window().cloned() {
      return swap_tiling_windows(window_to_move, neighbor_window, state);
    }
  }

  // No neighbor on current workspace; move to adjacent monitor.
  move_to_workspace_in_direction(
    &window_to_move.into(),
    direction,
    state,
    config,
  )
}

fn move_to_workspace_in_direction(
  window_to_move: &WindowContainer,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let parent = window_to_move.parent().context("No parent.")?;
  let workspace = window_to_move.workspace().context("No workspace.")?;
  let monitor = parent.monitor().context("No monitor.")?;

  let target_workspace = state
    .monitor_in_direction(&monitor, direction)?
    .and_then(|monitor| monitor.displayed_workspace());

  if let Some(target_workspace) = target_workspace {
    // Adjust for DPI when crossing monitors.
    if monitor.has_dpi_difference(&target_workspace.clone().into())? {
      window_to_move.set_has_pending_dpi_adjustment(true);
    }

    // Update floating placement since the window has to cross monitors.
    window_to_move.set_floating_placement(
      window_to_move
        .floating_placement()
        .translate_to_center(&target_workspace.to_rect()?),
    );

    if let WindowContainer::NonTilingWindow(window_to_move) =
      &window_to_move
    {
      window_to_move.set_insertion_target(None);
    }

    // Focus target after removal from source workspace.
    let focus_target = state.focus_target_after_removal(window_to_move);

    detach_container(window_to_move.clone().into())?;

    // Insert into target workspace using dwindle layout.
    let insertion_sibling = target_workspace
      .descendant_focus_order()
      .find(Container::is_tiling_window);

    if window_to_move.is_tiling_window() && insertion_sibling.is_some() {
      if let Some(sibling) = insertion_sibling {
        let (target_parent, target_index) =
          dwindle_insertion_target(&sibling, &config.value.gaps);

        attach_container(
          &window_to_move.clone().into(),
          &target_parent,
          Some(target_index),
        )?;
      }
    } else {
      attach_container(
        &window_to_move.clone().into(),
        &target_workspace.clone().into(),
        Some(target_workspace.child_count()),
      )?;
    }

    if let Some(focus_target) = focus_target {
      set_focused_descendant(
        &focus_target,
        Some(&workspace.clone().into()),
      );
    }

    set_focused_descendant(&window_to_move.clone().into(), None);

    state
      .pending_sync
      .queue_container_to_redraw(window_to_move.clone())
      .queue_containers_to_redraw(target_workspace.tiling_children())
      .queue_containers_to_redraw(parent.tiling_children())
      .queue_focus_change()
      .queue_cursor_jump()
      .queue_workspace_to_reorder(target_workspace);
  }

  Ok(())
}

fn move_floating_window(
  window_to_move: NonTilingWindow,
  direction: &Direction,
  state: &mut WmState,
) -> anyhow::Result<()> {
  let new_position =
    new_floating_position(&window_to_move, direction, state)?;

  if let Some((position_rect, target_monitor)) = new_position {
    let monitor = window_to_move.monitor().context("No monitor.")?;

    // Mark window as needing DPI adjustment if it crosses monitors.
    if monitor.id() != target_monitor.id()
      && monitor.has_dpi_difference(&target_monitor.into())?
    {
      window_to_move.set_has_pending_dpi_adjustment(true);
    }

    window_to_move.set_floating_placement(position_rect);
    state.pending_sync.queue_container_to_redraw(window_to_move);
  }

  Ok(())
}

/// Returns a tuple of the new floating position and the target monitor.
fn new_floating_position(
  window_to_move: &NonTilingWindow,
  direction: &Direction,
  state: &mut WmState,
) -> anyhow::Result<Option<(Rect, Monitor)>> {
  let monitor = window_to_move.monitor().context("No monitor.")?;
  let monitor_rect = monitor.native_properties().working_area;
  let window_pos = window_to_move.native_properties().frame;

  let is_on_monitor_edge = match direction {
    Direction::Up => window_pos.top == monitor_rect.top,
    Direction::Down => window_pos.bottom == monitor_rect.bottom,
    Direction::Left => window_pos.left == monitor_rect.left,
    Direction::Right => window_pos.right == monitor_rect.right,
  };

  // Window is on the edge of the monitor and should be moved to a
  // different monitor in the given direction.
  if is_on_monitor_edge {
    let next_monitor = state.monitor_in_direction(&monitor, direction)?;

    if let Some(next_monitor) = next_monitor {
      let monitor_rect = next_monitor.native().working_area()?.clone();

      let position = snap_to_monitor_edge(
        &window_pos,
        &monitor_rect,
        &direction.inverse(),
      )
      .clamp(&monitor_rect);

      return Ok(Some((position, next_monitor)));
    }

    return Ok(None);
  }

  let (monitor_length, window_length) = match direction {
    Direction::Up | Direction::Down => {
      (monitor_rect.height(), window_pos.height())
    }
    _ => (monitor_rect.width(), window_pos.width()),
  };

  let length_delta = monitor_length - window_length;

  // Calculate the distance the window should move based on the ratio of
  // the window's length to the monitor's length.
  #[allow(clippy::cast_precision_loss)]
  let move_distance = match window_length as f32 / monitor_length as f32 {
    x if (0.0..0.2).contains(&x) => length_delta / 5,
    x if (0.2..0.4).contains(&x) => length_delta / 4,
    x if (0.4..0.6).contains(&x) => length_delta / 3,
    _ => length_delta / 2,
  };

  // Snap the window to the current monitor's edge if it's within 15px of
  // it after the move.
  let should_snap_to_edge = match direction {
    Direction::Up => {
      window_pos.top - move_distance - SNAP_DISTANCE < monitor_rect.top
    }
    Direction::Down => {
      window_pos.bottom + move_distance + SNAP_DISTANCE
        > monitor_rect.bottom
    }
    Direction::Left => {
      window_pos.left - move_distance - SNAP_DISTANCE < monitor_rect.left
    }
    Direction::Right => {
      window_pos.right + move_distance + SNAP_DISTANCE > monitor_rect.right
    }
  };

  if should_snap_to_edge {
    let position =
      snap_to_monitor_edge(&window_pos, &monitor_rect, direction);

    return Ok(Some((position, monitor)));
  }

  // Snap the window to the current monitor's inverse edge if it's in
  // between two monitors or outside the bounds of the current monitor.
  let should_snap_to_inverse_edge = match direction {
    Direction::Up => window_pos.bottom > monitor_rect.bottom,
    Direction::Down => window_pos.top < monitor_rect.top,
    Direction::Left => window_pos.right > monitor_rect.right,
    Direction::Right => window_pos.left < monitor_rect.left,
  };

  let position = if should_snap_to_inverse_edge {
    snap_to_monitor_edge(&window_pos, &monitor_rect, &direction.inverse())
  } else {
    window_pos.translate_in_direction(direction, move_distance)
  };

  Ok(Some((position, monitor)))
}

fn snap_to_monitor_edge(
  window_pos: &Rect,
  monitor_rect: &Rect,
  edge: &Direction,
) -> Rect {
  let (x, y) = match edge {
    Direction::Up => (window_pos.x(), monitor_rect.top),
    Direction::Down => {
      (window_pos.x(), monitor_rect.bottom - window_pos.height())
    }
    Direction::Left => (monitor_rect.left, window_pos.y()),
    Direction::Right => {
      (monitor_rect.right - window_pos.width(), window_pos.y())
    }
  };

  window_pos.translate_to_coordinates(x, y)
}
