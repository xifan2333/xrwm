use super::*;

#[test]
fn test_app_state_ipc_gaps() {
    let mut state = AppState::new();
    let cmd = IpcCommand::ViewPadding(8);
    let res = state.handle_ipc_command(&cmd);
    assert!(res.is_ok());
    assert_eq!(state.layout_config.view_padding, 8);

    let op_cmd = IpcCommand::OuterPadding(12);
    let res_op = state.handle_ipc_command(&op_cmd);
    assert!(res_op.is_ok());
    assert_eq!(state.layout_config.outer_padding, 12);
}

#[test]
fn test_app_state_ipc_rules() {
    let mut state = AppState::new();
    let cmd = IpcCommand::RuleAdd {
        app_id: Some("mpv".into()),
        title: None,
        action: vec!["float".into()],
    };
    let res = state.handle_ipc_command(&cmd);
    assert!(res.is_ok());
    assert_eq!(state.rules.len(), 1);
    assert_eq!(state.rules[0].float, Some(true));
    assert_eq!(state.rules[0].ssd, None);

    let csd_cmd = IpcCommand::RuleAdd {
        app_id: Some("foot".into()),
        title: None,
        action: vec!["csd".into()],
    };
    let res_csd = state.handle_ipc_command(&csd_cmd);
    assert!(res_csd.is_ok());
    assert_eq!(state.rules[1].ssd, Some(false));

    let dim_cmd = IpcCommand::RuleAdd {
        app_id: Some("imv".into()),
        title: None,
        action: vec!["dimensions".into(), "960".into(), "540".into()],
    };
    assert!(state.handle_ipc_command(&dim_cmd).is_ok());
    assert_eq!(state.rules[2].dimensions, Some((960, 540)));

    let tag_cmd = IpcCommand::RuleAdd {
        app_id: Some("firefox".into()),
        title: None,
        action: vec!["tags".into(), "2".into()],
    };
    assert!(state.handle_ipc_command(&tag_cmd).is_ok());
    assert_eq!(state.rules[3].tags, Some(2));

    let pos_cmd = IpcCommand::RuleAdd {
        app_id: Some("calc".into()),
        title: None,
        action: vec!["position".into(), "100".into(), "200".into()],
    };
    assert!(state.handle_ipc_command(&pos_cmd).is_ok());
    assert_eq!(state.rules[4].position, Some((100, 200)));

    let fs_cmd = IpcCommand::RuleAdd {
        app_id: Some("gamescope".into()),
        title: None,
        action: vec!["fullscreen".into()],
    };
    assert!(state.handle_ipc_command(&fs_cmd).is_ok());
    assert_eq!(state.rules[5].fullscreen, Some(true));

    let out_cmd = IpcCommand::RuleAdd {
        app_id: Some("wechat".into()),
        title: None,
        action: vec!["output".into(), "DP-1".into()],
    };
    assert!(state.handle_ipc_command(&out_cmd).is_ok());
    assert_eq!(state.rules[6].output, Some("DP-1".into()));

    // Test list-rules
    let all_rules = state.list_rules(None).unwrap();
    assert!(all_rules.contains("-app-id mpv float"));
    assert!(all_rules.contains("-app-id calc position 100 200"));
    assert!(all_rules.contains("-app-id gamescope fullscreen"));

    let float_rules = state.list_rules(Some("float")).unwrap();
    assert!(float_rules.contains("-app-id mpv float"));
    assert!(!float_rules.contains("position"));

    // Test rule-del
    let del_cmd = IpcCommand::RuleDel {
        app_id: Some("mpv".into()),
        title: None,
        action: vec!["float".into()],
    };
    assert!(state.handle_ipc_command(&del_cmd).is_ok());
    assert_eq!(state.rules.len(), 6);

    let del_not_found = IpcCommand::RuleDel {
        app_id: Some("nonexistent".into()),
        title: None,
        action: vec!["float".into()],
    };
    assert_eq!(
        state.handle_ipc_command(&del_not_found).unwrap(),
        "no matching rule found"
    );
}

#[test]
fn test_app_state_ipc_tags() {
    let mut state = AppState::new();
    assert_eq!(state.tag_state.focused, 1);

    state
        .handle_ipc_command(&IpcCommand::SetFocusedTags(5))
        .unwrap();
    assert_eq!(state.tag_state.focused, 5);

    state
        .handle_ipc_command(&IpcCommand::ToggleFocusedTags(2))
        .unwrap();
    assert_eq!(state.tag_state.focused, 7);

    state
        .handle_ipc_command(&IpcCommand::SpawnTagmask(511))
        .unwrap();
    assert_eq!(state.spawn_tagmask, 511);
}

#[test]
fn test_app_state_ipc_layout_and_animation() {
    let mut state = AppState::new();
    state
        .handle_ipc_command(&IpcCommand::MainRatio("0.65".into()))
        .unwrap();
    assert!((state.layout_config.split_ratio - 0.65).abs() < f32::EPSILON);

    state
        .handle_ipc_command(&IpcCommand::StackRatio("0.70".into()))
        .unwrap();
    assert!((state.layout_config.stack_split_ratio - 0.70).abs() < f32::EPSILON);

    state
        .handle_ipc_command(&IpcCommand::MainCount("2".into()))
        .unwrap();
    assert_eq!(state.layout_config.main_count, 2);

    state
        .handle_ipc_command(&IpcCommand::Animation(false))
        .unwrap();
    assert!(!state.anim.enabled);

    state
        .handle_ipc_command(&IpcCommand::AnimationDuration(200))
        .unwrap();
    assert_eq!(state.anim.duration, Duration::from_millis(200));

    assert!(!state.layout_config.monocle);
    let res_monocle = state
        .handle_ipc_command(&IpcCommand::ToggleMonocle)
        .unwrap();
    assert_eq!(res_monocle, "monocle mode enabled");
    assert!(state.layout_config.monocle);
    let res_monocle2 = state
        .handle_ipc_command(&IpcCommand::ToggleMonocle)
        .unwrap();
    assert_eq!(res_monocle2, "monocle mode disabled");
    assert!(!state.layout_config.monocle);

    let res_spawn = state.handle_ipc_command(&IpcCommand::Spawn(vec!["true".into()]));
    assert!(res_spawn.is_ok());
    let res_spawn_empty = state.handle_ipc_command(&IpcCommand::Spawn(vec![]));
    assert!(res_spawn_empty.is_err());
}

