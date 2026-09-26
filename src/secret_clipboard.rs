//! Copies secrets to the clipboard while keeping them out of Windows clipboard history (Win+V) and
//! cloud clipboard sync.

use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SecretClipboardError {
    #[error("the clipboard is in use by another program; try again")]
    Busy,
    #[error("Windows could not write to the clipboard")]
    WriteFailed,
    #[error("copying secrets is only supported on Windows")]
    Unsupported,
}

#[cfg(windows)]
pub fn copy_secret_text(text: &str) -> Result<(), SecretClipboardError> {
    use std::thread;
    use std::time::Duration;

    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
    };

    const CF_UNICODETEXT: u32 = 13;
    // Clipboard history, cloud sync, and other clipboard monitors skip data carrying these.
    const PRIVACY_FORMATS: [&str; 3] = [
        "ExcludeClipboardContentFromMonitorProcessing",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
    ];

    // Another program may be holding the clipboard for a moment.
    let mut opened = false;
    for _ in 0..10 {
        // SAFETY: a null owner window is allowed; the clipboard is closed below.
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if !opened {
        return Err(SecretClipboardError::Busy);
    }

    let result = (|| {
        // SAFETY: the clipboard is open on this thread.
        if unsafe { EmptyClipboard() } == 0 {
            return Err(SecretClipboardError::WriteFailed);
        }

        let text_bytes = text
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_ne_bytes)
            .collect::<Vec<_>>();
        set_clipboard_bytes(CF_UNICODETEXT, &text_bytes)?;

        for format in PRIVACY_FORMATS {
            let name = format
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            // SAFETY: `name` is a null-terminated UTF-16 string.
            let format_id = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
            if format_id == 0 {
                return Err(SecretClipboardError::WriteFailed);
            }
            set_clipboard_bytes(format_id, &0u32.to_ne_bytes())?;
        }

        Ok(())
    })();

    // SAFETY: the clipboard was opened above.
    unsafe { CloseClipboard() };
    result
}

#[cfg(windows)]
fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), SecretClipboardError> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::SetClipboardData;
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };

    // SAFETY: the allocation is checked, written within its size, and either handed to the
    // clipboard (which then owns it) or freed.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if memory.is_null() {
            return Err(SecretClipboardError::WriteFailed);
        }

        let target = GlobalLock(memory);
        if target.is_null() {
            GlobalFree(memory);
            return Err(SecretClipboardError::WriteFailed);
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        GlobalUnlock(memory);

        if SetClipboardData(format, memory).is_null() {
            GlobalFree(memory);
            return Err(SecretClipboardError::WriteFailed);
        }
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn copy_secret_text(_text: &str) -> Result<(), SecretClipboardError> {
    Err(SecretClipboardError::Unsupported)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Overwrites the real clipboard, so it only runs when asked for:
    /// `cargo test copies_secret_text -- --ignored`.
    #[test]
    #[ignore]
    fn copies_secret_text() {
        copy_secret_text("prime clipboard test").expect("copy");
    }
}
