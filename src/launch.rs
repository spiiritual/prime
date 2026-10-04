use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use thiserror::Error;

use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const VALORANT_PROCESS_IMAGES: [&str; 2] = ["VALORANT-Win64-Shipping.exe", "VALORANT.exe"];
pub const RIOT_CLIENT_PROCESS_IMAGE: &str = "RiotClientServices.exe";
const RIOT_CLIENT_PROCESS_IMAGES: [&str; 2] = ["RiotClientUx.exe", RIOT_CLIENT_PROCESS_IMAGE];

pub fn launch_valorant(riot_client_path: Option<&Path>) -> Result<(), LaunchError> {
    start_riot_client(
        riot_client_path,
        &["--launch-product=valorant", "--launch-patchline=live"],
    )
}

pub fn launch_riot_login_capture(riot_client_path: Option<&Path>) -> Result<(), LaunchError> {
    start_riot_client(
        riot_client_path,
        &["--launch-product=valorant", "--allow-multiple-clients"],
    )
}

fn start_riot_client(riot_client_path: Option<&Path>, args: &[&str]) -> Result<(), LaunchError> {
    let executable = match riot_client_path {
        Some(path) => path.to_path_buf(),
        None => default_riot_client_candidates()
            .into_iter()
            .find(|path| path.exists())
            .ok_or(LaunchError::RiotClientNotFound)?,
    };

    Command::new(executable)
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .spawn()
        .map_err(LaunchError::Spawn)?;

    Ok(())
}

pub fn close_riot_processes() -> Result<(), LaunchError> {
    close_process_images(
        VALORANT_PROCESS_IMAGES
            .into_iter()
            .chain(RIOT_CLIENT_PROCESS_IMAGES),
    )
}

pub fn close_riot_client_processes() -> Result<(), LaunchError> {
    close_process_images(RIOT_CLIENT_PROCESS_IMAGES)
}

/// Ends every process running one of `images`, as `taskkill /F /IM` would, without starting a
/// taskkill for each. Returns once they have exited, so their files are free to replace.
fn close_process_images(images: impl IntoIterator<Item = &'static str>) -> Result<(), LaunchError> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };

    /// How long one process may take to exit before its files are touched anyway.
    const EXIT_WAIT_MS: u32 = 3_000;

    let images: Vec<&str> = images.into_iter().collect();
    let processes = running_processes().map_err(LaunchError::CloseProcess)?;
    // All are ended first and then waited for, so they exit together.
    // SAFETY: each handle is checked before use and closed after.
    let handles: Vec<_> = processes
        .iter()
        .filter(|process| process.is_one_of(&images))
        .filter_map(|process| unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, process.id);
            // A process that already exited, or that this user can't end, is left as it is,
            // like taskkill's errors were.
            (!handle.is_null()).then(|| {
                TerminateProcess(handle, 1);
                handle
            })
        })
        .collect();
    for handle in handles {
        // SAFETY: as above.
        unsafe {
            WaitForSingleObject(handle, EXIT_WAIT_MS);
            CloseHandle(handle);
        }
    }

    Ok(())
}

pub fn valorant_process_is_running() -> Result<bool, LaunchError> {
    Ok(running_processes()
        .map_err(LaunchError::ListProcesses)?
        .iter()
        .any(|process| process.is_one_of(&VALORANT_PROCESS_IMAGES)))
}

pub fn valorant_window_is_visible() -> Result<bool, LaunchError> {
    visible_window_belongs_to_process_image(&VALORANT_PROCESS_IMAGES)
}

pub fn riot_client_window_is_visible() -> Result<bool, LaunchError> {
    visible_window_belongs_to_process_image(&RIOT_CLIENT_PROCESS_IMAGES)
}

fn visible_window_belongs_to_process_image(image_names: &[&str]) -> Result<bool, LaunchError> {
    let visible_process_ids = visible_top_level_window_process_ids();

    if visible_process_ids.is_empty() {
        return Ok(false);
    }

    Ok(running_processes()
        .map_err(LaunchError::ListProcesses)?
        .iter()
        .any(|process| visible_process_ids.contains(&process.id) && process.is_one_of(image_names)))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RunningProcess {
    id: u32,
    image_name: String,
}

impl RunningProcess {
    fn is_one_of(&self, image_names: &[&str]) -> bool {
        image_names
            .iter()
            .any(|image| self.image_name.eq_ignore_ascii_case(image))
    }
}

/// Every running process's ID and executable name, from one Toolhelp snapshot. Starting
/// tasklist for this took hundreds of milliseconds.
fn running_processes() -> std::io::Result<Vec<RunningProcess>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: the snapshot handle is checked, only read through `entry` with its size set, and
    // closed before returning.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        let mut processes = Vec::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let name = &entry.szExeFile;
            let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            processes.push(RunningProcess {
                id: entry.th32ProcessID,
                image_name: String::from_utf16_lossy(&name[..len]),
            });
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);

        Ok(processes)
    }
}

