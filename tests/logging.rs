use xrwm::{LogFormat, init_daemon_logging, init_logging_with_format};

#[test]
fn test_log_format_default() {
    assert_eq!(LogFormat::default(), LogFormat::Json);
}

#[test]
fn test_init_logging_idempotent() {
    init_daemon_logging();
    init_logging_with_format(LogFormat::Text);
}
