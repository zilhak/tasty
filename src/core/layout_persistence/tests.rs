//! 저장 형식 호환성, 슬롯 저장·백업과 scrollback 참조를 검사한다.

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
    // scrollback_ref가 없는 이전 Terminal 형식도 읽어야 한다.
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
    // attach_mapping이 없는 이전 저장 형식.
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

#[test]
fn saved_workspace_without_category_field_defaults_to_normal() {
    use super::schema::SavedWorkspace;
    use crate::model::NORMAL_CATEGORY_ID;
    // category가 없는 이전 저장 형식.
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
    // categories가 없으면 역직렬화 결과는 빈 목록이다. 기본 분류 생성은 별도의 복원 처리다.
    let legacy = r#"{
        "version": 2,
        "active_workspace": 0,
        "workspaces": []
    }"#;
    let layout: SavedLayout = serde_json::from_str(legacy).unwrap();
    assert!(layout.categories.is_empty());
}

// 실제 홈 대신 디렉터리를 받는 저장 함수를 사용한다.

mod slots {
    use std::path::Path;

    use super::super::schema::{
        SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout, SavedTab,
        SavedWorkspace,
    };
    use super::super::{
        LAYOUT_VERSION, SlotLoad, delete_slot_in, gc_scrollback_orphans_all_slots_in,
        list_slots_in, load_slot_in, migrate_legacy_in, preserve_unparsable_slot, save_slot_in,
        slot_path_in,
    };

    fn expect_loaded(dir: &Path, slot: u32) -> SavedLayout {
        match load_slot_in(dir, slot) {
            SlotLoad::Loaded(layout) => layout,
            SlotLoad::Absent => panic!("슬롯 {slot} 이 없다"),
            SlotLoad::Unreadable => panic!("슬롯 {slot} 을 읽지 못했다"),
            SlotLoad::Unparsable => panic!("슬롯 {slot} 을 해석하지 못했다"),
        }
    }

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

    pub(super) fn write_valid_slot(dir: &Path, slot: u32, ws_name: &str) {
        write_slot(dir, slot, &layout_with(ws_name, None));
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
        assert_eq!(list_slots_in(dir), vec![1, 2, 10]);
    }

    #[test]
    fn list_slots_folds_padded_and_unpadded_names_into_one() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for name in ["1.json", "01.json"] {
            std::fs::write(dir.join(name), "{}").unwrap();
        }
        assert_eq!(list_slots_in(dir), vec![1]);
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
        let restored = expect_loaded(&layouts, 1);
        assert_eq!(restored.workspaces[0].name, "legacy-ws");

        migrate_legacy_in(home);
        assert!(matches!(load_slot_in(&layouts, 1), SlotLoad::Loaded(_)));
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

