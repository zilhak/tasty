use std::sync::atomic::AtomicU64;

use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryStore, Scope};
use tempfile::TempDir;

use super::*;
use crate::task::attempt::Completion;
use crate::task::contract::TaskContract;
use crate::task::{OnFailure, TaskCreateOpts, TaskResult, TaskStore};

fn limits(append: u64, block: u64) -> ReportLimits {
    ReportLimits {
        append_bytes: append,
        block_bytes: block,
    }
}

#[test]
fn the_limits_keep_append_below_block_and_inside_their_ranges() {
    assert!(ReportLimits::default().validate().is_ok());
    assert!(limits(64, 128).validate().is_ok());
    assert!(limits(1024, 1024).validate().is_err());
    assert!(limits(2048, 1024).validate().is_err());
    assert!(limits(63, 1024).validate().is_err());
    assert!(limits(1024, 128 * 1024 + 1).validate().is_err());
}

#[test]
fn an_append_over_the_limit_is_cut_on_a_char_boundary_and_marked() {
    let mut b = ReportBlock::new(1);
    // "가" 는 3바이트다. 64바이트 상한은 22번째 글자 중간에 걸린다.
    let text = "가".repeat(30);
    let out = b.append(ReportSource::Run, &text, limits(64, 1024), 5);
    let e = &b.entries[0];
    assert_eq!(e.text.len(), 63);
    assert_eq!(e.text, "가".repeat(21));
    assert_eq!(e.omit_by_limit, Some(90 - 63));
    assert_eq!(
        out,
        AppendOutcome::Stored {
            seq: 1,
            omit_by_limit: Some(27)
        }
    );
    let short = b.append(ReportSource::Agent, "ok", limits(64, 1024), 6);
    assert_eq!(
        short,
        AppendOutcome::Stored {
            seq: 2,
            omit_by_limit: None
        }
    );
    assert_eq!(b.stored_bytes, 65);
    assert_eq!(
        serde_json::to_value(&b.entries[1]).unwrap(),
        json!({"seq": 2, "at": 6, "source": "agent", "text": "ok"})
    );
}

#[test]
fn appends_past_the_block_limit_are_counted_but_not_stored() {
    let mut b = ReportBlock::new(1);
    let l = limits(100, 200);
    for _ in 0..2 {
        assert!(matches!(
            b.append(ReportSource::Run, &"x".repeat(100), l, 1),
            AppendOutcome::Stored { .. }
        ));
    }
    assert_eq!(
        b.append(ReportSource::Run, "y", l, 2),
        AppendOutcome::Omitted { seq: 3 }
    );
    assert_eq!(
        b.append(ReportSource::Run, "z", l, 3),
        AppendOutcome::Omitted { seq: 4 }
    );
    assert_eq!(b.entries.len(), 2);
    assert_eq!(b.omitted_appends, 2);
    assert_eq!(b.stored_bytes, 200);
}

