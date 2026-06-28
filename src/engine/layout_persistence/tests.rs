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

// ── 단계 7: SavedWorkspace.attach_mapping 영속 round-trip + 구버전 호환 ──

#[test]
fn saved_workspace_attach_mapping_round_trips() {
    use super::schema::{SavedPane, SavedPaneNode, SavedSurfaceLayout, SavedTab, SavedWorkspace};
    use crate::model::WorkspaceAttachMapping;

    let leaf = SavedSurfaceLayout::Leaf(SavedSurface::Terminal {
        cwd: None,
        restore_command: None,
        scrollback_ref: None,
    });
    let ws = SavedWorkspace {
        name: "remote-a".into(),
        subtitle: String::new(),
        description: String::new(),
        pane_layout: SavedPaneNode::Leaf(SavedPane {
            tabs: vec![SavedTab {
                name: "Shell".into(),
                explicit_name: None,
                surface: leaf,
            }],
            active_tab: 0,
        }),
        focused_pane_index: 0,
        attach_mapping: Some(WorkspaceAttachMapping::profile("gx10", Some(1))),
        category: 0,
    };
    let json = serde_json::to_string(&ws).unwrap();
    let back: SavedWorkspace = serde_json::from_str(&json).unwrap();
    assert_eq!(back.attach_mapping, ws.attach_mapping);
}

#[test]
fn saved_workspace_without_mapping_field_is_none() {
    use super::schema::SavedWorkspace;
    // 구버전 layout.json (attach_mapping 필드 없음) → serde(default) 로 None.
    let legacy = r#"{
        "name": "ws",
        "subtitle": "",
        "description": "",
        "pane_layout": { "Leaf": { "tabs": [
            { "name": "Shell", "explicit_name": null,
              "surface": { "Leaf": { "Terminal": {} } } }
        ], "active_tab": 0 } },
        "focused_pane_index": 0
    }"#;
    let ws: SavedWorkspace = serde_json::from_str(legacy).unwrap();
    assert!(ws.attach_mapping.is_none());
}

// ── S-WSCAT: SavedWorkspace.category / SavedLayout.categories 영속 ──

#[test]
fn saved_workspace_without_category_field_defaults_to_normal() {
    use super::schema::SavedWorkspace;
    use crate::model::NORMAL_CATEGORY_ID;
    // 구버전 layout.json (category 필드 없음) → serde(default) 로 normal(0).
    let legacy = r#"{
        "name": "ws",
        "subtitle": "",
        "description": "",
        "pane_layout": { "Leaf": { "tabs": [
            { "name": "Shell", "explicit_name": null,
              "surface": { "Leaf": { "Terminal": {} } } }
        ], "active_tab": 0 } },
        "focused_pane_index": 0
    }"#;
    let ws: SavedWorkspace = serde_json::from_str(legacy).unwrap();
    assert_eq!(ws.category, NORMAL_CATEGORY_ID);
}

#[test]
fn saved_layout_categories_round_trip() {
    use super::schema::{
        SavedCategory, SavedLayout, SavedPane, SavedPaneNode, SavedSurfaceLayout, SavedTab,
        SavedWorkspace,
    };
    let leaf = SavedSurfaceLayout::Leaf(SavedSurface::Terminal {
        cwd: None,
        restore_command: None,
        scrollback_ref: None,
    });
    let layout = SavedLayout {
        version: super::LAYOUT_VERSION,
        active_workspace: 0,
        categories: vec![
            SavedCategory {
                id: 0,
                name: "normal".into(),
                collapsed: false,
            },
            SavedCategory {
                id: 1,
                name: "work".into(),
                collapsed: true,
            },
        ],
        workspaces: vec![SavedWorkspace {
            name: "ws".into(),
            subtitle: String::new(),
            description: String::new(),
            pane_layout: SavedPaneNode::Leaf(SavedPane {
                tabs: vec![SavedTab {
                    name: "Shell".into(),
                    explicit_name: None,
                    surface: leaf,
                }],
                active_tab: 0,
            }),
            focused_pane_index: 0,
            attach_mapping: None,
            category: 1,
        }],
    };
    let json = serde_json::to_string(&layout).unwrap();
    let back: SavedLayout = serde_json::from_str(&json).unwrap();
    assert_eq!(back.categories.len(), 2);
    assert_eq!(back.categories[1].name, "work");
    assert!(back.categories[1].collapsed);
    assert_eq!(back.workspaces[0].category, 1);
}

#[test]
fn saved_layout_without_categories_field_is_empty() {
    use super::schema::SavedLayout;
    // 구버전 layout.json (categories 필드 없음) → serde(default) 로 빈 Vec.
    // restore 가 ensure_normal_category 로 normal 단일 마이그레이션한다.
    let legacy = r#"{
        "version": 2,
        "active_workspace": 0,
        "workspaces": []
    }"#;
    let layout: SavedLayout = serde_json::from_str(legacy).unwrap();
    assert!(layout.categories.is_empty());
}
