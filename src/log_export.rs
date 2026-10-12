//! Settings > Export logs: one zip a user can send when Prime or VALORANT misbehaves. It holds
//! Prime's logs, VALORANT's logs and crash reports (not their memory dumps), Riot Client's logs
//! and a summary of the PC and Prime's settings. Tokens are taken out of every file.

use std::fs::{self, File};
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Only the end of a longer log goes in, which is where a crash shows.
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// How many of VALORANT's and Riot Client's newest logs, and VALORANT's crash reports, go in.
const NEWEST_FILES: usize = 3;

/// Writes the zip to `logs\exports\`, replacing an earlier export, and returns its path.
pub fn export(system_info: &str) -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    let system_info = format!("{system_info}Windows: {}\n", windows_version());
    export_from(&local, &crate::logging::log_dir(), &system_info)
}

/// `export` with `%LOCALAPPDATA%` and Prime's log folder given, so tests use their own.
fn export_from(local: &Path, log_dir: &Path, system_info: &str) -> io::Result<PathBuf> {
    let dir = log_dir.join("exports");
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("prime-logs-{}.zip", file_stamp()));

    let mut zip = ZipWriter::new(File::create(&path)?);
    let options = SimpleFileOptions::default();
    let mut add = |name: &str, text: &str| -> io::Result<()> {
        zip.start_file(name, options).map_err(io::Error::other)?;
        zip.write_all(redact(text).as_bytes())
    };

    add("system-info.txt", system_info)?;
    for (folder, file) in sources(local, log_dir) {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // Riot may be writing to it; a file that can't be read is left out.
        if let Ok(text) = read_end(&file) {
            add(&format!("{folder}/{name}"), &text)?;
        }
    }
    zip.finish().map_err(io::Error::other)?;

    log::info!("exported logs to {}", path.display());
    Ok(path)
}

/// Each file to include, with the folder it goes in.
fn sources(local: &Path, log_dir: &Path) -> Vec<(String, PathBuf)> {
    let is_log = |path: &Path| path.extension().is_some_and(|ext| ext == "log");

    let mut files: Vec<(String, PathBuf)> = newest(log_dir, usize::MAX, is_log)
        .into_iter()
        .map(|file| ("prime".to_string(), file))
        .collect();
    for (folder, dir) in [
        ("valorant", local.join(r"VALORANT\Saved\Logs")),
        (
            "riot-client",
            local.join(r"Riot Games\Riot Client\Logs\Riot Client Logs"),
        ),
    ] {
        files.extend(
            newest(&dir, NEWEST_FILES, is_log)
                .into_iter()
                .map(|file| (folder.to_string(), file)),
        );
    }

    // Each crash is a folder; its text says where it happened, the dump is too big to send.
    let crashes = local.join(r"VALORANT\Saved\Crashes");
    for crash in newest(&crashes, NEWEST_FILES, Path::is_dir) {
        let Some(crash_name) = crash.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let folder = format!("valorant-crashes/{crash_name}");
        let is_text = |path: &Path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext, "log" | "xml" | "runtime-xml" | "txt" | "json"))
        };
        files.extend(
            newest(&crash, usize::MAX, is_text)
                .into_iter()
                .map(|file| (folder.clone(), file)),
        );
    }
    files
}

/// The newest `count` entries of `dir` that pass `keep`, newest first.
fn newest(dir: &Path, count: usize, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| keep(path))
        .map(|path| {
            let modified = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (modified, path)
        })
        .collect();
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found
        .into_iter()
        .take(count)
        .map(|(_, path)| path)
        .collect()
}

