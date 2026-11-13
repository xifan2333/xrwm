#![allow(clippy::unwrap_used)]

//! Unit tests for the window-rule action vocabulary (`RuleAction` / `RuleActionKind`).
//!
//! These tests are pure in-memory: the typed vocabulary can be exercised without a
//! compositor, a socket, or an `AppState`.

use xrwm::*;

fn tokens(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn test_rule_action_parse_decodes_every_documented_token() {
    let cases: &[(&[&str], RuleAction)] = &[
        (&["float"], RuleAction::Float(true)),
        (&["no-float"], RuleAction::Float(false)),
        (&["ssd"], RuleAction::Ssd(true)),
        (&["csd"], RuleAction::Ssd(false)),
        (&["no-ssd"], RuleAction::Ssd(false)),
        (&["no-border"], RuleAction::Ssd(false)),
        (&["fullscreen"], RuleAction::Fullscreen(true)),
        (&["no-fullscreen"], RuleAction::Fullscreen(false)),
        (&["tags", "2"], RuleAction::Tags(2)),
        (&["tags", "0x80000000"], RuleAction::Tags(1 << 31)),
        (
            &["dimensions", "960", "540"],
            RuleAction::Dimensions {
                width: 960,
                height: 540,
            },
        ),
        (
            &["position", "100", "-200"],
            RuleAction::Position { x: 100, y: -200 },
        ),
        (&["output", "DP-1"], RuleAction::Output("DP-1".into())),
    ];

    for (raw, expected) in cases {
        assert_eq!(
            &RuleAction::parse(&tokens(raw)).unwrap(),
            expected,
            "failed to decode {raw:?}"
        );
    }
}

#[test]
fn test_rule_action_parse_is_case_insensitive() {
    assert_eq!(
        RuleAction::parse(&tokens(&["FLOAT"])).unwrap(),
        RuleAction::Float(true)
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["Dimensions", "800", "600"])).unwrap(),
        RuleAction::Dimensions {
            width: 800,
            height: 600,
        }
    );
}

#[test]
fn test_rule_action_display_round_trips_through_parse() {
    let actions = [
        RuleAction::Float(true),
        RuleAction::Float(false),
        RuleAction::Ssd(true),
        RuleAction::Ssd(false),
        RuleAction::Tags(511),
        RuleAction::Dimensions {
            width: 1280,
            height: 720,
        },
        RuleAction::Position { x: -8, y: 16 },
        RuleAction::Fullscreen(true),
        RuleAction::Fullscreen(false),
        RuleAction::Output("eDP-1".into()),
    ];

    for action in actions {
        let rendered = action.to_string();
        let decoded = RuleAction::parse(&tokens(&rendered.split_whitespace().collect::<Vec<_>>()))
            .unwrap_or_else(|e| panic!("{rendered:?} failed to round-trip: {e}"));
        assert_eq!(decoded, action, "{rendered:?} did not round-trip");
    }
}

#[test]
fn test_rule_action_parse_rejects_invalid_actions() {
    // Unknown tokens never reach a state mutation.
    let err = RuleAction::parse(&tokens(&["toggle-float"])).unwrap_err();
    assert_eq!(err, "Unknown rule action: toggle-float");

    // Empty token list.
    assert_eq!(
        RuleAction::parse(&[]).unwrap_err(),
        "Rule action cannot be empty"
    );

    // A rule must select at least one tag, in decimal and hex spelling.
    assert_eq!(
        RuleAction::parse(&tokens(&["tags", "0"])).unwrap_err(),
        "a window rule must specify at least one tag"
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["tags", "0x0"])).unwrap_err(),
        "a window rule must specify at least one tag"
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["tags", "not-a-mask"])).unwrap_err(),
        "Invalid tag mask"
    );

    // Dimensions must stay addressable as signed geometry.
    assert_eq!(
        RuleAction::parse(&tokens(&["dimensions", &u32::MAX.to_string(), "600"])).unwrap_err(),
        "window dimensions exceed maximum allowed value"
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["dimensions", "960"])).unwrap_err(),
        "Usage: rule-add ... dimensions <width> <height>"
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["dimensions", "wide", "600"])).unwrap_err(),
        "Invalid width"
    );

    // Arity violations are reported with the documented usage line.
    assert_eq!(
        RuleAction::parse(&tokens(&["tags"])).unwrap_err(),
        "Usage: rule-add ... tags <mask>"
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["position", "10"])).unwrap_err(),
        "Usage: rule-add ... position <x> <y>"
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["position", "10", "20", "30"])).unwrap_err(),
        "Usage: rule-add ... position <x> <y>"
    );
    assert_eq!(
        RuleAction::parse(&tokens(&["output"])).unwrap_err(),
        "Usage: rule-add ... output <name|id>"
    );

    // Flag tokens take no arguments.
    assert_eq!(
        RuleAction::parse(&tokens(&["float", "yes"])).unwrap_err(),
        "Unexpected arguments for rule action: float"
    );
}

