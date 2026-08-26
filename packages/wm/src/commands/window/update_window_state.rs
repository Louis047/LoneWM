use anyhow::Context;
use tracing::{info, warn};
use wm_common::WindowState;
use wm_platform::NativeWindowWindowsExt;

use crate::{
  commands::{
    container::{
      move_container_within_tree, replace_container,
      resize_tiling_container,
    },
    window::manage_window::dwindle_insertion_target,
  },
  models::{Container, InsertionTarget, WindowContainer},
  traits::{CommonGetters, TilingSizeGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

/// Updates the state of a window.
///
/// Adds the window for redraw if there is a state change.
///
/// Returns the window after the state change.
pub fn update_window_state(
  window: WindowContainer,
  target_state: WindowState,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WindowContainer> {
  if window.state() == target_state {
    return Ok(window);
  }

  info!("Updating window state: {:?}.", target_state);

  // Refresh the window's cached shadow borders before the redraw. They
  // can change while a window is fullscreen (DWM adjusts the extended
  // frame bounds), and positioning with a stale cache misaligns the
  // window (e.g. overlapping the bar or missing gaps when leaving
  // fullscreen).
  //
  // See: <https://github.com/glzr-io/glazewm/issues/996>
  // See: <https://github.com/glzr-io/glazewm/issues/737>
  if !window.native().is_maximized().unwrap_or(false) {
    let shadow_borders = window.native().shadow_borders().ok();
    if let Some(shadow_borders) = shadow_borders {
      window.update_native_properties(|properties| {
        properties.shadow_borders = shadow_borders;
      });
    }
  }

  match (&window, &target_state) {
    (
      WindowContainer::TilingWindow(tiling_win),
      WindowState::Fullscreen(_),
    ) => {
      let current_state = tiling_win.state();
      tiling_win.set_prev_state(current_state);
      tiling_win.set_state(target_state);

      let workspace = tiling_win.workspace().context("No workspace.")?;
      state
        .pending_sync
        .queue_container_to_redraw(tiling_win.clone())
        .queue_workspace_to_reorder(workspace);

      Ok(tiling_win.clone().into())
    }
    (WindowContainer::TilingWindow(tiling_win), WindowState::Tiling) => {
      let current_state = tiling_win.state();
      tiling_win.set_prev_state(current_state);
      tiling_win.set_state(WindowState::Tiling);

      let workspace = tiling_win.workspace().context("No workspace.")?;
      state
        .pending_sync
        .queue_container_to_redraw(tiling_win.clone())
        .queue_workspace_to_reorder(workspace);

      Ok(tiling_win.clone().into())
    }
    (_, WindowState::Tiling) => set_tiling(&window, state, config),
    _ => set_non_tiling(window, target_state, state),
  }
}

/// Updates the state of a window to be `WindowState::Tiling`.
fn set_tiling(
  window: &WindowContainer,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WindowContainer> {
  let window = window
    .as_non_tiling_window()
    .context("Invalid window state.")?
    .clone();

  let workspace =
    window.workspace().context("Window has no workspace.")?;

  // Check whether insertion target is still valid.
  let insertion_target =
    window.insertion_target().filter(|insertion_target| {
      insertion_target
        .target_parent
        .workspace()
        .is_some_and(|workspace| workspace.is_displayed())
    });

  // Get the position in the tree to insert the new tiling window. This
  // will be the window's previous tiling position if it has one, or
  // instead beside the last focused tiling window in the workspace.
  let (target_parent, target_index) = insertion_target
    .as_ref()
    .map(|insertion_target| {
      (
        insertion_target.target_parent.clone(),
        insertion_target.target_index,
      )
    })
    // Fallback to the last focused tiling window within the workspace.
    .or_else(|| {
      let sibling = workspace
        .descendant_focus_order()
        .find(Container::is_tiling_window);

      sibling.map(|sibling| {
        dwindle_insertion_target(&sibling, &config.value.gaps)
      })
    })
    // Default to inserting at the end of the workspace.
    .unwrap_or((workspace.clone().into(), workspace.child_count()));

  let tiling_window = window.to_tiling(config.value.gaps.clone());

  // Replace the original window with the created tiling window.
  replace_container(
    &tiling_window.clone().into(),
    &window.parent().context("No parent.")?,
    window.index(),
  )?;

  move_container_within_tree(
    &tiling_window.clone().into(),
    &target_parent,
    target_index,
    state,
  )?;

  // Update the managed window index with the new container.
  state.index_window(&tiling_window.clone().into());

  #[allow(clippy::cast_precision_loss)]
  if let Some(insertion_target) = &insertion_target {
    let size_scale = (insertion_target.prev_sibling_count + 1) as f32
      / (tiling_window.tiling_siblings().count() + 1) as f32;

    // Scale the window's previous size based on the current number of
    // siblings. E.g. if the window was 0.5 with 1 sibling, and now has 2
    // siblings, scale to 0.5 * (2/3) to maintain proportional sizing.
    let target_size = insertion_target.prev_tiling_size * size_scale;
    resize_tiling_container(&tiling_window.clone().into(), target_size);
  }

  state
    .pending_sync
    .queue_containers_to_redraw(target_parent.tiling_children())
    .queue_workspace_to_reorder(workspace);

  Ok(tiling_window.into())
}

/// Updates the state of a window to be either `WindowState::Floating`,
/// `WindowState::Fullscreen`, or `WindowState::Minimized`.
fn set_non_tiling(
  window: WindowContainer,
  target_state: WindowState,
  state: &mut WmState,
) -> anyhow::Result<WindowContainer> {
  // A window can only be updated to a minimized state if it is
  // natively minimized.
  // TODO: Consider doing the same for maximized and fullscreen states.
  if target_state == WindowState::Minimized
    && !window.native_properties().is_minimized
  {
    info!("No window state update. Minimizing window.");

    // TODO: Instead of doing the platform call directly here, instead add
    // a `queue_state_change` method to `PendingSync`.
    if let Err(err) = window.native().minimize() {
      warn!("Failed to minimize window: {}", err);
    }

    return Ok(window);
  }

  let workspace = window.workspace().context("No workspace.")?;

  match window {
    WindowContainer::NonTilingWindow(window) => {
      let current_state = window.state();

      // Update the window's previous state if the discriminant changes.
      // TODO: Move out handling of active drag. Can then simplify calls to
      // `set_active_drag` in `handle_window_moved_or_resized_end`.
      if !current_state.is_same_state(&target_state)
        && window.active_drag().is_none()
      {
        window.set_prev_state(current_state);
        state.pending_sync.queue_workspace_to_reorder(workspace);
      }

      window.set_state(target_state);
      state.pending_sync.queue_container_to_redraw(window.clone());

      Ok(window.into())
    }
    WindowContainer::TilingWindow(window) => {
      let parent = window.parent().context("No parent")?;

      let non_tiling_window = window.to_non_tiling(
        target_state.clone(),
        Some(InsertionTarget {
          target_parent: parent.clone(),
          target_index: window.index(),
          prev_tiling_size: window.tiling_size(),
          prev_sibling_count: window.tiling_siblings().count(),
        }),
      );

      // Non-tiling windows should always be direct children of the
      // workspace.
      if parent != workspace.clone().into() {
        move_container_within_tree(
          &window.clone().into(),
          &workspace.clone().into(),
          workspace.child_count(),
          state,
        )?;
      }

      replace_container(
        &non_tiling_window.clone().into(),
        &workspace.clone().into(),
        window.index(),
      )?;

      // Update the managed window index with the new container.
      state.index_window(&non_tiling_window.clone().into());

      state
        .pending_sync
        .queue_container_to_redraw(non_tiling_window.clone())
        .queue_containers_to_redraw(workspace.tiling_children())
        .queue_workspace_to_reorder(workspace);

      Ok(non_tiling_window.into())
    }
  }
}
