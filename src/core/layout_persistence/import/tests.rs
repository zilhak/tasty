//! importer 시험. 실제 journal에 가져온 뒤 replay한 모델에서 슬롯을 다시 만들어 원본과 비교한다.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use tasty_domain::{JournalModel, SplitTree};
use tasty_event_store::{EventStore, PayloadRef, StoreError, WriterEpoch};

use super::super::schema::{
    SavedCategory, SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout,
    SavedTab, SavedWorkspace,
};
use super::{
    ImportError, ImportMapping, ImportOutcome, ImportedView, ScrollbackSource, import_slot, keys,
};
use crate::model::{PaneId, SurfaceId, WorkspaceCategoryId};
use crate::runtime::journal;

const JOURNAL: &str = "journal-import-test";

/// 모든 필드와 여러 단계의 분할, 0이 아닌 선택 위치를 담은 현재 형식 슬롯.
const FULL_SLOT: &str = r#"{
  "version": 2,
  "active_workspace": 1,
  "categories": [
    { "id": 0, "name": "normal", "collapsed": false },
    { "id": 7, "name": "work", "collapsed": true },
    { "id": 3, "name": "side" }
  ],
  "workspaces": [
    {
      "name": "main",
      "subtitle": "sub",
      "description": "desc",
      "category": 7,
      "attach_mapping": { "target": { "Profile": { "name": "box" } }, "remote_workspace": 4 },
      "focused_pane_index": 2,
      "pane_layout": { "Split": {
        "direction": "Vertical", "ratio": 0.3,
        "first": { "Leaf": { "active_tab": 1, "tabs": [
          { "name": "shell", "explicit_name": null,
            "surface": { "Leaf": { "Terminal": { "cwd": "/home", "restore_command": "vim", "scrollback_ref": "sb1" } } } },
          { "name": "build", "explicit_name": "Build!",
            "surface": { "Split": { "direction": "Horizontal", "ratio": 0.25,
              "first": { "Leaf": { "Terminal": { "cwd": "/tmp" } } },
              "second": { "Split": { "direction": "Vertical", "ratio": 0.6,
                "first": { "Leaf": { "Generic": { "kind": "markdown", "data": { "path": "/x.md", "n": [1, 2] } } } },
                "second": { "Leaf": { "Terminal": { "scrollback_ref": "gone" } } } } } } } }
        ] } },
        "second": { "Split": {
          "direction": "Horizontal", "ratio": 0.1,
          "first": { "Leaf": { "active_tab": 0, "tabs": [
            { "name": "notes", "explicit_name": "",
              "surface": { "Leaf": { "Generic": { "kind": "explorer" } } } }
          ] } },
          "second": { "Leaf": { "active_tab": 0, "tabs": [
            { "name": "logs", "explicit_name": null, "surface": { "Leaf": { "Terminal": {} } } }
          ] } }
        } }
      } }
    },
    {
      "name": "side",
      "subtitle": "",
      "description": "",
      "category": 3,
      "focused_pane_index": 0,
      "pane_layout": { "Leaf": { "active_tab": 0, "tabs": [
        { "name": "one", "explicit_name": null, "surface": { "Leaf": { "Terminal": {} } } }
      ] } }
    }
  ]
}"#;

/// categories·category·attach_mapping이 없는 이전 형식 슬롯.
const LEGACY_SLOT: &str = r#"{
  "version": 1,
  "active_workspace": 0,
  "workspaces": [
    {
      "name": "ws", "subtitle": "", "description": "",
      "focused_pane_index": 0,
      "pane_layout": { "Leaf": { "active_tab": 0, "tabs": [
        { "name": "Shell", "explicit_name": null, "surface": { "Leaf": { "Terminal": {} } } }
      ] } }
    }
  ]
}"#;

#[derive(Default)]
struct Files(BTreeMap<String, Vec<u8>>);

impl ScrollbackSource for Files {
    fn read_bytes(&self, persist_id: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.0.get(persist_id).cloned())
    }
}

