//! CoreState ↔ 구조 journal digest 시험. CoreState를 capture해 가져온 journal 모델이 CoreState와
//! 같은 구조를 나타내고, replay해도 digest가 같으며, 한쪽만 바꾸면 digest가 달라지는지 본다.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tasty_domain::{DomainEvent, IdKind};
use tasty_event_store::{
    CommitRequest, EventStore, ExpectedRevision, NewEvent, StoreError, StreamAppend, WriterEpoch,
};

use super::super::ScrollbackSource;
use super::{DecodedData, capture_and_import, capture_keeps_kind, core_canonical};
use crate::core::CoreState;
use crate::model::{
    Deferred, DeferredPlugin, DeferredSpawn, EmptySurface, Pane, PaneNode, SplitDirection, Surface,
    SurfaceLayout, Tab, TerminalSurface, Workspace, WorkspaceAttachMapping, WorkspaceCategory,
};
use crate::runtime::journal::{self, engine_stream, full_replay, save_snapshot};
use crate::runtime::shadow_digest::{
    CanonData, Canonical, DIGEST_EXCLUDED, KNOWN_MISMATCHES, StoreBytes, differences,
    known_mismatch,
};

const JOURNAL: &str = "journal-shadow-digest-test";

struct NoScrollback;

impl ScrollbackSource for NoScrollback {
    fn read_bytes(&self, _persist_id: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

/// 같은 시험 바이너리의 다른 시험이 fork한 자식이 잠금 파일 설명을 잠시 공유할 수 있어
/// writer 잠금 획득을 짧게 다시 시도한다.
fn open(path: &Path) -> (EventStore, WriterEpoch) {
    let mut store = EventStore::open(path, JOURNAL).expect("open journal");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match store.acquire_writer() {
            Ok(epoch) => return (store, epoch),
            Err(StoreError::WriterLocked) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("acquire writer: {error}"),
        }
    }
}

/// 등록되지 않은 kind의 surface.
struct Unregistered(u32);

impl Surface for Unregistered {
    tasty_model::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "shadow-unregistered"
    }
    fn type_name(&self) -> &'static str {
        "ShadowUnregistered"
    }
    fn surface_id(&self) -> Option<u32> {
        Some(self.0)
    }
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }
}

fn terminal(id: u32) -> Box<dyn Surface> {
    Box::new(TerminalSurface { id })
}

fn spawn(cwd: Option<&str>, command: Option<&str>, scrollback: Option<&str>) -> DeferredSpawn {
    DeferredSpawn {
        shell: None,
        shell_args: Vec::new(),
        extra_env: Vec::new(),
        cols: 80,
        rows: 24,
        working_dir: cwd.map(Into::into),
        restore_command: command.map(str::to_owned),
        scrollback_persist_id: scrollback.map(str::to_owned),
    }
}

fn deferred_terminal(id: u32, cwd: &str, command: &str) -> Box<dyn Surface> {
    Box::new(EmptySurface::new_deferred(
        id,
        spawn(Some(cwd), Some(command), None),
    ))
}

fn plugin(id: u32, kind: &str) -> Box<dyn Surface> {
    Box::new(EmptySurface::new_deferred_plugin(
        id,
        DeferredPlugin {
            kind: kind.to_owned(),
            snapshot: serde_json::json!({ "path": "/x.md", "n": [1, 2] }),
        },
    ))
}

fn split(
    direction: SplitDirection,
    ratio: f32,
    first: SurfaceLayout,
    second: SurfaceLayout,
) -> SurfaceLayout {
    SurfaceLayout::Split {
        direction,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
        focus_second: false,
    }
}

fn leaf(surface: Box<dyn Surface>) -> SurfaceLayout {
    SurfaceLayout::Leaf(surface)
}

