//! Game window discovery and HWND management.
//!
//! Enumerates visible windows and finds the "Duet Night Abyss" game window
//! using Win32 `EnumWindows` + `GetWindowTextW`.

use anyhow::{Result, bail};
use tracing::{debug, instrument};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextW, IsWindow,
};
use windows::core::BOOL;

/// Window title substring used to identify the game window.
pub const GAME_WINDOW_TITLE: &str = "Duet Night Abyss";

/// Find the game window by scanning all top-level windows.
///
/// Searches for a window whose title contains [`GAME_WINDOW_TITLE`].
/// Returns the first matching `HWND`.
///
/// # Errors
///
/// Returns an error if no window matching the game title is found.
#[instrument]
pub fn find_game() -> Result<HWND> {
    let mut found: Option<HWND> = None;

    // SAFETY: `EnumWindows` calls the provided callback for each top-level window.
    // The callback writes to `found` via the LPARAM pointer. The pointer remains
    // valid for the duration of the `EnumWindows` call because `found` lives on
    // the current stack frame.
    //
    // Note: when the callback returns FALSE (match found, stop enumeration),
    // `EnumWindows` returns FALSE which the `windows` crate maps to Err.
    // We ignore this "error" since it is the expected early-exit path.
    unsafe {
        #[allow(clippy::as_conversions)] // LPARAM requires isize from raw pointer
        let lparam = LPARAM(&raw mut found as isize);
        let _ = EnumWindows(Some(enum_window_callback), lparam);
    }

    match found {
        Some(hwnd) => {
            debug!(?hwnd, "found game window");
            Ok(hwnd)
        }
        None => bail!("no window matching \"{GAME_WINDOW_TITLE}\" found"),
    }
}

/// Check whether the given window handle is still valid.
#[instrument]
pub fn is_window_alive(hwnd: HWND) -> bool {
    // SAFETY: `IsWindow` is a read-only check that accepts any HWND value.
    // Passing an invalid HWND simply returns FALSE without side effects.
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

/// Check if the game window is currently the foreground (active) window.
#[must_use]
pub fn is_game_foreground() -> bool {
    // SAFETY: `GetForegroundWindow` returns the handle of the foreground window.
    // No special safety requirements beyond valid Win32 state.
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return false;
    }
    let mut title_buf = [0u16; 256];
    // SAFETY: Reading the window title into a stack buffer.
    let len = unsafe { GetWindowTextW(fg, &mut title_buf) };
    if len == 0 {
        return false;
    }
    let len_usize = usize::try_from(len).unwrap_or(0);
    #[allow(clippy::indexing_slicing)]
    let title = String::from_utf16_lossy(&title_buf[..len_usize]);
    title.contains(GAME_WINDOW_TITLE)
}

/// Callback for `EnumWindows`. Writes the first matching HWND into the
/// `Option<HWND>` pointed to by `lparam`.
///
/// # Safety
///
/// `lparam` must point to a valid `Option<HWND>` that outlives the callback.
unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut title_buf = [0u16; 256];

    // SAFETY: `GetWindowTextW` reads the window title into the provided buffer.
    // The buffer is stack-allocated and valid for the duration of this call.
    let len = unsafe { GetWindowTextW(hwnd, &mut title_buf) };
    if len == 0 {
        return BOOL(1); // continue enumeration
    }

    let len_usize = usize::try_from(len).unwrap_or(0);
    #[allow(clippy::indexing_slicing)] // len_usize is bounded by buffer size
    let title = String::from_utf16_lossy(&title_buf[..len_usize]);

    if title.contains(GAME_WINDOW_TITLE) {
        #[allow(clippy::as_conversions)] // LPARAM.0 to pointer cast is unavoidable
        // SAFETY: `lparam` points to a valid `Option<HWND>` per the function contract.
        let found = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
        *found = Some(hwnd);
        return BOOL(0); // stop enumeration
    }

    BOOL(1) // continue enumeration
}
