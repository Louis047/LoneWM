use std::{
  cell::{Ref, RefCell, RefMut},
  collections::VecDeque,
  rc::Rc,
};

use anyhow::Context;
use uuid::Uuid;
use wm_common::{
  ActiveDrag, ContainerDto, DisplayState, FullscreenMode, GapsConfig,
  TilingDirection, WindowDto, WindowRuleConfig, WindowState,
};
use wm_platform::{NativeWindow, Rect, RectDelta};

use crate::{
  impl_common_getters, impl_container_debug,
  impl_position_getters_as_resizable, impl_tiling_size_getters,
  impl_window_getters,
  models::{
    Container, DirectionContainer, InsertionTarget,
    NativeWindowProperties, NonTilingWindow, TilingContainer,
    WindowContainer,
  },
  traits::{
    CommonGetters, PositionGetters, TilingDirectionGetters,
    TilingSizeGetters, WindowGetters,
  },
};

#[derive(Clone)]
pub struct TilingWindow(Rc<RefCell<TilingWindowInner>>);

struct TilingWindowInner {
  id: Uuid,
  parent: Option<Container>,
  children: VecDeque<Container>,
  child_focus_order: VecDeque<Uuid>,
  tiling_size: f32,
  native: NativeWindow,
  native_properties: NativeWindowProperties,
  state: WindowState,
  prev_state: Option<WindowState>,
  display_state: DisplayState,
  border_delta: RectDelta,
  has_pending_dpi_adjustment: bool,
  floating_placement: Rect,
  has_custom_floating_placement: bool,
  gaps_config: GapsConfig,
  done_window_rules: Vec<WindowRuleConfig>,
  active_drag: Option<ActiveDrag>,
}

impl TilingWindow {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    id: Option<Uuid>,
    native: NativeWindow,
    properties: NativeWindowProperties,
    prev_state: Option<WindowState>,
    border_delta: RectDelta,
    floating_placement: Rect,
    has_custom_floating_placement: bool,
    gaps_config: GapsConfig,
    done_window_rules: Vec<WindowRuleConfig>,
    active_drag: Option<ActiveDrag>,
  ) -> Self {
    let window = TilingWindowInner {
      id: id.unwrap_or_else(Uuid::new_v4),
      parent: None,
      children: VecDeque::new(),
      child_focus_order: VecDeque::new(),
      tiling_size: 1.0,
      native,
      native_properties: properties,
      state: WindowState::Tiling,
      prev_state,
      display_state: DisplayState::Shown,
      border_delta,
      has_pending_dpi_adjustment: false,
      floating_placement,
      has_custom_floating_placement,
      gaps_config,
      done_window_rules,
      active_drag,
    };

    Self(Rc::new(RefCell::new(window)))
  }

  pub fn to_non_tiling(
    &self,
    state: WindowState,
    insertion_target: Option<InsertionTarget>,
  ) -> NonTilingWindow {
    // Default the floating placement to the window's current tiling rect
    // (in native frame coordinates). Otherwise, the window would be
    // redrawn at its stale manage-time placement (e.g. the app's small
    // default launch size) when transitioning to a non-tiling state.
    //
    // See: <https://github.com/glzr-io/glazewm/issues/1015>
    let floating_placement = if self.has_custom_floating_placement() {
      self.floating_placement()
    } else {
      self
        .to_rect()
        .and_then(|rect| {
          Ok(rect.apply_delta(&self.total_border_delta()?, None))
        })
        .unwrap_or_else(|_| self.floating_placement())
    };

    NonTilingWindow::new(
      Some(self.id()),
      self.native().clone(),
      self.native_properties().clone(),
      state,
      Some(WindowState::Tiling),
      self.border_delta(),
      insertion_target,
      floating_placement,
      self.has_custom_floating_placement(),
      self.done_window_rules(),
      self.active_drag(),
    )
  }

  pub fn to_dto(&self) -> anyhow::Result<ContainerDto> {
    let rect = self.to_rect()?;

    Ok(ContainerDto::Window(WindowDto {
      id: self.id(),
      parent_id: self.parent().map(|parent| parent.id()),
      has_focus: self.has_focus(None),
      tiling_size: Some(self.tiling_size()),
      width: rect.width(),
      height: rect.height(),
      x: rect.x(),
      y: rect.y(),
      state: self.state(),
      prev_state: self.prev_state(),
      display_state: self.display_state(),
      border_delta: self.border_delta(),
      floating_placement: self.floating_placement(),
      #[allow(clippy::cast_possible_wrap, clippy::unnecessary_cast)]
      handle: self.native().id().0 as isize,
      title: self.native_properties().title,
      class_name: self.native_properties().class_name,
      process_name: self.native_properties().process_name,
      active_drag: self.active_drag(),
    }))
  }
}

impl_container_debug!(TilingWindow);
impl_common_getters!(TilingWindow);
impl_tiling_size_getters!(TilingWindow);
impl_position_getters_as_resizable!(TilingWindow);
impl_window_getters!(TilingWindow);

impl PositionGetters for TilingWindow {
  fn to_tiling_rect(&self) -> anyhow::Result<Rect> {
    self.calculate_tiling_rect()
  }

  fn to_rect(&self) -> anyhow::Result<Rect> {
    match self.state() {
      WindowState::Fullscreen(config) => match config.effective_mode() {
        FullscreenMode::Full => {
          let monitor = self.monitor().context("No monitor.")?;
          monitor.to_rect()
        }
        FullscreenMode::Monocle => {
          let workspace = self.workspace().context("No workspace.")?;
          if config.respect_gaps {
            workspace.to_rect()
          } else {
            workspace.max_workspace_rect()
          }
        }
      },
      _ => self.calculate_tiling_rect(),
    }
  }
}
