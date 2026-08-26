use anyhow::Context;
use wm_common::WmEvent;

use super::set_focused_descendant;
use crate::{
  models::TilingWindow,
  traits::{CommonGetters, TilingSizeGetters},
  wm_state::WmState,
};

/// Atomically swaps the positions of two tiling windows within the
/// container tree.
///
/// In a binary dwindle tree, swapping leaf slots preserves 100% of the
/// internal split hierarchy, split ratios, and container geometry with
/// zero size distortion while retaining focus on the moved window.
#[allow(clippy::needless_pass_by_value)]
pub fn swap_tiling_windows(
  window_a: TilingWindow,
  window_b: TilingWindow,
  state: &mut WmState,
) -> anyhow::Result<()> {
  if window_a.id() == window_b.id() {
    return Ok(());
  }

  let parent_a = window_a.parent().context("Window A has no parent.")?;
  let parent_b = window_b.parent().context("Window B has no parent.")?;
  let index_a = window_a.index();
  let index_b = window_b.index();

  if parent_a.id() == parent_b.id() {
    // Sibling swap within the same split container.
    parent_a.borrow_children_mut().swap(index_a, index_b);
  } else {
    // Cross-container leaf swap:
    // 1. Swap children in parent arrays and update parent pointers.
    parent_a.borrow_children_mut()[index_a] = window_b.clone().into();
    *window_b.borrow_parent_mut() = Some(parent_a.clone());

    parent_b.borrow_children_mut()[index_b] = window_a.clone().into();
    *window_a.borrow_parent_mut() = Some(parent_b.clone());

    // 2. Exchange child focus order entries.
    let mut focus_order_a = parent_a.borrow_child_focus_order_mut();
    if let Some(pos) =
      focus_order_a.iter().position(|id| *id == window_a.id())
    {
      focus_order_a[pos] = window_b.id();
    }
    drop(focus_order_a);

    let mut focus_order_b = parent_b.borrow_child_focus_order_mut();
    if let Some(pos) =
      focus_order_b.iter().position(|id| *id == window_b.id())
    {
      focus_order_b[pos] = window_a.id();
    }
    drop(focus_order_b);

    // 3. Swap tiling_size values so each window adopts its new slot's
    //    proportion.
    let size_a = window_a.tiling_size();
    let size_b = window_b.tiling_size();
    window_a.set_tiling_size(size_b);
    window_b.set_tiling_size(size_a);
  }

  // Explicitly retain focus on the moved window (window_a) across all
  // ancestors.
  set_focused_descendant(&window_a.clone().into(), None);

  // Queue redraws and sync.
  state
    .pending_sync
    .queue_container_to_redraw(parent_a.clone())
    .queue_container_to_redraw(parent_b.clone())
    .queue_containers_to_redraw(parent_a.tiling_children())
    .queue_containers_to_redraw(parent_b.tiling_children())
    .queue_focus_change()
    .queue_cursor_jump();

  state.emit_event(WmEvent::FocusedContainerMoved {
    focused_container: window_a.to_dto()?,
  });

  Ok(())
}

#[cfg(test)]
mod tests {
  use wm_common::TilingDirection;

  use super::*;
  use crate::{
    commands::container::attach_container,
    models::{Monitor, SplitContainer, TilingWindow, Workspace},
  };

  #[test]
  fn test_cross_split_leaf_swap_preserves_tree_structure() {
    let mut state = WmState::mock();
    let workspace = Workspace::mock()
      .tiling_direction(TilingDirection::Horizontal)
      .call();
    let _monitor =
      Monitor::mock().workspaces(vec![workspace.clone()]).call();

    // Build tree: H[ W1, V[ W2, W3 ] ]
    let w1 = TilingWindow::mock().tiling_size(0.6).call();
    let w2 = TilingWindow::mock().tiling_size(0.4).call();
    let w3 = TilingWindow::mock().tiling_size(0.6).call();

    let split_v = SplitContainer::mock()
      .tiling_direction(TilingDirection::Vertical)
      .tiling_containers(vec![w2.clone().into(), w3.clone().into()])
      .call();
    split_v.set_tiling_size(0.4);

    let split_h = SplitContainer::mock()
      .tiling_direction(TilingDirection::Horizontal)
      .tiling_containers(vec![w1.clone().into(), split_v.clone().into()])
      .call();
    attach_container(
      &split_h.clone().into(),
      &workspace.clone().into(),
      None,
    )
    .unwrap();

    let parent_w1 = w1.parent().unwrap();
    let parent_w3 = w3.parent().unwrap();
    assert_eq!(parent_w1.id(), split_h.id());
    assert_eq!(parent_w3.id(), split_v.id());

    // Swap W1 and W3
    swap_tiling_windows(w1.clone(), w3.clone(), &mut state).unwrap();

    // Invariants after swap:
    // 1. W1 is now inside split_v at index 1 with size 0.5
    assert_eq!(w1.parent().unwrap().id(), split_v.id());
    assert_eq!(split_v.children()[1].id(), w1.id());
    assert!((w1.tiling_size() - 0.5).abs() < f32::EPSILON);

    // 2. W3 is now inside split_h at index 0 with size 0.5
    assert_eq!(w3.parent().unwrap().id(), split_h.id());
    assert_eq!(split_h.children()[0].id(), w3.id());
    assert!((w3.tiling_size() - 0.5).abs() < f32::EPSILON);

    // 3. Tree hierarchy structure was NOT altered (split_h and split_v
    //    remain intact)
    assert_eq!(workspace.child_count(), 1);
    assert_eq!(split_h.child_count(), 2);
    assert_eq!(split_v.child_count(), 2);
  }
}
