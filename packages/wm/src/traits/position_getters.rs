use ambassador::delegatable_trait;
use wm_platform::Rect;

#[delegatable_trait]
pub trait PositionGetters {
  fn to_rect(&self) -> anyhow::Result<Rect>;

  /// Returns the layout rectangle for tiling calculations.
  /// Defaults to `to_rect()`, but overridden by `TilingWindow` to always
  /// return its assigned split slot even when in
  /// `WindowState::Fullscreen`.
  fn to_tiling_rect(&self) -> anyhow::Result<Rect> {
    self.to_rect()
  }
}

/// Implements `calculate_tiling_rect` for tiling containers that can be
/// resized. This is used by `SplitContainer` and `TilingWindow`.
///
/// Expects that the struct has a wrapping `RefCell` containing a struct
/// with an `id` and a `parent` field.
#[macro_export]
macro_rules! impl_position_getters_as_resizable {
  ($struct_name:ident) => {
    impl $struct_name {
      pub fn calculate_tiling_rect(&self) -> anyhow::Result<Rect> {
        let parent = self
          .parent()
          .and_then(|parent| parent.as_direction_container().ok())
          .context("Parent does not have a tiling direction.")?;

        let parent_rect = parent.to_tiling_rect()?;

        let (horizontal_gap, vertical_gap) = self.inner_gaps()?;
        let inner_gap = match parent.tiling_direction() {
          TilingDirection::Vertical => vertical_gap,
          TilingDirection::Horizontal => horizontal_gap,
        };

        let immediate_prev_sibling = self
          .prev_siblings()
          .filter_map(|sibling| sibling.as_tiling_container().ok())
          .next();

        let is_last_sibling = self
          .next_siblings()
          .filter_map(|sibling| sibling.as_tiling_container().ok())
          .next()
          .is_none();

        let (x, y) = match immediate_prev_sibling {
          None => (parent_rect.x(), parent_rect.y()),
          Some(ref sibling) => {
            let sibling_rect = sibling.to_tiling_rect()?;

            match parent.tiling_direction() {
              TilingDirection::Vertical => (
                parent_rect.x(),
                sibling_rect.y() + sibling_rect.height() + inner_gap,
              ),
              TilingDirection::Horizontal => (
                sibling_rect.x() + sibling_rect.width() + inner_gap,
                parent_rect.y(),
              ),
            }
          }
        };

        #[allow(
          clippy::cast_precision_loss,
          clippy::cast_possible_truncation,
          clippy::cast_possible_wrap
        )]
        let (width, height) = match parent.tiling_direction() {
          TilingDirection::Vertical => {
            let height = if is_last_sibling
              && immediate_prev_sibling.is_some()
            {
              ((parent_rect.y() + parent_rect.height()) - y).max(0)
            } else {
              let available_height = (parent_rect.height()
                - inner_gap * self.tiling_siblings().count() as i32)
                .max(0);
              (available_height as f32 * self.tiling_size()).round() as i32
            };

            (parent_rect.width(), height)
          }
          TilingDirection::Horizontal => {
            let width = if is_last_sibling
              && immediate_prev_sibling.is_some()
            {
              ((parent_rect.x() + parent_rect.width()) - x).max(0)
            } else {
              let available_width = (parent_rect.width()
                - inner_gap * self.tiling_siblings().count() as i32)
                .max(0);
              (available_width as f32 * self.tiling_size()).round() as i32
            };

            (width, parent_rect.height())
          }
        };

        Ok(Rect::from_xy(x, y, width, height))
      }
    }
  };
}