#[test]
fn test_app_state_ipc_map() {
    let mut state = AppState::new();
    let map_cmd = IpcCommand::Map {
        mode: "normal".into(),
        modifiers: "Super".into(),
        key: "Return".into(),
        action: vec!["spawn".into(), "foot".into()],
    };
    let res = state.handle_ipc_command(&map_cmd);
    assert!(res.is_ok());
    assert_eq!(state.pending_key_bindings.len(), 1);

    let unmap_cmd = IpcCommand::Unmap {
        mode: "normal".into(),
        modifiers: "Super".into(),
        key: "Return".into(),
    };
    let res_unmap = state.handle_ipc_command(&unmap_cmd);
    assert!(res_unmap.is_ok());
    assert_eq!(state.pending_key_bindings.len(), 0);
}

#[test]
fn test_app_state_ipc_map_pointer() {
    let mut state = AppState::new();
    let cmd = IpcCommand::MapPointer {
        mode: "normal".into(),
        modifiers: "Super".into(),
        button: "BTN_LEFT".into(),
        action: vec!["move-view".into()],
    };
    assert!(state.handle_ipc_command(&cmd).is_ok());
    assert_eq!(state.pending_pointer_bindings.len(), 1);
    assert_eq!(state.pending_pointer_bindings[0].button, 0x110);

    let unmap_ptr_cmd = IpcCommand::UnmapPointer {
        mode: "normal".into(),
        modifiers: "Super".into(),
        button: "BTN_LEFT".into(),
    };
    let res_unmap_ptr = state.handle_ipc_command(&unmap_ptr_cmd);
    assert!(res_unmap_ptr.is_ok());
    assert_eq!(state.pending_pointer_bindings.len(), 0);
}

#[test]
fn test_app_state_status_formatting() {
    let state = AppState::new();
    let json = state.format_json_status();
    assert!(json.contains("\"focused_tags\":1"));
    assert!(json.contains("\"layout\":\"master-stack\""));

    let waybar = state.format_waybar_status();
    assert!(waybar.contains("\"text\":\"1\""));
}

#[test]
fn test_app_state_modal_modes_and_relative_ratio() {
    let mut state = AppState::new();
    assert_eq!(state.active_mode, "normal");

    state.declare_mode("resize").unwrap();
    assert!(state.modes.contains(&"resize".to_string()));

    state.enter_mode("resize").unwrap();
    assert_eq!(state.active_mode, "resize");

    state.enter_mode("normal").unwrap();
    assert_eq!(state.active_mode, "normal");

    // Relative ratio adjustment
    let initial_ratio = state.layout_config.split_ratio;
    state.set_main_ratio_arg("+0.05").unwrap();
    assert!((state.layout_config.split_ratio - (initial_ratio + 0.05)).abs() < 1e-4);

    state.set_main_ratio_arg("-0.10").unwrap();
    assert!((state.layout_config.split_ratio - (initial_ratio - 0.05)).abs() < 1e-4);

    // Stack ratio adjustment
    let initial_stack_ratio = state.layout_config.stack_split_ratio;
    state.set_stack_ratio_arg("+0.05").unwrap();
    assert!((state.layout_config.stack_split_ratio - (initial_stack_ratio + 0.05)).abs() < 1e-4);

    state.set_stack_ratio_arg("-0.10").unwrap();
    assert!((state.layout_config.stack_split_ratio - (initial_stack_ratio - 0.05)).abs() < 1e-4);

    state.set_stack_ratio(0.85).unwrap();
    assert!((state.layout_config.stack_split_ratio - 0.85).abs() < f32::EPSILON);

    // Clamping test
    state.set_stack_ratio(1.5).unwrap();
    assert!((state.layout_config.stack_split_ratio - 0.9).abs() < f32::EPSILON);
    state.set_stack_ratio(-0.5).unwrap();
    assert!((state.layout_config.stack_split_ratio - 0.1).abs() < f32::EPSILON);

    // Action tokens test
    state.execute_action_tokens(&["main-ratio".into(), "0.60".into()]);
    assert!((state.layout_config.split_ratio - 0.60).abs() < 1e-4);
    state.execute_action_tokens(&["stack-ratio".into(), "0.60".into()]);
    assert!((state.layout_config.stack_split_ratio - 0.60).abs() < 1e-4);
    state.execute_action_tokens(&["stack-ratio".into(), "+0.10".into()]);
    assert!((state.layout_config.stack_split_ratio - 0.70).abs() < 1e-4);
    state.execute_action_tokens(&["view-padding".into(), "12".into()]);
    assert_eq!(state.layout_config.view_padding, 12);
    state.execute_action_tokens(&["outer-padding".into(), "16".into()]);
    assert_eq!(state.layout_config.outer_padding, 16);

    // Attach mode test
    state.execute_action_tokens(&["default-attach-mode".into(), "bottom".into()]);
    assert_eq!(state.attach_mode, AttachMode::Bottom);
    state.execute_action_tokens(&["default-attach-mode".into(), "after".into(), "3".into()]);
    assert_eq!(state.attach_mode, AttachMode::After(3));

    // Cursor warp and focus-follows-cursor tests
    state.execute_action_tokens(&["set-cursor-warp".into(), "on-output-change".into()]);
    assert_eq!(state.cursor_warp, crate::wm::CursorWarp::OnOutputChange);
    state.execute_action_tokens(&["set-cursor-warp".into(), "disabled".into()]);
    assert_eq!(state.cursor_warp, crate::wm::CursorWarp::Disabled);

    state.execute_action_tokens(&["focus-follows-cursor".into(), "disabled".into()]);
    assert_eq!(
        state.focus_follows_cursor,
        crate::wm::FocusFollowsCursor::Disabled
    );
    state.execute_action_tokens(&["focus-follows-cursor".into(), "always".into()]);
    assert_eq!(
        state.focus_follows_cursor,
        crate::wm::FocusFollowsCursor::Always
    );

    // Hide cursor tests
    assert!(state.set_hide_cursor_timeout(3000).is_err());
    assert_eq!(state.cursor_hide_timeout, 0);
    assert!(state.set_hide_cursor_timeout(0).is_ok());
    assert_eq!(state.cursor_hide_timeout, 0);

    assert!(state.set_hide_cursor_when_typing(true).is_err());
    assert!(!state.cursor_hide_when_typing);
    assert!(state.set_hide_cursor_when_typing(false).is_ok());
    assert!(!state.cursor_hide_when_typing);

    // Relative count adjustment
    assert_eq!(state.layout_config.main_count, 1);
    state.set_main_count_arg("+1").unwrap();
    assert_eq!(state.layout_config.main_count, 2);
    state.set_main_count_arg("-1").unwrap();
    assert_eq!(state.layout_config.main_count, 1);
}

