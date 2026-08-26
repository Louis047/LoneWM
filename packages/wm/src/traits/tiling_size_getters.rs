use std::cell::Ref;

use ambassador::delegatable_trait;
use anyhow::Context;
use wm_common::{GapsConfig, TilingDirection};

use super::{CommonGetters, TilingDirectionGetters};
use crate::models::{Container, DirectionContainer, TilingContainer};

pub const MIN_TILING_SIZE: f32 = 0.01;

#[delegatable_trait]
pub trait TilingSizeGetters: CommonGetters {
  fn tiling_size(&self) -> f32;

  fn set_tiling_size(&self, tiling_size: f32);

  fn gaps_config(&self) -> Ref<'_, GapsConfig>;

  fn set_gaps_config(&self, gaps_config: GapsConfig);

  /// Gets the horizontal and vertical gaps between windows in pixels.
  fn inner_gaps(&self) -> anyhow::Result<(i32, i32)> {
    let monitor = self.monitor().context("No monitor.")?;
    let monitor_rect = monitor.native_properties().bounds;
    let gaps_config = self.gaps_config();

    let scale_factor = if gaps_config.scale_with_dpi {
      monitor.native_properties().scale_factor
    } else {
      1.
    };

    Ok((
      gaps_config
        .inner_gap
        .to_px(monitor_rect.width(), Some(scale_factor)),
      gaps_config
        .inner_gap
        .to_px(monitor_rect.height(), Some(scale_factor)),
    ))
  }

  /// Gets the container to resize when resizing a tiling window.
  ///
  /// Walks up the ancestor chain to find the first split container
  /// whose tiling direction matches the requested resize axis.
  /// Returns `None` if no suitable ancestor is found (e.g. the
  /// window is the only tiling child of the workspace).
  fn container_to_resize(
    &self,
    is_width_resize: bool,
  ) -> anyhow::Result<Option<TilingContainer>> {
    let target_direction = if is_width_resize {
      TilingDirection::Horizontal
    } else {
      TilingDirection::Vertical
    };

    // Walk up from self, looking for the first ancestor whose
    // parent splits in the target direction.
    let mut candidate: Container = self.as_container();
    loop {
      let parent =
        candidate.direction_container().context("No parent.")?;

      match parent {
        DirectionContainer::Workspace(_) => return Ok(None),
        DirectionContainer::Split(ref split) => {
          if parent.tiling_direction() == target_direction {
            // This parent splits in the right direction —
            // resize the candidate (child of this parent).
            return Ok(candidate.as_tiling_container().ok());
          }
          candidate = split.clone().into();
        }
      }
    }
  }
}

/// Implements the `TilingSizeGetters` trait for a given struct.
///
/// Expects that the struct has a wrapping `RefCell` containing a struct
/// with a `tiling_size` field.
#[macro_export]
macro_rules! impl_tiling_size_getters {
  ($struct_name:ident) => {
    impl TilingSizeGetters for $struct_name {
      fn tiling_size(&self) -> f32 {
        self.0.borrow().tiling_size
      }

      fn set_tiling_size(&self, tiling_size: f32) {
        self.0.borrow_mut().tiling_size = tiling_size;
      }

      fn gaps_config(&self) -> Ref<'_, GapsConfig> {
        Ref::map(self.0.borrow(), |inner| &inner.gaps_config)
      }

      fn set_gaps_config(&self, gaps_config: GapsConfig) {
        self.0.borrow_mut().gaps_config = gaps_config;
      }
    }
  };
}
