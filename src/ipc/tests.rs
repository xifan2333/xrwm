use super::*;

#[test]
fn test_parse_cli_args() {
    let args = vec![
        "map".into(),
        "normal".into(),
        "Super".into(),
        "Return".into(),
        "spawn".into(),
        "foot".into(),
    ];
    let cmd = parse_cli_args(&args).unwrap();
    assert_eq!(
        cmd,
        IpcCommand::Map {
            mode: "normal".into(),
            modifiers: "Super".into(),
            key: "Return".into(),
            action: vec!["spawn".into(), "foot".into()],
        }
    );

    let rule_args = vec![
        "rule-add".into(),
        "-app-id".into(),
        "mpv".into(),
        "float".into(),
    ];
    let rule_cmd = parse_cli_args(&rule_args).unwrap();
    assert_eq!(
        rule_cmd,
        IpcCommand::RuleAdd {
            app_id: Some("mpv".into()),
            title: None,
            action: vec!["float".into()],
        }
    );

    let dim_rule_args = vec![
        "rule-add".into(),
        "-app-id".into(),
        "mpv".into(),
        "dimensions".into(),
        "960".into(),
        "540".into(),
    ];
    assert_eq!(
        parse_cli_args(&dim_rule_args).unwrap(),
        IpcCommand::RuleAdd {
            app_id: Some("mpv".into()),
            title: None,
            action: vec!["dimensions".into(), "960".into(), "540".into()],
        }
    );

    let del_rule_args = vec![
        "rule-del".into(),
        "-app-id".into(),
        "mpv".into(),
        "float".into(),
    ];
    assert_eq!(
        parse_cli_args(&del_rule_args).unwrap(),
        IpcCommand::RuleDel {
            app_id: Some("mpv".into()),
            title: None,
            action: vec!["float".into()],
        }
    );

    assert_eq!(
        parse_cli_args(&["list-rules".into()]).unwrap(),
        IpcCommand::ListRules { action: None }
    );
    assert_eq!(
        parse_cli_args(&["list-rules".into(), "float".into()]).unwrap(),
        IpcCommand::ListRules {
            action: Some("float".into())
        }
    );

    assert_eq!(
        parse_cli_args(&["hide-cursor".into(), "timeout".into(), "3000".into()]).unwrap(),
        IpcCommand::HideCursorTimeout(3000)
    );
    assert_eq!(
        parse_cli_args(&["hide-cursor".into(), "when-typing".into(), "enabled".into()]).unwrap(),
        IpcCommand::HideCursorWhenTyping(true)
    );
    assert_eq!(
        parse_cli_args(&[
            "hide-cursor".into(),
            "when-typing".into(),
            "disabled".into()
        ])
        .unwrap(),
        IpcCommand::HideCursorWhenTyping(false)
    );
    assert_eq!(
        parse_cli_args(&["spawn-tagmask".into(), "511".into()]).unwrap(),
        IpcCommand::SpawnTagmask(511)
    );

    let gaps_args = vec!["view-padding".into(), "8".into()];
    assert_eq!(
        parse_cli_args(&gaps_args).unwrap(),
        IpcCommand::ViewPadding(8)
    );
    let op_args = vec!["outer-padding".into(), "12".into()];
    assert_eq!(
        parse_cli_args(&op_args).unwrap(),
        IpcCommand::OuterPadding(12)
    );

    assert_eq!(
        parse_cli_args(&["toggle-float".into()]).unwrap(),
        IpcCommand::ToggleFloat
    );
    assert_eq!(
        parse_cli_args(&["unmap".into(), "normal".into(), "Super".into(), "Q".into(),]).unwrap(),
        IpcCommand::Unmap {
            mode: "normal".into(),
            modifiers: "Super".into(),
            key: "Q".into(),
        }
    );
    assert_eq!(
        parse_cli_args(&[
            "unmap-pointer".into(),
            "normal".into(),
            "Super".into(),
            "BTN_LEFT".into(),
        ])
        .unwrap(),
        IpcCommand::UnmapPointer {
            mode: "normal".into(),
            modifiers: "Super".into(),
            button: "BTN_LEFT".into(),
        }
    );
    assert_eq!(
        parse_cli_args(&[
            "map-pointer".into(),
            "normal".into(),
            "Super".into(),
            "BTN_LEFT".into(),
            "move-view".into(),
        ])
        .unwrap(),
        IpcCommand::MapPointer {
            mode: "normal".into(),
            modifiers: "Super".into(),
            button: "BTN_LEFT".into(),
            action: vec!["move-view".into()],
        }
    );
    assert_eq!(parse_cli_args(&["zoom".into()]).unwrap(), IpcCommand::Zoom);
    assert_eq!(
        parse_cli_args(&["snap".into(), "left".into()]).unwrap(),
        IpcCommand::Snap("left".into())
    );
    assert_eq!(
        parse_cli_args(&["snap".into(), "right".into()]).unwrap(),
        IpcCommand::Snap("right".into())
    );
    assert_eq!(
        parse_cli_args(&["set-focused-tags".into(), "3".into()]).unwrap(),
        IpcCommand::SetFocusedTags(3)
    );
    assert_eq!(
        parse_cli_args(&["focus-view".into(), "next".into()]).unwrap(),
        IpcCommand::FocusView {
            direction: "next".into(),
            skip_floating: false,
        }
    );
    assert_eq!(
        parse_cli_args(&["focus-view".into(), "-skip-floating".into(), "left".into()]).unwrap(),
        IpcCommand::FocusView {
            direction: "left".into(),
            skip_floating: true,
        }
    );
    assert_eq!(
        parse_cli_args(&["focus-output".into(), "right".into()]).unwrap(),
        IpcCommand::FocusOutput("right".into())
    );
    assert_eq!(
        parse_cli_args(&["send-to-output".into(), "left".into()]).unwrap(),
        IpcCommand::SendToOutput {
            direction: "left".into(),
            current_tags: false,
        }
    );
    assert_eq!(
        parse_cli_args(&[
            "send-to-output".into(),
            "-current-tags".into(),
            "right".into()
        ])
        .unwrap(),
        IpcCommand::SendToOutput {
            direction: "right".into(),
            current_tags: true,
        }
    );
    assert_eq!(
        parse_cli_args(&["declare-mode".into(), "resize".into()]).unwrap(),
        IpcCommand::DeclareMode("resize".into())
    );
    assert_eq!(
        parse_cli_args(&["enter-mode".into(), "resize".into()]).unwrap(),
        IpcCommand::EnterMode("resize".into())
    );
    assert_eq!(
        parse_cli_args(&["move".into(), "left".into(), "50".into()]).unwrap(),
        IpcCommand::MoveWindow {
            direction: "left".into(),
            delta: 50,
        }
    );
    assert_eq!(
        parse_cli_args(&["resize".into(), "horizontal".into(), "20".into()]).unwrap(),
        IpcCommand::ResizeWindow {
            horizontal: true,
            delta: 20,
        }
    );
    assert_eq!(
        parse_cli_args(&["main-ratio".into(), "+0.05".into()]).unwrap(),
        IpcCommand::MainRatio("+0.05".into())
    );
    assert_eq!(
        parse_cli_args(&["stack-ratio".into(), "0.60".into()]).unwrap(),
        IpcCommand::StackRatio("0.60".into())
    );
    assert_eq!(
        parse_cli_args(&["stack-ratio".into(), "+0.05".into()]).unwrap(),
        IpcCommand::StackRatio("+0.05".into())
    );
    assert_eq!(
        parse_cli_args(&["stack-ratio".into(), "-0.05".into()]).unwrap(),
        IpcCommand::StackRatio("-0.05".into())
    );
    assert_eq!(
        parse_cli_args(&["main-count".into(), "+1".into()]).unwrap(),
        IpcCommand::MainCount("+1".into())
    );
    assert_eq!(
        parse_cli_args(&["main-ratio".into(), "0.60".into()]).unwrap(),
        IpcCommand::MainRatio("0.60".into())
    );
    assert_eq!(
        parse_cli_args(&["main-count".into(), "2".into()]).unwrap(),
        IpcCommand::MainCount("2".into())
    );
    assert_eq!(
        parse_cli_args(&["animation".into(), "true".into()]).unwrap(),
        IpcCommand::Animation(true)
    );
    assert_eq!(
        parse_cli_args(&["animation-duration".into(), "180".into()]).unwrap(),
        IpcCommand::AnimationDuration(180)
    );
    assert_eq!(
        parse_cli_args(&["status".into(), "--format".into(), "waybar".into()]).unwrap(),
        IpcCommand::Status {
            stream: false,
            format: Some("waybar".into()),
        }
    );
    assert_eq!(
        parse_cli_args(&["toggle-fullscreen".into()]).unwrap(),
        IpcCommand::ToggleFullscreen
    );
    assert_eq!(
        parse_cli_args(&["focus-previous-tags".into()]).unwrap(),
        IpcCommand::FocusPreviousTags
    );
    assert_eq!(
        parse_cli_args(&["send-to-previous-tags".into()]).unwrap(),
        IpcCommand::SendToPreviousTags
    );
    assert_eq!(
        parse_cli_args(&["default-attach-mode".into(), "bottom".into()]).unwrap(),
        IpcCommand::DefaultAttachMode(crate::wm::AttachMode::Bottom)
    );
    assert_eq!(
        parse_cli_args(&["default-attach-mode".into(), "after".into(), "2".into()]).unwrap(),
        IpcCommand::DefaultAttachMode(crate::wm::AttachMode::After(2))
    );
    assert_eq!(
        parse_cli_args(&["set-cursor-warp".into(), "on-output-change".into()]).unwrap(),
        IpcCommand::SetCursorWarp(crate::wm::CursorWarp::OnOutputChange)
    );
    assert_eq!(
        parse_cli_args(&["set-cursor-warp".into(), "disabled".into()]).unwrap(),
        IpcCommand::SetCursorWarp(crate::wm::CursorWarp::Disabled)
    );
    assert_eq!(
        parse_cli_args(&["focus-follows-cursor".into(), "always".into()]).unwrap(),
        IpcCommand::FocusFollowsCursor(crate::wm::FocusFollowsCursor::Always)
    );
    assert_eq!(
        parse_cli_args(&["main-location".into(), "top".into()]).unwrap(),
        IpcCommand::MainLocation(crate::layout::MainLocation::Top)
    );
    assert_eq!(
        parse_cli_args(&["swap".into(), "next".into()]).unwrap(),
        IpcCommand::Swap("next".into())
    );
    assert_eq!(
        parse_cli_args(&["swap".into(), "left".into()]).unwrap(),
        IpcCommand::Swap("left".into())
    );
    assert_eq!(parse_cli_args(&["ping".into()]).unwrap(), IpcCommand::Ping);
    assert_eq!(
        parse_cli_args(&["close".into()]).unwrap(),
        IpcCommand::Close
    );
    assert_eq!(parse_cli_args(&["exit".into()]).unwrap(), IpcCommand::Exit);
    assert_eq!(
        parse_cli_args(&["reload".into()]).unwrap(),
        IpcCommand::Reload
    );
}

