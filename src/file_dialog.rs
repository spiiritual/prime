//! Windows' own Open dialog.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

/// Asks for an existing `.exe`, starting in `start_dir`. Blocks until the dialog closes, and
/// returns `None` if it was cancelled.
pub fn pick_exe(title: &str, start_dir: Option<&Path>) -> Option<PathBuf> {
    let mut file = vec![0u16; 1024];
    let filter: Vec<u16> = "Programs (*.exe)\0*.exe\0\0".encode_utf16().collect();
    let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
    let start_dir: Option<Vec<u16>> =
        start_dir.map(|dir| dir.as_os_str().encode_wide().chain([0]).collect());

    let mut options = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrTitle: title.as_ptr(),
        lpstrInitialDir: start_dir
            .as_ref()
            .map_or(std::ptr::null(), |dir| dir.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };

    // SAFETY: every pointer in `options` points into a buffer above that outlives the call, and
    // `nMaxFile` is the length of `file`.
    if unsafe { GetOpenFileNameW(&mut options) } == 0 {
        return None;
    }

    let len = file
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(file.len());
    Some(PathBuf::from(OsString::from_wide(&file[..len])))
}
