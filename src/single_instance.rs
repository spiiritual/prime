//! One Prime per Windows sign-in: starting it again brings the running window forward instead.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

/// Development builds get their own name so `cargo run` works beside an installed Prime.
const EVENT_NAME: &str = if cfg!(debug_assertions) {
    "Local\\spiiritual.prime.show.dev"
} else {
    "Local\\spiiritual.prime.show"
};

/// The running instance's show-request event, kept as an address because handles aren't `Send`.
static SHOW_EVENT: OnceLock<usize> = OnceLock::new();

/// Returns false when Prime is already running, after asking that instance to show its window.
pub fn claim() -> bool {
    claim_named(EVENT_NAME)
}

fn claim_named(event_name: &str) -> bool {
    let name: Vec<u16> = event_name.encode_utf16().chain([0]).collect();
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call; the handle is kept
    // open for the life of the process, which is what holds the claim.
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
    if event.is_null() {
        // Can't tell; running a second copy is better than not starting at all.
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: `event` is a valid handle to the running instance's event. Windows only lets
        // the process the user just started take the foreground, so hand that right over.
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
            SetEvent(event);
        }
        return false;
    }
    let _ = SHOW_EVENT.set(event as usize);
    true
}

/// Blocks until another start of Prime asks this one to show its window. Returns at once with
/// false when `claim` never ran, as in tests.
pub fn wait_for_show_request() -> bool {
    let Some(&event) = SHOW_EVENT.get() else {
        return false;
    };
    // SAFETY: the handle stays open for the life of the process.
    unsafe { WaitForSingleObject(event as _, INFINITE) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_start_signals_the_first_and_stands_down() {
        // A name of its own, so the test never pokes a Prime that is really running.
        let name = format!("Local\\spiiritual.prime.test.{}", uuid::Uuid::new_v4());

        assert!(claim_named(&name));
        assert!(!claim_named(&name));
        assert!(wait_for_show_request());
    }
}