#[test]
fn test_workspace_slide_direction() {
    let mut state = AppState::new();
    assert_eq!(state.tag_state.focused, 1);
    assert!(state.tag_slide_dir.is_none());

    // Tag 1 -> Tag 2 (higher index) => SlideDirection::Right
    state.set_focused_tags(2).unwrap();
    assert_eq!(
        state.tag_slide_dir,
        Some(crate::animation::SlideDirection::Right)
    );
    assert_eq!(state.tag_anim_old_mask, 1);

    // Tag 2 -> Tag 1 (lower index) => SlideDirection::Left
    state.set_focused_tags(1).unwrap();
    assert_eq!(
        state.tag_slide_dir,
        Some(crate::animation::SlideDirection::Left)
    );
    assert_eq!(state.tag_anim_old_mask, 2);
}

#[test]
fn test_app_state_empty_actions() {
    let mut state = AppState::new();
    assert_eq!(state.close_focused().unwrap(), "no view focused to close");
    assert!(state.toggle_float_focused().is_err());
    assert!(state.zoom_focused().is_err());
    assert_eq!(state.focus_view(true).unwrap(), "no visible windows");
    assert!(state.snap_focused("left").is_err());
    assert!(state.move_window("left", 50).is_err());
    assert_eq!(
        state.focus_view_direction("next", true).unwrap(),
        "no visible windows"
    );

    state.execute_action_tokens(&["snap".into(), "left".into()]);
    state.execute_action_tokens(&["move".into(), "left".into(), "50".into()]);
    state.execute_action_tokens(&["focus-view".into(), "-skip-floating".into(), "next".into()]);
    state.execute_action_tokens(&[
        "send-to-output".into(),
        "-current-tags".into(),
        "right".into(),
    ]);

    // Multi-output on empty state
    assert_eq!(
        state
            .handle_ipc_command(&IpcCommand::FocusOutput("next".into()))
            .unwrap(),
        "no destination output found"
    );
    assert_eq!(
        state
            .handle_ipc_command(&IpcCommand::SendToOutput {
                direction: "next".into(),
                current_tags: false,
            })
            .unwrap(),
        "no destination output found"
    );
}

#[test]
fn test_pick_adjacent_output() {
    // Two side-by-side monitors:
    // Output A: 0, 0, 1920, 1080
    // Output B: 1920, 0, 1920, 1080
    let a = "A";
    let b = "B";
    let outputs = vec![(&a, 0, 0, 1920, 1080), (&b, 1920, 0, 1920, 1080)];

    // From Output A (idx 0):
    assert_eq!(pick_adjacent_output(&outputs, 0, "next"), Some("B"));
    assert_eq!(pick_adjacent_output(&outputs, 0, "right"), Some("B"));
    assert_eq!(pick_adjacent_output(&outputs, 0, "left"), None);
    assert_eq!(pick_adjacent_output(&outputs, 0, "up"), None);
    assert_eq!(pick_adjacent_output(&outputs, 0, "down"), None);

    // From Output B (idx 1):
    assert_eq!(pick_adjacent_output(&outputs, 1, "next"), Some("A"));
    assert_eq!(pick_adjacent_output(&outputs, 1, "previous"), Some("A"));
    assert_eq!(pick_adjacent_output(&outputs, 1, "left"), Some("A"));
    assert_eq!(pick_adjacent_output(&outputs, 1, "right"), None);

    // Two stacked monitors:
    // Top: 0, 0, 1920, 1080
    // Bottom: 0, 1080, 1920, 1080
    let top = "Top";
    let bottom = "Bottom";
    let stacked = vec![(&top, 0, 0, 1920, 1080), (&bottom, 0, 1080, 1920, 1080)];
    assert_eq!(pick_adjacent_output(&stacked, 0, "down"), Some("Bottom"));
    assert_eq!(pick_adjacent_output(&stacked, 0, "up"), None);
    assert_eq!(pick_adjacent_output(&stacked, 1, "up"), Some("Top"));
    assert_eq!(pick_adjacent_output(&stacked, 1, "down"), None);
}

#[test]
fn test_border_color_ipc_rejection_and_retention() {
    let mut state = AppState::new();
    let original_focused = state.border_color_focused.clone();

    // Non-ASCII input rejected
    let res = state.handle_ipc_command(&IpcCommand::BorderColorFocused("你好".into()));
    assert!(res.is_err());
    assert_eq!(state.border_color_focused, original_focused);

    // Invalid hex rejected
    let res2 = state.handle_ipc_command(&IpcCommand::BorderColorFocused("xyz123".into()));
    assert!(res2.is_err());
    assert_eq!(state.border_color_focused, original_focused);

    // Valid hex accepted
    let res3 = state.handle_ipc_command(&IpcCommand::BorderColorFocused("#112233".into()));
    assert!(res3.is_ok());
    assert_eq!(state.border_color_focused, "#112233");
}

