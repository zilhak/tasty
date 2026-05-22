//! Schema migration / round-trip tests for `SavedSurface`.

use serde_json::json;

use super::schema::SavedSurface;

fn parse(s: &str) -> SavedSurface {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("parse failed for {s:?}: {e}"))
}

#[test]
fn v1_markdown_now_fails_to_parse() {
    let result: Result<SavedSurface, _> =
        serde_json::from_str(r#"{"Markdown":{"path":"/tmp/x.md"}}"#);
    assert!(result.is_err());
}

#[test]
fn v2_terminal_round_trips() {
    let s = SavedSurface::Terminal {
        cwd: Some("/tmp".into()),
        restore_command: None,
        scrollback_ref: None,
    };
    let json = serde_json::to_string(&s).unwrap();
    match parse(&json) {
        SavedSurface::Terminal {
            cwd,
            restore_command,
            scrollback_ref,
        } => {
            assert_eq!(cwd.as_deref(), Some("/tmp"));
            assert!(restore_command.is_none());
            assert!(scrollback_ref.is_none());
        }
        _ => panic!("expected Terminal"),
    }
}

#[test]
fn legacy_terminal_without_scrollback_ref_parses() {
    // 본 기능 추가 전 layout.json 의 Terminal entry — scrollback_ref 필드가 없어도
    // #[serde(default)] 로 None 처리.
    let json = r#"{"Terminal":{"cwd":"/home","restore_command":null}}"#;
    match parse(json) {
        SavedSurface::Terminal {
            cwd,
            restore_command,
            scrollback_ref,
        } => {
            assert_eq!(cwd.as_deref(), Some("/home"));
            assert!(restore_command.is_none());
            assert!(scrollback_ref.is_none());
        }
        _ => panic!("expected Terminal"),
    }
}

#[test]
fn v2_terminal_with_scrollback_ref_round_trips() {
    let s = SavedSurface::Terminal {
        cwd: None,
        restore_command: None,
        scrollback_ref: Some("deadbeef".into()),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("scrollback_ref"));
    match parse(&json) {
        SavedSurface::Terminal { scrollback_ref, .. } => {
            assert_eq!(scrollback_ref.as_deref(), Some("deadbeef"));
        }
        _ => panic!("expected Terminal"),
    }
}

#[test]
fn v2_generic_round_trips() {
    let s = SavedSurface::Generic {
        kind: "markdown".into(),
        data: json!({"path": "/x.md"}),
    };
    let json = serde_json::to_string(&s).unwrap();
    match parse(&json) {
        SavedSurface::Generic { kind, data } => {
            assert_eq!(kind, "markdown");
            assert_eq!(data["path"], "/x.md");
        }
        _ => panic!("expected Generic"),
    }
}

#[test]
fn unknown_variant_is_rejected() {
    let result: Result<SavedSurface, _> = serde_json::from_str(r#"{"Bogus":{}}"#);
    assert!(result.is_err());
}
