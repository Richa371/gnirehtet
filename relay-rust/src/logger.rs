use jiff::Zoned;
use log::*;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::sync::Mutex;

static LOGGER: SimpleLogger = SimpleLogger;
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub struct SimpleLogger;

impl Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= current_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let now = Zoned::now();
            let msg = format!(
                "{} {} {}: {}\n",
                now.strftime("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.target(),
                record.args()
            );
            let mut file = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut f) = *file {
                let _ = f.write_all(msg.as_bytes());
            } else if record.level() == Level::Error {
                let _ = io::stderr().write_all(msg.as_bytes());
            } else {
                let _ = io::stdout().write_all(msg.as_bytes());
            }
        }
    }

    fn flush(&self) {
        let file = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref f) = *file {
            let _ = f.sync_all();
        }
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
    }
}

fn current_level() -> LevelFilter {
    match std::env::var("RUST_LOG").unwrap_or_default().to_lowercase().as_str() {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Info,
    }
}

pub fn init(log_file: Option<&str>) -> Result<(), SetLoggerError> {
    if let Some(path) = log_file {
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
                *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
            }
            Err(e) => {
                eprintln!("Cannot open log file '{}': {}", path, e);
            }
        }
    }
    let level = current_level();
    set_max_level(level);
    set_logger(&LOGGER)
}
