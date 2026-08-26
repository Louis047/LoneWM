use anyhow::Context;
use wm_common::{TilingDirection, WmEvent};

use crate::{
  models::{Container, DirectionContainer},
  traits::{CommonGetters, TilingDirectionGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn toggle_tiling_direction(
  container: Container,
  state: &mut WmState,
  _config: &UserConfig,
) -> anyhow::Result<()> {
  let direction_container: DirectionContainer = match container {
    Container::TilingWindow(tiling_window) => {
      let parent = tiling_window
        .parent()
        .and_then(|p| p.as_direction_container().ok())
        .context("No parent direction container.")?;

      parent.set_tiling_direction(parent.tiling_direction().inverse());
      parent
    }
    Container::Workspace(workspace) => {
      workspace
        .set_tiling_direction(workspace.tiling_direction().inverse());
      workspace.into()
    }
    Container::Split(split) => {
      split.set_tiling_direction(split.tiling_direction().inverse());
      split.into()
    }
    // Can only toggle tiling direction from a tiling window, split
    // container, or workspace.
    _ => return Ok(()),
  };

  state
    .pending_sync
    .queue_containers_to_redraw(direction_container.tiling_children());

  state.emit_event(WmEvent::TilingDirectionChanged {
    direction_container: direction_container.to_dto()?,
    new_tiling_direction: direction_container.tiling_direction(),
  });

  Ok(())
}

pub fn set_tiling_direction(
  container: Container,
  state: &mut WmState,
  config: &UserConfig,
  tiling_direction: &TilingDirection,
) -> anyhow::Result<()> {
  let direction_container = match container.as_direction_container() {
    Ok(dc) => dc,
    Err(_) => container
      .parent()
      .and_then(|p| p.as_direction_container().ok())
      .context("No direction container.")?,
  };

  if direction_container.tiling_direction() == *tiling_direction {
    Ok(())
  } else {
    toggle_tiling_direction(container, state, config)
  }
}
