use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use directories::ProjectDirs;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;
use thiserror::Error;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GITHUB_REPOSITORY: &str = "spiiritual/prime";

const GITHUB_API_VERSION: &str = "2022-11-28";
const USER_AGENT_VALUE: &str = concat!("prime/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailableUpdate {
    pub current_version: String,
    pub latest_version: String,
    pub release_name: Option<String>,
    pub release_url: String,
    pub asset: ReleaseAsset,
}

impl AvailableUpdate {
    pub fn display_name(&self) -> &str {
        self.release_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.latest_version)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size_bytes: u64,
}

pub async fn check_for_update() -> Result<Option<AvailableUpdate>, UpdateError> {
    let release = fetch_latest_release(GITHUB_REPOSITORY).await?;
    update_from_release(CURRENT_VERSION, release)
}

pub async fn download_and_prepare_update(update: AvailableUpdate) -> Result<(), UpdateError> {
    let staged_exe = download_release_asset(&update).await?;
    launch_self_replace(&staged_exe)
}

async fn fetch_latest_release(repository: &str) -> Result<GitHubRelease, UpdateError> {
    let url = format!("https://api.github.com/repos/{repository}/releases/latest");

    reqwest::Client::new()
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .map_err(UpdateError::Http)
}

fn update_from_release(
    current_version: &str,
    release: GitHubRelease,
) -> Result<Option<AvailableUpdate>, UpdateError> {
    if !latest_version_is_newer(current_version, &release.tag_name)? {
        return Ok(None);
    }

    let latest_version = display_version(&release.tag_name);
    let asset = compatible_release_asset(&release).ok_or_else(|| {
        UpdateError::NoCompatibleReleaseAsset {
            latest_version: latest_version.clone(),
        }
    })?;

    Ok(Some(AvailableUpdate {
        current_version: current_version.to_string(),
        latest_version,
        release_name: release.name,
        release_url: release.html_url,
        asset,
    }))
}

fn compatible_release_asset(release: &GitHubRelease) -> Option<ReleaseAsset> {
    select_release_asset(
        &release.assets,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .map(|asset| ReleaseAsset {
        name: asset.name.clone(),
        download_url: asset.browser_download_url.clone(),
        size_bytes: asset.size,
    })
}

fn select_release_asset<'a>(
    assets: &'a [GitHubAsset],
    target_os: &str,
    target_arch: &str,
) -> Option<&'a GitHubAsset> {
    assets
        .iter()
        .filter_map(|asset| asset_score(asset, target_os, target_arch).map(|score| (score, asset)))
        .max_by_key(|(score, _)| *score)
        .map(|(_, asset)| asset)
}

fn asset_score(asset: &GitHubAsset, target_os: &str, target_arch: &str) -> Option<u32> {
    if asset.browser_download_url.trim().is_empty() {
        return None;
    }

    let name = asset.name.to_ascii_lowercase();
    let mut score = 0;

    if name.contains("prime") {
        score += 8;
    }

    match target_os {
        "windows" => {
            let is_exe = name.ends_with(".exe");
            let is_zip = name.ends_with(".zip");

            if !is_exe && !is_zip {
                return None;
            }

            score += if is_exe { 40 } else { 25 };

            if name == "prime.exe" {
                score += 30;
            }

            if contains_any(&name, &["windows", "win32", "win64", "win"]) {
                score += 20;
            }
        }
        "macos" => {
            if !contains_any(&name, &["macos", "darwin", "apple"]) {
                return None;
            }

            score += 20;
        }
        "linux" => {
            if !contains_any(&name, &["linux", "gnu", "musl", "appimage"]) {
                return None;
            }

            score += 20;
        }
        other => {
            if !name.contains(other) {
                return None;
            }

            score += 10;
        }
    }

    let mentions_arch = mentions_known_arch(&name);
    let matches_arch = matches_target_arch(&name, target_arch);

    if mentions_arch && !matches_arch {
        return None;
    }

    if matches_arch {
        score += 20;
    }

    Some(score)
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn arch_aliases(arch: &str) -> &'static [&'static str] {
    match arch {
        "x86_64" => &["x86_64", "x64", "amd64"],
        "x86" => &["i686", "i386", "win32"],
        "aarch64" => &["aarch64", "arm64"],
        "arm" => &["arm"],
        _ => &[],
    }
}

fn mentions_known_arch(name: &str) -> bool {
    contains_any(
        name,
        &[
            "x86_64", "x64", "amd64", "i686", "i386", "win32", "aarch64", "arm64",
        ],
    ) || (name.contains("x86") && !name.contains("x86_64"))
}

fn matches_target_arch(name: &str, target_arch: &str) -> bool {
    contains_any(name, arch_aliases(target_arch))
        || (target_arch == "x86" && name.contains("x86") && !name.contains("x86_64"))
}