fn tab(id: u32, name: &str, explicit: Option<&str>, layout: SurfaceLayout) -> Tab {
    let mut tab = Tab::new_named(
        id,
        name.to_owned(),
        explicit.map(str::to_owned),
        Box::new(EmptySurface::new(0)),
    );
    tab.focused_surface = crate::model::BinaryTree::first_id(&layout).unwrap_or(0);
    tab.layout_opt = Some(layout);
    tab
}

fn pane(id: u32, tabs: Vec<Tab>) -> PaneNode {
    PaneNode::Leaf(Pane {
        id,
        tabs,
        active_tab: 0,
    })
}

fn pane_split(
    direction: SplitDirection,
    ratio: f32,
    first: PaneNode,
    second: PaneNode,
) -> PaneNode {
    PaneNode::Split {
        direction,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn workspace(id: u32, name: &str, category: u32, layout: PaneNode) -> Workspace {
    let first = crate::model::BinaryTree::first_id(&layout).unwrap_or(0);
    let mut ws = Workspace::new_with_pane(id, name.to_owned(), Pane::default());
    *ws.pane_layout_mut() = layout;
    ws.focused_pane = first;
    ws.category = category;
    ws
}

/// scrollback 저장을 끈 빈 엔진. 이 capture는 디스크를 쓰지 않는다.
fn engine() -> CoreState {
    let waker: tasty_terminal::Waker = Arc::new(|| {});
    let mut engine = CoreState::new(80, 24, waker).expect("engine");
    engine.settings.general.restore_surface_content = false;
    engine
}

/// 여러 단계 분할, typed 필드, 여러 surface kind, 빈 카테고리와 mirror workspace를 담은 엔진.
fn full_engine() -> CoreState {
    let mut engine = engine();
    let mut side = WorkspaceCategory::new(6, "side".to_owned());
    side.collapsed = true;
    engine.categories = vec![
        WorkspaceCategory::normal(),
        WorkspaceCategory::new(5, "work".to_owned()),
        side,
        WorkspaceCategory::new(8, "empty".to_owned()),
    ];
    let mut main = workspace(
        10,
        "main",
        5,
        pane_split(
            SplitDirection::Vertical,
            0.3,
            pane(
                20,
                vec![
                    tab(30, "shell", None, leaf(terminal(40))),
                    tab(
                        31,
                        "build",
                        Some("Build!"),
                        split(
                            SplitDirection::Horizontal,
                            0.25,
                            leaf(deferred_terminal(41, "/tmp", "vim")),
                            split(
                                SplitDirection::Vertical,
                                0.1 + 0.2,
                                leaf(plugin(42, "markdown")),
                                leaf(terminal(43)),
                            ),
                        ),
                    ),
                ],
            ),
            pane_split(
                SplitDirection::Horizontal,
                0.1,
                pane(
                    21,
                    vec![tab(
                        32,
                        "notes",
                        Some(""),
                        leaf(Box::new(EmptySurface::new(44))),
                    )],
                ),
                pane(22, vec![tab(33, "logs", None, leaf(terminal(45)))]),
            ),
        ),
    );
    main.subtitle = "sub".to_owned();
    main.description = "desc".to_owned();
    main.attach_mapping = Some(WorkspaceAttachMapping::profile("box", Some(4)));
    let side_ws = workspace(
        11,
        "side",
        6,
        pane(23, vec![tab(34, "one", None, leaf(terminal(46)))]),
    );
    let mut mirror = workspace(
        12,
        "mirror",
        0,
        pane(24, vec![tab(35, "m", None, leaf(terminal(47)))]),
    );
    mirror.mirror = true;
    let normal_ws = workspace(
        13,
        "plain",
        0,
        pane(25, vec![tab(36, "Shell", None, leaf(terminal(48)))]),
    );
    engine.workspaces = vec![main, mirror, side_ws, normal_ws];
    engine
}

/// workspace 하나, pane 하나, tab 하나인 엔진.
fn minimal_engine() -> CoreState {
    let mut engine = engine();
    engine.categories = vec![WorkspaceCategory::normal()];
    engine.workspaces = vec![workspace(
        1,
        "ws",
        0,
        pane(1, vec![tab(1, "Shell", None, leaf(terminal(1)))]),
    )];
    engine
}

/// 같은 pane에 tab 여럿, 같은 tab에 같은 방향 분할을 겹친 엔진.
fn tabs_engine() -> CoreState {
    let mut engine = engine();
    engine.categories = vec![
        WorkspaceCategory::normal(),
        WorkspaceCategory::new(2, "b".to_owned()),
    ];
    let tabs = (0..4)
        .map(|i| {
            tab(
                100 + i,
                &format!("t{i}"),
                (i % 2 == 0).then_some("named"),
                split(
                    SplitDirection::Vertical,
                    0.5,
                    leaf(terminal(200 + i * 3)),
                    split(
                        SplitDirection::Vertical,
                        0.5,
                        leaf(terminal(201 + i * 3)),
                        leaf(plugin(202 + i * 3, "explorer")),
                    ),
                ),
            )
        })
        .collect();
    engine.workspaces = vec![
        workspace(3, "b-first", 2, pane(9, tabs)),
        workspace(
            4,
            "normal",
            0,
            pane(8, vec![tab(99, "x", None, leaf(terminal(250)))]),
        ),
    ];
    engine
}

fn scenarios() -> Vec<(&'static str, CoreState)> {
    vec![
        ("full", full_engine()),
        ("minimal", minimal_engine()),
        ("tabs", tabs_engine()),
    ]
}

/// CoreState를 가져온 journal과 그 모델의 정규 표현(자료 해석 포함).
fn imported(
    engine: &mut CoreState,
    dir: &tempfile::TempDir,
) -> (EventStore, WriterEpoch, Canonical) {
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let (_, model) =
        capture_and_import(engine, &mut store, epoch, 1, &NoScrollback).expect("import");
    let canonical = Canonical::of_journal(&model, &DecodedData(&store));
    (store, epoch, canonical)
}

fn core_side(engine: &CoreState) -> Canonical {
    core_canonical(engine).positional()
}

fn journal_side(canonical: &Canonical) -> Canonical {
    canonical.without_data().positional()
}

#[test]
fn core_state_and_its_imported_journal_describe_the_same_structure() {
    for (name, mut engine) in scenarios() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (_store, _, journal) = imported(&mut engine, &dir);
        assert!(journal.defects.is_empty(), "{name}: {:?}", journal.defects);
        let diffs = differences(&core_side(&engine), &journal_side(&journal));
        assert!(diffs.is_empty(), "{name}: {}", render(&diffs));
        assert_eq!(core_side(&engine).digest(), journal_side(&journal).digest());
        // importer가 새 ID를 받으므로 원래 ID로는 비교할 수 없다.
        if name != "minimal" {
            assert_ne!(
                core_canonical(&engine).digest(),
                journal.without_data().digest()
            );
        }
    }
}

#[test]
fn replaying_the_imported_journal_gives_the_import_digest() {
    for (name, mut engine) in scenarios() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut store, epoch, journal) = imported(&mut engine, &dir);
        let digest = journal.digest();
        let bytes = Canonical::of_journal(
            &journal::load(&store, &engine_stream(1)).expect("load"),
            &StoreBytes(&store),
        );

        let models = journal::load_all(&store).expect("load all");
        save_snapshot(&mut store, epoch, &models).expect("snapshot");
        drop(store);
        let (store, _) = open(&dir.path().join("journal.db"));
        assert!(
            store
                .snapshot_and_tail(tasty_domain::MODEL_VERSION)
                .expect("replay")
                .snapshot
                .is_some()
        );
        let fast = journal::load(&store, &engine_stream(1)).expect("snapshot + tail");
        let full = full_replay(&store)
            .expect("full")
            .stream(engine_stream(1).as_str());
        for (path, model) in [("snapshot + tail", &fast), ("full replay", &full)] {
            assert_eq!(
                Canonical::of_journal(model, &DecodedData(&store)).digest(),
                digest,
                "{name}: {path}"
            );
            assert_eq!(
                Canonical::of_journal(model, &StoreBytes(&store)),
                bytes,
                "{name}: {path}"
            );
        }
    }
}

