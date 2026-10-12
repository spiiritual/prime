//! Prime's log file, for working out problems users report, such as VALORANT crashing. Each start
//! writes `prime.log` in `%LOCALAPPDATA%\spiiritual\prime\logs\` and keeps the runs before it as
//! `prime.1.log` to `prime.4.log`. Lines are written straight to the file, so a crash loses none.
//! Nothing logged may hold a token or password.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Runs kept, the current one included.
const KEPT_RUNS: usize = 5;
/// A run that logs more than this stops logging, so a loop can't fill the disk.
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

pub fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join(r"spiiritual\prime\logs"))
        .unwrap_or_else(|| PathBuf::from("logs"))
}

/// Debug builds log apart, so `cargo run` doesn't push an installed Prime's runs out.
fn log_name(run: usize) -> String {
    let stem = if cfg!(debug_assertions) {
        "prime-dev"
    } else {
        "prime"
    };
    match run {
        0 => format!("{stem}.log"),
        run => format!("{stem}.{run}.log"),
    }
}

/// Starts logging to a new `prime.log` and logs panics. Call once, after the single-instance
/// check, so a second start that only hands over to the running Prime keeps its log.
pub fn init() {
    let dir = log_dir();
    let file = fs::create_dir_all(&dir).and_then(|()| {
        rotate(&dir);
        File::create(dir.join(log_name(0)))
    });
    let logger = FileLogger {
        file: Mutex::new(file.ok().map(|file| (file, 0))),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    log::info!(
        "Prime {} started, update channel {:?}",
        crate::updater::CURRENT_VERSION,
        crate::updater::current_channel()
    );

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!(
            "panic: {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        default_hook(info);
    }));
}

/// Moves each kept run up one name, dropping the oldest.
fn rotate(dir: &Path) {
    for run in (0..KEPT_RUNS - 1).rev() {
        let _ = fs::rename(dir.join(log_name(run)), dir.join(log_name(run + 1)));
    }
}

/// Prime's own lines at info; other crates' only from warnings up, except the GPU adapter and
/// backend iced and wgpu report at start, since a graphics driver is a likely cause of crashes.
fn level_for(target: &str) -> Level {
    if target.starts_with("prime") || target.starts_with("iced_wgpu") {
        Level::Info
    } else {
        Level::Warn
    }
}

struct FileLogger {
    /// The log and how much this run has written to it.
    file: Mutex<Option<(File, u64)>>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= level_for(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}\n",
            timestamp(),
            record.level(),
            record.target(),
            record.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        let mut file = self.file.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((file, written)) = file.as_mut()
            && *written < MAX_LOG_BYTES
        {
            *written += line.len() as u64;
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {}
}

/// Local time with its offset from UTC, such as `2026-10-12 19:05:42.123 -04:00`.
fn timestamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let offset = now.offset();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {}{:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond(),
        if offset.is_negative() { '-' } else { '+' },
        offset.whole_hours().unsigned_abs(),
        offset.minutes_past_hour().unsigned_abs(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_start_keeps_the_last_runs() {
        let dir = tempfile::tempdir().unwrap();
        for run in 0..KEPT_RUNS + 2 {
            rotate(dir.path());
            fs::write(dir.path().join(log_name(0)), run.to_string()).unwrap();
        }

        let newest = KEPT_RUNS + 1;
        for run in 0..KEPT_RUNS {
            let kept = fs::read_to_string(dir.path().join(log_name(run))).unwrap();
            assert_eq!(kept, (newest - run).to_string());
        }
        assert!(!dir.path().join(log_name(KEPT_RUNS)).exists());
    }

    #[test]
    fn only_prime_and_the_gpu_adapter_log_below_warnings() {
        assert_eq!(level_for("prime::launch"), Level::Info);
        assert_eq!(level_for("iced_wgpu::window::compositor"), Level::Info);
        assert_eq!(level_for("reqwest::connect"), Level::Warn);
        assert_eq!(level_for("wgpu_hal::vulkan"), Level::Warn);
    }
}