fn files() -> Files {
    Files(BTreeMap::from([(
        "sb1".to_owned(),
        b"scrollback bytes".to_vec(),
    )]))
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

fn parse(json: &str) -> SavedLayout {
    serde_json::from_str(json).expect("fixture")
}

fn import(store: &mut EventStore, epoch: WriterEpoch, json: &str) -> ImportOutcome {
    import_slot(store, epoch, 1, json, &files()).expect("import")
}

/// replay한 모델과 대응·화면 상태로 슬롯을 다시 만든다.
fn reconstruct(
    model: &JournalModel,
    mapping: &ImportMapping,
    view: &ImportedView,
    version: u32,
) -> SavedLayout {
    let saved_category =
        |id: WorkspaceCategoryId| mapping.saved_category(id).expect("mapped category");
    SavedLayout {
        version,
        active_workspace: position(&model.workspace_order, view.active_workspace),
        categories: model
            .category_order
            .iter()
            .map(|id| SavedCategory {
                id: saved_category(*id),
                name: model.categories[id].name.clone(),
                collapsed: view.collapsed_categories.contains(id),
            })
            .collect(),
        workspaces: model
            .workspace_order
            .iter()
            .map(|id| {
                let ws = &model.workspaces[id];
                let meta = |key: &str| ws.metadata.get(key).cloned();
                SavedWorkspace {
                    name: ws.name.clone(),
                    subtitle: meta(keys::SUBTITLE).unwrap_or_default(),
                    description: meta(keys::DESCRIPTION).unwrap_or_default(),
                    pane_layout: pane_node(model, view, &ws.metadata, &ws.layout),
                    focused_pane_index: position(
                        &ws.layout.leaves(),
                        view.focused_panes.get(id).copied(),
                    ),
                    attach_mapping: meta(keys::ATTACH_MAPPING)
                        .map(|json| serde_json::from_str(&json).expect("attach mapping")),
                    category: saved_category(ws.category),
                }
            })
            .collect(),
    }
}

fn position(list: &[u32], selected: Option<u32>) -> usize {
    selected
        .and_then(|id| list.iter().position(|x| *x == id))
        .unwrap_or(usize::MAX)
}

fn pane_node(
    model: &JournalModel,
    view: &ImportedView,
    ws_meta: &BTreeMap<String, String>,
    node: &SplitTree<PaneId>,
) -> SavedPaneNode {
    match node {
        SplitTree::Leaf(pane) => {
            let tabs = &model.panes[pane].tabs;
            SavedPaneNode::Leaf(SavedPane {
                active_tab: position(tabs, view.active_tabs.get(pane).copied()),
                tabs: tabs
                    .iter()
                    .map(|tab| SavedTab {
                        name: model.tabs[tab].name.clone(),
                        explicit_name: ws_meta.get(&keys::tab_explicit_name(*tab)).cloned(),
                        surface: surface_node(model, &model.tabs[tab].layout),
                    })
                    .collect(),
            })
        }
        SplitTree::Split {
            direction,
            ratio,
            first,
            second,
        } => SavedPaneNode::Split {
            direction: (*direction).into(),
            ratio: ratio.to_f32(),
            first: Box::new(pane_node(model, view, ws_meta, first)),
            second: Box::new(pane_node(model, view, ws_meta, second)),
        },
    }
}

fn surface_node(model: &JournalModel, node: &SplitTree<SurfaceId>) -> SavedSurfaceLayout {
    match node {
        SplitTree::Leaf(id) => {
            let surface = &model.surfaces[id];
            let meta = |key: &str| surface.metadata.get(key).cloned();
            SavedSurfaceLayout::Leaf(match meta(keys::GENERIC_DATA) {
                Some(json) => SavedSurface::Generic {
                    kind: surface.kind.clone(),
                    data: serde_json::from_str(&json).expect("generic data"),
                },
                None => SavedSurface::Terminal {
                    cwd: meta(keys::CWD),
                    restore_command: meta(keys::RESTORE_COMMAND),
                    scrollback_ref: meta(keys::SCROLLBACK_REF),
                },
            })
        }
        SplitTree::Split {
            direction,
            ratio,
            first,
            second,
        } => SavedSurfaceLayout::Split {
            direction: (*direction).into(),
            ratio: ratio.to_f32(),
            first: Box::new(surface_node(model, first)),
            second: Box::new(surface_node(model, second)),
        },
    }
}

fn value(layout: &SavedLayout) -> serde_json::Value {
    serde_json::to_value(layout).expect("serialize")
}

#[test]
fn full_slot_round_trips_through_the_journal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let outcome = import(&mut store, epoch, FULL_SLOT);
    assert!(outcome.batch.is_some());
    assert_eq!(outcome.missing_scrollback, vec!["gone".to_owned()]);
    assert!(outcome.moved_to_normal.is_empty());

    let model = journal::load(&store).expect("replay");
    assert_eq!(journal::full_replay(&store).expect("full replay"), model);
    let rebuilt = reconstruct(&model, &outcome.mapping, &outcome.view, 2);
    assert_eq!(value(&rebuilt), value(&parse(FULL_SLOT)));
}

