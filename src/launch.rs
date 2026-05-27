use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

use thiserror::Error;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::Stdio;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const VALORANT_PROCESS_IMAGES: [&str; 2] = ["VALORANT-Win64-Shipping.exe", "VALORANT.exe"];
pub const RIOT_CLIENT_PROCESS_IMAGE: &str = "RiotClientServices.exe";
const RIOT_CLIENT_WINDOW_PROCESS_IMAGES: [&str; 2] =
    ["RiotClientUx.exe", RIOT_CLIENT_PROCESS_IMAGE];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchTargetProcess {
    Valorant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchConfig {
    pub riot_client_path: Option<PathBuf>,
    pub product: String,
    pub patchline: String,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            riot_client_path: None,
            product: "valorant".to_string(),
            patchline: "live".to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchPlan {
    pub executable: PathBuf,
    pub args: Vec<String>,
}

pub fn build_launch_plan(config: &LaunchConfig) -> Result<LaunchPlan, LaunchError> {
    let executable = resolve_riot_client_executable(config)?;

    Ok(LaunchPlan {
        executable,
        args: vec![
            format!("--launch-product={}", config.product),
            format!("--launch-patchline={}", config.patchline),
        ],
    })
}

pub fn build_launcher_login_capture_plan(config: &LaunchConfig) -> Result<LaunchPlan, LaunchError> {
    let executable = resolve_riot_client_executable(config)?;

    Ok(LaunchPlan {
        executable,
        args: vec![
            format!("--launch-product={}", config.product),
            "--allow-multiple-clients".to_string(),
        ],
    })
}

pub fn launch_valorant(config: &LaunchConfig) -> Result<(), LaunchError> {
    let plan = build_launch_plan(config)?;

    spawn_launch_plan(&plan)
}

pub fn launch_riot_login_capture(config: &LaunchConfig) -> Result<(), LaunchError> {
    let plan = build_launcher_login_capture_plan(config)?;

    spawn_launch_plan(&plan)
}

fn spawn_launch_plan(plan: &LaunchPlan) -> Result<(), LaunchError> {
    let mut command = Command::new(&plan.executable);
    configure_no_console_window(&mut command);

    command
        .args(&plan.args)
        .spawn()
        .map_err(LaunchError::Spawn)?;

    Ok(())
}

fn resolve_riot_client_executable(config: &LaunchConfig) -> Result<PathBuf, LaunchError> {
    match &config.riot_client_path {
        Some(path) => Ok(path.clone()),
        None => default_riot_client_candidates()
            .into_iter()
            .find(|path| path.exists())
            .ok_or(LaunchError::RiotClientNotFound),
    }
}

pub fn close_riot_processes() -> Result<(), LaunchError> {
    #[cfg(windows)]
    {
        for image in VALORANT_PROCESS_IMAGES
            .into_iter()
            .chain([RIOT_CLIENT_PROCESS_IMAGE])
        {
            let mut command = Command::new("taskkill");
            configure_no_console_window(&mut command);

            let _ = command
                .args(["/F", "/IM", image])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map_err(LaunchError::CloseProcess)?;
        }
    }

    Ok(())
}

pub fn valorant_process_is_running() -> Result<bool, LaunchError> {
    for image_name in VALORANT_PROCESS_IMAGES {
        if process_is_running(image_name)? {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn launch_target_window_is_visible() -> Result<Option<LaunchTargetProcess>, LaunchError> {
    if valorant_window_is_visible()? {
        return Ok(Some(LaunchTargetProcess::Valorant));
    }

    Ok(None)
}

#[cfg(windows)]
pub fn valorant_window_is_visible() -> Result<bool, LaunchError> {
    visible_window_belongs_to_process_image(&VALORANT_PROCESS_IMAGES)
}

#[cfg(not(windows))]
pub fn valorant_window_is_visible() -> Result<bool, LaunchError> {
    Ok(false)
}

#[cfg(windows)]
pub fn riot_client_window_is_visible() -> Result<bool, LaunchError> {
    visible_window_belongs_to_process_image(&RIOT_CLIENT_WINDOW_PROCESS_IMAGES)
}

#[cfg(not(windows))]
pub fn riot_client_window_is_visible() -> Result<bool, LaunchError> {
    Ok(false)
}

#[cfg(windows)]
fn visible_window_belongs_to_process_image(image_names: &[&str]) -> Result<bool, LaunchError> {
    let visible_process_ids = visible_top_level_window_process_ids();

    if visible_process_ids.is_empty() {
        return Ok(false);
    }

    let mut command = Command::new("tasklist");
    configure_no_console_window(&mut command);

    let output = command
        .args(["/FO", "CSV", "/NH"])
        .stderr(Stdio::null())
        .output()
        .map_err(LaunchError::ListProcesses)?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(tasklist_contains_pid_with_image(
        &stdout,
        &visible_process_ids,
        image_names,
    ))
}

#[cfg(windows)]
fn process_is_running(image_name: &str) -> Result<bool, LaunchError> {
    let filter = format!("IMAGENAME eq {image_name}");
    let mut command = Command::new("tasklist");
    configure_no_console_window(&mut command);

    let output = command
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .stderr(Stdio::null())
        .output()
        .map_err(LaunchError::ListProcesses)?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(tasklist_contains_image(&stdout, image_name))
}

#[cfg(not(windows))]
fn process_is_running(_: &str) -> Result<bool, LaunchError> {
    Ok(false)
}

fn configure_no_console_window(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
}

fn tasklist_contains_image(output: &str, image_name: &str) -> bool {
    output.lines().any(|line| {
        tasklist_image_name(line)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(image_name))
    })
}

fn tasklist_image_name(line: &str) -> Option<&str> {
    let line = line.trim();

    if let Some(rest) = line.strip_prefix('"') {
        return rest.split_once('"').map(|(image_name, _)| image_name);
    }

    line.split_whitespace().next()
}

fn tasklist_contains_pid_with_image(
    output: &str,
    process_ids: &[u32],
    image_names: &[&str],
) -> bool {
    output.lines().any(|line| {
        tasklist_process_info(line).is_some_and(|process| {
            process_ids.contains(&process.process_id)
                && image_names
                    .iter()
                    .any(|image| process.image_name.eq_ignore_ascii_case(image))
        })
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TasklistProcessInfo {
    image_name: String,
    process_id: u32,
}

fn tasklist_process_info(line: &str) -> Option<TasklistProcessInfo> {
    let (image_name, process_id) = tasklist_first_two_fields(line)?;
    let process_id = process_id.trim().parse::<u32>().ok()?;

    Some(TasklistProcessInfo {
        image_name: image_name.to_string(),
        process_id,
    })
}

fn tasklist_first_two_fields(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();

    if let Some(rest) = line.strip_prefix('"') {
        let (image_name, rest) = rest.split_once('"')?;
        let process_id = rest.strip_prefix(',')?.trim_start();

        if let Some(process_id) = process_id.strip_prefix('"') {
            return process_id
                .split_once('"')
                .map(|(process_id, _)| (image_name, process_id));
        }

        return process_id
            .split_once(',')
            .map(|(process_id, _)| (image_name, process_id));
    }

    let mut fields = line.split_whitespace();
    Some((fields.next()?, fields.next()?))
}

#[cfg(windows)]
fn visible_top_level_window_process_ids() -> Vec<u32> {
    mod user32 {
        use std::ffi::c_void;

        pub type Bool = i32;
        pub type Hwnd = *mut c_void;
        pub type Lparam = isize;

        #[link(name = "user32")]
        unsafe extern "system" {
            pub fn EnumWindows(
                enum_func: Option<unsafe extern "system" fn(Hwnd, Lparam) -> Bool>,
                lparam: Lparam,
            ) -> Bool;
            pub fn IsWindowVisible(hwnd: Hwnd) -> Bool;
            pub fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
        }
    }

    unsafe extern "system" fn collect_visible_window_process_id(
        hwnd: user32::Hwnd,
        lparam: user32::Lparam,
    ) -> user32::Bool {
        if unsafe { user32::IsWindowVisible(hwnd) } == 0 {
            return 1;
        }

        let process_ids = unsafe { &mut *(lparam as *mut Vec<u32>) };
        let mut process_id = 0;
        unsafe {
            user32::GetWindowThreadProcessId(hwnd, &mut process_id);
        }

        if process_id != 0 {
            process_ids.push(process_id);
        }

        1
    }

    let mut process_ids = Vec::new();
    unsafe {
        user32::EnumWindows(
            Some(collect_visible_window_process_id),
            (&mut process_ids as *mut Vec<u32>) as user32::Lparam,
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

#[cfg(test)]
fn riot_client_paths_from_install_manifest(contents: &str) -> Vec<PathBuf> {
    riot_client_paths_from_install_manifest_json(contents)
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
    fn explicit_path_builds_valorant_launch_arguments() {
        let config = LaunchConfig {
            riot_client_path: Some(PathBuf::from(
                r"C:\Riot Games\Riot Client\RiotClientServices.exe",
            )),
            ..LaunchConfig::default()
        };

        let plan = build_launch_plan(&config).expect("launch plan");

        assert_eq!(
            plan.args,
            vec![
                "--launch-product=valorant".to_string(),
                "--launch-patchline=live".to_string()
            ]
        );
    }

    #[test]
    fn explicit_path_builds_login_capture_arguments() {
        let config = LaunchConfig {
            riot_client_path: Some(PathBuf::from(
                r"C:\Riot Games\Riot Client\RiotClientServices.exe",
            )),
            ..LaunchConfig::default()
        };

        let plan = build_launcher_login_capture_plan(&config).expect("launch plan");

        assert_eq!(
            plan.args,
            vec![
                "--launch-product=valorant".to_string(),
                "--allow-multiple-clients".to_string()
            ]
        );
    }

    #[test]
    fn parses_executable_paths_from_riot_install_manifest() {
        let paths = riot_client_paths_from_install_manifest(
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
    fn tasklist_output_detects_valorant_process() {
        let output = r#""VALORANT-Win64-Shipping.exe","1234","Console","1","120,000 K""#;

        assert!(tasklist_contains_image(
            output,
            "VALORANT-Win64-Shipping.exe"
        ));
    }

    #[test]
    fn tasklist_output_ignores_no_matching_process_message() {
        let output = "INFO: No tasks are running which match the specified criteria.";

        assert!(!tasklist_contains_image(
            output,
            "VALORANT-Win64-Shipping.exe"
        ));
    }

    #[test]
    fn tasklist_output_supports_table_fallback() {
        let output = "VALORANT.exe   1234 Console  1  120,000 K";

        assert!(tasklist_contains_image(output, "VALORANT.exe"));
    }

    #[test]
    fn tasklist_process_info_parses_csv_pid() {
        let output = r#""RiotClientUx.exe","1234","Console","1","120,000 K""#;

        assert_eq!(
            tasklist_process_info(output),
            Some(TasklistProcessInfo {
                image_name: "RiotClientUx.exe".to_string(),
                process_id: 1234,
            })
        );
    }

    #[test]
    fn tasklist_process_info_supports_table_fallback() {
        let output = "RiotClientUx.exe   1234 Console  1  120,000 K";

        assert_eq!(
            tasklist_process_info(output),
            Some(TasklistProcessInfo {
                image_name: "RiotClientUx.exe".to_string(),
                process_id: 1234,
            })
        );
    }

    #[test]
    fn riot_client_visible_window_match_requires_visible_pid() {
        let output = r#""RiotClientUx.exe","1234","Console","1","120,000 K"
"VALORANT.exe","5678","Console","1","120,000 K""#;

        assert!(tasklist_contains_pid_with_image(
            output,
            &[1234],
            &RIOT_CLIENT_WINDOW_PROCESS_IMAGES
        ));
        assert!(!tasklist_contains_pid_with_image(
            output,
            &[5678],
            &RIOT_CLIENT_WINDOW_PROCESS_IMAGES
        ));
    }

    #[test]
    fn valorant_visible_window_match_requires_visible_pid() {
        let output = r#""VALORANT-Win64-Shipping.exe","1234","Console","1","120,000 K"
"RiotClientUx.exe","5678","Console","1","120,000 K""#;

        assert!(tasklist_contains_pid_with_image(
            output,
            &[1234],
            &VALORANT_PROCESS_IMAGES
        ));
        assert!(!tasklist_contains_pid_with_image(
            output,
            &[5678],
            &VALORANT_PROCESS_IMAGES
        ));
    }

    #[test]
    fn riot_client_visible_window_match_supports_service_process() {
        let output = r#""RiotClientServices.exe","1234","Console","1","120,000 K""#;

        assert!(tasklist_contains_pid_with_image(
            output,
            &[1234],
            &RIOT_CLIENT_WINDOW_PROCESS_IMAGES
        ));
    }
}