#[test]
fn surface_data_is_compared_by_content() {
    let mut engine = full_engine();
    let dir = tempfile::tempdir().expect("tempdir");
    let (_store, _, journal) = imported(&mut engine, &dir);
    let tabs = &journal.workspaces[0].panes[0].tabs;
    // marker terminal은 저장할 값이 없어 자료가 없다.
    assert_eq!(tabs[0].surfaces[0].data, CanonData::Absent);
    let build = &tabs[1].surfaces;
    assert_eq!(
        build[0].data,
        CanonData::Decoded(serde_json::json!({ "terminal": {
            "cwd": "/tmp", "restore_command": "vim", "scrollback_ref": null, "scrollback": null,
        }}))
    );
    assert_eq!(
        build[1].data,
        CanonData::Decoded(serde_json::json!({ "generic": { "n": [1, 2], "path": "/x.md" } }))
    );
}

/// CoreState만 바꾸는 변경. 가져온 journal과의 digest가 달라져야 한다.
type CoreMutation = (&'static str, fn(&mut CoreState));

fn first_pane(engine: &mut CoreState) -> &mut Pane {
    match engine.workspaces[0].pane_layout_mut() {
        PaneNode::Split { first, .. } => match first.as_mut() {
            PaneNode::Leaf(pane) => pane,
            PaneNode::Split { .. } => panic!("fixture: first pane is a leaf"),
        },
        PaneNode::Leaf(pane) => pane,
    }
}

const CORE_MUTATIONS: &[CoreMutation] = &[
    ("category name", |e| e.categories[1].name.push('x')),
    ("category order", |e| e.categories.swap(1, 2)),
    ("workspace order", |e| e.workspaces.swap(0, 2)),
    ("workspace name", |e| e.workspaces[0].name.push('x')),
    ("workspace category", |e| e.workspaces[0].category = 8),
    ("workspace subtitle", |e| e.workspaces[0].subtitle.clear()),
    ("workspace description", |e| {
        e.workspaces[0].description.push('x')
    }),
    ("workspace attach mapping", |e| {
        e.workspaces[0].attach_mapping = None
    }),
    ("pane split ratio", |e| {
        if let PaneNode::Split { ratio, .. } = e.workspaces[0].pane_layout_mut() {
            *ratio += f32::EPSILON;
        }
    }),
    ("pane split direction", |e| {
        if let PaneNode::Split { direction, .. } = e.workspaces[0].pane_layout_mut() {
            *direction = SplitDirection::Horizontal;
        }
    }),
    ("tab order", |e| first_pane(e).tabs.swap(0, 1)),
    ("tab name", |e| first_pane(e).tabs[0].name.push('x')),
    ("tab explicit name", |e| {
        first_pane(e).tabs[1].explicit_name = None
    }),
    ("tab closed", |e| {
        first_pane(e).tabs.pop();
    }),
    ("surface split order", |e| {
        if let Some(SurfaceLayout::Split { first, second, .. }) =
            first_pane(e).tabs[1].layout_opt.as_mut()
        {
            std::mem::swap(first, second);
        }
    }),
    ("surface kind", |e| {
        first_pane(e).tabs[0].layout_opt = Some(leaf(plugin(40, "markdown")));
    }),
];

/// 비교에서 뺀 CoreState 자료만 바꾸는 변경. digest가 그대로여야 한다.
const EXCLUDED_MUTATIONS: &[CoreMutation] = &[
    ("focused pane", |e| e.workspaces[0].focused_pane = 22),
    ("active tab", |e| first_pane(e).active_tab = 1),
    ("focused surface", |e| {
        first_pane(e).tabs[1].focused_surface = 43
    }),
    ("focus second", |e| {
        if let Some(SurfaceLayout::Split { focus_second, .. }) =
            first_pane(e).tabs[1].layout_opt.as_mut()
        {
            *focus_second = true;
        }
    }),
    ("category collapsed", |e| e.categories[1].collapsed = true),
    ("osc title", |e| {
        first_pane(e).tabs[0].osc_title = Some("vim".to_owned())
    }),
    ("cached display name", |e| {
        first_pane(e).tabs[0].cached_display_name = Some("~".to_owned())
    }),
    ("mirror workspace", |e| {
        let mut ws = workspace(
            90,
            "m2",
            0,
            pane(91, vec![tab(92, "m", None, leaf(terminal(93)))]),
        );
        ws.mirror = true;
        e.workspaces.insert(0, ws);
    }),
    ("spawn attempts", |e| {
        let tab = &mut first_pane(e).tabs[1];
        if let Some(SurfaceLayout::Split { first, .. }) = tab.layout_opt.as_mut()
            && let SurfaceLayout::Leaf(surface) = first.as_mut()
            && let Some(empty) = surface.as_any_mut().downcast_mut::<EmptySurface>()
        {
            empty.spawn_attempts = 3;
        }
    }),
];

#[test]
fn changing_only_core_state_changes_the_digest() {
    let mut engine = full_engine();
    let dir = tempfile::tempdir().expect("tempdir");
    let (_store, _, journal) = imported(&mut engine, &dir);
    let digest = journal_side(&journal).digest();
    assert_eq!(core_side(&engine).digest(), digest);
    for (name, mutate) in CORE_MUTATIONS {
        let mut changed = full_engine();
        mutate(&mut changed);
        assert_ne!(core_side(&changed).digest(), digest, "{name}");
    }
    for (name, mutate) in EXCLUDED_MUTATIONS {
        let mut changed = full_engine();
        mutate(&mut changed);
        assert_eq!(core_side(&changed).digest(), digest, "{name}");
    }
}

#[test]
fn changing_only_the_journal_changes_the_replayed_digest() {
    let mut engine = full_engine();
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch, journal) = imported(&mut engine, &dir);
    let digest = journal_side(&journal).digest();
    let model = journal::load(&store, &engine_stream(1)).expect("load");
    let tab = model.panes[&model.workspaces[&model.workspace_order[0]].layout.leaves()[0]].tabs[0];
    let head = store.stream_revision(&engine_stream(1)).expect("head");
    let mut request = CommitRequest::new(epoch);
    request.appends.push(StreamAppend {
        stream_id: engine_stream(1),
        expected: head.map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact),
        events: vec![NewEvent {
            event_id: "shadow-rename".to_owned(),
            payload: journal::to_payload(&DomainEvent::TabRenamed {
                id: tab,
                name: "renamed".to_owned(),
            })
            .expect("encode"),
            recorded_at_ms: 0,
            causation_id: None,
            actor: "test".to_owned(),
            origin: "test".to_owned(),
            payload_refs: Vec::new(),
        }],
    });
    store.commit(&request).expect("commit");
    let replayed = Canonical::of_journal(
        &journal::load(&store, &engine_stream(1)).expect("load"),
        &DecodedData(&store),
    );
    assert_ne!(journal_side(&replayed).digest(), digest);
    assert_ne!(
        journal_side(&replayed).digest(),
        core_side(&engine).digest()
    );
    let diffs = differences(&core_side(&engine), &journal_side(&replayed));
    assert_eq!(diffs.len(), 1, "{}", render(&diffs));
    assert_eq!(diffs[0].path, "workspaces[0].panes[0].tabs[0].name");
}

