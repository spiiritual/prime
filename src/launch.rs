use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

use thiserror::Error;

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
    let executable = match &config.riot_client_path {
        Some(path) => path.clone(),
        None => default_riot_client_candidates()
            .into_iter()
            .find(|path| path.exists())
            .ok_or(LaunchError::RiotClientNotFound)?,
    };

    Ok(LaunchPlan {
        executable,
        args: vec![
            format!("--launch-product={}", config.product),
            format!("--launch-patchline={}", config.patchline),
        ],
    })
}

pub fn launch_valorant(config: &LaunchConfig) -> Result<(), LaunchError> {
    let plan = build_launch_plan(config)?;

    Command::new(&plan.executable)
        .args(&plan.args)
        .spawn()
        .map_err(LaunchError::Spawn)?;

    Ok(())
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
}
