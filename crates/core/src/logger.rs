use crate::config::{Config, LogRotation};
use crate::error::Result;

use std::io::{self, IsTerminal};
use tracing::error;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{self, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Registry, filter::LevelFilter, fmt, layer::SubscriberExt, prelude::*,
};

/// Guards for non-blocking writers so they flush on shutdown.
#[derive(Debug)]
pub struct LoggingGuards {
    pub file_guard: Option<WorkerGuard>,
    pub error_file_guard: Option<WorkerGuard>,
}

impl Default for LoggingGuards {
    fn default() -> Self {
        Self {
            file_guard: None,
            error_file_guard: None,
        }
    }
}

/// Initialize global tracing subscriber based on `Config`.
///
/// Layers:
/// - Console (always on)
/// - Optional rolling app log at `log_path`
/// - Optional rolling error-only log at `log_error_path`
///
/// Returns guards that must be kept alive to ensure logs are flushed.
pub fn init_logger(cfg: &Config) -> Result<LoggingGuards> {
    // Ensure all directories exist per config.
    cfg.ensure_dirs()?;

    // Build a base filter from cfg.log_level (e.g., "trace", "debug", "info", ...).
    let env_filter =
        EnvFilter::try_new(cfg.log_level.clone()).unwrap_or_else(|_| EnvFilter::new("info"));
    let use_ansi = io::stdout().is_terminal();

    // Console layer (human-friendly formatting to stdout).
    let console_layer = fmt::layer()
        .with_target(true)
        .with_ansi(use_ansi)
        .with_filter(env_filter.clone());

    let rotation = match cfg.log_rotation {
        LogRotation::Daily => Rotation::DAILY,
        LogRotation::Hourly => Rotation::HOURLY,
    };

    // Optional: app log file layer
    let (file_layer_opt, file_guard_opt): (Option<_>, Option<WorkerGuard>) =
        if let Some(dir) = &cfg.log_path {
            let appender = rolling::Builder::new()
                .rotation(rotation.clone())
                .filename_prefix("ferri")
                .filename_suffix("log")
                .build(dir)
                .map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("failed to create error log appender: {e}"),
                    )
                })?;

            let (nb, guard) = tracing_appender::non_blocking(appender);
            let layer = fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(nb)
                .with_filter(env_filter.clone());
            (Some(layer), Some(guard))
        } else {
            (None, None)
        };

    // Optional: error-only log file layer
    let (error_layer_opt, error_guard_opt): (Option<_>, Option<WorkerGuard>) =
        if let Some(dir) = &cfg.log_error_path {
            let appender = rolling::Builder::new()
                .rotation(rotation)
                .filename_prefix("ferri-error")
                .filename_suffix("log")
                .build(dir)
                .map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("failed to create error log appender: {e}"),
                    )
                })?;

            let (nb, guard) = tracing_appender::non_blocking(appender);
            let layer = fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(nb)
                .with_filter(LevelFilter::ERROR);
            (Some(layer), Some(guard))
        } else {
            (None, None)
        };

    // Compose subscriber with optional layers.
    let subscriber = Registry::default()
        .with(console_layer)
        .with(file_layer_opt)
        .with(error_layer_opt);

    // Install globally. Use try_init so we return an io::Error instead of panicking
    // if someone else already initialized a subscriber.
    subscriber
        .try_init()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to init logger: {e}")))?;

    // Panic hook to route panics through tracing (to reach error log).
    install_panic_hook();

    Ok(LoggingGuards {
        file_guard: file_guard_opt,
        error_file_guard: error_guard_opt,
    })
}

/// Install a panic hook that logs panics via `tracing::error!`.
fn install_panic_hook() {
    // Only install once; subsequent calls keep the first hook.
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Forward to tracing first so it's captured by our layers.
            if let Some(s) = info.payload().downcast_ref::<&str>() {
                error!(target: "panic", "panic: {}", s);
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                error!(target: "panic", "panic: {}", s);
            } else {
                error!(target: "panic", "panic occurred");
            }
            // Still call the default hook so backtraces (if enabled) print to stderr.
            default(info);
        }));
    });
}
