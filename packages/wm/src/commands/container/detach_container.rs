use anyhow::Context;

use super::flatten_split_container;
use crate::{
  models::Container,
  traits::{CommonGetters, TilingSizeGetters, MIN_TILING_SIZE},
};

/// Removes a container from the tree.
///
/// If the container is a tiling container, the siblings will be resized to
/// fill the freed up space. Will flatten empty parent split containers.
#[allow(clippy::needless_pass_by_value)]
pub fn detach_container(child_to_remove: Container) -> anyhow::Result<()> {
  let parent = child_to_remove.parent().context("No parent.")?;

  parent
    .borrow_children_mut()
    .retain(|c| c.id() != child_to_remove.id());

  parent
    .borrow_child_focus_order_mut()
    .retain(|id| *id != child_to_remove.id());

  *child_to_remove.borrow_parent_mut() = None;

  // Resize the siblings if it is a tiling container.
  if let Ok(child_to_remove) = child_to_remove.as_tiling_container() {
    let tiling_siblings = parent.tiling_children().collect::<Vec<_>>();

    if !tiling_siblings.is_empty() {
      let available_size =
        tiling_siblings.iter().fold(0.0, |sum, container| {
          sum + container.tiling_size() - MIN_TILING_SIZE
        });

      if available_size > 0.0 {
        // Adjust size of the siblings based on the freed up space.
        for sibling in &tiling_siblings {
          let resize_factor =
            (sibling.tiling_size() - MIN_TILING_SIZE) / available_size;

          let size_delta = resize_factor * child_to_remove.tiling_size();
          sibling.set_tiling_size(sibling.tiling_size() + size_delta);
        }
      }
    }
  }

  // Flatten the parent split container if it now has only 1 child, or
  // detach it if it is completely empty.
  if let Some(split_parent) = parent.as_split().cloned() {
    if split_parent.child_count() == 1 {
      flatten_split_container(split_parent)?;
    } else if split_parent.child_count() == 0 {
      detach_container(split_parent.into())?;
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use wm_common::TilingDirection;

  use super::*;
  use crate::{
    commands::container::attach_container,
    models::{SplitContainer, TilingWindow, Workspace},
  };

  #[test]
  fn detach_window_collapses_parent_split_to_single_survivor() {
    let workspace = Workspace::mock()
      .tiling_direction(TilingDirection::Horizontal)
      .call();

    // Tree: Workspace -> Split_H[ W1 (0.5), Split_V (0.5)[ W2 (0.5), W3
    // (0.5) ] ]
    let w1 = TilingWindow::mock().tiling_size(0.5).call();
    let w2 = TilingWindow::mock().tiling_size(0.5).call();
    let w3 = TilingWindow::mock().tiling_size(0.5).call();

    let split_v = SplitContainer::mock()
      .tiling_direction(TilingDirection::Vertical)
      .tiling_containers(vec![w2.clone().into(), w3.clone().into()])
      .call();
    split_v.set_tiling_size(0.5);

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

    // Detach W3 -> split_v only has W2 left -> split_v is flattened into
    // split_h
    detach_container(w3.into()).unwrap();

    // Invariants:
    // 1. split_v is destroyed/detached
    assert!(split_v.is_detached());
    // 2. split_h now directly contains W1 and W2
    assert_eq!(split_h.child_count(), 2);
    assert_eq!(split_h.children()[0].id(), w1.id());
    assert_eq!(split_h.children()[1].id(), w2.id());
    // 3. W2's parent is now split_h, and its tiling_size scaled
    assert_eq!(w2.parent().unwrap().id(), split_h.id());
  }
}
