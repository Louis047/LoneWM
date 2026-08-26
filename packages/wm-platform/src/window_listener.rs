use std::{
  collections::HashSet,
  sync::{Arc, Mutex},
};

use tokio::sync::mpsc;

use crate::{platform_impl, Dispatcher, WindowEvent, WindowId};

/// A listener for system-wide window events.
pub struct WindowListener {
  event_rx: mpsc::UnboundedReceiver<WindowEvent>,

  /// Inner platform-specific window listener.
  inner: platform_impl::WindowListener,
}

impl WindowListener {
  /// Creates a new window listener.
  ///
  /// The `managed_window_ids` set is used to filter high-frequency
  /// events (e.g. location changes) for windows that aren't managed by
  /// the WM.
  pub fn new(
    managed_window_ids: Arc<Mutex<HashSet<WindowId>>>,
    dispatcher: &Dispatcher,
  ) -> crate::Result<Self> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let inner = platform_impl::WindowListener::new(
      managed_window_ids,
      event_tx,
      dispatcher,
    )?;

    Ok(Self { event_rx, inner })
  }

  /// Returns the next window event from the listener.
  ///
  /// This will block until a window event is available.
  pub async fn next_event(&mut self) -> Option<WindowEvent> {
    self.event_rx.recv().await
  }

  /// Terminates the window listener.
  pub fn terminate(&mut self) {
    self.inner.terminate();
  }
}
