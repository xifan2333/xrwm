use super::*;

#[test]
fn test_attach_mode_parse() {
    assert_eq!(AttachMode::parse("top").unwrap(), AttachMode::Top);
    assert_eq!(AttachMode::parse("bottom").unwrap(), AttachMode::Bottom);
    assert_eq!(AttachMode::parse("above").unwrap(), AttachMode::Above);
    assert_eq!(AttachMode::parse("below").unwrap(), AttachMode::Below);
    assert_eq!(AttachMode::parse("after 2").unwrap(), AttachMode::After(2));
    assert_eq!(AttachMode::parse("after 0").unwrap(), AttachMode::After(0));
    assert!(AttachMode::parse("after").is_err());
    assert!(AttachMode::parse("after foo").is_err());
    assert!(AttachMode::parse("invalid").is_err());
}

#[test]
fn test_attach_mode_insert_index() {
    // When empty
    assert_eq!(AttachMode::Top.calculate_insert_index(None, 0), 0);
    assert_eq!(AttachMode::Bottom.calculate_insert_index(None, 0), 0);
    assert_eq!(AttachMode::Above.calculate_insert_index(None, 0), 0);
    assert_eq!(AttachMode::Below.calculate_insert_index(None, 0), 0);
    assert_eq!(AttachMode::After(3).calculate_insert_index(None, 0), 0);

    // When 3 windows exist, focused at index 1
    let focused = Some(1);
    let len = 3;
    assert_eq!(AttachMode::Top.calculate_insert_index(focused, len), 0);
    assert_eq!(AttachMode::Bottom.calculate_insert_index(focused, len), 3);
    assert_eq!(AttachMode::Above.calculate_insert_index(focused, len), 1);
    assert_eq!(AttachMode::Below.calculate_insert_index(focused, len), 2);
    assert_eq!(AttachMode::After(0).calculate_insert_index(focused, len), 0);
    assert_eq!(AttachMode::After(1).calculate_insert_index(focused, len), 1);
    assert_eq!(AttachMode::After(2).calculate_insert_index(focused, len), 2);
    assert_eq!(AttachMode::After(5).calculate_insert_index(focused, len), 3);

    // When 3 windows exist, no focus
    assert_eq!(AttachMode::Above.calculate_insert_index(None, len), 0);
    assert_eq!(AttachMode::Below.calculate_insert_index(None, len), 3);
}

#[test]
fn test_broadcast_status_preserves_healthy_subscribers() {
    let mut state = AppState::new();
    let (server1, _client1) = UnixStream::pair().unwrap();
    let (server2, _client2) = UnixStream::pair().unwrap();

    state.status_listeners.push((server1, None));
    state.status_listeners.push((server2, None));

    state.broadcast_status();
    assert_eq!(state.status_listeners.len(), 2);
}

#[test]
fn test_broadcast_status_drops_broken_pipe_subscriber() {
    let mut state = AppState::new();
    let (server1, client1) = UnixStream::pair().unwrap();
    let (server2, _client2) = UnixStream::pair().unwrap();

    drop(client1); // peer disconnected

    state.status_listeners.push((server1, None));
    state.status_listeners.push((server2, None));

    state.broadcast_status();
    // Broken pipe subscriber should be evicted, while healthy one remains
    assert_eq!(state.status_listeners.len(), 1);
}

#[test]
fn test_parse_hex_color_valid() {
    assert!(parse_hex_color("#61afef").is_ok());
    assert!(parse_hex_color("0x61afef").is_ok());
    assert!(parse_hex_color("61afef").is_ok());
    assert!(parse_hex_color("#61afef80").is_ok());
    assert!(parse_hex_color("0x61afef80").is_ok());
    assert!(parse_hex_color("61afef80").is_ok());

    let (r, g, b, a) = parse_hex_color("#ffffff").unwrap();
    assert_eq!(r, u32::MAX);
    assert_eq!(g, u32::MAX);
    assert_eq!(b, u32::MAX);
    assert_eq!(a, u32::MAX);

    // 8-digit hex with premultiplied alpha
    let (r, g, b, a) = parse_hex_color("#ff000080").unwrap();
    assert_eq!(a, 0x80 * (u32::MAX / 255));
    let expected_r = ((u32::MAX as u64 * a as u64) / u32::MAX as u64) as u32;
    assert_eq!(r, expected_r);
    assert_eq!(g, 0);
    assert_eq!(b, 0);
}

#[test]
fn test_parse_hex_color_invalid() {
    // Non-ASCII string (would panic with naive slicing)
    assert!(parse_hex_color("你好").is_err());
    assert_eq!(
        hex_to_river_rgba("你好"),
        (u32::MAX, u32::MAX, u32::MAX, u32::MAX)
    );

    // Invalid hex characters
    assert!(parse_hex_color("#12345z").is_err());
    assert!(parse_hex_color("0xhello!").is_err());

    // Too short
    assert!(parse_hex_color("").is_err());
    assert!(parse_hex_color("#123").is_err());
    assert!(parse_hex_color("0x").is_err());
    assert!(parse_hex_color("#12345").is_err());

    // Too long
    assert!(parse_hex_color("#1234567").is_err());
    assert!(parse_hex_color("#123456789").is_err());
}