#[test]
fn positional_ids_match_across_journals_with_different_numbering() {
    let dir_a = tempfile::tempdir().expect("tempdir");
    let dir_b = tempfile::tempdir().expect("tempdir");
    let (_a, _, first) = imported(&mut full_engine(), &dir_a);
    // 두 번째 journal은 ID를 먼저 예약해 가져온 ID가 달라지게 한다.
    let (mut store, epoch) = open(&dir_b.path().join("journal.db"));
    for kind in [
        IdKind::Workspace,
        IdKind::Pane,
        IdKind::Tab,
        IdKind::Surface,
    ] {
        store
            .reserve_ids(epoch, kind.label(), 5, u64::from(u32::MAX))
            .expect("reserve");
    }
    let (_, model) = capture_and_import(&mut full_engine(), &mut store, epoch, 1, &NoScrollback)
        .expect("import");
    let second = Canonical::of_journal(&model, &DecodedData(&store));
    assert_ne!(first.digest(), second.digest());
    assert_eq!(first.positional().digest(), second.positional().digest());
}

#[test]
fn an_unregistered_surface_kind_is_the_only_known_mismatch() {
    let mut engine = full_engine();
    first_pane(&mut engine).tabs[0].layout_opt = Some(leaf(Box::new(Unregistered(40))));
    let dir = tempfile::tempdir().expect("tempdir");
    let (_store, _, journal) = imported(&mut engine, &dir);
    let diffs = differences(&core_side(&engine), &journal_side(&journal));
    assert_eq!(diffs.len(), 1, "{}", render(&diffs));
    assert_eq!(
        diffs[0].path,
        "workspaces[0].panes[0].tabs[0].surfaces[0].kind"
    );
    let kept = |kind: &str| capture_keeps_kind(&engine, kind);
    assert!(!kept("shadow-unregistered"));
    assert_eq!(
        known_mismatch(&diffs[0], &kept).map(|k| k.id),
        Some("unregistered-surface-kind")
    );
    assert_eq!(KNOWN_MISMATCHES.len(), 1);
    // 알려진 불일치가 아닌 차이는 판정하지 않는다.
    let mut renamed = full_engine();
    first_pane(&mut renamed).tabs[0].name.push('x');
    let other = differences(&core_side(&renamed), &core_side(&full_engine()));
    assert!(other.iter().all(|d| known_mismatch(d, &kept).is_none()));
}