#[test]
fn test_i32_bounds_ipc_rejection() {
    let mut state = AppState::new();

    // Border width > i32::MAX rejected
    let orig_bw = state.border_width;
    let res = state.handle_ipc_command(&IpcCommand::BorderWidth(u32::MAX));
    assert!(res.is_err());
    assert_eq!(state.border_width, orig_bw);

    // View padding > i32::MAX rejected
    let orig_vp = state.layout_config.view_padding;
    let res_vp = state.handle_ipc_command(&IpcCommand::ViewPadding(u32::MAX));
    assert!(res_vp.is_err());
    assert_eq!(state.layout_config.view_padding, orig_vp);

    // Outer padding > i32::MAX rejected
    let orig_op = state.layout_config.outer_padding;
    let res_op = state.handle_ipc_command(&IpcCommand::OuterPadding(u32::MAX));
    assert!(res_op.is_err());
    assert_eq!(state.layout_config.outer_padding, orig_op);

    // Dimensions > i32::MAX in rule-add rejected
    let orig_rules_len = state.rules.len();
    let res_rule = state.handle_ipc_command(&IpcCommand::RuleAdd {
        app_id: Some("mpv".into()),
        title: None,
        action: vec!["dimensions".into(), "4294967295".into(), "600".into()],
    });
    assert!(res_rule.is_err());
    assert_eq!(state.rules.len(), orig_rules_len);

    // Valid dimensions accepted
    let res_valid = state.handle_ipc_command(&IpcCommand::RuleAdd {
        app_id: Some("mpv".into()),
        title: None,
        action: vec!["dimensions".into(), "960".into(), "540".into()],
    });
    assert!(res_valid.is_ok());
    assert_eq!(state.rules.len(), orig_rules_len + 1);
}

fn wait_for_process_exit(pid: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            if stat.contains(") Z") || stat.contains(") X") {
                return;
            }
        } else {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
#[allow(clippy::zombie_processes)]
fn test_reap_zombies_cleans_exited_children() {
    let _guard = crate::wm::state::PROCESS_TEST_MUTEX.lock().unwrap();
    let child1 = std::process::Command::new("true").spawn().unwrap();
    let child2 = std::process::Command::new("true").spawn().unwrap();
    let child3 = std::process::Command::new("true").spawn().unwrap();

    wait_for_process_exit(child1.id());
    wait_for_process_exit(child2.id());
    wait_for_process_exit(child3.id());

    // Reaping must drain all dead child processes
    reap_zombies();

    // Verifying with waitpid for child1 should yield ECHILD because it's already reaped
    let pid = rustix::process::Pid::from_raw(child1.id() as i32).unwrap();
    let res = rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG);
    assert!(matches!(res, Err(rustix::io::Errno::CHILD)));

    let pid2 = rustix::process::Pid::from_raw(child2.id() as i32).unwrap();
    let res2 = rustix::process::waitpid(Some(pid2), rustix::process::WaitOptions::NOHANG);
    assert!(matches!(res2, Err(rustix::io::Errno::CHILD)));

    let pid3 = rustix::process::Pid::from_raw(child3.id() as i32).unwrap();
    let res3 = rustix::process::waitpid(Some(pid3), rustix::process::WaitOptions::NOHANG);
    assert!(matches!(res3, Err(rustix::io::Errno::CHILD)));
}

#[test]
fn test_reap_zombies_does_not_block_on_running_child() {
    let _guard = crate::wm::state::PROCESS_TEST_MUTEX.lock().unwrap();
    // Spawn a long-running process
    let mut child = std::process::Command::new("sleep")
        .arg("10")
        .spawn()
        .unwrap();

    // reap_zombies must return immediately without blocking
    let start = std::time::Instant::now();
    reap_zombies();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));

    // Child process should still be running
    assert!(child.try_wait().unwrap().is_none());

    // Cleanup
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
#[allow(clippy::zombie_processes)]
fn test_spawn_and_reload_actions_reap_children() {
    let _guard = crate::wm::state::PROCESS_TEST_MUTEX.lock().unwrap();
    let mut state = AppState::new();

    // Spawn a child and wait for it to exit
    let child1 = std::process::Command::new("true").spawn().unwrap();
    let pid1 = child1.id();
    wait_for_process_exit(pid1);

    // Subsequent spawn action automatically reaps previous dead children
    state.execute_action_tokens(&["spawn".into(), "true".into()]);
    let rustix_pid1 = rustix::process::Pid::from_raw(pid1 as i32).unwrap();
    assert!(matches!(
        rustix::process::waitpid(Some(rustix_pid1), rustix::process::WaitOptions::NOHANG),
        Err(rustix::io::Errno::CHILD)
    ));

    // Spawn another child and wait for it to exit
    let child2 = std::process::Command::new("true").spawn().unwrap();
    let pid2 = child2.id();
    wait_for_process_exit(pid2);

    // Reload command also reaps dead children
    let res = state.handle_ipc_command(&IpcCommand::Reload);
    assert!(res.is_ok());

    let rustix_pid2 = rustix::process::Pid::from_raw(pid2 as i32).unwrap();
    assert!(matches!(
        rustix::process::waitpid(Some(rustix_pid2), rustix::process::WaitOptions::NOHANG),
        Err(rustix::io::Errno::CHILD)
    ));
}

fn split_shell_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for ch in input.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn validate_action_tokens(action: &[String]) {
    assert!(!action.is_empty(), "Action tokens cannot be empty");
    if action[0] == "spawn" {
        assert!(
            action.len() >= 2,
            "spawn action requires at least 1 command argument: {:?}",
            action
        );
    } else {
        let parsed = crate::ipc::parse_cli_args(action).unwrap_or_else(|e| {
            panic!("Invalid action tokens in map: {:?}, error: {}", action, e);
        });
        let mut test_state = AppState::new();
        let _ = test_state.handle_ipc_command(&parsed);
    }
}

