use std::cell::Cell;

use windows::Win32::{
  Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
  System::Threading::GetCurrentThreadId,
  UI::{
    Input::KeyboardAndMouse::{
      GetAsyncKeyState, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
      VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
    },
    WindowsAndMessaging::{
      CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
      UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
      WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
    },
  },
};

use crate::{Dispatcher, Key, KeyCode};

/// Callback stored in [`HOOK`] for intercepting keyboard events.
type HookCallback = Box<dyn Fn(KeyEvent) -> bool>;

thread_local! {
  /// Stores the hook callback for the current thread.
  ///
  /// The hook callback is called for every keyboard event and returns
  /// `true` if the event should be intercepted.
  static HOOK: Cell<Option<HookCallback>> = Cell::default();
}

/// A key event received from the keyboard hook.
#[derive(Clone, Debug)]
pub struct KeyEvent {
  /// The key that was pressed or released.
  pub key: Key,

  /// Key code that generated this event.
  #[allow(dead_code)]
  pub key_code: KeyCode,

  /// Whether the event is for a key press or release.
  pub is_keypress: bool,
}

impl KeyEvent {
  /// Gets whether the specified key is currently pressed.
  ///
  /// NOTE: `GetAsyncKeyState` is used rather than `GetKeyState` since the
  /// hook runs on a dedicated thread that doesn't process window
  /// messages. `GetKeyState` reflects the message queue state of the
  /// calling thread, which can lag behind the physical keyboard state.
  #[allow(clippy::unused_self)]
  pub fn is_key_down(&self, key: Key) -> bool {
    match key {
      Key::Win => {
        Self::is_key_down_raw(VK_LWIN.0)
          || Self::is_key_down_raw(VK_RWIN.0)
      }
      Key::Alt => {
        Self::is_key_down_raw(VK_LMENU.0)
          || Self::is_key_down_raw(VK_RMENU.0)
      }
      Key::Ctrl => {
        Self::is_key_down_raw(VK_LCONTROL.0)
          || Self::is_key_down_raw(VK_RCONTROL.0)
      }
      Key::Shift => {
        Self::is_key_down_raw(VK_LSHIFT.0)
          || Self::is_key_down_raw(VK_RSHIFT.0)
      }
      _ => {
        if let Ok(key_code) = KeyCode::try_from(key) {
          Self::is_key_down_raw(key_code.0)
        } else {
          false
        }
      }
    }
  }

  /// Gets whether the specified key is currently down using the raw key
  /// code.
  ///
  /// A key is down when the high-order bit of the returned state is set,
  /// i.e. the state is negative.
  fn is_key_down_raw(key: u16) -> bool {
    let state = unsafe { GetAsyncKeyState(key.into()) };
    state < 0
  }
}

/// A system-wide low-level keyboard hook.
#[derive(Debug)]
pub struct KeyboardHook {
  /// Thread ID of the dedicated hook thread.
  thread_id: u32,
}

impl KeyboardHook {
  /// Creates an instance of `KeyboardHook`.
  ///
  /// The callback is called for every keyboard event and returns `true`
  /// if the event should be intercepted.
  ///
  /// The hook is installed on a dedicated thread with its own message
  /// loop, so that keyboard events are delivered promptly regardless of
  /// how busy the shared event loop thread is (e.g. during window event
  /// floods).
  pub fn new<F>(
    callback: F,
    _dispatcher: &Dispatcher,
  ) -> crate::Result<Self>
  where
    F: Fn(KeyEvent) -> bool + Send + Sync + 'static,
  {
    let (result_tx, result_rx): (
      std::sync::mpsc::Sender<crate::Result<u32>>,
      std::sync::mpsc::Receiver<crate::Result<u32>>,
    ) = std::sync::mpsc::channel();

    std::thread::Builder::new()
      .name("keyboard-hook".to_string())
      .spawn(move || {
        HOOK.with(|state| {
          state.set(Some(Box::new(callback)));
        });

        let handle = match unsafe {
          SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(Self::hook_proc),
            HINSTANCE::default(),
            0,
          )
        } {
          Ok(handle) => handle,
          Err(err) => {
            let _ = result_tx.send(Err(err.into()));
            HOOK.with(Cell::take);
            return;
          }
        };

        let thread_id = unsafe { GetCurrentThreadId() };

        if result_tx.send(Ok(thread_id)).is_err() {
          // Receiver was dropped; clean up the hook.
          let _ = unsafe { UnhookWindowsHookEx(handle) };
          HOOK.with(Cell::take);
          return;
        }

        // Low-level hook callbacks are delivered through this thread's
        // message loop, so pump messages until a quit message is
        // received.
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&raw mut msg, None, 0, 0) }.as_bool() {
          // No window to dispatch messages to.
        }

        let _ = unsafe { UnhookWindowsHookEx(handle) };
        HOOK.with(Cell::take);
      })
      .map_err(|_| {
        crate::Error::Platform(
          "Failed to spawn keyboard hook thread.".to_string(),
        )
      })?;

    let thread_id = result_rx.recv().map_err(|_| {
      crate::Error::Platform(
        "Keyboard hook thread failed to start.".to_string(),
      )
    })??;

    Ok(Self { thread_id })
  }

  /// Terminates the keyboard hook by unregistering it.
  pub fn terminate(&mut self) -> crate::Result<()> {
    // Stop the hook thread's message loop. The thread unhooks and
    // cleans up its state on exit.
    unsafe {
      PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0))
    }?;

    Ok(())
  }

  /// Hook procedure for keyboard events.
  ///
  /// For use with `SetWindowsHookExW`.
  extern "system" fn hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
  ) -> LRESULT {
    // If the code is less than zero, the hook procedure must pass the hook
    // notification directly to other applications.
    if code != 0 {
      return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    // Get struct with the keyboard input event.
    let input = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };

    #[allow(clippy::cast_possible_truncation)]
    let key_code = KeyCode(input.vkCode as u16);
    #[allow(clippy::cast_possible_truncation)]
    let is_keypress =
      wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;

    let Ok(key) = Key::try_from(key_code) else {
      return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };

    let key_event = KeyEvent {
      key,
      key_code,
      is_keypress,
    };

    let should_intercept = HOOK.with(|state| {
      if let Some(callback) = state.take() {
        let result = callback(key_event);
        state.set(Some(callback));
        result
      } else {
        false
      }
    });

    if should_intercept {
      return LRESULT(1);
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
  }
}

impl Drop for KeyboardHook {
  fn drop(&mut self) {
    let _ = self.terminate();
  }
}