#[test]
fn legacy_slot_round_trips_with_default_categories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let outcome = import(&mut store, epoch, LEGACY_SLOT);
    let model = journal::load(&store).expect("replay");

    let mut expected = parse(LEGACY_SLOT);
    // 복원과 같이 분류 목록이 없던 슬롯은 normal 하나를 가진 것으로 본다.
    expected.categories = vec![SavedCategory {
        id: 0,
        name: "normal".to_owned(),
        collapsed: false,
    }];
    let rebuilt = reconstruct(&model, &outcome.mapping, &outcome.view, 1);
    assert_eq!(value(&rebuilt), value(&expected));
}

#[test]
fn mapping_translates_every_position_both_ways() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let mapping = import(&mut store, epoch, FULL_SLOT).mapping;
    let model = journal::load(&store).expect("replay");

    assert_eq!(mapping.slot, 1);
    assert_eq!(mapping.category(0), Some(0));
    let work = mapping.category(7).expect("work");
    assert_eq!(mapping.saved_category(work), Some(7));
    assert!(model.categories.contains_key(&work));

    let mut surfaces = 0;
    for (wi, ws) in mapping.workspaces.iter().enumerate() {
        assert_eq!(mapping.workspace_index(ws.id), Some(wi));
        assert_eq!(model.workspace_order[wi], ws.id);
        assert_eq!(
            model.workspaces[&ws.id].layout.leaves(),
            ws.panes.iter().map(|p| p.id).collect::<Vec<_>>()
        );
        for (pi, pane) in ws.panes.iter().enumerate() {
            assert_eq!(mapping.pane_index(pane.id), Some((wi, pi)));
            for (ti, tab) in pane.tabs.iter().enumerate() {
                assert_eq!(mapping.tab_index(tab.id), Some((wi, pi, ti)));
                assert_eq!(model.panes[&pane.id].tabs[ti], tab.id);
                assert_eq!(model.tabs[&tab.id].layout.leaves(), tab.surfaces);
                for (si, surface) in tab.surfaces.iter().enumerate() {
                    assert_eq!(mapping.surface_index(*surface), Some((wi, pi, ti, si)));
                    surfaces += 1;
                }
            }
        }
    }
    assert_eq!(surfaces, 7);
    assert_eq!(model.surfaces.len(), 7);
    assert_eq!(mapping.workspace_index(9999), None);
}