#[test]
fn test_examples_init_all_commands_are_valid() {
    let content = std::fs::read_to_string("examples/init").expect("examples/init must exist");
    let mut state = AppState::new();

    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("xrwm ") {
            continue;
        }

        // Skip template lines inside loops (e.g. "$i", "$tags") which are tested separately
        if trimmed.contains("$i") || trimmed.contains("$tags") {
            continue;
        }

        let cmd_str = &trimmed[5..];
        let tokens = split_shell_tokens(cmd_str);
        assert!(
            !tokens.is_empty(),
            "Empty command at line {}: {}",
            line_no + 1,
            line
        );

        let cmd = crate::ipc::parse_cli_args(&tokens).unwrap_or_else(|e| {
            panic!(
                "Failed to parse CLI args at line {}: {}\nError: {}",
                line_no + 1,
                line,
                e
            );
        });

        // If command is a key mapping, separately validate its action tokens
        if let IpcCommand::Map { ref action, .. } = cmd {
            validate_action_tokens(action);
        }

        state.handle_ipc_command(&cmd).unwrap_or_else(|e| {
            panic!(
                "Failed to handle IPC command at line {}: {}\nError: {}",
                line_no + 1,
                line,
                e
            );
        });
    }

    // Test loop lines with concrete values
    for i in 1..=9 {
        let tags = 1 << (i - 1);
        let action1 = vec!["set-focused-tags".into(), tags.to_string()];
        validate_action_tokens(&action1);
        let cmd1 = crate::ipc::parse_cli_args(&[
            "map".into(),
            "normal".into(),
            "Super".into(),
            i.to_string(),
            "set-focused-tags".into(),
            tags.to_string(),
        ])
        .unwrap();
        state.handle_ipc_command(&cmd1).unwrap();

        let action2 = vec!["set-view-tags".into(), tags.to_string()];
        validate_action_tokens(&action2);
        let cmd2 = crate::ipc::parse_cli_args(&[
            "map".into(),
            "normal".into(),
            "Super+Shift".into(),
            i.to_string(),
            "set-view-tags".into(),
            tags.to_string(),
        ])
        .unwrap();
        state.handle_ipc_command(&cmd2).unwrap();
    }
}

#[test]
fn test_stack_ratio_keybindings_parsing_and_handling() {
    let mut state = AppState::new();
    state.layout_config.stack_split_ratio = 0.50;

    let cmd_left = crate::ipc::parse_cli_args(&[
        "map".into(),
        "normal".into(),
        "Super".into(),
        "bracketleft".into(),
        "stack-ratio".into(),
        "-0.05".into(),
    ])
    .unwrap();
    assert!(state.handle_ipc_command(&cmd_left).is_ok());

    // Validate and execute the action tokens for bracketleft
    if let IpcCommand::Map { ref action, .. } = cmd_left {
        validate_action_tokens(action);
        state.execute_action_tokens(action);
        assert!(
            (state.layout_config.stack_split_ratio - 0.45).abs() < 1e-4,
            "Expected stack-ratio to decrease to 0.45, got {}",
            state.layout_config.stack_split_ratio
        );
    } else {
        panic!("Expected IpcCommand::Map");
    }

    let cmd_right = crate::ipc::parse_cli_args(&[
        "map".into(),
        "normal".into(),
        "Super".into(),
        "bracketright".into(),
        "stack-ratio".into(),
        "+0.05".into(),
    ])
    .unwrap();
    assert!(state.handle_ipc_command(&cmd_right).is_ok());

    // Validate and execute the action tokens for bracketright
    if let IpcCommand::Map { ref action, .. } = cmd_right {
        validate_action_tokens(action);
        state.execute_action_tokens(action);
        assert!(
            (state.layout_config.stack_split_ratio - 0.50).abs() < 1e-4,
            "Expected stack-ratio to increase back to 0.50, got {}",
            state.layout_config.stack_split_ratio
        );
    } else {
        panic!("Expected IpcCommand::Map");
    }
}

#[test]
fn test_toggle_focused_tags_updates_previous_tags_history() {
    let mut state = AppState::new();
    assert_eq!(state.tag_state.focused, 1);
    assert_eq!(state.previous_focused_tags, 1);

    // 1. set-focused-tags 2
    state
        .handle_ipc_command(&IpcCommand::SetFocusedTags(2))
        .unwrap();
    assert_eq!(state.tag_state.focused, 2);
    assert_eq!(state.previous_focused_tags, 1);

    // 2. toggle-focused-tags 4 -> focused becomes 6 (2 | 4)
    // previous_focused_tags must be updated to 2
    state
        .handle_ipc_command(&IpcCommand::ToggleFocusedTags(4))
        .unwrap();
    assert_eq!(state.tag_state.focused, 6);
    assert_eq!(state.previous_focused_tags, 2);

    // 3. focus-previous-tags returns to 2
    state
        .handle_ipc_command(&IpcCommand::FocusPreviousTags)
        .unwrap();
    assert_eq!(state.tag_state.focused, 2);
    assert_eq!(state.previous_focused_tags, 6);

    // 4. focus-previous-tags returns back to 6
    state
        .handle_ipc_command(&IpcCommand::FocusPreviousTags)
        .unwrap();
    assert_eq!(state.tag_state.focused, 6);
    assert_eq!(state.previous_focused_tags, 2);

    // 5. No-op toggle (0) does not pollute history
    state
        .handle_ipc_command(&IpcCommand::ToggleFocusedTags(0))
        .unwrap();
    assert_eq!(state.tag_state.focused, 6);
    assert_eq!(state.previous_focused_tags, 2);

    // 6. Invalid toggle (would zero all tags) does not change focused tags and does not pollute history
    state
        .handle_ipc_command(&IpcCommand::ToggleFocusedTags(6))
        .unwrap();
    assert_eq!(state.tag_state.focused, 6);
    assert_eq!(state.previous_focused_tags, 2);
}