async fn download_release_asset(update: &AvailableUpdate) -> Result<PathBuf, UpdateError> {
    let bytes = reqwest::Client::new()
        .get(&update.asset.download_url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    if bytes.is_empty() {
        return Err(UpdateError::EmptyDownload);
    }

    let staging_dir = update_staging_dir(&update.latest_version);
    fs::create_dir_all(&staging_dir)?;

    let extension = Path::new(&update.asset.name)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.trim().is_empty())
        .unwrap_or("exe");
    let staged_exe = staging_dir.join(format!(
        "prime-{}.{}",
        sanitize_path_component(&update.latest_version),
        extension
    ));
    fs::write(&staged_exe, bytes)?;

    Ok(staged_exe)
}

fn update_staging_dir(version: &str) -> PathBuf {
    ProjectDirs::from("dev", "spiiritual", "prime")
        .map(|dirs| dirs.cache_dir().join("updates"))
        .unwrap_or_else(|| std::env::temp_dir().join("prime-updates"))
        .join(sanitize_path_component(version))
}

fn launch_self_replace(staged_exe: &Path) -> Result<(), UpdateError> {
    let current_exe = std::env::current_exe()?;
    let script_path = staged_exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("install-prime-update.ps1");

    fs::write(&script_path, POWERSHELL_SELF_UPDATE_SCRIPT)?;

    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .arg("-ProcessId")
        .arg(std::process::id().to_string())
        .arg("-Source")
        .arg(staged_exe)
        .arg("-Destination")
        .arg(&current_exe)
        .arg("-Relaunch")
        .arg(&current_exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    command.creation_flags(0x0800_0000);

    command.spawn()?;
    Ok(())
}

const POWERSHELL_SELF_UPDATE_SCRIPT: &str = r#"
param(
    [int]$ProcessId,
    [string]$Source,
    [string]$Destination,
    [string]$Relaunch
)

$ErrorActionPreference = 'Stop'

Wait-Process -Id $ProcessId -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

$Backup = "$Destination.old"

if (Test-Path -LiteralPath $Backup) {
    Remove-Item -LiteralPath $Backup -Force -ErrorAction SilentlyContinue
}

if (Test-Path -LiteralPath $Destination) {
    Move-Item -LiteralPath $Destination -Destination $Backup -Force
}

$Payload = $Source
$ExtractDir = Join-Path (Split-Path -Parent $Source) 'payload'

if ([System.IO.Path]::GetExtension($Source) -ieq '.zip') {
    if (Test-Path -LiteralPath $ExtractDir) {
        Remove-Item -LiteralPath $ExtractDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    New-Item -ItemType Directory -Path $ExtractDir -Force | Out-Null
    Expand-Archive -LiteralPath $Source -DestinationPath $ExtractDir -Force

    $Payload = Get-ChildItem -LiteralPath $ExtractDir -Recurse -File -Filter '*.exe' |
        Where-Object { $_.Name -like 'prime*.exe' } |
        Select-Object -First 1 -ExpandProperty FullName

    if (-not $Payload) {
        throw 'The update archive did not contain a Prime executable.'
    }
}

Move-Item -LiteralPath $Payload -Destination $Destination -Force
Start-Process -FilePath $Relaunch

if (Test-Path -LiteralPath $Backup) {
    Remove-Item -LiteralPath $Backup -Force -ErrorAction SilentlyContinue
}

if (Test-Path -LiteralPath $ExtractDir) {
    Remove-Item -LiteralPath $ExtractDir -Recurse -Force -ErrorAction SilentlyContinue
}

if (Test-Path -LiteralPath $Source) {
    Remove-Item -LiteralPath $Source -Force -ErrorAction SilentlyContinue
}

Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
"#;

fn latest_version_is_newer(current: &str, latest: &str) -> Result<bool, UpdateError> {
    compare_version_strings(current, latest)
        .map(|ordering| ordering == Ordering::Less)
        .ok_or_else(|| UpdateError::InvalidReleaseVersion(latest.to_string()))
}

fn compare_version_strings(left: &str, right: &str) -> Option<Ordering> {
    let left = ParsedVersion::parse(left)?;
    let right = ParsedVersion::parse(right)?;
    let max_len = left.numbers.len().max(right.numbers.len()).max(3);

    for index in 0..max_len {
        let left_part = *left.numbers.get(index).unwrap_or(&0);
        let right_part = *right.numbers.get(index).unwrap_or(&0);

        match left_part.cmp(&right_part) {
            Ordering::Equal => {}
            ordering => return Some(ordering),
        }
    }

    match (&left.pre_release, &right.pre_release) {
        (None, None) => Some(Ordering::Equal),
        (None, Some(_)) => Some(Ordering::Greater),
        (Some(_), None) => Some(Ordering::Less),
        (Some(left), Some(right)) => Some(compare_prerelease(left, right)),
    }
}

fn compare_prerelease(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('.');
    let mut right_parts = right.split('.');

    loop {
        match (left_parts.next(), right_parts.next()) {
            (Some(left), Some(right)) => {
                let ordering = compare_prerelease_part(left, right);

                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}

fn compare_prerelease_part(left: &str, right: &str) -> Ordering {
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

fn display_version(tag_name: &str) -> String {
    tag_name
        .trim()
        .trim_start_matches(|ch| ch == 'v' || ch == 'V')
        .to_string()
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "update".to_string()
    } else {
        sanitized
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedVersion {
    numbers: Vec<u64>,
    pre_release: Option<String>,
}

impl ParsedVersion {
    fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        let mut candidate = trimmed.trim_start_matches(|ch| ch == 'v' || ch == 'V');

        if candidate == trimmed
            && let Some(index) = trimmed.find(|ch: char| ch.is_ascii_digit())
        {
            candidate = &trimmed[index..];
        }

        let candidate = candidate
            .split_once('+')
            .map_or(candidate, |(core, _)| core);
        let (core, pre_release) = candidate
            .split_once('-')
            .map_or((candidate, None), |(core, pre_release)| {
                (core, Some(pre_release.to_string()))
            });
        let numbers = core
            .split('.')
            .map(|part| {
                if part.is_empty() {
                    None
                } else {
                    part.parse::<u64>().ok()
                }
            })
            .collect::<Option<Vec<_>>>()?;

        if numbers.is_empty() {
            return None;
        }

        Some(Self {
            numbers,
            pre_release,
        })
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("GitHub release HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("update I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("latest GitHub release version could not be compared: {0}")]
    InvalidReleaseVersion(String),
    #[error(
        "latest GitHub release {latest_version} does not include a compatible executable asset"
    )]
    NoCompatibleReleaseAsset { latest_version: String },
    #[error("downloaded update was empty")]
    EmptyDownload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_accepts_v_prefixed_tags() {
        assert!(latest_version_is_newer("0.1.0", "v0.2.0").expect("comparison"));
        assert!(!latest_version_is_newer("0.2.0", "v0.2.0").expect("comparison"));
        assert!(!latest_version_is_newer("0.3.0", "v0.2.0").expect("comparison"));
    }

    #[test]
    fn version_comparison_handles_prerelease_ordering() {
        assert!(latest_version_is_newer("0.2.0-beta.1", "0.2.0").expect("comparison"));
        assert!(!latest_version_is_newer("0.2.0", "0.2.0-beta.1").expect("comparison"));
        assert!(latest_version_is_newer("0.2.0-beta.1", "0.2.0-beta.2").expect("comparison"));
    }

    #[test]
    fn version_parser_can_find_version_inside_tag_name() {
        assert!(latest_version_is_newer("0.1.0", "prime-v0.2.0").expect("comparison"));
    }

    #[test]
    fn windows_asset_selection_prefers_prime_executable_for_arch() {
        let assets = vec![
            asset("prime-linux-x86_64", "https://example.com/linux", 10),
            asset(
                "prime-windows-x86_64.exe",
                "https://example.com/windows",
                20,
            ),
            asset("prime.exe", "https://example.com/plain", 30),
        ];

        let selected = select_release_asset(&assets, "windows", "x86_64").expect("asset");

        assert_eq!(selected.name, "prime-windows-x86_64.exe");
    }

    #[test]
    fn windows_asset_selection_accepts_release_archives() {
        let assets = vec![asset(
            "prime-windows-x86_64.zip",
            "https://example.com/windows.zip",
            20,
        )];

        let selected = select_release_asset(&assets, "windows", "x86_64").expect("asset");

        assert_eq!(selected.name, "prime-windows-x86_64.zip");
    }

    #[test]
    fn windows_asset_selection_prefers_executable_over_archive() {
        let assets = vec![
            asset(
                "prime-windows-x86_64.zip",
                "https://example.com/windows.zip",
                20,
            ),
            asset(
                "prime-windows-x86_64.exe",
                "https://example.com/windows.exe",
                20,
            ),
        ];

        let selected = select_release_asset(&assets, "windows", "x86_64").expect("asset");

        assert_eq!(selected.name, "prime-windows-x86_64.exe");
    }

    #[test]
    fn windows_asset_selection_rejects_wrong_architecture() {
        let assets = vec![asset(
            "prime-windows-arm64.exe",
            "https://example.com/windows-arm64.exe",
            20,
        )];

        assert!(select_release_asset(&assets, "windows", "x86_64").is_none());
    }

    #[test]
    fn newer_release_without_asset_is_an_error() {
        let release = GitHubRelease {
            tag_name: "v0.2.0".to_string(),
            name: Some("Prime 0.2.0".to_string()),
            html_url: "https://github.com/spiiritual/prime/releases/tag/v0.2.0".to_string(),
            assets: vec![],
        };

        let error = update_from_release("0.1.0", release).expect_err("asset error");

        assert!(matches!(
            error,
            UpdateError::NoCompatibleReleaseAsset { .. }
        ));
    }

    fn asset(name: &str, url: &str, size: u64) -> GitHubAsset {
        GitHubAsset {
            name: name.to_string(),
            browser_download_url: url.to_string(),
            size,
        }
    }
}
