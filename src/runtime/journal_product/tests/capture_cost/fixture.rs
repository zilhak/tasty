use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

struct NoHistory;
impl crate::core::layout_persistence::import::ScrollbackSource for NoHistory {
    fn read_bytes(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}
pub(super) fn fixture(
    rows: usize,
    width: usize,
) -> (
    tempfile::TempDir,
    JournalWorker,
    crate::runtime::engine_session::EngineSession,
    EngineBinding,
    u32,
) {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("structure");
    std::fs::create_dir(&dir).unwrap();
    let id = "journal-00112233445566778899aabbccddeeff";
    std::fs::write(
        dir.join("journal.json"),
        serde_json::to_vec(&json!({"version":1,"journal_id":id,"phase":"Preparing"})).unwrap(),
    )
    .unwrap();
    let mut store = tasty_event_store::EventStore::open(&dir.join("journal.db"), id).unwrap();
    let epoch = store.acquire_writer().unwrap();
    let layout = json!({"version":2,"active_workspace":0,"workspaces":[{"name":"fixture","subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"fixture","explicit_name":null,"surface":{"Leaf":{"Terminal":{"cwd":"/synthetic-capture-cost","restore_command":null,"scrollback_ref":null}}}}],"active_tab":0}}}]});
    for slot in [1, 2] {
        crate::core::layout_persistence::import::import_slot(
            &mut store,
            epoch,
            slot,
            &layout.to_string(),
            &NoHistory,
        )
        .unwrap();
    }
    // Legacy import supplies canonical data but no View manifest. Seed that dormant owner
    // through the same checked checkpoint API used by the product.
    let models = crate::runtime::journal::load_all(&store).unwrap();
    let dormant = &models.streams["structure:slot-2"];
    let view = crate::runtime::journal_product::view_record::StoredView::imported(
        EngineBinding {
            journal_id: id.into(),
            stream: "structure:slot-2".into(),
            incarnation: dormant.engine_incarnation,
            runtime_epoch: epoch.0,
            published_cut: dormant.applied.batch,
            revision: dormant.applied.revision,
        },
        Default::default(),
    );
    store
        .save_restore_checkpoint(
            epoch,
            &tasty_event_store::NewSnapshot {
                batch_id: models.batch.unwrap(),
                model_version: tasty_core::MODEL_VERSION,
                bytes: tasty_core::encode_snapshot(&models).unwrap(),
                referenced_payloads: models
                    .streams
                    .values()
                    .flat_map(tasty_core::JournalModel::data_refs)
                    .map(|r| tasty_event_store::PayloadRef(r.0))
                    .collect(),
            },
            &tasty_event_store::NewRestoreManifest {
                restore_key: "view:slot-2".into(),
                incarnation: dormant.engine_incarnation,
                runtime_epoch: epoch.0,
                sequence: 0,
                snapshot_id: 0,
                view: serde_json::to_vec(&view).unwrap(),
                referenced_payloads: dormant
                    .data_refs()
                    .map(|r| tasty_event_store::PayloadRef(r.0))
                    .collect(),
            },
        )
        .unwrap();
    drop(store);
    let worker = start(home.path());
    submit(
        &worker,
        1,
        Work::OpenEngine {
            selection: EngineSelection::Slot {
                slot: 1,
                resume: true,
            },
            normal_category_name: "Normal".into(),
            surface_floor: SurfaceFloor::Fixed(0),
        },
    );
    let bound = loop {
        match receive(&worker) {
            Completion::Publish { batch, .. } => {
                worker.acknowledge(batch.batch_id, Ok(())).unwrap()
            }
            Completion::Finished { result, .. } => {
                let ResultValue::Bound(bound) = result.unwrap() else {
                    panic!("bound")
                };
                break bound;
            }
            other => panic!("{other:?}"),
        }
    };
    let (_, mut session) = crate::state::tests::test_state_from_model(bound.model);
    session.runtime.settings.general.restore_surface_content = true;
    let sid = session.core_state.local_workspaces()[0].all_surface_ids()[0];
    // Empty metadata triggers the production decode/re-encode path without storing any command.
    session.runtime.memory_access.submit_detached(
        tasty_memory::writer::MemoryRequest::Write(tasty_memory::WriteOp::Put {
            owner: tasty_memory::HOST_OWNER.into(),
            scope: tasty_memory::Scope::Surface(sid),
            key: "restore.command".into(),
            value: tasty_memory::MemoryValue::Text(String::new()),
            opts: tasty_memory::PutOpts::default(),
        }),
        "capture cost metadata",
        None,
    );
    let terminal = session.runtime.terminals.get_mut(sid).unwrap();
    terminal.set_scrollback_limit(512);
    terminal.enable_disk_scrollback(sid);
    for row in 0..8 {
        if row != 0 {
            terminal.process_bytes(b"\r\n");
        }
        terminal.process_bytes(b"visible fixture");
    }
    terminal.inject_scrollback(lines(0, rows, width));
    assert_eq!(terminal.scrollback_len(), rows);
    (home, worker, session, bound.binding, sid)
}
pub(super) fn lines(
    start: usize,
    count: usize,
    width: usize,
) -> Vec<tasty_terminal::ScrollbackLine> {
    let attrs = termwiz::cell::CellAttributes::default();
    (start..start + count)
        .map(|row| {
            let prefix = format!("{row:08}");
            let cells: Vec<String> = (0..width)
                .map(|i| {
                    if i < 8 {
                        prefix[i..i + 1].to_owned()
                    } else {
                        ["x", "한", "λ"][(row + i) % 3].to_owned()
                    }
                })
                .collect();
            tasty_terminal::ScrollbackLine::from_cells(
                cells.iter().map(|s| (s.as_str(), &attrs)),
                row.is_multiple_of(2),
            )
        })
        .collect()
}
pub(super) fn read_model(
    worker: &JournalWorker,
    stream: &str,
    ticket: u64,
) -> tasty_core::JournalModel {
    submit(worker, ticket, Work::ReadEngine(stream.into()));
    let ResultValue::Engine(model) = finished(worker, ticket).unwrap() else {
        panic!("model")
    };
    model
}
pub(super) fn lazy_control(
    session: &mut crate::runtime::engine_session::EngineSession,
    sid: u32,
    saved: &tasty_core::Surface,
) {
    let old = session
        .runtime
        .surfaces
        .insert(
            sid,
            Box::new(crate::runtime::surface_restorer::JournalPlaceholder {
                id: sid,
                kind: "terminal".into(),
                data: saved.data,
                creation_seed: saved.creation_seed,
                activation: saved.activation,
                attempts: 0,
                failure: None,
                recovery_blocked: false,
            }),
        )
        .unwrap();
    assert!(surface_capture::capture(session).unwrap().is_empty());
    session.runtime.surfaces.insert(sid, old);
}
pub(super) fn sql(home: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(
        home.join("structure/journal.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}
pub(super) fn database(home: &Path) -> serde_json::Value {
    let c = sql(home);
    let (max, bytes, count):(u64,u64,u64) = c.query_row("SELECT COALESCE(MAX(payload_id),0),COALESCE(SUM(length(bytes)),0),COUNT(*) FROM payloads",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    let cut:u64 = c.query_row("SELECT COALESCE(MAX(batch_id),0) FROM consumer_checkpoints WHERE consumer_id='structure-maintenance'",[],|r|r.get(0)).unwrap();
    let (payload,view):(u64,Vec<u8>) = c.query_row("SELECT p.payload_id,p.bytes FROM restore_manifests m JOIN payloads p ON p.payload_id=m.payload_id WHERE m.restore_key='view:slot-2'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    let view_hash = format!("{:x}", Sha256::digest(&view));
    let content: u64 = c
        .query_row(
            "SELECT MIN(payload_id) FROM restore_manifest_refs WHERE restore_key='view:slot-2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    json!({"max_payload":max,"retained_payload_bytes":bytes,"payload_count":count,"maintenance_cut":cut,
        "dormant_payload":content,"dormant_view_payload":payload,"dormant_view_hash":view_hash})
}
pub(super) fn read_body(home: &Path, id: u64) -> Vec<u8> {
    let bytes: Vec<u8> = sql(home)
        .query_row(
            "SELECT bytes FROM payloads WHERE payload_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let SurfaceData::Terminal {
        scrollback: Some(body),
        ..
    } = SurfaceData::decode(&bytes).unwrap()
    else {
        panic!("fullbody")
    };
    body
}
pub(super) fn read_payload_len(home: &Path, id: u64) -> usize {
    sql(home)
        .query_row(
            "SELECT length(bytes) FROM payloads WHERE payload_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}
pub(super) fn elapsed(start: Instant) -> u64 {
    start.elapsed().as_micros() as u64
}
pub(super) fn current_tid() -> u32 {
    std::fs::read_link("/proc/thread-self")
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap()
}
pub(super) fn journal_tid() -> u32 {
    let ids: Vec<u32> = std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            std::fs::read_to_string(e.path().join("comm"))
                .is_ok_and(|s| s.trim() == "structure-journ")
        })
        .map(|e| e.file_name().to_str().unwrap().parse().unwrap())
        .collect();
    assert_eq!(ids.len(), 1, "one actual production worker");
    ids[0]
}
pub(super) fn thread_io(tid: u32) -> BTreeMap<String, u64> {
    std::fs::read_to_string(format!("/proc/self/task/{tid}/io"))
        .unwrap()
        .lines()
        .map(|line| {
            let (key, value) = line.split_once(':').unwrap();
            (key.to_owned(), value.trim().parse().unwrap())
        })
        .collect()
}
pub(super) fn io_delta(
    before: &BTreeMap<String, u64>,
    after: &BTreeMap<String, u64>,
) -> BTreeMap<String, u64> {
    after
        .iter()
        .map(|(k, v)| (k.clone(), v - before[k]))
        .collect()
}

pub(super) fn swap_files() -> serde_json::Value {
    // Observe the production file already owned by this process, rather than enumerating
    // another process's swap namespace. No file is opened for content or changed here.
    let prefix = format!("surface-{}-", std::process::id());
    let mut paths = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir("/proc/self/fd").unwrap() {
        let Ok(path) = std::fs::read_link(entry.unwrap().path()) else {
            continue;
        };
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".scrollback"))
        {
            paths.insert(path);
        }
    }
    let bytes: u64 = paths
        .iter()
        .map(|path| path.metadata().unwrap().len())
        .sum();
    assert_eq!(paths.len(), 1, "one owned production disk swap");
    json!({"files": paths.len(), "bytes": bytes})
}