#[test]
fn test_set_ratio_rejects_nan_and_preserves_previous_ratio() {
    let mut state = AppState::new();
    state.layout_config.split_ratio = 0.55;
    state.layout_config.stack_split_ratio = 0.50;

    // 1. main-ratio rejects non-finite values and preserves previous ratio
    for bad in ["NaN", "+NaN", "-NaN", "inf", "+inf", "-inf", "infinity"] {
        let res = state.handle_ipc_command(&IpcCommand::MainRatio(bad.into()));
        assert!(res.is_err(), "Expected error for main-ratio {}", bad);
        assert_eq!(state.layout_config.split_ratio, 0.55);
    }

    // Relative adjustment still functions properly
    state
        .handle_ipc_command(&IpcCommand::MainRatio("+0.05".into()))
        .unwrap();
    assert!((state.layout_config.split_ratio - 0.60).abs() < 1e-4);

    // 2. stack-ratio rejects non-finite values and preserves previous ratio
    for bad in ["NaN", "+NaN", "-NaN", "inf", "+inf", "-inf", "infinity"] {
        let res = state.handle_ipc_command(&IpcCommand::StackRatio(bad.into()));
        assert!(res.is_err(), "Expected error for stack-ratio {}", bad);
        assert_eq!(state.layout_config.stack_split_ratio, 0.50);
    }

    // Relative adjustment still functions properly
    state
        .handle_ipc_command(&IpcCommand::StackRatio("-0.05".into()))
        .unwrap();
    assert!((state.layout_config.stack_split_ratio - 0.45).abs() < 1e-4);

    // Direct f32 setters also reject non-finite values
    assert!(state.set_main_ratio(f32::NAN).is_err());
    assert!(state.set_main_ratio(f32::INFINITY).is_err());
    assert!(state.set_stack_ratio(f32::NAN).is_err());
    assert!(state.set_stack_ratio(f32::NEG_INFINITY).is_err());
}

#[test]
fn test_hide_cursor_unsupported_rejection_and_disable_acceptance() {
    let mut state = AppState::new();

    // 1. hide-cursor timeout with timeout > 0 must be rejected with informative error
    let res = state.handle_ipc_command(&IpcCommand::HideCursorTimeout(1000));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("not supported"));
    assert!(err.contains("River protocol"));
    assert_eq!(state.cursor_hide_timeout, 0);

    // Action token execution with positive timeout should also fail safely
    state.execute_action_tokens(&["hide-cursor".into(), "timeout".into(), "5000".into()]);
    assert_eq!(state.cursor_hide_timeout, 0);

    // hide-cursor timeout 0 (disable) is accepted
    let res_disable = state.handle_ipc_command(&IpcCommand::HideCursorTimeout(0));
    assert!(res_disable.is_ok());
    assert_eq!(state.cursor_hide_timeout, 0);

    // 2. hide-cursor when-typing enabled must be rejected with informative error
    let res_typing = state.handle_ipc_command(&IpcCommand::HideCursorWhenTyping(true));
    assert!(res_typing.is_err());
    let err_typing = res_typing.unwrap_err();
    assert!(err_typing.contains("not supported"));
    assert!(err_typing.contains("River protocol"));
    assert!(!state.cursor_hide_when_typing);

    // Action token execution with enabled should also fail safely
    state.execute_action_tokens(&["hide-cursor".into(), "when-typing".into(), "enabled".into()]);
    assert!(!state.cursor_hide_when_typing);

    // hide-cursor when-typing disabled is accepted
    let res_disable_typing = state.handle_ipc_command(&IpcCommand::HideCursorWhenTyping(false));
    assert!(res_disable_typing.is_ok());
    assert!(!state.cursor_hide_when_typing);

    // 3. cursor hidden/unhidden toggle transitions
    assert!(!state.cursor_hidden);
    state.hide_cursor();
    assert!(state.cursor_hidden);
    state.unhide_cursor();
    assert!(!state.cursor_hidden);
}

#[test]
fn test_session_locked_and_unlocked_lifecycle() {
    let mut state = AppState::new();
    assert_eq!(state.active_mode, "normal");
    assert!(!state.session_locked);
    assert!(state.pre_lock_mode.is_none());

    // 1. Normal -> Locked
    state.handle_session_locked();
    assert!(state.session_locked);
    assert_eq!(state.active_mode, "locked");
    assert_eq!(state.pre_lock_mode.as_deref(), Some("normal"));
    assert!(state.mode_dirty);

    // While locked, switching to normal or another mode via enter_mode is blocked
    assert!(state.enter_mode("normal").is_err());
    assert_eq!(state.active_mode, "locked");

    // Entering "locked" while locked is allowed
    assert!(state.enter_mode("locked").is_ok());

    // Redundant SessionLocked event preserves the original pre_lock_mode
    state.handle_session_locked();
    assert_eq!(state.pre_lock_mode.as_deref(), Some("normal"));
    assert_eq!(state.active_mode, "locked");

    // Unlock restores pre-lock mode ("normal")
    state.mode_dirty = false;
    state.handle_session_unlocked();
    assert!(!state.session_locked);
    assert_eq!(state.active_mode, "normal");
    assert!(state.pre_lock_mode.is_none());
    assert!(state.mode_dirty);

    // 2. Custom mode -> Locked -> Custom mode
    state.declare_mode("passthrough").unwrap();
    state.enter_mode("passthrough").unwrap();
    assert_eq!(state.active_mode, "passthrough");

    state.handle_session_locked();
    assert!(state.session_locked);
    assert_eq!(state.active_mode, "locked");
    assert_eq!(state.pre_lock_mode.as_deref(), Some("passthrough"));

    state.handle_session_unlocked();
    assert!(!state.session_locked);
    assert_eq!(state.active_mode, "passthrough");
    assert!(state.pre_lock_mode.is_none());

    // 3. Fallback to normal if pre_lock_mode is missing or invalid
    state.handle_session_locked();
    state.pre_lock_mode = None;
    state.handle_session_unlocked();
    assert_eq!(state.active_mode, "normal");
}

