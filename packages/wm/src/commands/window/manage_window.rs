use anyhow::Context;
use tracing::info;
use wm_common::{
  try_warn, GapsConfig, WindowRuleEvent, WindowState, WmEvent,
};
use wm_platform::{NativeWindow, NativeWindowWindowsExt, RectDelta};

use crate::{
  commands::{
    container::{
      attach_container, set_focused_descendant, wrap_in_split_container,
    },
    window::run_window_rules,
  },
  models::{
    Container, Monitor, NativeWindowProperties, NonTilingWindow,
    SplitContainer, TilingWindow, WindowContainer,
  },
  traits::{
    CommonGetters, PositionGetters, TilingDirectionGetters, WindowGetters,
  },
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn manage_window(
  native_window: NativeWindow,
  target_parent: Option<Container>,
  state: &mut WmState,
  config: &mut UserConfig,
) -> anyhow::Result<()> {
  let Some(native_properties) =
    check_is_manageable(&native_window).unwrap_or(None)
  else {
    return Ok(());
  };

  // Elevated windows can't be managed unless the WM itself is elevated or
  // has UIAccess — the OS silently blocks moving, resizing and focusing
  // them, leaving a phantom slot in the layout. Ignore them instead.
  //
  // See: <https://github.com/glzr-io/glazewm/issues/867>
  // See: <https://github.com/glzr-io/glazewm/issues/1041>
  if !state.can_manage_elevated
    && native_window.is_elevated().unwrap_or(false)
  {
    tracing::warn!(
      "Ignoring elevated window '{}'. To manage elevated windows, run the \
       WM as admin or use the signed installer build (UIAccess).",
      native_properties.title
    );

    state.ignored_windows.push(native_window);
    return Ok(());
  }

  // Create the window instance. This may fail if the window handle has
  // already been destroyed.
  let window = try_warn!(create_window(
    native_window,
    native_properties,
    target_parent,
    state,
    config
  ));

  // Record when the window was managed, so that app-initiated placement
  // changes shortly after launch can be distinguished from user-initiated
  // ones.
  state
    .managed_timestamps
    .insert(window.id(), std::time::Instant::now());

  // Index the window for O(1) lookups and hook-side event filtering.
  state.index_window(&window);

  // Set the newly added window as focus descendant. This means the window
  // rules will be run as if the window is focused.
  set_focused_descendant(&window.clone().into(), None);

  // Window might be detached if `ignore` command has been invoked.
  let updated_window = run_window_rules(
    window.clone(),
    &WindowRuleEvent::Manage,
    state,
    config,
  )?;

  if let Some(window) = updated_window {
    info!("New window managed: {window}");

    state.emit_event(WmEvent::WindowManaged {
      managed_window: window.to_dto()?,
    });

    // OS focus should be set to the newly added window in case it's not
    // already focused.
    state.is_focus_synced = true;
    state.pending_sync.queue_focus_change();

    // Normally, a `PlatformEvent::WindowFocused` event is what triggers
    // focus effects and workspace reordering to be applied. However, when
    // a window is first launched, this event can come before the
    // window is managed, and so we need to force an update here.
    state.pending_sync.queue_focused_effect_update();
    state.pending_sync.queue_workspace_to_reorder(
      window.workspace().context("No workspace.")?,
    );

    // Sibling containers need to be redrawn if the window is tiling.
    state.pending_sync.queue_container_to_redraw(
      if window.state() == WindowState::Tiling {
        window.parent().context("No parent.")?
      } else {
        window.into()
      },
    );
  }

  Ok(())
}

/// Checks if a window is manageable and retrieves its native properties.
///
/// Returns `Ok(Some(properties))` if the window is manageable and its
/// properties were retrieved successfully.
fn check_is_manageable(
  native_window: &NativeWindow,
) -> anyhow::Result<Option<NativeWindowProperties>> {
  if !native_window.is_visible()? {
    return Ok(None);
  }

  // Ensure window has a valid process name, title, etc.
  let native_properties = NativeWindowProperties::try_from(native_window)?;

  {
    use wm_platform::{
      NativeWindowWindowsExt, WS_CAPTION, WS_CHILD, WS_EX_NOACTIVATE,
      WS_EX_TOOLWINDOW,
    };

    // Class names of top-level shell host windows (e.g. taskbar thumbnail
    // previews, tray overflow flyouts) that would otherwise get managed,
    // steal focus, and get the focused border applied around the whole
    // screen.
    //
    // See: <https://github.com/glzr-io/glazewm/issues/978>
    const SHELL_WINDOW_CLASSES: [&str; 5] = [
      "TaskListThumbnailWnd",
      "XamlExplorerHostIslandWindow",
      "TopLevelWindowForOverflowXamlIsland",
      "Windows.UI.Composition.DesktopWindowContentBridge",
      "Shell_SecondaryTrayWnd",
    ];

    if native_properties.process_name == "explorer"
      && SHELL_WINDOW_CLASSES
        .contains(&native_properties.class_name.as_str())
    {
      return Ok(None);
    }

    // TODO: Temporary fix for managing Flow Launcher until a force manage
    // command is added.
    let is_flow_launcher = native_properties.process_name
      == "Flow.Launcher"
      && native_properties.title == "Flow.Launcher";

    if !is_flow_launcher {
      // Ensure window is top-level (i.e. not a child window). Ignore
      // windows that cannot be focused or if they're unavailable in
      // task switcher (alt+tab menu).
      if native_window.has_window_style(WS_CHILD)
        || native_window
          .has_window_style_ex(WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW)
      {
        return Ok(None);
      }

      // Some applications spawn top-level windows for menus that
      // should be ignored. This includes the autocomplete popup in
      // Notepad++ and title bar menu in Keepass. Although not
      // foolproof, these can typically be identified by having an
      // owner window and no title bar.
      if native_window.has_owner_window()
        && !native_window.has_window_style(WS_CAPTION)
      {
        return Ok(None);
      }
    }
  }

  Ok(Some(native_properties))
}

fn create_window(
  native_window: NativeWindow,
  native_properties: NativeWindowProperties,
  target_parent: Option<Container>,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WindowContainer> {
  let nearest_monitor = state
    .nearest_monitor(&native_window)
    .context("No nearest monitor.")?;

  let nearest_workspace = nearest_monitor
    .displayed_workspace()
    .context("No nearest workspace.")?;

  let gaps_config = config.value.gaps.clone();
  let window_state =
    window_state_to_create(&native_properties, &nearest_monitor, config)?;

  // Attach the new window as the first child of the target parent (if
  // provided), otherwise, add as a sibling of the focused container.
  let (target_parent, target_index) = match target_parent {
    Some(parent) => {
      // The target parent is only provided when populating the initial
      // state on startup. Tile pre-existing windows in dwindle spirals.
      if window_state == WindowState::Tiling {
        let workspace = parent.workspace().context("No workspace.")?;

        let sibling = workspace
          .descendant_focus_order()
          .find(Container::is_tiling_window);

        if let Some(sibling) = sibling {
          let (parent, index) =
            dwindle_insertion_target(&sibling, &config.value.gaps);

          (parent, index)
        } else {
          (parent, 0)
        }
      } else {
        (parent, 0)
      }
    }
    None => insertion_target(&window_state, state, config)?,
  };

  let target_workspace =
    target_parent.workspace().context("No target workspace.")?;

  let prefers_centered = config
    .value
    .window_behavior
    .state_defaults
    .floating
    .centered;

  // Calculate where window should be placed when floating is enabled. Use
  // the original width/height of the window and optionally position it in
  // the center of the workspace.
  let is_same_workspace = nearest_workspace.id() == target_workspace.id();
  let floating_placement = {
    let placement = if !is_same_workspace || prefers_centered {
      native_properties
        .frame
        .translate_to_center(&target_workspace.to_rect()?)
    } else {
      native_properties.frame.clone()
    };

    // Clamp the window size to be within the workspace's outer gaps. 10px
    // is arbitrary - helps differentiate from tiling windows.
    let max_workspace_rect = target_workspace.max_workspace_rect()?;
    placement.clamp_size(
      max_workspace_rect.width() - 10,
      max_workspace_rect.height() - 10,
    )
  };

  // Window has no border delta unless it's later changed via the
  // `adjust_borders` command.
  let border_delta = RectDelta::zero();

  // Seed the previous state for windows that start as fullscreen (e.g.
  // an app that was fullscreened before the WM started), so that state
  // transitions out of fullscreen work and the window gets properly
  // marked as fullscreen with the OS.
  //
  // See: <https://github.com/glzr-io/glazewm/issues/682>
  let prev_state = matches!(window_state, WindowState::Fullscreen(_))
    .then(|| WindowState::default_from_config(&config.value));

  let window_container: WindowContainer = match window_state {
    WindowState::Tiling => TilingWindow::new(
      None,
      native_window,
      native_properties,
      prev_state,
      border_delta,
      floating_placement,
      false,
      gaps_config,
      Vec::new(),
      None,
    )
    .into(),
    _ => NonTilingWindow::new(
      None,
      native_window,
      native_properties,
      window_state,
      prev_state,
      border_delta,
      None,
      floating_placement,
      !prefers_centered,
      Vec::new(),
      None,
    )
    .into(),
  };

  attach_container(
    &window_container.clone().into(),
    &target_parent,
    Some(target_index),
  )?;

  // The OS might spawn the window on a different monitor to the target
  // parent, so adjustments might need to be made because of DPI.
  if nearest_monitor
    .has_dpi_difference(&window_container.clone().into())?
  {
    window_container.set_has_pending_dpi_adjustment(true);
  }

  Ok(window_container)
}

/// Gets the initial state for a window based on its native state.
///
/// Note that maximized windows are initialized as tiling.
fn window_state_to_create(
  native_properties: &NativeWindowProperties,
  nearest_monitor: &Monitor,
  config: &UserConfig,
) -> anyhow::Result<WindowState> {
  if native_properties.is_minimized {
    return Ok(WindowState::Minimized);
  }

  let nearest_workspace = nearest_monitor
    .displayed_workspace()
    .context("No workspace.")?;

  // Only initialize as fullscreen if the window *exceeds* the workspace
  // bounds by more than the fullscreen tolerance.
  //
  // For example, with 0px outer gaps and a window that covers the entire
  // workspace, it would still not be initialized as fullscreen. The window
  // needs to extend past the workspace's outer gaps by the tolerance on
  // each side.
  //
  // See: <https://github.com/glzr-io/glazewm/issues/682>
  if !native_properties.is_maximized
    && native_properties
      .frame
      .inset(crate::traits::ENTER_FULLSCREEN_TOLERANCE)
      .contains_rect(&nearest_workspace.max_workspace_rect()?)
  {
    return Ok(WindowState::Fullscreen(
      config.value.window_behavior.state_defaults.fullscreen,
    ));
  }

  // Initialize windows that can't be resized as floating.
  if !native_properties.is_resizable {
    return Ok(WindowState::Floating(
      config.value.window_behavior.state_defaults.floating.clone(),
    ));
  }

  Ok(WindowState::default_from_config(&config.value))
}

/// Gets where to insert a new window in the container tree.
///
/// Rules:
/// - For non-tiling windows: Always append to the workspace.
/// - For tiling windows:
///   1. Try to insert after the focused tiling window if one exists.
///   2. If a non-tiling window is focused, try to insert after the first
///      tiling window found.
///   3. If no tiling windows exist, append to the workspace.
///
/// Returns tuple of (parent container, insertion index).
fn insertion_target(
  window_state: &WindowState,
  state: &WmState,
  config: &UserConfig,
) -> anyhow::Result<(Container, usize)> {
  let focused_container =
    state.focused_container().context("No focused container.")?;

  let focused_workspace =
    focused_container.workspace().context("No workspace.")?;

  // For tiling windows, try to find a suitable tiling window to insert
  // next to.
  if *window_state == WindowState::Tiling {
    let sibling = match focused_container {
      Container::TilingWindow(_) => Some(focused_container),
      _ => focused_workspace
        .descendant_focus_order()
        .find(Container::is_tiling_window),
    };

    if let Some(sibling) = sibling {
      return Ok(dwindle_insertion_target(&sibling, &config.value.gaps));
    }
  }

  // Default to appending to workspace.
  Ok((
    focused_workspace.clone().into(),
    focused_workspace.child_count(),
  ))
}

/// Gets the insertion target for a new tiling window in a dwindle
/// workspace: the focused tiling window is wrapped in a new split
/// container, and the new window goes into that split as its second
/// child.
///
/// The split direction alternates by tree depth, relative to the
/// workspace's tiling direction — producing the characteristic
/// side-by-side, then top-bottom, … spiral toward the bottom-right
/// corner. Splits are persistent once created (like Hyprland's
/// `preserve_split`): closing a window collapses its parent split, and
/// manual `toggle-tiling-direction` overrides are kept.
pub(crate) fn dwindle_insertion_target(
  sibling: &Container,
  gaps_config: &GapsConfig,
) -> (Container, usize) {
  let workspace = sibling.workspace().expect(
    "Dwindle sibling is attached to the workspace tree and always has \
     one.",
  );

  // Derive the new split's direction from the sibling's immediate
  // parent: invert the parent's direction so splits alternate.
  // If the sibling is a direct workspace child (no parent split),
  // use the workspace's own tiling direction.
  let split_direction = match sibling.parent() {
    Some(Container::Split(parent_split)) => {
      parent_split.tiling_direction().inverse()
    }
    // Direct workspace child — use workspace direction.
    _ => workspace.tiling_direction(),
  };

  let split_container =
    SplitContainer::new(split_direction, gaps_config.clone());

  let parent = sibling.parent().expect(
    "Dwindle sibling is attached to the workspace tree and always has \
     a parent.",
  );

  wrap_in_split_container(
    &split_container,
    &parent,
    &[sibling.clone().as_tiling_container().expect(
      "Dwindle sibling is a tiling window and thus a tiling container.",
    )],
  )
  .expect("Wrapping a tiling sibling in a split cannot fail.");

  // The wrapped sibling is at index 0; the new window goes after it,
  // i.e. to the right/bottom of the split.
  (split_container.into(), 1)
}

#[cfg(test)]
mod dwindle_tests {
  use wm_common::{GapsConfig, TilingDirection};

  use super::dwindle_insertion_target;
  use crate::{
    commands::container::attach_container,
    models::{TilingWindow, Workspace},
    traits::{CommonGetters, TilingDirectionGetters, TilingSizeGetters},
  };

  fn mock_workspace(direction: TilingDirection) -> Workspace {
    Workspace::mock().tiling_direction(direction).call()
  }

  fn mock_window() -> TilingWindow {
    TilingWindow::mock().call()
  }

  /// First insertion: the only window is wrapped in a split whose
  /// direction matches the workspace's direction (side-by-side).
  #[test]
  fn first_split_matches_workspace_direction() {
    let workspace = mock_workspace(TilingDirection::Horizontal);
    let window = mock_window();

    attach_container(
      &window.clone().into(),
      &workspace.clone().into(),
      None,
    )
    .unwrap();

    let (target_parent, target_index) = dwindle_insertion_target(
      &window.clone().into(),
      &GapsConfig::default(),
    );

    let split = target_parent.as_split().unwrap();
    assert_eq!(split.tiling_direction(), TilingDirection::Horizontal);
    assert_eq!(split.parent().unwrap().id(), workspace.id());
    assert_eq!(split.child_count(), 1);
    assert_eq!(split.children()[0].id(), window.id());
    assert_eq!(target_index, 1);
  }

  /// Second insertion: a window inside one split is wrapped in a split
  /// with the inverse direction (top-bottom), cascading toward the
  /// bottom-right.
  #[test]
  fn second_split_inverts_direction() {
    let workspace = mock_workspace(TilingDirection::Horizontal);
    let window = mock_window();

    attach_container(
      &window.clone().into(),
      &workspace.clone().into(),
      None,
    )
    .unwrap();

    let (outer_parent, _) = dwindle_insertion_target(
      &window.clone().into(),
      &GapsConfig::default(),
    );

    let second_window = mock_window();
    attach_container(
      &second_window.clone().into(),
      &outer_parent,
      Some(1),
    )
    .unwrap();

    let (inner_parent, _) = dwindle_insertion_target(
      &second_window.clone().into(),
      &GapsConfig::default(),
    );

    let inner_split = inner_parent.as_split().unwrap();
    assert_eq!(inner_split.tiling_direction(), TilingDirection::Vertical);
    assert_eq!(inner_split.parent().unwrap().id(), outer_parent.id());
    assert_eq!(
      inner_split.parent().unwrap().as_split().unwrap().id(),
      outer_parent.as_split().unwrap().id()
    );
  }

  /// Third insertion: depth 2 returns to the workspace's direction,
  /// continuing the alternating spiral.
  #[test]
  fn third_split_matches_workspace_direction_again() {
    let workspace = mock_workspace(TilingDirection::Horizontal);
    let window = mock_window();

    attach_container(
      &window.clone().into(),
      &workspace.clone().into(),
      None,
    )
    .unwrap();

    let (outer_parent, _) = dwindle_insertion_target(
      &window.clone().into(),
      &GapsConfig::default(),
    );

    let second_window = mock_window();
    attach_container(
      &second_window.clone().into(),
      &outer_parent,
      Some(1),
    )
    .unwrap();

    let (inner_parent, _) = dwindle_insertion_target(
      &second_window.clone().into(),
      &GapsConfig::default(),
    );

    let third_window = mock_window();
    attach_container(&third_window.clone().into(), &inner_parent, Some(1))
      .unwrap();

    let (deepest_parent, _) = dwindle_insertion_target(
      &third_window.clone().into(),
      &GapsConfig::default(),
    );

    let deepest_split = deepest_parent.as_split().unwrap();
    assert_eq!(
      deepest_split.tiling_direction(),
      TilingDirection::Horizontal
    );
  }

  /// Attaching into a dwindle split yields an even 50/50 split, matching
  /// the dwindle split ratio.
  #[test]
  fn attached_window_gets_half_of_split() {
    let workspace = mock_workspace(TilingDirection::Horizontal);
    let window = mock_window();

    attach_container(
      &window.clone().into(),
      &workspace.clone().into(),
      None,
    )
    .unwrap();

    let (target_parent, target_index) = dwindle_insertion_target(
      &window.clone().into(),
      &GapsConfig::default(),
    );

    let new_window = mock_window();
    attach_container(
      &new_window.clone().into(),
      &target_parent,
      Some(target_index),
    )
    .unwrap();

    let split = target_parent.as_split().unwrap();
    let sizes: Vec<f32> = split
      .children()
      .iter()
      .map(|child| child.as_tiling_container().unwrap().tiling_size())
      .collect();

    assert!((sizes[0] - 0.5).abs() < f32::EPSILON);
    assert!((sizes[1] - 0.5).abs() < f32::EPSILON);
  }

  /// A vertical workspace spirals bottom-first instead of right-first.
  #[test]
  fn vertical_workspace_starts_vertical() {
    let workspace = mock_workspace(TilingDirection::Vertical);
    let window = mock_window();

    attach_container(
      &window.clone().into(),
      &workspace.clone().into(),
      None,
    )
    .unwrap();

    let (target_parent, _) = dwindle_insertion_target(
      &window.clone().into(),
      &GapsConfig::default(),
    );

    let split = target_parent.as_split().unwrap();
    assert_eq!(split.tiling_direction(), TilingDirection::Vertical);
  }

  /// Multi-window dwindle trees correctly detect multiple windows for
  /// outer gaps.
  #[test]
  fn multi_window_dwindle_outer_gaps() {
    use wm_platform::LengthValue;

    let gaps_config = GapsConfig {
      outer_gap: wm_platform::RectDelta {
        top: LengthValue::from_px(16),
        right: LengthValue::from_px(16),
        bottom: LengthValue::from_px(16),
        left: LengthValue::from_px(16),
      },
      single_window_outer_gap: Some(wm_platform::RectDelta {
        top: LengthValue::from_px(32),
        right: LengthValue::from_px(32),
        bottom: LengthValue::from_px(32),
        left: LengthValue::from_px(32),
      }),
      ..Default::default()
    };

    let workspace =
      Workspace::mock().gaps_config(gaps_config.clone()).call();
    let w1 = mock_window();
    attach_container(&w1.clone().into(), &workspace.clone().into(), None)
      .unwrap();

    // 1 window -> uses single_window_outer_gap (32px)
    assert_eq!(workspace.outer_gaps().top, LengthValue::from_px(32));

    // Add w2 via dwindle wrapping
    let (p1, idx1) = dwindle_insertion_target(&w1.into(), &gaps_config);
    let w2 = mock_window();
    attach_container(&w2.clone().into(), &p1, Some(idx1)).unwrap();

    // 2 windows -> uses multi-window outer_gap (16px)
    assert_eq!(workspace.outer_gaps().top, LengthValue::from_px(16));
  }

  /// Fullscreen mode state equality and transitions.
  #[test]
  fn fullscreen_mode_transitions() {
    use wm_common::{FullscreenMode, FullscreenStateConfig, WindowState};

    let full_state = WindowState::Fullscreen(FullscreenStateConfig {
      mode: FullscreenMode::Full,
      maximized: false,
      shown_on_top: false,
      respect_gaps: true,
    });

    let monocle_state = WindowState::Fullscreen(FullscreenStateConfig {
      mode: FullscreenMode::Monocle,
      maximized: false,
      shown_on_top: false,
      respect_gaps: true,
    });

    // Full vs Monocle are distinct states for smooth transitions
    assert!(!full_state.is_same_state(&monocle_state));
    assert!(full_state.is_same_state(&full_state));
    assert!(monocle_state.is_same_state(&monocle_state));
  }

  /// Fullscreen toggle in a 4-window dwindle workspace preserves the tree
  /// topology and sibling window dimensions without squishing or
  /// re-wrapping.
  #[test]
  fn fullscreen_toggle_preserves_tree_topology_and_sizing() {
    use wm_common::WindowState;

    use crate::{
      commands::window::update_window_state, user_config::UserConfig,
      wm_state::WmState,
    };

    let gaps = GapsConfig::default();
    let workspace = mock_workspace(TilingDirection::Horizontal);
    let mut state = WmState::mock();

    let w1 = mock_window(); // e.g. Zen Browser
    attach_container(&w1.clone().into(), &workspace.clone().into(), None)
      .unwrap();

    let (p1, idx1) = dwindle_insertion_target(&w1.clone().into(), &gaps);
    let w2 = mock_window();
    attach_container(&w2.clone().into(), &p1, Some(idx1)).unwrap();

    let (p2, idx2) = dwindle_insertion_target(&w2.clone().into(), &gaps);
    let w3 = mock_window();
    attach_container(&w3.clone().into(), &p2, Some(idx2)).unwrap();

    let (p3, idx3) = dwindle_insertion_target(&w3.clone().into(), &gaps);
    let w4 = mock_window();
    attach_container(&w4.clone().into(), &p3, Some(idx3)).unwrap();

    let w1_parent_before = w1.parent().unwrap().id();
    let w2_parent_before = w2.parent().unwrap().id();

    // Toggle w2 to Monocle Fullscreen
    let monocle_state =
      WindowState::Fullscreen(wm_common::FullscreenStateConfig {
        mode: wm_common::FullscreenMode::Monocle,
        maximized: false,
        shown_on_top: false,
        respect_gaps: true,
      });

    let config = UserConfig::mock();
    let w2_fs = update_window_state(
      w2.clone().into(),
      monocle_state,
      &mut state,
      &config,
    )
    .unwrap();

    // w2 stays in tree as TilingWindow with Fullscreen state
    assert!(w2_fs.is_tiling_window());
    assert_eq!(w1.parent().unwrap().id(), w1_parent_before);
    assert_eq!(w2.parent().unwrap().id(), w2_parent_before);

    // Toggle w2 back to Tiling
    let w2_tiled =
      update_window_state(w2_fs, WindowState::Tiling, &mut state, &config)
        .unwrap();

    assert!(w2_tiled.is_tiling_window());
    assert_eq!(w1.parent().unwrap().id(), w1_parent_before);
    assert_eq!(w2.parent().unwrap().id(), w2_parent_before);
  }
}