#[test]
#[allow(clippy::zombie_processes)]
fn test_spawn_init_script_at_supports_spaces_and_special_characters() {
    let _guard = PROCESS_TEST_MUTEX.lock().unwrap();
    let temp_dir = std::env::temp_dir().join(format!(
        "xrwm test config spaces & $special_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let script_path = temp_dir.join("init");
    let marker_path = temp_dir.join("marker.txt");

    // Write an executable bash script that outputs to marker.txt
    let script_content = format!(
        "#!/usr/bin/env bash\necho 'init_executed_successfully' > '{}'\n",
        marker_path.display()
    );
    std::fs::write(&script_path, script_content).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }

    let mut child = spawn_init_script_at(&script_path).expect("should spawn script directly");
    let status = child.wait().expect("child should complete");
    assert!(status.success());

    let marker = std::fs::read_to_string(&marker_path).unwrap();
    assert_eq!(marker.trim(), "init_executed_successfully");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_spawn_init_script_at_handles_non_existent_path() {
    let _guard = PROCESS_TEST_MUTEX.lock().unwrap();
    let non_existent = std::path::Path::new("/tmp/non_existent_xrwm_init_path_99999999/init");
    // bash reports error and exits with non-zero status
    if let Ok(mut child) = spawn_init_script_at(non_existent) {
        let status = child.wait().unwrap();
        assert!(!status.success());
    }
}

#[test]
#[allow(clippy::zombie_processes)]
fn test_spawn_init_script_from_config_spaces_and_non_existent() {
    let _guard = PROCESS_TEST_MUTEX.lock().unwrap();
    let temp_dir = std::env::temp_dir().join(format!(
        "xrwm config dir with spaces & $metachars_{}",
        std::process::id()
    ));
    let xrwm_dir = temp_dir.join("xrwm");
    std::fs::create_dir_all(&xrwm_dir).unwrap();

    // 1. When init script does not exist, returns None
    assert!(spawn_init_script_from_config(&temp_dir).is_none());

    // 2. When init script exists in path with spaces and special characters
    let script_path = xrwm_dir.join("init");
    let marker_path = temp_dir.join("marker.txt");
    let script_content = format!(
        "#!/usr/bin/env bash\necho 'from_config_success' > '{}'\n",
        marker_path.display()
    );
    std::fs::write(&script_path, script_content).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }

    let child_res = spawn_init_script_from_config(&temp_dir);
    assert!(child_res.is_some());
    let mut child = child_res.unwrap().expect("child should spawn successfully");
    let status = child.wait().expect("child should complete");
    assert!(status.success());

    let marker = std::fs::read_to_string(&marker_path).unwrap();
    assert_eq!(marker.trim(), "from_config_success");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_glob_match_exhaustive() {
    // 1. Exact string matches
    assert!(glob_match("firefox", "firefox"));
    assert!(!glob_match("firefox", "chrome"));

    // 2. Empty string edge cases
    assert!(glob_match("", ""));
    assert!(glob_match("*", ""));
    assert!(glob_match("***", ""));
    assert!(!glob_match("", "a"));
    assert!(!glob_match("a", ""));
    assert!(!glob_match("a*", ""));
    assert!(!glob_match("*a", ""));

    // 3. Prefix wildcard
    assert!(glob_match("*calc", "gnome-calc"));
    assert!(glob_match("*calc", "calc"));
    assert!(!glob_match("*calc", "calculator"));

    // 4. Suffix wildcard
    assert!(glob_match("calc*", "calculator"));
    assert!(glob_match("calc*", "calc"));
    assert!(!glob_match("calc*", "gnome-calc"));

    // 5. Multi-segment / internal wildcards
    assert!(glob_match("org.*.App", "org.test.App"));
    assert!(glob_match("org.*.App", "org.my.long.name.App"));
    assert!(!glob_match("org.*.App", "org.test.Application"));
    assert!(!glob_match("org.*.App", "com.test.App"));
    assert!(glob_match("*foo*bar*", "123foomidbar456"));
    assert!(glob_match("*foo*bar*", "foobar"));
    assert!(!glob_match("*foo*bar*", "foobaz"));
    assert!(glob_match("a*b*c", "a_middle_b_end_c"));
    assert!(!glob_match("a*b*c", "a_middle_b_end_d"));

    // 6. Consecutive asterisks
    assert!(glob_match("foo**bar", "foobar"));
    assert!(glob_match("foo**bar", "foobazbar"));
    assert!(glob_match("***", "anything"));

    // 7. Multibyte Unicode characters
    assert!(glob_match("终端*", "终端窗口"));
    assert!(glob_match("*测*试*", "这是一个测试页面"));
    assert!(!glob_match("*测*试*", "这是一个页面"));
    assert!(glob_match("🎉*🚀", "🎉 celebration 🚀"));
    assert!(!glob_match("🎉*🚀", "🎉 celebration 🛸"));
}