/// capture가 그대로 저장하는 kind가 empty가 되면 알려진 불일치로 가리지 않는다.
#[test]
fn a_kept_kind_saved_as_empty_stays_an_unclassified_difference() {
    let mut engine = full_engine();
    let dir = tempfile::tempdir().expect("tempdir");
    let (_store, _, journal) = imported(&mut engine, &dir);
    let mut regressed = journal_side(&journal);
    let surface = &mut regressed.workspaces[0].panes[0].tabs[0].surfaces[0];
    assert_eq!(surface.kind, "terminal");
    surface.kind = "empty".to_owned();
    let diffs = differences(&core_side(&engine), &regressed);
    assert_eq!(diffs.len(), 1, "{}", render(&diffs));
    assert_eq!(
        diffs[0].path,
        "workspaces[0].panes[0].tabs[0].surfaces[0].kind"
    );
    let kept = |kind: &str| capture_keeps_kind(&engine, kind);
    assert!(kept("terminal"));
    assert_eq!(known_mismatch(&diffs[0], &kept).map(|k| k.id), None);
}

/// 제외 목록과 알려진 불일치는 아키텍처 문서에도 같은 이름으로 적는다.
#[test]
fn the_architecture_doc_lists_every_exclusion_and_known_mismatch() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/architecture/index.md");
    let doc = std::fs::read_to_string(&path).expect("architecture doc");
    let section = doc
        .split("구조 digest(")
        .nth(1)
        .and_then(|rest| rest.split("\n\n###").next())
        .expect("structure digest section");
    for (field, _) in DIGEST_EXCLUDED {
        assert!(section.contains(&format!("`{field}`")), "{field}");
    }
    for known in KNOWN_MISMATCHES {
        assert!(section.contains(&format!("`{}`", known.id)), "{}", known.id);
    }
}

