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

// ── 슬롯 파일 저장소 ────────────────────────────────────────────────────
//
// process-global `tasty_home()` 을 건드리지 않도록 전부 `*_in(dir, ..)` 내부
// helper 로 검증한다 (`store::scrollback` 테스트와 같은 관례).

mod slots {
    use std::path::Path;

    use super::super::schema::{
        SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout, SavedTab,
        SavedWorkspace,
    };
    use super::super::{
        LAYOUT_VERSION, delete_slot_in, gc_scrollback_orphans_all_slots_in, list_slots_in,
        load_slot_in, migrate_legacy_in, save_slot_in, slot_path_in,
    };

    /// workspace 이름 하나 + terminal surface 하나(주어진 `scrollback_ref`)짜리
    /// 최소 레이아웃.
    fn layout_with(ws_name: &str, scrollback_ref: Option<&str>) -> SavedLayout {
        SavedLayout {
            version: LAYOUT_VERSION,
            active_workspace: 0,
            categories: Vec::new(),
            workspaces: vec![SavedWorkspace {
                name: ws_name.into(),
                subtitle: String::new(),
                description: String::new(),
                pane_layout: SavedPaneNode::Leaf(SavedPane {
                    tabs: vec![SavedTab {
                        name: "Shell".into(),
                        explicit_name: None,
                        surface: SavedSurfaceLayout::Leaf(SavedSurface::Terminal {
                            cwd: None,
                            restore_command: None,
                            scrollback_ref: scrollback_ref.map(str::to_string),
                        }),
                    }],
                    active_tab: 0,
                }),
                focused_pane_index: 0,
                attach_mapping: None,
                category: 0,
            }],
        }
    }

    fn write_slot(dir: &Path, slot: u32, layout: &SavedLayout) {
        save_slot_in(dir, slot, &serde_json::to_string_pretty(layout).unwrap());
    }

