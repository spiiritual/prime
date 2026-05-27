use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

use thiserror::Error;

pub const VALORANT_PROCESS_IMAGES: [&str; 2] = ["VALORANT-Win64-Shipping.exe", "VALORANT.exe"];

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
    Command::new(&plan.executable)
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
            .chain(["RiotClientServices.exe"])
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/IM", image])
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

#[cfg(windows)]
fn process_is_running(image_name: &str) -> Result<bool, LaunchError> {
    let filter = format!("IMAGENAME eq {image_name}");
    let output = Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .map_err(LaunchError::ListProcesses)?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(tasklist_contains_image(&stdout, image_name))
}

#[cfg(not(windows))]
fn process_is_running(_: &str) -> Result<bool, LaunchError> {
    Ok(false)
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
    #[error("failed to check whether VALORANT is running: {0}")]
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
}
