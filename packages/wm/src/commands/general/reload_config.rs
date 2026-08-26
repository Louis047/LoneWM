use anyhow::Context;
use tracing::{info, warn};
use wm_common::{HideMethod, ParsedConfig, WindowRuleEvent, WmEvent};
use wm_platform::NativeWindowWindowsExt;

use crate::{
  commands::{window::run_window_rules, workspace::sort_workspaces},
  traits::{CommonGetters, TilingSizeGetters, WindowGetters},
  user_config::UserConfig,
  wm::WindowManager,
  wm_state::WmState,
};

pub fn reload_config(
  state: &mut WmState,
  config: &mut UserConfig,
) -> anyhow::Result<()> {
  info!("Config reloaded.");

  // Keep reference to old config for comparison.
  let old_config = config.value.clone();

  // Re-evaluate user config file and set its values in state.
  config.reload()?;

  // Re-run window rules on all active windows.
  for window in state.windows() {
    window.set_done_window_rules(Vec::new());
    run_window_rules(window, &WindowRuleEvent::Manage, state, config)?;
  }

  update_workspace_configs(state, config)?;

  update_container_gaps(state, config);

  update_window_effects(&old_config, state, config)?;

  // Reset window visibility when the hide method is changed away from
  // cloaking, since cloaked windows stay invisible until uncloaked.
  if old_config.general.hide_method != config.value.general.hide_method
    && old_config.general.hide_method == HideMethod::Cloak
  {
    for window in state.windows() {
      let _ = window.native().set_cloaked(false);
    }
  }

  // Ensure all windows are shown when hide method is changed.
  if old_config.general.hide_method != config.value.general.hide_method
    && config.value.general.hide_method == HideMethod::Cloak
  {
    for window in state.windows() {
      let _ = window.native().show();
    }
  }

  // Ensure taskbar visibility matches `show_all_in_taskbar` when it's
  // changed. Windows on hidden workspaces need their taskbar buttons
  // removed when turning it off.
  //
  // See: <https://github.com/glzr-io/glazewm/issues/1394>
  if old_config.general.show_all_in_taskbar
    != config.value.general.show_all_in_taskbar
  {
    for window in state.windows() {
      let is_displayed = window
        .workspace()
        .is_some_and(|workspace| workspace.is_displayed());

      let should_show =
        config.value.general.show_all_in_taskbar || is_displayed;

      let _ = window.native().set_taskbar_visibility(should_show);
    }
  }

  // Clear active binding modes.
  state.binding_modes = Vec::new();

  // Redraw full container tree.
  state
    .pending_sync
    .queue_container_to_redraw(state.root_container.clone());

  // Emit the updated config.
  state.emit_event(WmEvent::UserConfigChanged {
    config_path: config
      .path
      .to_str()
      .context("Invalid config path.")?
      .to_string(),
    config_string: config.value_str.clone(),
    parsed_config: config.value.clone(),
  });

  // Run config reload commands.
  WindowManager::run_commands(
    &config.value.general.config_reload_commands.clone(),
    state.focused_container().context("No focused container.")?,
    state,
    config,
  )?;

  Ok(())
}

/// Update configs of active workspaces.
fn update_workspace_configs(
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let workspaces = state.workspaces();

  for workspace in &workspaces {
    let monitor = workspace.monitor().context("No monitor.")?;

    let workspace_config = config
      .value
      .workspaces
      .iter()
      .find(|config| config.name == workspace.config().name)
      .or_else(|| {
        // When the workspace config is not found, the current name of the
        // workspace has been removed. So, we reassign the first suitable
        // workspace config to the workspace.
        config
          .workspace_config_for_monitor(&monitor, &workspaces)
          .or_else(|| config.next_inactive_workspace_config(&workspaces))
      });

    match workspace_config {
      None => {
        warn!(
          "Unable to update workspace config. No available workspace configs."
        );
      }
      Some(workspace_config) => {
        if *workspace_config != workspace.config() {
          workspace.set_config(workspace_config.clone());

          sort_workspaces(&monitor, config)?;

          state.emit_event(WmEvent::WorkspaceUpdated {
            updated_workspace: workspace.to_dto()?,
          });
        }
      }
    }
  }

  Ok(())
}

/// Updates outer gap of workspaces and inner gaps of tiling containers.
fn update_container_gaps(state: &mut WmState, config: &UserConfig) {
  let tiling_containers = state
    .root_container
    .self_and_descendants()
    .filter_map(|container| container.as_tiling_container().ok());

  for container in tiling_containers {
    container.set_gaps_config(config.value.gaps.clone());
  }

  for workspace in state.workspaces() {
    workspace.set_gaps_config(config.value.gaps.clone());
  }
}

fn update_window_effects(
  old_config: &ParsedConfig,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let focused_container =
    state.focused_container().context("No focused container.")?;

  let window_effects = &config.value.window_effects;
  let old_window_effects = &old_config.window_effects;

  // Window border effects are left at system defaults if disabled in the
  // config. However, when transitioning from colored borders to having
  // them disabled, it's best to reset to the system defaults.
  if !window_effects.focused_window.border.enabled
    && old_window_effects.focused_window.border.enabled
  {
    if let Ok(window) = focused_container.as_window_container() {
      let handle = window.native().id().0;
      let needs_reset = state
        .border_stamp_cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&handle).cloned())
        .is_some_and(|stamp| stamp.color.is_some());

      if needs_reset {
        _ = window.native().set_border_color(None);
      }

      if let Ok(mut cache) = state.border_stamp_cache.lock() {
        cache.remove(&handle);
      }
    }
  }

  if !window_effects.other_windows.border.enabled
    && old_window_effects.other_windows.border.enabled
  {
    let unfocused_windows = state
      .windows()
      .into_iter()
      .filter(|window| window.id() != focused_container.id());

    for window in unfocused_windows {
      let handle = window.native().id().0;
      let needs_reset = state
        .border_stamp_cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&handle).cloned())
        .is_some_and(|stamp| stamp.color.is_some());

      if needs_reset {
        _ = window.native().set_border_color(None);
      }

      if let Ok(mut cache) = state.border_stamp_cache.lock() {
        cache.remove(&handle);
      }
    }
  }

  state.pending_sync.queue_all_effects_update();

  Ok(())
}