    /// Unix inode 또는 Windows file index로 같은 파일에 덮어쓴 경우를 구별한다.
    /// ID가 바뀌었다는 사실만으로 동시 읽기의 원자성이나 디스크 내구성을 증명하지는 않는다.
    fn file_identity(path: &Path) -> Option<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(std::fs::metadata(path).ok()?.ino())
        }
        #[cfg(windows)]
        {
            // 열린 파일 핸들에서 file index를 조회한다. 실패는 None이며 호출자가 검사 실패로 처리한다.
            use std::os::windows::io::AsRawHandle;

            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
            };

            let file = std::fs::File::open(path).ok()?;
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: 호출 동안 file 핸들이 살아 있고 info는 초기화된 출력 구조체다.
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
            Some((u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
        }
        #[cfg(not(any(unix, windows)))]
        {
            // 이 플랫폼에서는 파일 ID 비교를 지원하지 않는다.
            let _ = path;
            None
        }
    }

    #[test]
    fn save_slot_is_atomic() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("layouts");
        write_slot(&dir, 1, &layout_with("first", None));
        let path = slot_path_in(&dir, 1);
        let before = file_identity(&path);

        write_slot(&dir, 1, &layout_with("second", None));

        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["01.json".to_string()], "tmp 잔재가 없어야 한다");
        assert_eq!(expect_loaded(&dir, 1).workspaces[0].name, "second");

        // 같은 파일에 직접 쓰면 ID가 유지되므로 내용·tmp 잔재 검사만으로 놓친 변경을 검출한다.
        match (before, file_identity(&path)) {
            (Some(b), Some(a)) => assert_ne!(b, a, "슬롯 교체 전후의 파일 ID가 같다"),
            _ => {
                // 지원 플랫폼에서 ID 조회 실패를 검사 생략으로 처리하지 않는다.
                #[cfg(any(unix, windows))]
                panic!("file identity unavailable; cannot check replacement");
            }
        }
    }

    #[test]
    fn delete_slot_removes_the_file_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("layouts");
        write_slot(&dir, 2, &layout_with("a", None));
        delete_slot_in(&dir, 2);
        assert!(matches!(load_slot_in(&dir, 2), SlotLoad::Absent));
        delete_slot_in(&dir, 2); // 없는 파일은 no-op
    }

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
            "다른 슬롯이 참조하는 scrollback도 보존해야 한다"
        );
        assert!(
            !bin_exists(&scrollback, "ccc"),
            "어느 슬롯도 참조하지 않는다"
        );
    }

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

    #[test]
    fn gc_with_no_slots_treats_everything_as_orphan() {
        let tmp = tempfile::tempdir().unwrap();
        let scrollback = tmp.path().join("scrollback");
        touch_bin(&scrollback, "aaa");

        gc_scrollback_orphans_all_slots_in(&tmp.path().join("layouts"), &scrollback);

        assert!(!bin_exists(&scrollback, "aaa"));
    }

    #[test]
    fn slot_newer_than_supported_version_is_unreadable_and_blocks_gc() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let scrollback = tmp.path().join("scrollback");
        let mut future = layout_with("w1", None);
        future.version = LAYOUT_VERSION + 1;
        write_slot(&layouts, 1, &future);
        touch_bin(&scrollback, "aaa");

        assert!(matches!(load_slot_in(&layouts, 1), SlotLoad::Unreadable));
        gc_scrollback_orphans_all_slots_in(&layouts, &scrollback);
        assert!(bin_exists(&scrollback, "aaa"));
    }

    #[test]
    fn future_version_slot_file_is_left_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut future = layout_with("w1", None);
        future.version = LAYOUT_VERSION + 1;
        write_slot(&layouts, 1, &future);
        let before = std::fs::read_to_string(slot_path_in(&layouts, 1)).unwrap();

        assert!(matches!(load_slot_in(&layouts, 1), SlotLoad::Unreadable));

        let after = std::fs::read_to_string(slot_path_in(&layouts, 1)).unwrap();
        assert_eq!(before, after, "미래 version 파일은 그대로 있어야 한다");
        assert!(
            !slot_path_in(&layouts, 1)
                .with_extension("json.bak")
                .exists(),
            "백업을 만들지 않는다"
        );
    }

    #[test]
    fn unparsable_slot_is_left_in_place_by_load() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        std::fs::create_dir_all(&layouts).unwrap();
        let path = slot_path_in(&layouts, 1);
        std::fs::write(&path, "{ this is not valid json").unwrap();

        for _ in 0..2 {
            assert!(matches!(load_slot_in(&layouts, 1), SlotLoad::Unparsable));
        }

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ this is not valid json",
            "읽기는 원본을 옮기지 않는다"
        );
        assert!(!layouts.join("01.json.bak").exists());
    }

    #[test]
    fn preserving_an_unparsable_slot_moves_it_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        std::fs::create_dir_all(&layouts).unwrap();
        let path = slot_path_in(&layouts, 1);
        std::fs::write(&path, "first corrupt").unwrap();

        assert!(preserve_unparsable_slot(&layouts, 1));
        assert!(!path.exists(), "원래 경로에는 파일이 남지 않아야 한다");
        assert_eq!(
            std::fs::read_to_string(layouts.join("01.json.bak")).unwrap(),
            "first corrupt"
        );

        std::fs::write(&path, "second corrupt").unwrap();
        assert!(preserve_unparsable_slot(&layouts, 1));
        assert_eq!(
            std::fs::read_to_string(layouts.join("01.json.bak")).unwrap(),
            "first corrupt"
        );
        assert_eq!(
            std::fs::read_to_string(layouts.join("01.json.bak.2")).unwrap(),
            "second corrupt"
        );

        assert!(preserve_unparsable_slot(&layouts, 1));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_slot_is_left_in_place_and_locked() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        write_slot(&layouts, 1, &layout_with("mine", None));
        let path = slot_path_in(&layouts, 1);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let verdict = load_slot_in(&layouts, 1);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(verdict, SlotLoad::Unreadable));
        assert_eq!(
            expect_loaded(&layouts, 1).workspaces[0].name,
            "mine",
            "권한을 되돌리면 원본이 그대로 있어야 한다"
        );
        assert!(
            !layouts.join("01.json.bak").exists(),
            "백업을 만들지 않는다"
        );
    }
}

