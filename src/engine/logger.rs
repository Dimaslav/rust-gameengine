//! Перехват логов движка для панели Console.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn from_log(l: log::Level) -> Self {
        match l {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug => LogLevel::Debug,
            log::Level::Trace => LogLevel::Trace,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub time_ms: f64,
}

const MAX_LOG_ENTRIES: usize = 10_000;

pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
    pub start: std::time::Instant,
}

static LOG_BUFFER: OnceLock<LogBuffer> = OnceLock::new();

impl LogBuffer {
    fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(1024)),
            start: std::time::Instant::now(),
        }
    }

    pub fn global() -> &'static Self {
        LOG_BUFFER.get_or_init(LogBuffer::new)
    }

    pub fn push(&self, entry: LogEntry) {
        let mut lock = self.entries.lock().unwrap();
        if lock.len() >= MAX_LOG_ENTRIES {
            lock.pop_front();
        }
        lock.push_back(entry);
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

/// Комбинированный логгер: пишет в stdout через `env_logger` и
/// сохраняет копию в `LogBuffer` для UI.
pub struct DualLogger {
    env: env_logger::Logger,
}

impl log::Log for DualLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.env.enabled(metadata) || metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.env.enabled(record.metadata()) {
            self.env.log(record);
        }

        // Сохраняем в буфер только Info и выше (Error, Warn, Info).
        if record.level() <= log::Level::Info {
            let entry = LogEntry {
                level: LogLevel::from_log(record.level()),
                target: record.target().to_string(),
                message: record.args().to_string(),
                file: record.file().unwrap_or("?").to_string(),
                line: record.line().unwrap_or(0),
                time_ms: LOG_BUFFER
                    .get()
                    .map(|b| b.start.elapsed().as_secs_f64() * 1000.0)
                    .unwrap_or(0.0),
            };
            LogBuffer::global().push(entry);
        }
    }

    fn flush(&self) {
        self.env.flush();
    }
}

/// Инициализация глобального логгера.
pub fn init() {
    LogBuffer::global();
    let env = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("rust_engine=info,warn"),
    )
    .build();

    let _ = log::set_boxed_logger(Box::new(DualLogger { env }));
    log::set_max_level(log::LevelFilter::Info);
}