#[test]
fn exclusion_list_names_every_excluded_mutation() {
    let listed: Vec<&str> = DIGEST_EXCLUDED.iter().map(|(field, _)| *field).collect();
    assert!(DIGEST_EXCLUDED.iter().all(|(_, reason)| !reason.is_empty()));
    assert!(KNOWN_MISMATCHES.iter().all(|k| !k.reason.is_empty()));
    for field in [
        "Workspace.focused_pane",
        "Pane.active_tab",
        "Tab.focused_surface",
        "SurfaceLayout::Split.focus_second",
        "WorkspaceCategory.collapsed",
        "Tab.osc_title",
        "Tab.cached_display_name",
        "Workspace.mirror",
        "EmptySurface.spawn_attempts",
    ] {
        assert!(listed.contains(&field), "{field}");
    }
}

/// scrollback 저장을 끈 capture는 구조도 capture 결과도 바꾸지 않는다.
#[test]
fn capture_without_scrollback_leaves_core_state_unchanged() {
    let mut engine = full_engine();
    let before = core_canonical(&engine);
    let first = serde_json::to_string(
        &crate::core::layout_persistence::schema::SavedLayout::capture(&mut engine, 0),
    )
    .expect("json");
    let second = serde_json::to_string(
        &crate::core::layout_persistence::schema::SavedLayout::capture(&mut engine, 0),
    )
    .expect("json");
    assert_eq!(first, second);
    assert_eq!(core_canonical(&engine), before);
}