#[test]
fn test_rule_action_kind_resolves_synonyms() {
    let cases = [
        ("float", RuleActionKind::Float),
        ("no-float", RuleActionKind::Float),
        ("ssd", RuleActionKind::Ssd),
        ("csd", RuleActionKind::Ssd),
        ("no-ssd", RuleActionKind::Ssd),
        ("no-border", RuleActionKind::Ssd),
        ("fullscreen", RuleActionKind::Fullscreen),
        ("no-fullscreen", RuleActionKind::Fullscreen),
        ("tags", RuleActionKind::Tags),
        ("dimensions", RuleActionKind::Dimensions),
        ("position", RuleActionKind::Position),
        ("output", RuleActionKind::Output),
    ];

    for (token, expected) in cases {
        assert_eq!(RuleActionKind::parse(token).unwrap(), expected, "{token}");
        // The canonical keyword always maps back to the same kind.
        assert_eq!(RuleActionKind::parse(expected.keyword()).unwrap(), expected);
    }

    assert_eq!(
        RuleActionKind::parse("toggle-monocle").unwrap_err(),
        "Unknown rule action: toggle-monocle"
    );
}

#[test]
fn test_window_rule_projects_typed_actions() {
    let action = RuleAction::Dimensions {
        width: 800,
        height: 600,
    };
    let rule = WindowRule::new(Some("imv".into()), None, &action);

    assert_eq!(rule.app_id.as_deref(), Some("imv"));
    assert_eq!(rule.dimensions, Some((800, 600)));
    assert_eq!(rule.actions(), vec![action.clone()]);

    // The selector reports exactly which properties are configured.
    assert!(RuleActionKind::Dimensions.is_set_for(&rule));
    assert!(!RuleActionKind::Float.is_set_for(&rule));
}

#[test]
fn test_window_rule_projection_matches_list_rules_order() {
    let mut rule = WindowRule::new(None, None, &RuleAction::Float(true));
    rule.set(&RuleAction::Ssd(false));
    rule.set(&RuleAction::Tags(4));
    rule.set(&RuleAction::Dimensions {
        width: 400,
        height: 300,
    });
    rule.set(&RuleAction::Position { x: 8, y: 16 });
    rule.set(&RuleAction::Fullscreen(false));
    rule.set(&RuleAction::Output("HDMI-A-1".into()));

    let rendered: Vec<String> = rule.actions().iter().map(|a| a.to_string()).collect();

    // `fullscreen: Some(false)` is intentionally not advertised, matching the
    // historical `list-rules` output.
    assert_eq!(
        rendered,
        vec![
            "float",
            "csd",
            "tags 4",
            "dimensions 400 300",
            "position 8 16",
            "output HDMI-A-1",
        ]
    );
}

#[test]
fn test_rule_action_serde_round_trip_preserves_typed_payload() {
    let actions = [
        RuleAction::Float(false),
        RuleAction::Tags(1 << 31),
        RuleAction::Dimensions {
            width: 1920,
            height: 1080,
        },
        RuleAction::Position { x: -1, y: 1 },
        RuleAction::Output("DP-2".into()),
    ];

    for action in actions {
        let json = serde_json::to_string(&action).unwrap();
        let decoded: RuleAction = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, action, "serde round-trip failed for {json}");
    }
}