#[test]
fn view_state_is_returned_but_not_recorded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let outcome = import(&mut store, epoch, FULL_SLOT);
    let mapping = &outcome.mapping;
    let main = &mapping.workspaces[0];
    assert_eq!(
        outcome.view.active_workspace,
        Some(mapping.workspaces[1].id)
    );
    assert_eq!(outcome.view.focused_panes[&main.id], main.panes[2].id);
    assert_eq!(
        outcome.view.active_tabs[&main.panes[0].id],
        main.panes[0].tabs[1].id
    );
    assert_eq!(
        outcome.view.collapsed_categories,
        vec![mapping.category(7).expect("work")]
    );
    let model = journal::load(&store).expect("replay");
    let recorded = serde_json::to_string(&model).expect("model");
    assert!(!recorded.contains("collapsed") && !recorded.contains("active"));
}

#[test]
fn scrollback_is_stored_as_a_pinned_payload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let mapping = import(&mut store, epoch, FULL_SLOT).mapping;
    let model = journal::load(&store).expect("replay");
    let first = mapping.workspaces[0].panes[0].tabs[0].surfaces[0];
    let data = model.surfaces[&first].data.expect("scrollback payload");
    let payload = PayloadRef(data.0);
    assert_eq!(
        store.read_payload(payload).expect("payload"),
        b"scrollback bytes"
    );
    let holders = store.payload_holders(payload).expect("holders");
    assert!(
        holders.iter().any(|h| h.starts_with("event:")),
        "{holders:?}"
    );

    // 파일이 없던 참조는 metadata에만 남는다.
    let gone = mapping.workspaces[0].panes[0].tabs[1].surfaces[2];
    assert_eq!(model.surfaces[&gone].data, None);
    assert_eq!(
        model.surfaces[&gone].metadata.get(keys::SCROLLBACK_REF),
        Some(&"gone".to_owned())
    );
}

#[test]
fn reimport_returns_the_stored_mapping_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let first = import(&mut store, epoch, FULL_SLOT);
    let cut = store.current_cut().expect("cut");

    let again = import(&mut store, epoch, FULL_SLOT);
    assert_eq!(again.batch, None);
    assert_eq!(again.mapping, first.mapping);
    assert_eq!(again.view, first.view);
    assert_eq!(store.current_cut().expect("cut"), cut);

    let changed = FULL_SLOT.replace("\"main\"", "\"renamed\"");
    assert!(matches!(
        import_slot(&mut store, epoch, 1, &changed, &files()),
        Err(ImportError::AlreadyImportedDifferently(1))
    ));
    assert_eq!(store.current_cut().expect("cut"), cut);
}

#[test]
fn second_slot_is_appended_after_the_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    import(&mut store, epoch, FULL_SLOT);
    let second = import_slot(&mut store, epoch, 2, LEGACY_SLOT, &files()).expect("import");
    let model = journal::load(&store).expect("replay");
    assert_eq!(model.workspace_order.len(), 3);
    assert_eq!(model.workspace_order[2], second.mapping.workspaces[0].id);
    // normal은 이미 있으므로 다시 만들지 않는다.
    assert_eq!(model.category_order.iter().filter(|c| **c == 0).count(), 1);
}

#[test]
fn unreadable_slots_are_rejected_without_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    assert!(matches!(
        import_slot(&mut store, epoch, 1, "{ not json", &files()),
        Err(ImportError::Unparsable)
    ));
    let newer = LEGACY_SLOT.replace("\"version\": 1", "\"version\": 99");
    assert!(matches!(
        import_slot(&mut store, epoch, 1, &newer, &files()),
        Err(ImportError::UnsupportedVersion)
    ));
    assert_eq!(store.current_cut().expect("cut").last_batch, None);
}

#[test]
fn unknown_category_moves_the_workspace_to_normal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&dir.path().join("journal.db"));
    let slot = LEGACY_SLOT.replace("\"name\": \"ws\",", "\"name\": \"ws\", \"category\": 42,");
    let outcome = import_slot(&mut store, epoch, 1, &slot, &files()).expect("import");
    assert_eq!(outcome.moved_to_normal, vec![0]);
    let model = journal::load(&store).expect("replay");
    assert_eq!(
        model.workspaces[&outcome.mapping.workspaces[0].id].category,
        0
    );
}