    fn touch_bin(dir: &Path, id: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("{id}.bin")), b"x").unwrap();
    }

    fn bin_exists(dir: &Path, id: &str) -> bool {
        dir.join(format!("{id}.bin")).exists()
    }

    #[test]
    fn list_slots_sorts_numerically_and_skips_junk() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for name in ["01.json", "02.json", "10.json", "notes.txt", "01.json.tmp"] {
            std::fs::write(dir.join(name), "{}").unwrap();
        }
        // 사전순이면 10 이 2 보다 앞선다 — 숫자 정렬이어야 한다.
        assert_eq!(list_slots_in(dir), vec![1, 2, 10]);
    }

    #[test]
    fn list_slots_is_empty_when_the_dir_does_not_exist() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_slots_in(&tmp.path().join("layouts")).is_empty());
    }

    #[test]
    fn migrate_legacy_moves_layout_json_to_slot_one() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let legacy = home.join("layout.json");
        std::fs::write(
            &legacy,
            serde_json::to_string_pretty(&layout_with("legacy-ws", None)).unwrap(),
        )
        .unwrap();

        migrate_legacy_in(home);

        let layouts = home.join("layouts");
        assert!(!legacy.exists(), "레거시 파일은 복사가 아니라 이동이다");
        assert!(slot_path_in(&layouts, 1).exists());
        let restored = load_slot_in(&layouts, 1).expect("슬롯 1 이 읽혀야 한다");
        assert_eq!(restored.workspaces[0].name, "legacy-ws");

        // 멱등: 두 번째 호출은 no-op (레거시가 이미 없다).
        migrate_legacy_in(home);
        assert!(load_slot_in(&layouts, 1).is_some());
    }

    #[test]
    fn migrate_legacy_is_noop_when_layouts_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let legacy = home.join("layout.json");
        std::fs::write(
            &legacy,
            serde_json::to_string_pretty(&layout_with("legacy-ws", None)).unwrap(),
        )
        .unwrap();
        let layouts = home.join("layouts");
        std::fs::create_dir_all(&layouts).unwrap();

        migrate_legacy_in(home);

        assert!(legacy.exists(), "이미 슬롯을 쓰는 인스턴스면 손대지 않는다");
        assert!(!slot_path_in(&layouts, 1).exists());
    }

    #[test]
    fn save_slot_is_atomic() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("layouts");
        write_slot(&dir, 1, &layout_with("a", None));

        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["01.json".to_string()], "tmp 잔재가 없어야 한다");
        assert_eq!(load_slot_in(&dir, 1).unwrap().workspaces[0].name, "a");
    }

    #[test]
    fn delete_slot_removes_the_file_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("layouts");
        write_slot(&dir, 2, &layout_with("a", None));
        delete_slot_in(&dir, 2);
        assert!(load_slot_in(&dir, 2).is_none());
        delete_slot_in(&dir, 2); // 없는 파일은 no-op
    }

    /// 이 트랙의 핵심 회귀 — 슬롯별 GC 였다면 다른 슬롯의 `.bin` 이 지워진다.
    #[test]
    fn gc_union_keeps_refs_from_every_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let scrollback = tmp.path().join("scrollback");
        write_slot(&layouts, 1, &layout_with("w1", Some("aaa")));
        write_slot(&layouts, 2, &layout_with("w2", Some("bbb")));
        for id in ["aaa", "bbb", "ccc"] {
            touch_bin(&scrollback, id);
        }

        gc_scrollback_orphans_all_slots_in(&layouts, &scrollback);

        assert!(bin_exists(&scrollback, "aaa"));
        assert!(
            bin_exists(&scrollback, "bbb"),
            "단일 슬롯 GC 면 여기서 지워진다 — union 이어야 산다"
        );
        assert!(
            !bin_exists(&scrollback, "ccc"),
            "어느 슬롯도 참조하지 않는다"
        );
    }

    /// "모르면 지우지 않는다" — 손상 슬롯 하나가 다른 슬롯의 scrollback 을
    /// 데려가지 않게 GC 자체를 건너뛴다.
    #[test]
    fn gc_skips_entirely_when_any_slot_fails_to_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let scrollback = tmp.path().join("scrollback");
        write_slot(&layouts, 1, &layout_with("w1", Some("aaa")));
        std::fs::write(slot_path_in(&layouts, 2), "{ truncated").unwrap();
        for id in ["aaa", "zzz"] {
            touch_bin(&scrollback, id);
        }

        gc_scrollback_orphans_all_slots_in(&layouts, &scrollback);

        assert!(bin_exists(&scrollback, "aaa"));
        assert!(
            bin_exists(&scrollback, "zzz"),
            "손상 슬롯이 있으면 아무것도 지우지 않는다"
        );
    }

    /// 슬롯이 하나도 없으면 알려진 ref 가 없다 → 전부 orphan (슬롯 도입 전과 동일).
    #[test]
    fn gc_with_no_slots_treats_everything_as_orphan() {
        let tmp = tempfile::tempdir().unwrap();
        let scrollback = tmp.path().join("scrollback");
        touch_bin(&scrollback, "aaa");

        gc_scrollback_orphans_all_slots_in(&tmp.path().join("layouts"), &scrollback);

        assert!(!bin_exists(&scrollback, "aaa"));
    }

    /// version gate 는 슬롯 경로에서도 살아 있고, 그런 슬롯은 GC 에서 "모르는 것"
    /// 으로 취급된다.
    #[test]
    fn slot_newer_than_supported_version_is_unreadable_and_blocks_gc() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let scrollback = tmp.path().join("scrollback");
        let mut future = layout_with("w1", None);
        future.version = LAYOUT_VERSION + 1;
        write_slot(&layouts, 1, &future);
        touch_bin(&scrollback, "aaa");

        assert!(load_slot_in(&layouts, 1).is_none());
        gc_scrollback_orphans_all_slots_in(&layouts, &scrollback);
        assert!(bin_exists(&scrollback, "aaa"));
    }
}
