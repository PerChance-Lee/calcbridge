//! The Log tab: every `log` record goes to a ring buffer and, live, to the window.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const KEEP: usize = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    /// Milliseconds since 1970.
    pub time: u64,
    pub level: &'static str,
    pub message: String,
}

struct AppLog {
    lines: Mutex<VecDeque<LogLine>>,
    app: OnceLock<AppHandle>,
}

static LOG: AppLog = AppLog { lines: Mutex::new(VecDeque::new()), app: OnceLock::new() };

impl log::Log for AppLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info || metadata.target().starts_with("calcbridge")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = LogLine {
            time: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
            level: match record.level() {
                log::Level::Error => "error",
                log::Level::Warn => "warn",
                log::Level::Info => "info",
                _ => "debug",
            },
            message: record.args().to_string(),
        };
        if cfg!(debug_assertions) {
            eprintln!("[{}] {}", line.level, line.message);
        }
        {
            let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
            if lines.len() == KEEP {
                lines.pop_front();
            }
            lines.push_back(line.clone());
        }
        if let Some(app) = self.app.get() {
            let _ = app.emit("log", line);
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    if log::set_logger(&LOG).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}

pub fn attach(app: AppHandle) {
    let _ = LOG.app.set(app);
}

pub fn lines() -> Vec<LogLine> {
    LOG.lines.lock().unwrap_or_else(PoisonError::into_inner).iter().cloned().collect()
}