#[test]
fn test_configured_bindings_retention_and_seat_inheritance() {
    let mut state = AppState::new();

    // 1. Initial state has zero configured or pending bindings
    assert!(state.configured_key_bindings.is_empty());
    assert!(state.pending_key_bindings.is_empty());
    assert!(state.configured_pointer_bindings.is_empty());
    assert!(state.pending_pointer_bindings.is_empty());

    // 2. Configure keybindings before any seat exists
    let map_cmd1 = IpcCommand::Map {
        mode: "normal".into(),
        modifiers: "Super".into(),
        key: "Return".into(),
        action: vec!["spawn".into(), "foot".into()],
    };
    state.handle_ipc_command(&map_cmd1).unwrap();

    let map_cmd2 = IpcCommand::Map {
        mode: "normal".into(),
        modifiers: "Super+Shift".into(),
        key: "Q".into(),
        action: vec!["close".into()],
    };
    state.handle_ipc_command(&map_cmd2).unwrap();

    assert_eq!(state.configured_key_bindings.len(), 2);
    assert_eq!(state.pending_key_bindings.len(), 2);

    // Re-mapping Super+Return with a new action replaces it in configured_key_bindings
    let map_cmd1_update = IpcCommand::Map {
        mode: "normal".into(),
        modifiers: "Super".into(),
        key: "Return".into(),
        action: vec!["spawn".into(), "alacritty".into()],
    };
    state.handle_ipc_command(&map_cmd1_update).unwrap();
    assert_eq!(state.configured_key_bindings.len(), 2);
    assert_eq!(
        state.configured_key_bindings[1].action,
        vec!["spawn".to_string(), "alacritty".to_string()]
    );

    // 3. Configure pointer bindings before any seat exists
    let ptr_cmd1 = IpcCommand::MapPointer {
        mode: "normal".into(),
        modifiers: "Super".into(),
        button: "BTN_LEFT".into(),
        action: vec!["move-view".into()],
    };
    state.handle_ipc_command(&ptr_cmd1).unwrap();

    let ptr_cmd2 = IpcCommand::MapPointer {
        mode: "normal".into(),
        modifiers: "Super".into(),
        button: "BTN_RIGHT".into(),
        action: vec!["resize-view".into()],
    };
    state.handle_ipc_command(&ptr_cmd2).unwrap();

    assert_eq!(state.configured_pointer_bindings.len(), 2);
    assert_eq!(state.pending_pointer_bindings.len(), 2);

    // 4. Unmapping removes from configured_key_bindings and configured_pointer_bindings
    let unmap_key_cmd = IpcCommand::Unmap {
        mode: "normal".into(),
        modifiers: "Super+Shift".into(),
        key: "Q".into(),
    };
    state.handle_ipc_command(&unmap_key_cmd).unwrap();
    assert_eq!(state.configured_key_bindings.len(), 1);

    let unmap_ptr_cmd = IpcCommand::UnmapPointer {
        mode: "normal".into(),
        modifiers: "Super".into(),
        button: "BTN_RIGHT".into(),
    };
    state.handle_ipc_command(&unmap_ptr_cmd).unwrap();
    assert_eq!(state.configured_pointer_bindings.len(), 1);
}

#[test]
fn test_unknown_modifiers_rejected_without_state_mutation() {
    let mut state = AppState::new();

    // 1. Map command with unknown modifier must return Err
    let bad_map = IpcCommand::Map {
        mode: "normal".into(),
        modifiers: "Supr".into(),
        key: "q".into(),
        action: vec!["close".into()],
    };
    let res = state.handle_ipc_command(&bad_map);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("Unknown modifier: Supr"));
    assert!(state.configured_key_bindings.is_empty());
    assert!(state.pending_key_bindings.is_empty());

    // 2. MapPointer command with unknown modifier must return Err
    let bad_map_ptr = IpcCommand::MapPointer {
        mode: "normal".into(),
        modifiers: "Super+Unknown".into(),
        button: "BTN_LEFT".into(),
        action: vec!["move-view".into()],
    };
    let res_ptr = state.handle_ipc_command(&bad_map_ptr);
    assert!(res_ptr.is_err());
    assert!(state.configured_pointer_bindings.is_empty());
    assert!(state.pending_pointer_bindings.is_empty());

    // 3. Unmap commands with unknown modifier must return Err
    let bad_unmap = IpcCommand::Unmap {
        mode: "normal".into(),
        modifiers: "BadMod".into(),
        key: "q".into(),
    };
    assert!(state.handle_ipc_command(&bad_unmap).is_err());

    let bad_unmap_ptr = IpcCommand::UnmapPointer {
        mode: "normal".into(),
        modifiers: "BadMod".into(),
        button: "BTN_LEFT".into(),
    };
    assert!(state.handle_ipc_command(&bad_unmap_ptr).is_err());

    // 4. Action tokens execution with unknown modifier fails safely
    state.execute_action_tokens(&[
        "map".into(),
        "normal".into(),
        "Supr".into(),
        "q".into(),
        "close".into(),
    ]);
    assert!(state.configured_key_bindings.is_empty());
}