/// 부팅 판정에서 engine 플래그, 저장 거절·백업까지 연결되는지 검사한다.
#[cfg(test)]
mod wiring {
    use std::path::Path;

    use super::super::{SlotLoad, load_slot_in, save_slot_in_dir, slot_path_in};
    use crate::core::{Core, CoreState};

    fn engine_with_layouts(dir: &Path) -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).unwrap();
        std::fs::create_dir_all(dir).unwrap();
        engine.layouts_dir_override = Some(dir.to_path_buf());
        engine.layout_slot = Some(1);
        engine.settings.general.restore_layout = true;
        engine
    }

    #[test]
    fn saving_over_an_unparsable_slot_moves_it_aside_first() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        std::fs::write(slot_path_in(&layouts, 1), "{ NOT JSON {{").unwrap();
        // 실제 부팅 대신 판정 결과를 주입한다.
        engine.accept_slot_load(SlotLoad::Unparsable, 1);

        save_slot_in_dir(&mut engine, 0, 1, &layouts);

        assert_eq!(
            std::fs::read_to_string(layouts.join("01.json.bak")).unwrap(),
            "{ NOT JSON {{",
            "원본은 .bak 으로 옮겨져 있어야 한다"
        );
        assert!(
            matches!(load_slot_in(&layouts, 1), SlotLoad::Loaded(_)),
            "옮긴 뒤 자리에는 이번 세션의 레이아웃이 쓰여야 한다"
        );
        assert!(
            !engine.layout_slot_unparsable,
            "한 번 옮겼으면 플래그는 내려간다 — 다음 저장이 정상 파일을 또 옮기면 안 된다"
        );
    }

    #[test]
    fn a_slot_that_became_valid_is_not_moved_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        super::slots::write_valid_slot(&layouts, 1, "written-by-another-instance");
        engine.accept_slot_load(SlotLoad::Unparsable, 1);

        save_slot_in_dir(&mut engine, 0, 1, &layouts);

        assert!(
            !layouts.join("01.json.bak").exists(),
            "정상으로 바뀐 파일의 백업을 만들면 안 된다"
        );
        assert!(matches!(load_slot_in(&layouts, 1), SlotLoad::Loaded(_)));
    }

    #[test]
    fn boot_verdict_becomes_the_engine_flags() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);

        engine.accept_slot_load(SlotLoad::Absent, 1);
        assert!(!engine.layout_slot_protected && !engine.layout_slot_unparsable);

        engine.accept_slot_load(SlotLoad::Unreadable, 1);
        assert!(
            engine.layout_slot_protected,
            "읽지 못한 슬롯은 잠가야 저장이 사용자 레이아웃을 덮지 않는다"
        );

        let mut engine = engine_with_layouts(&layouts);
        engine.accept_slot_load(SlotLoad::Unparsable, 1);
        assert!(
            engine.layout_slot_unparsable,
            "해석 못한 슬롯은 저장 직전에 옮기도록 표시해야 한다"
        );
        assert!(!engine.layout_slot_protected);
        assert!(
            !engine.layout_slot_preserve_failed,
            "백업 공간이 있으면 보존 실패로 표시하면 안 된다"
        );
    }

    fn exhaust_backup_budget(dir: &Path, slot: u32) {
        let base = slot_path_in(dir, slot);
        let mut name = base.as_os_str().to_os_string();
        name.push(".bak");
        std::fs::write(std::path::PathBuf::from(name), "older backup").unwrap();
        for n in 2..=9 {
            let mut name = base.as_os_str().to_os_string();
            name.push(format!(".bak.{n}"));
            std::fs::write(std::path::PathBuf::from(name), "older backup").unwrap();
        }
    }

    #[test]
    fn a_full_backup_budget_makes_the_boot_verdict_say_preservation_is_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        std::fs::write(slot_path_in(&layouts, 1), "{ NOT JSON {{").unwrap();
        exhaust_backup_budget(&layouts, 1);

        engine.accept_slot_load(SlotLoad::Unparsable, 1);

        assert!(
            engine.layout_slot_preserve_failed,
            "부팅 때 백업 공간 부족을 표시해 저장 차단 안내를 선택해야 한다"
        );
        assert!(engine.layout_slot_unparsable);
    }

    #[test]
    fn a_save_that_cannot_preserve_records_it_and_keeps_the_original() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        let path = slot_path_in(&layouts, 1);
        std::fs::write(&path, "{ NOT JSON {{").unwrap();
        engine.accept_slot_load(SlotLoad::Unparsable, 1);
        // 부팅 뒤 백업 공간이 찬 경우는 저장 때도 확인해야 한다.
        exhaust_backup_budget(&layouts, 1);
        engine.layout_slot_preserve_failed = false;

        save_slot_in_dir(&mut engine, 0, 1, &layouts);

        assert!(
            engine.layout_slot_preserve_failed,
            "백업 실패를 표시해 저장이 차단된 이유를 안내해야 한다"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ NOT JSON {{",
            "옮기지 못했으면 원본을 덮어쓰지 않는다"
        );
    }

    #[test]
    fn a_slot_that_became_a_newer_version_is_neither_moved_nor_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        let path = slot_path_in(&layouts, 1);
        let from_the_future = format!(
            r#"{{"version":{},"workspaces":[],"active_workspace":0}}"#,
            super::super::LAYOUT_VERSION + 1
        );
        std::fs::write(&path, &from_the_future).unwrap();
        engine.accept_slot_load(SlotLoad::Unparsable, 1);

        save_slot_in_dir(&mut engine, 0, 1, &layouts);

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            from_the_future,
            "신버전 레이아웃을 덮어쓰면 안 된다"
        );
        assert!(
            !layouts.join("01.json.bak").exists(),
            "신버전 레이아웃은 백업으로도 치우지 않는다 — 다음 신버전 실행이 그 자리에서 읽는다"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_slot_that_cannot_be_re_read_is_neither_moved_nor_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        let path = slot_path_in(&layouts, 1);
        std::fs::write(&path, "{ NOT JSON {{").unwrap();
        engine.accept_slot_load(SlotLoad::Unparsable, 1);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        save_slot_in_dir(&mut engine, 0, 1, &layouts);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ NOT JSON {{",
            "다시 읽지 못한 파일을 덮어쓰면 안 된다"
        );
        assert!(
            !layouts.join("01.json.bak").exists(),
            "내용을 확인하지 못한 파일은 옮기지도 않는다"
        );
    }

    #[test]
    fn a_locked_slot_is_never_written() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        let path = slot_path_in(&layouts, 1);
        std::fs::write(&path, "user layout we could not read").unwrap();
        engine.accept_slot_load(SlotLoad::Unreadable, 1);
        engine.mark_layout_dirty();

        Core::apply_save_layout_now(&mut engine, 0, true);

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "user layout we could not read",
            "읽지 못한 사용자 레이아웃을 덮어쓰면 안 된다"
        );
        assert!(
            !layouts.join("01.json.bak").exists(),
            "내용을 모르는 파일은 옮기지도 않는다"
        );
        assert!(
            engine.layout_dirty.is_dirty(),
            "보호된 슬롯의 저장을 건너뛰면 dirty를 유지해야 한다"
        );
    }

    #[test]
    fn an_unlocked_slot_is_written_as_usual() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        engine.accept_slot_load(SlotLoad::Absent, 1);
        engine.mark_layout_dirty();

        Core::apply_save_layout_now(&mut engine, 0, true);

        assert!(
            matches!(load_slot_in(&layouts, 1), SlotLoad::Loaded(_)),
            "정상 슬롯은 저장돼야 한다"
        );
        assert!(
            !engine.layout_dirty.is_dirty(),
            "저장했으면 dirty 를 내린다"
        );
    }
}