#[test]
fn test_parse_cli_args_modifier_validation() {
    // Unknown modifiers should fail in CLI argument parsing
    assert!(
        parse_cli_args(&[
            "map".into(),
            "normal".into(),
            "Supr".into(),
            "q".into(),
            "close".into()
        ])
        .is_err()
    );
    assert!(
        parse_cli_args(&[
            "unmap".into(),
            "normal".into(),
            "InvalidMod".into(),
            "q".into()
        ])
        .is_err()
    );
    assert!(
        parse_cli_args(&[
            "map-pointer".into(),
            "normal".into(),
            "BadMod".into(),
            "BTN_LEFT".into(),
            "move-view".into()
        ])
        .is_err()
    );
    assert!(
        parse_cli_args(&[
            "unmap-pointer".into(),
            "normal".into(),
            "Super+Unknown".into(),
            "BTN_LEFT".into()
        ])
        .is_err()
    );

    // Valid aliases should succeed
    for valid in [
        "Super",
        "Mod4",
        "logo",
        "win",
        "Ctrl",
        "control",
        "Alt",
        "mod1",
        "Shift",
        "None",
        "None+Super",
    ] {
        assert!(
            parse_cli_args(&[
                "map".into(),
                "normal".into(),
                valid.into(),
                "q".into(),
                "close".into()
            ])
            .is_ok()
        );
    }
}

