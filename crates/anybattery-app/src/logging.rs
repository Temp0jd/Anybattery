//! Rolling file log and panic hook.
//!
//! The release build is a GUI subsystem binary, so stderr is invisible.  A
//! small append-only log in the app data directory is the only durable
//! evidence when a user reports a crash or a scan failure.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriter;

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const LOG_FILE: &str = "anybattery.log";

static LOG: OnceLock<Mutex<LogSink>> = OnceLock::new();

pub fn log_dir() -> PathBuf {
    crate::storage::data_dir().join("logs")
}

pub fn log_path() -> PathBuf {
    log_dir().join(LOG_FILE)
}

/// Install the file subscriber.  Returns false when another subscriber was
/// already installed; the caller may continue either way.
pub fn init() -> bool {
    // The binary crate's tracing target is `AnyBattery`, not `anybattery`;
    // use the global info level so startup/panic lines are never filtered out.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let sink = LogSink::open(log_path());
    let _ = LOG.set(Mutex::new(sink));

    let installed = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true)
        .with_writer(LogWriter)
        .try_init()
        .is_ok();
    if !installed {
        eprintln!("AnyBattery: tracing subscriber already installed");
    }
    installed
}

/// Record panics into the same file, including a forced backtrace.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("<unnamed>");
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "<unknown>".to_owned());
        let message = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_owned()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "<non-string panic payload>".to_owned()
        };
        let backtrace = std::backtrace::Backtrace::force_capture();
        let entry = format!(
            "{} PANIC thread={thread} location={location} message={message}\n{backtrace}\n",
            timestamp()
        );
        write_raw(&entry);
        eprintln!("{entry}");
    }));
}

fn write_raw(text: &str) {
    if let Some(sink) = LOG.get() {
        // A panic may happen while the logger lock is held.  Never deadlock a
        // panic hook; fall back to stderr instead.
        if let Ok(mut sink) = sink.try_lock() {
            sink.write_buf(text.as_bytes());
            return;
        }
    }
    eprintln!("{text}");
}

struct LogWriter;

impl<'a> MakeWriter<'a> for LogWriter {
    type Writer = LogGuard;

    fn make_writer(&'a self) -> Self::Writer {
        LogGuard {
            sink: LOG.get_or_init(|| Mutex::new(LogSink::open(log_path()))),
        }
    }
}

struct LogGuard {
    sink: &'static Mutex<LogSink>,
}

impl Write for LogGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut sink = self
            .sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sink.write_buf(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut sink = self
            .sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sink.flush();
        Ok(())
    }
}

struct LogSink {
    path: PathBuf,
    file: Option<File>,
    size: u64,
}

impl LogSink {
    fn open(path: PathBuf) -> Self {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        rotate_if_needed(&path);
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
                Self {
                    path,
                    file: Some(file),
                    size,
                }
            }
            Err(err) => {
                eprintln!("AnyBattery: cannot open log file {}: {err}", path.display());
                Self {
                    path,
                    file: None,
                    size: 0,
                }
            }
        }
    }

    fn write_buf(&mut self, buf: &[u8]) {
        if self.size.saturating_add(buf.len() as u64) > MAX_LOG_BYTES {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut() {
            if file.write_all(buf).is_ok() {
                self.size = self.size.saturating_add(buf.len() as u64);
            }
        }
    }

    fn flush(&mut self) {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
    }

    fn rotate(&mut self) {
        self.file.take();
        let backup = backup_path(&self.path);
        let _ = fs::remove_file(&backup);
        let _ = fs::rename(&self.path, &backup);
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(file) => {
                self.file = Some(file);
                self.size = 0;
            }
            Err(_) => {
                self.file = None;
                self.size = 0;
            }
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

fn rotate_if_needed(path: &Path) {
    let too_big = fs::metadata(path)
        .map(|meta| meta.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if too_big {
        let backup = backup_path(path);
        let _ = fs::remove_file(&backup);
        let _ = fs::rename(path, &backup);
    }
}

fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format_utc(seconds)
}

/// Convert Unix seconds to a compact UTC timestamp without a date library.
fn format_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's civil_from_days algorithm (days since 1970-01-01).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_is_iso_like() {
        let stamp = format_utc(1_700_000_000);
        assert_eq!(stamp.len(), 20);
        assert_eq!(stamp.as_bytes()[4], b'-');
        assert_eq!(stamp.as_bytes()[10], b' ');
        assert_eq!(stamp.as_bytes()[19], b'Z');
    }

    #[test]
    fn epoch_is_1970() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00Z");
    }

    #[test]
    fn backup_name_appends_suffix() {
        let path = Path::new("anybattery.log");
        assert_eq!(backup_path(path), PathBuf::from("anybattery.log.1"));
    }
}
