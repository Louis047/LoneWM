use anyhow::Context;
use wm_common::WindowState;

use crate::{
  commands::container::detach_container,
  models::WindowContainer,
  traits::{CommonGetters, WindowGetters},
  wm_state::WmState,
};

#[allow(clippy::needless_pass_by_value)]
pub fn ignore_window(
  window: WindowContainer,
  state: &mut WmState,
) -> anyhow::Result<()> {
  // Remove the window from the managed window index.
  state.unindex_window(window.native().id());

  // Create iterator of parent, grandparent, and great-grandparent.
  let ancestors = window.ancestors().take(3).collect::<Vec<_>>();

  // Uncloak the window in case it was cloaked for being on an inactive
  // workspace. Otherwise, it would remain invisible and unreachable after
  // being ignored.
  //
  // See: <https://github.com/glzr-io/glazewm/issues/1358>
  {
    use wm_platform::NativeWindowWindowsExt;

    let _ = window.native().set_cloaked(false);
    let _ = window.native().show();
    let _ = window.native().set_taskbar_visibility(true);
  }

  state.ignored_windows.push(window.native().clone());
  detach_container(window.clone().into())?;

  // Sibling containers need to be redrawn if the window was tiling.
  if window.state() == WindowState::Tiling {
    let ancestor_to_redraw = ancestors
      .into_iter()
      .find(|ancestor| !ancestor.is_detached())
      .context("No ancestor to redraw.")?;

    state
      .pending_sync
      .queue_containers_to_redraw(ancestor_to_redraw.tiling_children());
  }

  Ok(())
}