#[test]
fn test_read_ipc_request_success() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    client.write_all(b"{\"type\":\"Ping\"}\n").unwrap();
    let line = read_ipc_request(
        &mut server,
        std::time::Instant::now() + std::time::Duration::from_millis(100),
        MAX_IPC_REQUEST_BYTES,
    )
    .unwrap();
    assert_eq!(line, "{\"type\":\"Ping\"}");
}

#[test]
fn test_read_ipc_request_exceeds_max_size() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    client.write_all(&[b'x'; 200]).unwrap();
    let res = read_ipc_request(
        &mut server,
        std::time::Instant::now() + std::time::Duration::from_millis(50),
        100,
    );
    assert!(res.is_err());
}

#[test]
fn test_read_ipc_request_slow_fragmented_input_deadline() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = stop.clone();
    let sender_thread = std::thread::spawn(move || {
        while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
            if client.write_all(b" ").is_err() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_millis(20);
    let res = read_ipc_request(&mut server, deadline, MAX_IPC_REQUEST_BYTES);

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = sender_thread.join();

    let elapsed = start.elapsed();
    assert!(
        res.is_err(),
        "Expected timeout error on fragmented slow input without newline"
    );
    assert!(
        elapsed >= std::time::Duration::from_millis(15)
            && elapsed < std::time::Duration::from_millis(80),
        "Expected elapsed around 20ms, got {elapsed:?}"
    );
}

#[test]
fn test_write_ipc_response_deadline_on_blocked_receiver() {
    let (mut server, _client) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();

    // Fill OS socket buffer with large buffer without reading on client side
    let big_chunk = vec![0u8; 1024 * 1024];
    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_millis(20);

    let res = write_ipc_response(&mut server, &big_chunk, deadline);
    let elapsed = start.elapsed();
    assert!(res.is_err(), "Expected write to timeout when buffer fills");
    assert!(
        elapsed >= std::time::Duration::from_millis(15)
            && elapsed < std::time::Duration::from_millis(80),
        "Expected elapsed around 20ms, got {elapsed:?}"
    );
}

#[test]
fn test_create_ipc_server_protects_active_socket() {
    let temp_dir = std::env::temp_dir();
    let socket_path = temp_dir.join(format!("xrwm-test-active-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);

    let listener = create_ipc_server_at(&socket_path).unwrap();

    // Attempting to bind another server on the active socket must fail with AddrInUse
    let second_res = create_ipc_server_at(&socket_path);
    assert!(second_res.is_err());
    assert_eq!(
        second_res.unwrap_err().kind(),
        std::io::ErrorKind::AddrInUse
    );

    // The probe connection from second_res was queued in listener's backlog
    let (probe_conn, _) = listener.accept().unwrap();
    drop(probe_conn);

    // Verify the original listener is still active and can accept connections
    let mut client = UnixStream::connect(&socket_path).unwrap();
    client.write_all(b"test").unwrap();
    let (mut accepted, _) = listener.accept().unwrap();
    let mut buf = [0u8; 4];
    accepted.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"test");

    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_create_ipc_server_cleans_stale_socket() {
    let temp_dir = std::env::temp_dir();
    let socket_path = temp_dir.join(format!("xrwm-test-stale-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);

    // Create a listener and immediately drop it, leaving a stale socket file
    {
        let _l = UnixListener::bind(&socket_path).unwrap();
    }
    assert!(socket_path.exists());

    // create_ipc_server_at should recognize it is stale, remove it, and successfully bind
    let listener = create_ipc_server_at(&socket_path).unwrap();
    assert!(socket_path.exists());

    let _ = std::fs::remove_file(&socket_path);
    drop(listener);
}

#[test]
fn test_ipc_server_guard_drop_cleans_socket() {
    let temp_dir = std::env::temp_dir();
    let socket_path = temp_dir.join(format!("xrwm-test-guard-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).unwrap();
    assert!(socket_path.exists());

    {
        let _guard = IpcServerGuard::for_path(socket_path.clone()).unwrap();
    }
    // Guard was dropped and inode matched, file should be unlinked
    assert!(!socket_path.exists());
    drop(listener);
}

#[test]
fn test_ipc_server_guard_does_not_unlink_replaced_socket() {
    let temp_dir = std::env::temp_dir();
    let socket_path = temp_dir.join(format!(
        "xrwm-test-guard-replace-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).unwrap();
    let guard = IpcServerGuard::for_path(socket_path.clone()).unwrap();

    // Simulate replacement: original file unlinked and replaced with a new inode
    let _ = std::fs::remove_file(&socket_path);
    let replacement_listener = UnixListener::bind(&socket_path).unwrap();

    // When original guard drops, it must NOT delete the replacement socket
    drop(guard);
    assert!(
        socket_path.exists(),
        "Replacement socket was incorrectly unlinked by old guard"
    );

    let _ = std::fs::remove_file(&socket_path);
    drop(listener);
    drop(replacement_listener);
}

#[test]
fn test_parse_cli_args_border_color_validation() {
    assert!(parse_cli_args(&["border-color-focused".into(), "你好".into()]).is_err());
    assert!(parse_cli_args(&["border-color-focused".into(), "#123".into()]).is_err());
    assert!(parse_cli_args(&["border-color-focused".into(), "#61afef".into()]).is_ok());
}

#[test]
fn test_parse_cli_args_i32_bounds_validation() {
    // Values > i32::MAX should be rejected
    assert!(parse_cli_args(&["border-width".into(), "4294967295".into()]).is_err());
    assert!(parse_cli_args(&["view-padding".into(), "4294967295".into()]).is_err());
    assert!(parse_cli_args(&["outer-padding".into(), "4294967295".into()]).is_err());

    // Valid values should succeed
    assert!(parse_cli_args(&["border-width".into(), "4".into()]).is_ok());
    assert!(parse_cli_args(&["view-padding".into(), "8".into()]).is_ok());
    assert!(parse_cli_args(&["outer-padding".into(), "12".into()]).is_ok());
}

#[test]
fn test_format_socket_name_relative_display() {
    assert_eq!(format_socket_name("wayland-0"), "xrwm-wayland-0.sock");
    assert_eq!(format_socket_name("wayland-1"), "xrwm-wayland-1.sock");
    assert_eq!(
        format_socket_name("custom_display.1"),
        "xrwm-custom_display.1.sock"
    );
    // Empty or whitespace-only falls back to default wayland-0
    assert_eq!(format_socket_name(""), "xrwm-wayland-0.sock");
    assert_eq!(format_socket_name("   "), "xrwm-wayland-0.sock");

    // Preserves non-empty display value with leading/trailing whitespace without collision
    let trimmed_name = format_socket_name("wayland-0");
    let trailing_space_name = format_socket_name("wayland-0 ");
    let leading_space_name = format_socket_name(" wayland-0");
    assert_ne!(trailing_space_name, trimmed_name);
    assert_ne!(leading_space_name, trimmed_name);
    assert_ne!(trailing_space_name, leading_space_name);
}

#[test]
fn test_format_socket_name_absolute_paths_isolation() {
    let path1 = "/run/user/1000/wayland-0";
    let path2 = "/tmp/nested/test/wayland-0";

    let name1 = format_socket_name(path1);
    let name2 = format_socket_name(path2);

    // Neither contains path separators
    assert!(!name1.contains('/'));
    assert!(!name2.contains('/'));
    assert!(name1.starts_with("xrwm-wayland-0-"));
    assert!(name2.starts_with("xrwm-wayland-0-"));
    assert!(name1.ends_with(".sock"));
    assert!(name2.ends_with(".sock"));

    // Different absolute paths with identical basenames MUST have distinct socket names
    assert_ne!(name1, name2);

    // Deterministic: formatting the same path twice yields identical results
    assert_eq!(format_socket_name(path1), name1);
}

#[test]
fn test_format_socket_name_special_characters_and_edge_cases() {
    // Path with trailing slash or no valid basename
    let root_name = format_socket_name("/");
    assert!(!root_name.contains('/'));
    assert!(root_name.starts_with("xrwm-display-"));
    assert!(root_name.ends_with(".sock"));

    // Name with special characters
    let special_name = format_socket_name("display:with!special@chars");
    assert!(!special_name.contains(':'));
    assert!(!special_name.contains('!'));
    assert!(!special_name.contains('@'));
    assert!(special_name.ends_with(".sock"));
}

#[test]
fn test_socket_path_length_bounds_and_server_binding() {
    let temp_dir = std::env::temp_dir();
    // Extremely long compositor socket path (exceeding standard SUN_LEN if directly appended)
    let long_display = "/var/run/user/1000/very/deeply/nested/compositor/instance/with/a/super/long/path/hierarchy/that/would/exceed/sun_len/wayland-99.sock";

    let socket_path = socket_path_for_display(&temp_dir, long_display);

    // The overall path must be strictly within SUN_LEN limit (107 bytes)
    assert!(
        socket_path.as_os_str().len() <= MAX_SUN_LEN,
        "Socket path length {} exceeds MAX_SUN_LEN {MAX_SUN_LEN}",
        socket_path.as_os_str().len()
    );

    // Verify the generated path can actually be bound and connected
    let _ = std::fs::remove_file(&socket_path);
    let listener = create_ipc_server_at(&socket_path).unwrap();
    assert!(socket_path.exists());

    let client = UnixStream::connect(&socket_path).unwrap();
    drop(client);
    drop(listener);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_socket_path_excessive_xdg_dir_falls_back_to_tmp() {
    // Two distinct overlong XDG_RUNTIME_DIR paths (> 90 chars)
    let long_xdg1 = Path::new(
        "/var/run/user/100000/extremely/long/runtime/dir/one/that/leaves/no/room/for/any/reasonable/socket/name",
    );
    let long_xdg2 = Path::new(
        "/var/run/user/100000/extremely/long/runtime/dir/two/that/leaves/no/room/for/any/reasonable/socket/name",
    );
    let display = "/run/user/1000/wayland-0";

    let socket_path1 = socket_path_for_display(long_xdg1, display);
    let socket_path2 = socket_path_for_display(long_xdg2, display);

    // They must produce distinct fallback paths
    assert_ne!(socket_path1, socket_path2);

    // Both must be located under user-private /tmp/xrwm-<uid> fallback
    assert!(socket_path1.starts_with("/tmp"));
    assert!(socket_path2.starts_with("/tmp"));
    assert!(socket_path1.to_string_lossy().starts_with("/tmp/xrwm-"));
    assert!(socket_path2.to_string_lossy().starts_with("/tmp/xrwm-"));
    assert!(socket_path1.as_os_str().len() <= MAX_SUN_LEN);
    assert!(socket_path2.as_os_str().len() <= MAX_SUN_LEN);

    // Verify fallback directory and socket can actually be created and bound
    let _ = std::fs::remove_file(&socket_path1);
    let listener = create_ipc_server_at(&socket_path1).unwrap();
    assert!(socket_path1.exists());

    let client = UnixStream::connect(&socket_path1).unwrap();
    drop(client);
    drop(listener);
    let _ = std::fs::remove_file(&socket_path1);
}

#[test]
fn test_parse_cli_args_ratio_nan_validation() {
    for cmd in ["main-ratio", "stack-ratio"] {
        // Rejects NaN, inf, and empty
        assert!(parse_cli_args(&[cmd.into(), "NaN".into()]).is_err());
        assert!(parse_cli_args(&[cmd.into(), "+NaN".into()]).is_err());
        assert!(parse_cli_args(&[cmd.into(), "-NaN".into()]).is_err());
        assert!(parse_cli_args(&[cmd.into(), "inf".into()]).is_err());
        assert!(parse_cli_args(&[cmd.into(), "+inf".into()]).is_err());
        assert!(parse_cli_args(&[cmd.into(), "-inf".into()]).is_err());
        assert!(parse_cli_args(&[cmd.into(), "infinity".into()]).is_err());
        assert!(parse_cli_args(&[cmd.into(), "".into()]).is_err());

        // Accepts valid finite floats
        assert!(parse_cli_args(&[cmd.into(), "0.55".into()]).is_ok());
        assert!(parse_cli_args(&[cmd.into(), "+0.05".into()]).is_ok());
        assert!(parse_cli_args(&[cmd.into(), "-0.05".into()]).is_ok());
    }
}

#[test]
fn test_parse_cli_args_rule_missing_selector_value() {
    // -app-id at the end of rule-add with action already present
    let err1 = parse_cli_args(&["rule-add".into(), "float".into(), "-app-id".into()]).unwrap_err();
    assert!(err1.contains("Missing value for -app-id in rule-add"));

    // -title at the end of rule-add with action already present
    let err2 = parse_cli_args(&["rule-add".into(), "float".into(), "-title".into()]).unwrap_err();
    assert!(err2.contains("Missing value for -title in rule-add"));

    // -app-id at the end of rule-del with action already present
    let err3 = parse_cli_args(&["rule-del".into(), "float".into(), "-app-id".into()]).unwrap_err();
    assert!(err3.contains("Missing value for -app-id in rule-del"));

    // -title at the end of rule-del with action already present
    let err4 = parse_cli_args(&["rule-del".into(), "float".into(), "-title".into()]).unwrap_err();
    assert!(err4.contains("Missing value for -title in rule-del"));

    // Combination: valid -app-id followed by missing -title
    let err5 = parse_cli_args(&[
        "rule-add".into(),
        "-app-id".into(),
        "firefox".into(),
        "float".into(),
        "-title".into(),
    ])
    .unwrap_err();
    assert!(err5.contains("Missing value for -title in rule-add"));

    // Consecutive options: -app-id followed immediately by -title must be rejected
    let err_consecutive1 = parse_cli_args(&[
        "rule-add".into(),
        "-app-id".into(),
        "-title".into(),
        "float".into(),
    ])
    .unwrap_err();
    assert!(err_consecutive1.contains("Missing value for -app-id in rule-add"));

    let err_consecutive2 = parse_cli_args(&[
        "rule-del".into(),
        "-title".into(),
        "-app-id".into(),
        "float".into(),
    ])
    .unwrap_err();
    assert!(err_consecutive2.contains("Missing value for -title in rule-del"));

    // Valid combinations work as expected regardless of position
    let ok1 = parse_cli_args(&[
        "rule-add".into(),
        "float".into(),
        "-app-id".into(),
        "firefox".into(),
        "-title".into(),
        "Picture-in-Picture".into(),
    ])
    .unwrap();
    assert_eq!(
        ok1,
        IpcCommand::RuleAdd {
            app_id: Some("firefox".into()),
            title: Some("Picture-in-Picture".into()),
            action: vec!["float".into()],
        }
    );
}

#[test]
fn test_parse_cli_args_spawn_and_monocle() {
    assert_eq!(
        parse_cli_args(&["spawn".into(), "foot".into(), "-e".into(), "top".into()]).unwrap(),
        IpcCommand::Spawn(vec!["foot".into(), "-e".into(), "top".into()])
    );
    assert!(parse_cli_args(&["spawn".into()]).is_err());

    assert_eq!(
        parse_cli_args(&["toggle-monocle".into()]).unwrap(),
        IpcCommand::ToggleMonocle
    );
}
