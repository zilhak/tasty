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
        LAYOUT_VERSION, SlotLoad, delete_slot_in, gc_scrollback_orphans_all_slots_in,
        list_slots_in, load_slot_in, migrate_legacy_in, preserve_unparsable_slot, save_slot_in,
        slot_path_in,
    };

    /// 슬롯이 정상 로드돼야 하는 자리. 실패하면 어떤 상태였는지 알려준다.
    fn expect_loaded(dir: &Path, slot: u32) -> SavedLayout {
        match load_slot_in(dir, slot) {
            SlotLoad::Loaded(layout) => layout,
            SlotLoad::Absent => panic!("슬롯 {slot} 이 없다"),
            SlotLoad::Unreadable => panic!("슬롯 {slot} 을 읽지 못했다"),
            SlotLoad::Unparsable => panic!("슬롯 {slot} 을 해석하지 못했다"),
        }
    }

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

    /// `wiring` 모듈이 쓰는 정상 슬롯 작성기 — 레이아웃 조립 헬퍼를 공유한다.
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
        // 사전순이면 10 이 2 보다 앞선다 — 숫자 정렬이어야 한다.
        assert_eq!(list_slots_in(dir), vec![1, 2, 10]);
    }

    /// zero-pad 유무가 다른 두 파일은 같은 슬롯 번호로 접힌다 — 목록에 같은 번호가
    /// 두 번 나오면 호출자(union GC 등)가 같은 슬롯을 두 번 읽는다.
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

        // 멱등: 두 번째 호출은 no-op (레거시가 이미 없다).
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

    /// 파일 **실체**의 id — unix inode / windows file index.
    ///
    /// 원자적 교체(tmp write → rename)는 디렉터리 엔트리가 새 실체를 가리키게 하므로
    /// 값이 바뀌고, `fs::write` 직접 호출은 같은 실체를 in-place 로 잘라 쓰므로 값이
    /// 그대로다. 원자성의 관측 가능한 흔적이 이 차이 하나다 — "덮어쓴 뒤 `.tmp` 가
    /// 없더라"만으로는 비원자적 write 와 구분되지 않는다.
    fn file_identity(path: &Path) -> Option<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(std::fs::metadata(path).ok()?.ino())
        }
        #[cfg(windows)]
        {
            // Windows 의 file index 는 `Metadata` 가 아니라 **열린 핸들**에서만 나온다.
            // std 의 `MetadataExt::file_index()` 는 unstable feature `windows_by_handle`
            // 이라 stable 툴체인에서 컴파일되지 않으므로 Win32
            // `GetFileInformationByHandle` 을 직접 호출한다(agent-stream plugin 의 tail
            // 과 같은 방식). 실패(핸들 열기·API)는 `None` — 호출자가 vacuous 판정으로
            // 처리한다.
            use std::os::windows::io::AsRawHandle;

            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
            };

            let file = std::fs::File::open(path).ok()?;
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: `file` 이 살아 있는 동안 그 raw handle 을 넘기고, 출력 버퍼는 위에서
            // 초기화한 유효한 `BY_HANDLE_FILE_INFORMATION` 하나다 — Win32 계약을 만족한다.
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
            Some((u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
        }
        #[cfg(not(any(unix, windows)))]
        {
            // 이 플랫폼은 파일 실체 id 를 노출하지 않는다 — 경로는 쓰지 않고 버린다
            // (호출자가 `None` 을 받아 단정을 어떻게 다룰지 정한다).
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

        // 같은 슬롯을 다시 저장 — 이때가 "기존 파일을 교체" 하는 경로다.
        write_slot(&dir, 1, &layout_with("second", None));

        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["01.json".to_string()], "tmp 잔재가 없어야 한다");
        assert_eq!(expect_loaded(&dir, 1).workspaces[0].name, "second");

        // 판별 단정: 교체여야 한다. `.tmp` + rename 을 `fs::write(path, json)` 로
        // "단순화" 하면 위 두 단정은 그대로 통과하고 여기서만 걸린다.
        match (before, file_identity(&path)) {
            (Some(b), Some(a)) => assert_ne!(
                b, a,
                "슬롯 덮어쓰기가 in-place 였다 — tmp write → rename 이어야 한다"
            ),
            _ => {
                // unix/windows 는 항상 실체 id 를 노출한다. 못 얻었다면 단정이
                // 조용히 사라진 것이므로 통과시키지 않는다.
                #[cfg(any(unix, windows))]
                panic!("file identity unavailable — atomicity assertion would be vacuous");
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

        // 미래 version 은 "없음" 이 아니라 "읽지 못함" 이다 — 저장을 막아 신버전이
        // 저장한 레이아웃을 구버전이 덮어쓰지 않게 한다.
        assert!(matches!(load_slot_in(&layouts, 1), SlotLoad::Unreadable));
        gc_scrollback_orphans_all_slots_in(&layouts, &scrollback);
        assert!(bin_exists(&scrollback, "aaa"));
    }

    /// 미래 version 슬롯은 **손대지 않는다** — 백업으로 옮기면 그 파일을 읽을 수 있는
    /// 새 빌드가 다음에 켜졌을 때 레이아웃이 사라진 것으로 보인다.
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

    /// 손상 JSON 은 **읽기 시점에 건드리지 않는다** — 부팅 중 이 슬롯을 읽는 곳이 GC 와
    /// engine 둘이라, 읽는 쪽이 옮기면 나중에 읽는 쪽은 사건 자체를 못 본다.
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

    /// 보존은 실제로 덮어쓰는 순간에 일어난다. 옮긴 뒤에는 원본이 `.bak` 에 남고 그
    /// 자리는 비므로, 이어지는 write 가 사용자 레이아웃을 지우지 않는다.
    #[test]
    fn preserving_an_unparsable_slot_moves_it_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        std::fs::create_dir_all(&layouts).unwrap();
        let path = slot_path_in(&layouts, 1);
        std::fs::write(&path, "first corrupt").unwrap();

        assert!(preserve_unparsable_slot(&layouts, 1));
        assert!(!path.exists(), "원본은 자리를 떠야 한다");
        assert_eq!(
            std::fs::read_to_string(layouts.join("01.json.bak")).unwrap(),
            "first corrupt"
        );

        // 먼저 만들어진 백업이 더 원본에 가깝다 — 덮어쓰지 않는다.
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

        // 옮길 것이 없으면 실패가 아니다 — 덮어써도 잃을 것이 없다.
        assert!(preserve_unparsable_slot(&layouts, 1));
    }

    /// 읽기 자체가 실패하면(권한) 파일을 **건드리지 않고** `Unreadable` 을 돌려준다.
    /// 일시적 오류에 사용자 레이아웃이 자리를 뜨면 안 된다.
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

/// 보호 장치의 **배선**을 검사한다 — 판정 함수와 백업 헬퍼가 각각 옳은 것만으로는
/// 사용자 파일이 지켜지지 않는다. 판정이 engine 플래그가 되고, 그 플래그가 저장을
/// 막거나 백업을 부르는 고리까지 이어져야 한다. 이 모듈이 그 고리를 지난다.
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

    /// 손상 슬롯 위에 저장하면 **먼저 옮기고** 쓴다. 보존 단계를 빼면 여기서 걸린다.
    #[test]
    fn saving_over_an_unparsable_slot_moves_it_aside_first() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        std::fs::write(slot_path_in(&layouts, 1), "{ NOT JSON {{").unwrap();
        // 부팅이 손상으로 판정했다고 치자.
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

    /// 플래그가 선 뒤 파일이 **정상으로 바뀌어 있으면** 옮기지 않는다. 같은
    /// `TASTY_HOME` 을 쓰는 다른 인스턴스가 그 사이에 써 넣는 경우다.
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
            "멀쩡한 파일을 백업으로 흘리면 9개뿐인 예산을 정상 파일이 깎는다"
        );
        assert!(matches!(load_slot_in(&layouts, 1), SlotLoad::Loaded(_)));
    }

    /// 부팅 판정이 engine 플래그로 이어진다 — 이 고리가 끊기면 아래 두 테스트가
    /// 지키는 저장 측 보호가 애초에 발동하지 않는다.
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
            "백업 자리가 남아 있으면 부팅은 '옮길 수 있다' 로 판정한다 — 아래 예산 소진 테스트의 대조군"
        );
    }

    /// `.bak` … `.bak.9` 를 미리 채워 보존 예산을 소진시킨다.
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

    /// **부팅 알림이 거짓말을 하지 않는다.** 백업 자리가 이미 다 찼으면 첫 저장이
    /// 통째로 거부되는데, 그 사실은 저장 시점(= `finish_boot` 이후)에야 확정된다.
    /// 부팅 알림은 그보다 먼저 뜨므로 판정도 부팅 때 서야 한다 — 서지 않으면 사용자는
    /// "옆에 `.bak` 으로 보관합니다" 라는 **사실과 반대인** 안내를 받고 원본을 지운다.
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
            "부팅 시점에 이미 옮길 자리가 없다면 그 사실이 서야 한다 — 이 플래그가 \
             `persistence.warn.layout_unparsable_blocked` 를 고른다"
        );
        assert!(engine.layout_slot_unparsable);
    }

    /// 저장이 보존에 실패하면 **그 사실을 남기고** 원본을 그대로 둔다. 플래그를
    /// 세우지 않으면 같은 세션에서 저장이 계속 거부되는데 사용자는 그 이유를 알 길이 없다.
    #[test]
    fn a_save_that_cannot_preserve_records_it_and_keeps_the_original() {
        let tmp = tempfile::tempdir().unwrap();
        let layouts = tmp.path().join("layouts");
        let mut engine = engine_with_layouts(&layouts);
        let path = slot_path_in(&layouts, 1);
        std::fs::write(&path, "{ NOT JSON {{").unwrap();
        engine.accept_slot_load(SlotLoad::Unparsable, 1);
        // 부팅 뒤에 예산이 찬 경우 — 부팅 판정만으로는 잡히지 않는 자리다.
        exhaust_backup_budget(&layouts, 1);
        engine.layout_slot_preserve_failed = false;

        save_slot_in_dir(&mut engine, 0, 1, &layouts);

        assert!(
            engine.layout_slot_preserve_failed,
            "옮기지 못했으면 그 사실이 서야 한다 — 안 세우면 알림이 반대로 나간다"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ NOT JSON {{",
            "옮기지 못했으면 원본을 덮어쓰지 않는다"
        );
    }

    /// 그 사이 **신버전이 써 놓은** 슬롯은 옮기지도 덮어쓰지도 않는다. 구버전으로
    /// 한 번 켰다고 신버전 레이아웃이 사라지면 안 된다.
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

    /// 저장 직전에 **다시 읽지 못한** 슬롯도 옮기지도 덮어쓰지도 않는다. 내용을
    /// 모르는 파일을 치우면 일시적 오류 한 번에 사용자 레이아웃이 자리를 뜬다.
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

    /// 잠긴 슬롯에는 **아무것도 쓰지 않는다.** 저장 경로 전체를 지나며 확인한다.
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
            "저장을 건너뛰었으면 dirty 는 남는다 — 지우면 나중에 저장할 기회까지 잃는다"
        );
    }

    /// 잠기지 않은 슬롯은 평소대로 저장된다 — 위 테스트가 "항상 안 쓴다" 로
    /// 통과하는 것을 막는 대조군이다.
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
