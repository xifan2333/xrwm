//! Logging subsystem for xrwm daemon and client utilities.
//!
//! Provides structured, agent-friendly JSONL logging by default, with optional
//! human-readable compact text formatting via `XRWM_LOG_FORMAT=text`.

/// Supported log output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Flattened JSON Lines format optimized for machine and agent consumption.
    #[default]
    Json,
    /// Compact text format optimized for human terminal inspection.
    Text,
}

impl LogFormat {
    /// Detects log format from the `XRWM_LOG_FORMAT` environment variable.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("XRWM_LOG_FORMAT").as_deref() {
            Ok("text" | "compact" | "human") => Self::Text,
            _ => Self::Json,
        }
    }
}

/// Initializes tracing subscriber for the xrwm daemon using environment settings.
///
/// Defaults to JSONL output format written to `stderr` with filter `"xrwm=info"`,
/// which can be overridden via `RUST_LOG` and `XRWM_LOG_FORMAT`.
pub fn init_daemon_logging() {
    init_logging_with_format(LogFormat::from_env());
}

/// Initializes tracing subscriber with an explicit format choice.
pub fn init_logging_with_format(format: LogFormat) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("xrwm=info"));

    match format {
        LogFormat::Json => {
            let _ = tracing_subscriber::fmt()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .try_init();
        }
        LogFormat::Text => {
            let _ = tracing_subscriber::fmt()
                .compact()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .try_init();
        }
    }
}
