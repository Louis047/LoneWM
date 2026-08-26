use anyhow::Context;
use wm_platform::Display;

use crate::{
  commands::monitor::{
    add_monitor, move_bounded_workspaces_to_new_monitor, remove_monitor,
    sort_monitors, update_monitor,
  },
  models::{Monitor, NativeMonitorProperties},
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn handle_display_settings_changed(
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  tracing::info!("Display settings changed.");

  // Ignore the event if retrieval of the displays fails (can happen
  // transiently during sleep/wake).
  let displays = match state.dispatcher.sorted_displays() {
    Ok(displays) => displays,
    Err(err) => {
      tracing::warn!("Failed to get displays: {err}");
      state.needs_display_resync = true;
      return Ok(());
    }
  };

  // Retrieval of a display's properties can fail transiently (e.g. when a
  // monitor is connected but not yet enumerable). Rather than aborting the
  // entire reconciliation — which would leave the WM's monitor state stale
  // until restart — flag a resync and keep the current state until the
  // retry succeeds.
  //
  // See: <https://github.com/glzr-io/glazewm/issues/1233>
  let mut has_unresolved_displays = false;

  let displays: Vec<_> = displays
    .into_iter()
    .filter_map(|display| {
      match NativeMonitorProperties::try_from(&display) {
        Ok(properties) => Some((display, properties)),
        Err(err) => {
          tracing::warn!("Failed to get properties of display: {err}");
          has_unresolved_displays = true;
          None
        }
      }
    })
    .collect();

  if has_unresolved_displays {
    tracing::warn!(
      "Failed to get properties of one or more displays. Will retry."
    );

    state.needs_display_resync = true;
    return Ok(());
  }

  // Skip reconciliation when the display state is unchanged. Display
  // settings changed events can be triggered by non-display changes (e.g.
  // a USB device being connected), and reconciling would needlessly
  // reposition floating windows.
  //
  // See: <https://github.com/glzr-io/glazewm/issues/1411>
  if displays_unchanged(&displays, state) {
    tracing::debug!(
      "Display state is unchanged. Skipping reconciliation."
    );

    return Ok(());
  }

  let mut pending_monitors = state.monitors();
  let mut unmatched_displays = Vec::new();

  // Match each display to an existing monitor and update it.
  for (display, properties) in displays {
    match find_matching_monitor(&pending_monitors, &properties) {
      Some((monitor, index)) => {
        update_monitor(monitor, &display, properties, state)?;
        pending_monitors.remove(index);
      }
      None => unmatched_displays.push((display, properties)),
    }
  }

  let mut new_monitors: Vec<Monitor> = Vec::new();

  // Pair unmatched displays with unmatched monitors, or add new ones.
  for (display, properties) in unmatched_displays {
    if pending_monitors.is_empty() {
      let monitor = add_monitor(display, properties, state)?;
      new_monitors.push(monitor);
    } else {
      let monitor = pending_monitors.remove(0);
      update_monitor(&monitor, &display, properties, state)?;
    }
  }

  // Remove monitors that no longer have a corresponding display and move
  // their workspaces to other monitors.
  //
  // Prevent removal of the last monitor (i.e. for when all monitors are
  // disconnected). This will cause the WM's monitors to temporarily
  // mismatch the OS monitor state, however, it'll be updated correctly
  // when a new monitor is connected again.
  for monitor in pending_monitors {
    if state.monitors().len() > 1 {
      remove_monitor(monitor, state, config)?;
    }
  }

  // Sort monitors by position.
  sort_monitors(&state.root_container)?;

  for new_monitor in new_monitors {
    move_bounded_workspaces_to_new_monitor(&new_monitor, state, config)?;
  }

  for window in state.windows() {
    // Display setting changes can spread windows out sporadically, so mark
    // all windows as needing a DPI adjustment (just in case).
    window.set_has_pending_dpi_adjustment(true);

    // Need to update floating position of moved windows when a monitor is
    // disconnected or if the primary display is changed. The primary
    // display dictates the position of 0,0.
    let workspace = window.workspace().context("No workspace.")?;

    let should_recenter = if window.has_custom_floating_placement() {
      // Compare against the max workspace rect (which extends into the
      // outer gaps) rather than the gapped workspace rect, so that
      // floating windows positioned within the outer gaps aren't
      // needlessly recentered.
      let workspace_rect = workspace.max_workspace_rect()?;

      // Keep the placement if it still intersects the workspace, since
      // `PlatformEvent::DisplaySettingsChanged` can be triggered by
      // non-monitor changes (e.g. unplugging a USB device).
      window
        .floating_placement()
        .intersection_area(&workspace_rect)
        == 0
    } else {
      true
    };

    if should_recenter {
      // Clamp the placement so that it fits within the workspace (e.g.
      // when a monitor is disconnected and its windows move to a smaller
      // one). An oversized placement can be misclassified as fullscreen.
      //
      // See: <https://github.com/glzr-io/glazewm/issues/856>
      let max_workspace_rect = workspace.max_workspace_rect()?;

      let clamped_placement = window
        .floating_placement()
        .clamp_size(
          (max_workspace_rect.width() - 10).max(100),
          (max_workspace_rect.height() - 10).max(100),
        )
        .translate_to_center(&workspace.to_rect()?);

      window.set_floating_placement(clamped_placement);
    }
  }

  // Redraw full container tree.
  state
    .pending_sync
    .queue_container_to_redraw(state.root_container.clone());

  Ok(())
}

/// Returns whether the displays are identical to the WM's existing
/// monitors (same count, and each display matches a monitor with equal
/// properties).
fn displays_unchanged(
  displays: &[(Display, NativeMonitorProperties)],
  state: &WmState,
) -> bool {
  let monitors = state.monitors();

  displays.len() == monitors.len()
    && displays.iter().all(|(_, properties)| {
      monitors.iter().any(|monitor| {
        monitor_properties_match(&monitor.native_properties(), properties)
      })
    })
}

/// Returns whether the properties of an existing monitor match the
/// properties of a display.
fn monitor_properties_match(
  existing: &NativeMonitorProperties,
  new: &NativeMonitorProperties,
) -> bool {
  let identity_matches = existing.handle == new.handle;

  identity_matches
    && existing.device_name == new.device_name
    && existing.working_area == new.working_area
    && existing.bounds == new.bounds
    && existing.dpi == new.dpi
    && (existing.scale_factor - new.scale_factor).abs() < f32::EPSILON
}

/// Finds the monitor matching the given display properties.
///
/// Returns the monitor and its index within the list of monitors.
fn find_matching_monitor<'a>(
  monitors: &'a [Monitor],
  properties: &NativeMonitorProperties,
) -> Option<(&'a Monitor, usize)> {
  monitors.iter().enumerate().find_map(|(index, monitor)| {
    let existing = monitor.native_properties();

    let is_match = {
      // Match the monitor by:
      // 1. Its handle
      // 2. Its device path
      // 3. Its hardware ID (if unique)
      //
      // Monitor handles and device paths are unique, but can change over
      // time. The hardware ID is not guaranteed to be unique, so we
      // match against that last.
      {
        existing.handle == properties.handle
          || existing.device_path.as_deref().is_some_and(|device_path| {
            properties.device_path.as_deref() == Some(device_path)
          })
          || existing.hardware_id.as_deref().is_some_and(|hardware_id| {
            let is_unique = monitors
              .iter()
              .filter(|other_monitor| {
                other_monitor.native_properties().hardware_id.as_deref()
                  == Some(hardware_id)
              })
              .count()
              == 1;

            is_unique
              && properties.hardware_id.as_deref() == Some(hardware_id)
          })
      }
    };

    is_match.then_some((monitor, index))
  })
}