#[test]
fn test_mode_case_insensitivity_and_binding_retention() {
    let mut state = AppState::new();

    // 1. Enter built-in normal mode with uppercase NORMAL
    assert_eq!(state.active_mode, "normal");
    state.enter_mode("NORMAL").unwrap();
    assert_eq!(state.active_mode, "normal");

    // 2. Map keys with mixed case mode "Normal"
    let map_cmd = IpcCommand::Map {
        mode: "Normal".into(),
        modifiers: "Super".into(),
        key: "Return".into(),
        action: vec!["spawn".into(), "foot".into()],
    };
    state.handle_ipc_command(&map_cmd).unwrap();
    assert_eq!(state.configured_key_bindings.len(), 1);
    assert_eq!(state.configured_key_bindings[0].mode, "normal");

    // 3. Declare custom mode with mixed case and leading/trailing whitespace
    state.declare_mode("  ReSize  ").unwrap();
    assert!(state.modes.contains(&"resize".to_string()));

    // Enter custom mode using all caps
    state.enter_mode("RESIZE").unwrap();
    assert_eq!(state.active_mode, "resize");

    // Map pointer using all caps mode "RESIZE"
    let ptr_cmd = IpcCommand::MapPointer {
        mode: "RESIZE".into(),
        modifiers: "Super".into(),
        button: "BTN_LEFT".into(),
        action: vec!["move-view".into()],
    };
    state.handle_ipc_command(&ptr_cmd).unwrap();
    assert_eq!(state.configured_pointer_bindings.len(), 1);
    assert_eq!(state.configured_pointer_bindings[0].mode, "resize");

    // 4. Unmap using mixed case
    let unmap_ptr = IpcCommand::UnmapPointer {
        mode: "ReSiZe".into(),
        modifiers: "Super".into(),
        button: "BTN_LEFT".into(),
    };
    state.handle_ipc_command(&unmap_ptr).unwrap();
    assert!(state.configured_pointer_bindings.is_empty());

    let unmap_key = IpcCommand::Unmap {
        mode: "NORMAL".into(),
        modifiers: "Super".into(),
        key: "Return".into(),
    };
    state.handle_ipc_command(&unmap_key).unwrap();
    assert!(state.configured_key_bindings.is_empty());

    // 5. Undeclared mode fails
    assert!(state.enter_mode("NonExistentMode").is_err());
}

#[test]
fn test_remapping_replaces_duplicate_bindings_without_accumulation() {
    let mut state = AppState::new();

    // 1. Initial key mapping
    let map_cmd1 = IpcCommand::Map {
        mode: "normal".into(),
        modifiers: "Super".into(),
        key: "q".into(),
        action: vec!["close".into()],
    };
    state.handle_ipc_command(&map_cmd1).unwrap();
    assert_eq!(state.configured_key_bindings.len(), 1);
    assert_eq!(state.pending_key_bindings.len(), 1);
    assert_eq!(state.configured_key_bindings[0].action, vec!["close"]);

    // Re-map with different action replaces the binding
    let map_cmd2 = IpcCommand::Map {
        mode: "normal".into(),
        modifiers: "Super".into(),
        key: "q".into(),
        action: vec!["toggle-float".into()],
    };
    state.handle_ipc_command(&map_cmd2).unwrap();
    assert_eq!(state.configured_key_bindings.len(), 1);
    assert_eq!(state.pending_key_bindings.len(), 1);
    assert_eq!(
        state.configured_key_bindings[0].action,
        vec!["toggle-float"]
    );

    // Multiple reloads (re-applying the exact same map) do not accumulate bindings
    for _ in 0..3 {
        state.handle_ipc_command(&map_cmd2).unwrap();
    }
    assert_eq!(state.configured_key_bindings.len(), 1);
    assert_eq!(state.pending_key_bindings.len(), 1);

    // 2. Initial pointer mapping
    let ptr_cmd1 = IpcCommand::MapPointer {
        mode: "normal".into(),
        modifiers: "Super".into(),
        button: "BTN_LEFT".into(),
        action: vec!["move-view".into()],
    };
    state.handle_ipc_command(&ptr_cmd1).unwrap();
    assert_eq!(state.configured_pointer_bindings.len(), 1);
    assert_eq!(state.pending_pointer_bindings.len(), 1);
    assert_eq!(
        state.configured_pointer_bindings[0].action,
        crate::wm::seat::PointerAction::Move
    );

    // Re-map with different action replaces the pointer binding
    let ptr_cmd2 = IpcCommand::MapPointer {
        mode: "normal".into(),
        modifiers: "Super".into(),
        button: "BTN_LEFT".into(),
        action: vec!["resize-view".into()],
    };
    state.handle_ipc_command(&ptr_cmd2).unwrap();
    assert_eq!(state.configured_pointer_bindings.len(), 1);
    assert_eq!(state.pending_pointer_bindings.len(), 1);
    assert_eq!(
        state.configured_pointer_bindings[0].action,
        crate::wm::seat::PointerAction::Resize
    );

    // Multiple reloads do not accumulate pointer bindings
    for _ in 0..3 {
        state.handle_ipc_command(&ptr_cmd2).unwrap();
    }
    assert_eq!(state.configured_pointer_bindings.len(), 1);
    assert_eq!(state.pending_pointer_bindings.len(), 1);
}

#[test]
fn test_rule_add_tags_direct_bitmask_and_zero_rejection() {
    let mut state = AppState::new();

    // 1. Zero tagmask must be rejected
    let zero_cmd = IpcCommand::RuleAdd {
        app_id: Some("demo".into()),
        title: None,
        action: vec!["tags".into(), "0".into()],
    };
    assert!(state.handle_ipc_command(&zero_cmd).is_err());

    let zero_hex_cmd = IpcCommand::RuleAdd {
        app_id: Some("demo".into()),
        title: None,
        action: vec!["tags".into(), "0x0".into()],
    };
    assert!(state.handle_ipc_command(&zero_hex_cmd).is_err());
    assert!(state.rules.is_empty());

    // 2. Small integers are interpreted directly as bitmasks
    for (input, expected) in [
        ("1", 1),
        ("2", 2),
        ("3", 3),
        ("4", 4),
        ("16", 16),
        ("32", 32),
        ("511", 511),
        ("0x80000000", 2147483648),
    ] {
        let cmd = IpcCommand::RuleAdd {
            app_id: Some(format!("app_{input}")),
            title: None,
            action: vec!["tags".into(), input.into()],
        };
        assert!(state.handle_ipc_command(&cmd).is_ok());
        let rule = state
            .rules
            .iter()
            .find(|r| r.app_id.as_deref() == Some(&format!("app_{input}")))
            .unwrap();
        assert_eq!(rule.tags, Some(expected));
    }

    // 3. list-rules output displays the exact bitmask stored
    let list = state.list_rules(Some("tags")).unwrap();
    assert!(list.contains("-app-id app_3 tags 3"));
    assert!(list.contains("-app-id app_4 tags 4"));
    assert!(list.contains("-app-id app_16 tags 16"));
    assert!(list.contains("-app-id app_32 tags 32"));
}