/// scrollback 저장을 켠 capture는 겹친 저장 ID를 새로 정해 대기 terminal의 spawn 자료를 바꾼다.
/// 구조 digest에는 들어가지 않는다.
#[test]
fn capture_with_scrollback_reassigns_a_duplicate_deferred_scrollback_id() {
    // capture가 겹친 ID의 scrollback 파일을 읽으려 하므로 실제 홈 대신 임시 홈을 쓴다.
    let real_home = tasty_utils::path::tasty_home();
    let _home = tasty_test_support::IsolatedHome::new();
    assert_ne!(tasty_utils::path::tasty_home(), real_home);
    let mut engine = minimal_engine();
    engine.settings.general.restore_surface_content = true;
    let duplicate = format!("shadow-digest-missing-{}", std::process::id());
    let surface = |id| -> Box<dyn Surface> {
        Box::new(EmptySurface::new_deferred(
            id,
            spawn(None, None, Some(&duplicate)),
        ))
    };
    first_pane_of_minimal(&mut engine).tabs[0].layout_opt = Some(split(
        SplitDirection::Vertical,
        0.5,
        leaf(surface(1)),
        leaf(surface(2)),
    ));
    let before = core_canonical(&engine);
    crate::core::layout_persistence::schema::SavedLayout::capture(&mut engine, 0);
    assert_eq!(core_canonical(&engine), before);
    let ids = scrollback_ids(&mut engine);
    assert_eq!(ids[0].as_deref(), Some(duplicate.as_str()));
    assert!(ids[1].is_some());
    assert_ne!(ids[1], ids[0], "the second capture keeps the reassigned id");
}

fn first_pane_of_minimal(engine: &mut CoreState) -> &mut Pane {
    match engine.workspaces[0].pane_layout_mut() {
        PaneNode::Leaf(pane) => pane,
        PaneNode::Split { .. } => panic!("fixture: one pane"),
    }
}

fn scrollback_ids(engine: &mut CoreState) -> Vec<Option<String>> {
    let mut out = Vec::new();
    if let Some(SurfaceLayout::Split { first, second, .. }) =
        first_pane_of_minimal(engine).tabs[0].layout_opt.as_mut()
    {
        for node in [first, second] {
            if let SurfaceLayout::Leaf(surface) = node.as_mut()
                && let Some(empty) = surface.as_any_mut().downcast_mut::<EmptySurface>()
                && let Some(Deferred::Terminal(spawn)) = &empty.deferred
            {
                out.push(spawn.scrollback_persist_id.clone());
            }
        }
    }
    out
}

fn render(diffs: &[crate::runtime::shadow_digest::Difference]) -> String {
    diffs
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