fn read_end(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    let start = bytes.len().saturating_sub(MAX_FILE_BYTES);
    Ok(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

/// Such as "Microsoft Windows [Version 10.0.26200.6584]".
fn windows_version() -> String {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/c", "ver"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|error| format!("unknown ({error})"))
}

fn file_stamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

/// Names whose value is a secret, wherever they're followed by `=`, `:` or a quote.
const SECRET_KEYS: [&str; 9] = [
    "access_token",
    "refresh_token",
    "id_token",
    "entitlements_token",
    "ssid",
    "password",
    "x-riot-entitlements-jwt",
    "authorization",
    "bearer",
];

/// Takes tokens out of a log: anything shaped like a JWT, which is what Riot's access, ID,
/// entitlement and session tokens are, and the value after any of `SECRET_KEYS`.
pub fn redact(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let starts_word = i == 0 || !is_word(bytes[i - 1]);
        if starts_word && text[i..].starts_with("eyJ") {
            let end = run_end(bytes, i, |byte| is_word(byte) || byte == b'.');
            if end - i >= 20 && text[i..end].contains('.') {
                out.push_str("[redacted]");
                i = end;
                continue;
            }
        }
        if starts_word
            && let Some(key) = SECRET_KEYS.iter().find(|key| {
                text.get(i..i + key.len())
                    .is_some_and(|word| word.eq_ignore_ascii_case(key))
                    && bytes.get(i + key.len()).is_none_or(|&byte| !is_word(byte))
            })
        {
            let after_key = i + key.len();
            let separators = run_end(bytes, after_key, |byte| b" \t\"':=".contains(&byte));
            let separator = &text[after_key..separators];
            // A word in a sentence, such as "the password field", has no `=` or `:` after it.
            // "Bearer" is followed by its token after a space.
            let assigns = separator.contains([':', '=']) || (*key == "bearer" && separator == " ");
            if assigns && separator.len() <= 4 {
                let value =
                    |start| run_end(bytes, start, |byte| !b" \t\r\n\"'&,;}".contains(&byte));
                let mut value_end = value(separators);
                // "Authorization: Bearer <token>" hides the token, not just the scheme.
                if text[separators..value_end].eq_ignore_ascii_case("bearer")
                    && bytes.get(value_end) == Some(&b' ')
                {
                    value_end = value(value_end + 1);
                }
                if value_end > separators {
                    out.push_str(&text[i..separators]);
                    out.push_str("[redacted]");
                    i = value_end;
                    continue;
                }
            }
        }
        let next = text[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&text[i..i + next]);
        i += next;
    }
    out
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// The first index from `start` whose byte doesn't pass `keep`. Every byte it passes over is
/// ASCII, so the index is always a char boundary.
fn run_end(bytes: &[u8], start: usize, keep: impl Fn(u8) -> bool) -> usize {
    bytes[start..]
        .iter()
        .position(|&byte| !byte.is_ascii() || !keep(byte))
        .map_or(bytes.len(), |offset| start + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JWT: &str = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJl";

    #[test]
    fn tokens_are_taken_out() {
        assert_eq!(
            redact(&format!("got token {JWT} for shop")),
            "got token [redacted] for shop"
        );
        assert_eq!(
            redact("GET /x?access_token=abc123&expires_in=3600"),
            "GET /x?access_token=[redacted]&expires_in=3600"
        );
        assert_eq!(
            redact(r#"{"refresh_token": "abc.def", "ok": true}"#),
            r#"{"refresh_token": "[redacted]", "ok": true}"#
        );
        assert_eq!(
            redact("Authorization: Bearer abc123"),
            "Authorization: [redacted]"
        );
        assert_eq!(
            redact("Cookie: ssid=abc; tdid=x"),
            "Cookie: ssid=[redacted]; tdid=x"
        );
        assert_eq!(redact("sent Bearer abc123"), "sent Bearer [redacted]");
    }

    #[test]
    fn everything_else_is_left_as_it_is() {
        for text in [
            "Sign in with the password field below",
            "the passwords match",
            "LogNet: Join succeeded: Pl\u{e4}yer#EUW",
            "eyJ is short",
            "",
        ] {
            assert_eq!(redact(text), text);
        }
    }

    #[test]
    fn an_export_holds_every_log_without_tokens_or_dumps() {
        let local = tempfile::tempdir().unwrap();
        let logs = local.path().join(r"spiiritual\prime\logs");
        let files = [
            (logs.join("prime.log"), "launch".to_string()),
            (
                local.path().join(r"VALORANT\Saved\Logs\ShooterGame.log"),
                format!("token {JWT}"),
            ),
            (
                local
                    .path()
                    .join(r"VALORANT\Saved\Crashes\UECC-1\CrashContext.runtime-xml"),
                "<Crash/>".to_string(),
            ),
            (
                local
                    .path()
                    .join(r"VALORANT\Saved\Crashes\UECC-1\UEMinidump.dmp"),
                "memory".to_string(),
            ),
            (
                local
                    .path()
                    .join(r"Riot Games\Riot Client\Logs\Riot Client Logs\client.log"),
                "ssid=secret".to_string(),
            ),
        ];
        for (path, text) in &files {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        let path = export_from(local.path(), &logs, "Prime 1.0\n").unwrap();
        // A second export replaces the first.
        let path = {
            let again = export_from(local.path(), &logs, "Prime 1.0\n").unwrap();
            assert!(!path.exists() || path == again);
            again
        };

        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut read = |name: &str| {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut text).unwrap();
            text
        };
        assert_eq!(read("system-info.txt"), "Prime 1.0\n");
        assert_eq!(read("prime/prime.log"), "launch");
        assert_eq!(read("valorant/ShooterGame.log"), "token [redacted]");
        assert_eq!(
            read("valorant-crashes/UECC-1/CrashContext.runtime-xml"),
            "<Crash/>"
        );
        assert_eq!(read("riot-client/client.log"), "ssid=[redacted]");
        assert_eq!(zip.len(), 5, "no memory dump");
    }

    #[test]
    fn newest_files_come_first() {
        let dir = tempfile::tempdir().unwrap();
        for (name, age) in [
            ("old.log", 30),
            ("new.log", 0),
            ("mid.log", 10),
            ("skip.dmp", 0),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, name).unwrap();
            let modified = SystemTime::now() - std::time::Duration::from_secs(age);
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }

        let names: Vec<_> = newest(dir.path(), 2, |path| {
            path.extension().is_some_and(|ext| ext == "log")
        })
        .into_iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
        assert_eq!(names, ["new.log", "mid.log"]);
    }
}
