// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Logging to logcat (tag `teyvatme`) and to `latest.log` in the app's files
//! folder, which Activity's Share log sends. The previous run's log is kept as
//! `previous.log`.
//!
//! This crate logs through `log`; the core and protocol crates log through
//! `tracing`, whose events (INFO and up, as the desktop logs them; spans
//! are context, not lines) are formatted by a `tracing-subscriber` and
//! handed to the same logger.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, Once};

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Logger {
    #[cfg(target_os = "android")]
    logcat: android_logger::AndroidLogger,
    file: Option<Mutex<File>>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= LevelFilter::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        #[cfg(target_os = "android")]
        self.logcat.log(record);
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = writeln!(
                file,
                "{} {:5} {}: {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = file.flush();
        }
    }
}

/// Where the tracing subscriber writes: each formatted event becomes one
/// `log` record (target `tracing`), at the level its text starts with.
struct TracingToLog;

impl Write for TracingToLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let text = text.trim_end();
        if !text.is_empty() {
            let level = match text.split_whitespace().next() {
                Some("ERROR") => Level::Error,
                Some("WARN") => Level::Warn,
                _ => Level::Info,
            };
            log::logger().log(
                &Record::builder()
                    .level(level)
                    .target("tracing")
                    .args(format_args!("{text}"))
                    .build(),
            );
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Once per process: later calls (another service start) do nothing.
pub fn init(dir: &Path) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let latest = dir.join("latest.log");
        let _ = fs::rename(&latest, dir.join("previous.log"));
        let file = File::create(&latest).ok().map(Mutex::new);
        let logger = Logger {
            #[cfg(target_os = "android")]
            logcat: android_logger::AndroidLogger::new(
                android_logger::Config::default()
                    .with_tag("teyvatme")
                    .with_max_level(LevelFilter::Info),
            ),
            file,
        };
        if log::set_boxed_logger(Box::new(logger)).is_ok() {
            log::set_max_level(LevelFilter::Info);
        }
        // A panic reaches latest.log (Share log), not only logcat.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}");
            log::logger().flush();
            previous(info);
        }));
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .without_time()
            .with_writer(|| TracingToLog)
            .try_init();
        log::info!(
            "Teyvat.me native {}, game data {}",
            env!("CARGO_PKG_VERSION"),
            option_env!("TEYVATME_GAME_DATA_SHA").unwrap_or("unknown")
        );
    });
}
