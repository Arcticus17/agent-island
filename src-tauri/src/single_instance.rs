//! Single-instance guard for the resident island.
//!
//! A second launch must not create a second island. The duplicate would show a
//! second always-on-top window, run a second poller and write the same statistics
//! files, while the hook server keeps port 127.0.0.1:8799 bound to the first
//! process — leaving the duplicate deaf to Claude hook events. This guard holds a
//! named mutex for the process lifetime; when another island owns it, the caller
//! surfaces that window instead of starting anything.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, BOOL, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_SHOW,
};

/// Mutex owned by the running island. The `Local\` prefix scopes it to the
/// current sign-in session, so two signed-in users can each run their own island.
pub const ISLAND_MUTEX: &str = r"Local\AgentIsland.SingleInstance";

/// Returned when another process already owns the requested instance name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyRunning;

/// Holds the instance mutex for as long as it is alive.
#[derive(Debug)]
pub struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Claims `name` for this process.
///
/// `Err(AlreadyRunning)` means another island already owns the name; the caller
/// must stop before creating windows, the tray, the poller or the hook server.
pub fn acquire(name: &str) -> Result<InstanceGuard, AlreadyRunning> {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    let handle = unsafe { CreateMutexW(None, BOOL(0), PCWSTR(wide.as_ptr())) };
    let Ok(handle) = handle else {
        return Err(AlreadyRunning);
    };
    // The last error must be read before any other fallible call can overwrite it.
    let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already_running {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(AlreadyRunning);
    }
    Ok(InstanceGuard(handle))
}

/// Best-effort: bring the island that already owns the mutex to the front.
///
/// The title must stay in sync with the `main` window title in `tauri.conf.json`;
/// the `w!` macro only accepts a literal, so the two cannot share a constant.
pub fn focus_running_island() {
    unsafe {
        let Ok(window) = FindWindowW(PCWSTR::null(), w!("Agent Island")) else {
            return;
        };
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_name(suffix: &str) -> String {
        format!(r"Local\AgentIsland.Test.{}.{suffix}", std::process::id())
    }

    #[test]
    fn second_acquire_of_the_same_name_reports_already_running() {
        let name = test_name("same");
        let guard = acquire(&name).expect("first acquire must succeed");

        assert_eq!(acquire(&name).err(), Some(AlreadyRunning));

        drop(guard);
        assert!(
            acquire(&name).is_ok(),
            "a released instance name must be acquirable again"
        );
    }

    #[test]
    fn distinct_names_do_not_block_each_other() {
        let first = acquire(&test_name("a")).expect("first name must succeed");
        let second = acquire(&test_name("b")).expect("an unrelated name must not be blocked");

        drop(first);
        drop(second);
    }
}