#[test]
fn the_env_address_round_trips_and_keeps_the_task_id_whole() {
    let a = ReportAddress {
        workspace_id: 7,
        task_id: "build.lint-2".into(),
        attempt: 3,
        source: ReportSource::ReduceCustom,
        token: "ab12".into(),
    };
    let v = a.to_env_value();
    assert_eq!(v, "7/3/reduce_custom/ab12/build.lint-2");
    assert_eq!(ReportAddress::parse(&v).unwrap(), a);
    for bad in [
        "",
        "7/3/run/ab12",
        "x/3/run/t/a",
        "7/3/nope/t/a",
        "7//run/t/a",
    ] {
        assert!(ReportAddress::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn report_keys_of_different_tasks_never_collide() {
    assert_eq!(
        report_key(&"a".to_string(), 11).unwrap(),
        "tasty.agent.task_report.a.11"
    );
    assert_ne!(
        report_key(&"a.1".to_string(), 1).unwrap(),
        report_key(&"a".to_string(), 11).unwrap()
    );
}

// ── 저장소 ──────────────────────────────────────────────────────────────

fn fresh() -> (TempDir, MemoryStore, AtomicU64) {
    let td = tempfile::tempdir().expect("tempdir");
    let mem = MemoryStore::open(&td.path().join("mem.db")).expect("mem");
    (td, mem, AtomicU64::new(0))
}

fn typed_run(store: &mut TaskStore, name: &str) -> TaskId {
    let c: TaskContract = serde_json::from_value(json!({"contract_version": 2})).unwrap();
    let opts = TaskCreateOpts {
        workspace_id: 1,
        name: name.to_string(),
        command: TaskCommand::Run {
            command: vec!["true".into()],
            workspace_id: 1,
            cwd: None,
        },
        depends_on: vec![],
        on_failure: OnFailure::Abort,
        metadata: Value::Null,
        now_ms: 0,
    };
    store.create_typed(opts, c).unwrap().id
}

/// dispatch 처럼 토큰을 발급하고 Running 으로 넘긴다.
fn open_attempt(store: &mut TaskStore, id: &TaskId, n: u32, token: &str) -> ReportAddress {
    store
        .issue_report_token(
            1,
            id,
            ReportToken {
                attempt: n,
                token: token.into(),
            },
        )
        .unwrap();
    store.set_state(1, id, TaskState::Running, 10).unwrap();
    ReportAddress {
        workspace_id: 1,
        task_id: id.clone(),
        attempt: n,
        source: ReportSource::Run,
        token: token.into(),
    }
}

fn exit(code: i32) -> TaskResult {
    TaskResult {
        exit_code: Some(code),
        output: Some(json!({"stdout": {"text": ""}})),
        error: None,
    }
}

fn texts(b: &ReportBlock) -> Vec<&str> {
    b.entries.iter().map(|e| e.text.as_str()).collect()
}

#[test]
fn appends_are_refused_once_the_attempt_settles_and_for_another_tasks_token() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let a = typed_run(&mut store, "a");
    let b = typed_run(&mut store, "b");
    let addr_a = open_attempt(&mut store, &a, 1, "tok-a");
    let addr_b = open_attempt(&mut store, &b, 1, "tok-b");
    let l = ReportLimits::default();
    store.append_report(&addr_a, "first", l, 11).unwrap();

    // b 의 토큰으로 a 의 블록을 쓸 수 없다.
    let forged = ReportAddress {
        token: addr_b.token.clone(),
        ..addr_a.clone()
    };
    let e = store.append_report(&forged, "x", l, 12).unwrap_err();
    assert!(e.to_string().contains("does not open"), "{e}");
    // 회차 번호만 바꿔도 거절한다.
    let other_attempt = ReportAddress {
        attempt: 2,
        ..addr_a.clone()
    };
    assert!(store.append_report(&other_attempt, "x", l, 12).is_err());

    store
        .complete(1, &a, Completion::failed(None, "boom".into()), 20)
        .unwrap();
    let e = store.append_report(&addr_a, "late", l, 21).unwrap_err();
    assert!(e.to_string().contains("closed (failed)"), "{e}");
    let blocks = store
        .report_blocks(&store.get(1, &a).unwrap().unwrap())
        .unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(texts(&blocks[0]), ["first"]);

    // 결과를 회수하지 못한 회차(unknown)도 닫힌다.
    store.append_report(&addr_b, "b1", l, 13).unwrap();
    store
        .complete(1, &b, Completion::lost(None, "gone".into()), 22)
        .unwrap();
    assert!(store.append_report(&addr_b, "b2", l, 23).is_err());
}

#[test]
fn each_attempt_keeps_its_own_block_across_retry_and_a_reopened_store() {
    let (_td, mut mem, seq) = fresh();
    let id;
    {
        let mut store = TaskStore::new(&mut mem, "_host", &seq);
        id = typed_run(&mut store, "r");
        let l = ReportLimits::default();
        let first = open_attempt(&mut store, &id, 1, "t1");
        store.append_report(&first, "try 1", l, 11).unwrap();
        store
            .complete(1, &id, Completion::failed(None, "boom".into()), 12)
            .unwrap();
        store.retry(1, &id, false, 13).unwrap();
        // 같은 회차를 두 번 남기지 않는다(retry 뒤 실행 전에 다시 끝난 경우).
        store.set_state(1, &id, TaskState::Cancelled, 14).unwrap();
        store.retry(1, &id, false, 15).unwrap();
        let second = open_attempt(&mut store, &id, 2, "t2");
        // 이전 회차의 토큰은 더 쓰지 못한다.
        assert!(store.append_report(&first, "stale", l, 16).is_err());
        store.append_report(&second, "try 2", l, 16).unwrap();
    }
    // 재시작 뒤에도 열린 회차의 토큰과 기록이 남는다.
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let again = ReportAddress {
        workspace_id: 1,
        task_id: id.clone(),
        attempt: 2,
        source: ReportSource::StderrMarker,
        token: "t2".into(),
    };
    store
        .append_report(&again, "after restart", ReportLimits::default(), 17)
        .unwrap();
    store
        .complete(1, &id, Completion::succeeded(None, exit(0)), 18)
        .unwrap();
    let task = store.get(1, &id).unwrap().unwrap();
    let blocks = store.report_blocks(&task).unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(texts(&blocks[0]), ["try 1"]);
    assert!(matches!(blocks[0].settled, Some(TaskState::Failed { .. })));
    assert_eq!(texts(&blocks[1]), ["try 2", "after restart"]);
    assert_eq!(blocks[1].settled, None);

    let report = project_task(&task, &blocks, None, false);
    let attempts = report["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0]["state"]["kind"], json!("failed"));
    assert_eq!(attempts[1]["state"]["kind"], json!("succeeded"));
    assert_eq!(report["auto"]["exit_code"], json!(0));
    // 이전 회차만 고르면 auto 는 없다.
    let only_first = project_task(&task, &blocks, Some(1), false);
    assert_eq!(only_first["auto"], Value::Null);
    assert_eq!(only_first["attempts"].as_array().unwrap().len(), 1);
}

#[test]
fn deleting_a_task_removes_its_blocks() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let id = typed_run(&mut store, "d");
    let keep = typed_run(&mut store, "keep");
    let addr = open_attempt(&mut store, &id, 1, "t");
    let kept = open_attempt(&mut store, &keep, 1, "k");
    store
        .append_report(&addr, "x", ReportLimits::default(), 1)
        .unwrap();
    store
        .append_report(&kept, "y", ReportLimits::default(), 1)
        .unwrap();
    store.delete(1, &id).unwrap();
    drop(store);
    let keys: Vec<String> = mem
        .list(
            &Scope::Workspace(1),
            &ListOpts {
                prefix: Some(REPORT_KEY_PREFIX.to_string()),
                ..Default::default()
            },
        )
        .unwrap()
        .into_iter()
        .map(|e| e.key)
        .collect();
    assert_eq!(keys, vec![format!("{REPORT_KEY_PREFIX}{keep}.1")]);
}

#[test]
fn the_auto_part_projects_stored_values_and_hides_streams_unless_asked() {
    let (_td, mut mem, seq) = fresh();
    let mut store = TaskStore::new(&mut mem, "_host", &seq);
    let ok = typed_run(&mut store, "ok");
    open_attempt(&mut store, &ok, 1, "t");
    let result = TaskResult {
        exit_code: Some(0),
        output: Some(json!({"stdout": {"text": "out"}, "stderr": {"text": "err"}})),
        error: None,
    };
    store
        .complete(1, &ok, Completion::succeeded(None, result), 5)
        .unwrap();
    let task = store.get(1, &ok).unwrap().unwrap();
    let r = project_task(&task, &[], None, false);
    assert_eq!(r["auto"]["kind"], json!("run"));
    assert_eq!(r["auto"]["state"]["kind"], json!("succeeded"));
    assert_eq!(r["auto"]["exit_code"], json!(0));
    // int64 출력은 wire 형식(10진 문자열)이다.
    assert_eq!(r["auto"]["output"], json!("0"));
    assert!(r["auto"].get("raw").is_none());
    let with_raw = project_task(&task, &[], None, true);
    assert_eq!(with_raw["auto"]["raw"]["stdout"]["text"], json!("out"));
    assert_eq!(with_raw["auto"]["raw"]["stderr"]["text"], json!("err"));
    // 블록이 없어도 실행한 회차는 나온다.
    assert_eq!(r["attempts"][0]["attempt"], json!(1));
    assert_eq!(r["attempts"][0]["custom"]["entries"], json!([]));

    let bad = typed_run(&mut store, "bad");
    open_attempt(&mut store, &bad, 1, "u");
    store
        .complete(1, &bad, Completion::failed(None, "exit 3".into()), 6)
        .unwrap();
    let task = store.get(1, &bad).unwrap().unwrap();
    let r = project_task(&task, &[], None, false);
    assert_eq!(r["auto"]["failure"]["stage"], json!("execution"));
    assert_eq!(r["auto"]["failure"]["message"], json!("exit 3"));
    assert_eq!(r["auto"]["output"], Value::Null);
}