fn visible_top_level_window_process_ids() -> Vec<u32> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows_sys::core::BOOL;

    unsafe extern "system" fn collect_visible_window_process_id(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> BOOL {
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }

        let process_ids = unsafe { &mut *(lparam as *mut Vec<u32>) };
        let mut process_id = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut process_id);
        }

        if process_id != 0 {
            process_ids.push(process_id);
        }

        1
    }

    let mut process_ids = Vec::new();
    unsafe {
        EnumWindows(
            Some(collect_visible_window_process_id),
            (&mut process_ids as *mut Vec<u32>) as LPARAM,
        );
    }

    process_ids.sort_unstable();
    process_ids.dedup();
    process_ids
}

pub fn default_riot_client_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from(
        r"C:\Riot Games\Riot Client\RiotClientServices.exe",
    )];

    if let Some(program_data) = env::var_os("ProgramData") {
        let manifest = PathBuf::from(program_data)
            .join("Riot Games")
            .join("RiotClientInstalls.json");

        if let Ok(contents) = fs::read_to_string(manifest) {
            candidates.extend(riot_client_paths_from_install_manifest_json(&contents));
        }
    }

    candidates
}

fn riot_client_paths_from_install_manifest_json(contents: &str) -> Vec<PathBuf> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    collect_riot_client_paths(&value, &mut paths);
    paths.sort();
    paths.dedup();
    paths
}

fn collect_riot_client_paths(value: &serde_json::Value, paths: &mut Vec<PathBuf>) {
    match value {
        serde_json::Value::String(path)
            if path
                .to_ascii_lowercase()
                .ends_with(r"riotclientservices.exe") =>
        {
            paths.push(PathBuf::from(path));
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_riot_client_paths(value, paths);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_riot_client_paths(value, paths);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("RiotClientServices.exe was not found; set the Riot Client path in Settings")]
    RiotClientNotFound,
    #[error("failed to launch Riot Client: {0}")]
    Spawn(std::io::Error),
    #[error("failed to close Riot process before switching accounts: {0}")]
    CloseProcess(std::io::Error),
    #[error("failed to check running processes: {0}")]
    ListProcesses(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_executable_paths_from_riot_install_manifest() {
        let paths = riot_client_paths_from_install_manifest_json(
            r#"{
                "rc_default": "D:\\Riot Games\\Riot Client\\RiotClientServices.exe",
                "ignored": "D:\\Riot Games\\Riot Client\\RiotClientInstalls.json",
                "nested": {
                    "rc_live": "E:\\Riot Games\\Riot Client\\RiotClientServices.exe"
                }
            }"#,
        );

        assert_eq!(
            paths,
            vec![
                PathBuf::from(r"D:\Riot Games\Riot Client\RiotClientServices.exe"),
                PathBuf::from(r"E:\Riot Games\Riot Client\RiotClientServices.exe")
            ]
        );
    }

    #[test]
    fn process_image_names_match_in_any_case() {
        let process = RunningProcess {
            id: 1234,
            image_name: "valorant-win64-shipping.exe".to_string(),
        };

        assert!(process.is_one_of(&VALORANT_PROCESS_IMAGES));
        assert!(!process.is_one_of(&RIOT_CLIENT_PROCESS_IMAGES));
    }

    #[test]
    fn closing_an_image_ends_its_processes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("prime-close-test.exe");
        fs::copy(r"C:\Windows\System32\ping.exe", &exe).expect("copy ping");
        let mut child = Command::new(&exe)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("start ping copy");

        close_process_images(["prime-close-test.exe"]).expect("close");

        // Already exited by the time closing returns.
        let status = child.try_wait().expect("check").expect("exited");
        assert_eq!(status.code(), Some(1));
    }

    #[test]
    fn running_processes_include_this_test() {
        let this = std::process::id();

        let processes = running_processes().expect("process snapshot");

        assert!(processes.iter().any(|process| process.id == this
            && process.image_name.to_ascii_lowercase().ends_with(".exe")));
    }
}
