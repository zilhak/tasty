//! transcript tail 루프 — plugin 전용 background thread 에서 돈다.
//!
//! SDK 는 async 를 지원하지 않고 모든 콜백이 동기다. 파일 I/O 를 메인 dispatch 콜백에서
//! 하면 호스트 healthcheck(15s ping / 60s 무응답 시 강제 재시작)가 깨지므로, 상주 tail 은
//! 반드시 별도 스레드여야 한다(`docs/dev-guide/plugin-development.md` §10).
//!
//! 루프는 두 가지 주기를 가진다:
//!
//! - **매 tick** — 파일을 읽어 새 라인을 이벤트로 바꾼다. 호스트 IPC 를 부르지 않는다.
//! - **verify tick**(느린 주기) — 호스트에 대상 생존과 세션 id 를 되묻는다. 사라진
//!   대상은 종료 이벤트를 남기고 걷어내고, 세션이 바뀌었으면 tail 대상을 교체한다.
//!   IPC 왕복이라 매 tick 돌리면 대상 수에 비례해 낭비가 크다.
//!
//! transcript 루트는 함수 인자로 주입한다 — 테스트가 프로세스 전역 환경변수를 건드리지
//! 않고 임시 디렉토리를 루트로 쓸 수 있게 하기 위해서다.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tasty_plugin_sdk::HostHandle;

use crate::record::{self, StreamEvent};
use crate::registry::StreamRegistry;
use crate::registry::TailCheckout;
use crate::resolve::{self, HostCall};
use crate::tail::TailPoll;

/// 파일 폴링 간격. "응답 직후 수 초 내" 관측을 만족하면서도 idle 시 부하가 무시할 수준.
pub const TICK: Duration = Duration::from_millis(300);

/// 몇 tick 마다 호스트에 생존/세션을 되물을지. 300ms × 10 = 3s.
const VERIFY_EVERY: u64 = 10;

type Shared = Arc<Mutex<StreamRegistry>>;

pub fn tail_loop(registry: Shared, host: HostHandle) {
    let mut tick_count: u64 = 0;
    loop {
        std::thread::sleep(TICK);
        tick_count += 1;
        // 루트는 매 tick 다시 계산한다 — 홈 디렉토리 확인 실패 등이 영구 상태가 되지
        // 않게(설정이 뒤늦게 갖춰지면 그 tick 부터 정상 동작한다).
        let root = resolve::transcript_root();
        if tick_count.is_multiple_of(VERIFY_EVERY) {
            verify_targets(&registry, &host, root.as_deref());
        }
        pump_all(&registry, root.as_deref());
        match registry.lock() {
            Ok(mut reg) => {
                // 활동 없이 오래 열린 correlation 턴을 닫는다(막힌 턴 안전망). pump 직후에
                // 돌려 방금 들어온 이벤트가 활동 시각을 이미 갱신한 뒤 판정하게 한다.
                reg.sweep_stale_turns(std::time::Instant::now());
                reg.save_if_dirty();
            }
            Err(e) => {
                tracing::error!("agent-stream registry mutex poisoned: {e}");
                return;
            }
        }
    }
}

/// 호스트에 대상 생존과 세션 id 를 되묻는다. lock 은 IPC **바깥**에서만 잡는다.
pub fn verify_targets<H: HostCall>(registry: &Shared, host: &H, root: Option<&Path>) {
    let targets = match registry.lock() {
        Ok(reg) => reg.targets(),
        Err(e) => {
            tracing::error!("agent-stream registry mutex poisoned: {e}");
            return;
        }
    };
    for (surface_id, session_id, _) in targets {
        verify_one(registry, host, root, surface_id, &session_id);
    }
}

fn verify_one<H: HostCall>(
    registry: &Shared,
    host: &H,
    root: Option<&Path>,
    surface_id: u32,
    session_id: &str,
) {
    if !resolve::surface_exists(host, surface_id) {
        // 대상 자체가 사라졌다 — 소비자가 영원히 기다리지 않도록 턴을 닫고 해제한다.
        // 임계구역은 registry 조작뿐이라 poison 이어도 값은 성립한다. 조용히 건너뛰면
        // 사라진 대상의 턴이 열린 채 남아 **소비자가 영원히 기다린다** — 복구해서 닫는다.
        let mut reg = match registry.lock() {
            Ok(reg) => reg,
            Err(poisoned) => {
                tracing::warn!(
                    "agent-stream: the registry lock is poisoned (the tail thread panicked) — \
                     recovering it to close the turn of a surface that is already gone, \
                     otherwise its consumers would wait forever"
                );
                poisoned.into_inner()
            }
        };
        reg.remove(surface_id, record::REASON_SESSION_ENDED);
        return;
    }
    // meta 조회 실패(일시적 IPC 오류 포함)는 대상 유지 — 다음 verify 에서 재시도한다.
    let Ok(current) = resolve::session_id_for_surface(host, surface_id) else {
        return;
    };
    if current == session_id {
        return;
    }
    // 새 세션 파일이 아직 없으면 경로 미해결(빈 경로)로 바꿔 둔다 — 매 tick 재해석한다.
    let path = root
        .and_then(|r| resolve::find_transcript(r, &current))
        .unwrap_or_default();
    if let Ok(mut reg) = registry.lock() {
        reg.switch_session(surface_id, current, path);
    }
}

/// 등록된 모든 대상의 파일을 한 번씩 읽어 이벤트를 만든다.
pub fn pump_all(registry: &Shared, root: Option<&Path>) {
    let surfaces: Vec<u32> = match registry.lock() {
        Ok(reg) => reg.targets().into_iter().map(|(id, _, _)| id).collect(),
        Err(e) => {
            tracing::error!("agent-stream registry mutex poisoned: {e}");
            return;
        }
    };
    for surface_id in surfaces {
        pump_one(registry, surface_id, root);
    }
}

/// 대상 하나를 한 번 읽는다.
///
/// **파일 I/O 구간에서는 레지스트리 락을 잡지 않는다.** 락을 쥔 채 readdir 이나 최대
/// 4 MiB read 를 하면 IPC 핸들러가 그 락을 기다리고, SDK 가 ping 을 worker(dispatch)
/// 스레드에서 응답하므로 healthcheck 응답까지 함께 밀린다 — 이 모듈이 "dispatch 에서
/// 파일 I/O 금지" 로 세운 불변식이 락을 통해 되살아난다. 그래서 tail 상태를 잠시 꺼내
/// (`check_out`) I/O 를 마친 뒤 되돌린다(`check_in`).
fn pump_one(registry: &Shared, surface_id: u32, root: Option<&Path>) {
    resolve_pending_path(registry, surface_id, root);
    let Some(mut checkout) = check_out(registry, surface_id) else {
        return;
    };

    // ── 락 없음: 파일 읽기 ──────────────────────────────────────────────
    let poll = checkout.tail.poll(&checkout.transcript);
    // ───────────────────────────────────────────────────────────────────

    apply_poll(registry, checkout, poll);
}

fn check_out(registry: &Shared, surface_id: u32) -> Option<TailCheckout> {
    registry.lock().ok()?.check_out(surface_id)
}

/// 읽어온 결과를 되돌리고 이벤트로 바꾼다.
fn apply_poll(registry: &Shared, checkout: TailCheckout, poll: std::io::Result<TailPoll>) {
    let surface_id = checkout.surface_id;
    let session_id = checkout.session_id.clone();
    let Ok(mut reg) = registry.lock() else {
        return;
    };
    // 꺼내간 동안 대상이 바뀌었으면(unwatch · 세션 교체 · 재-watch) 읽어온 것은 옛 대상의
    // 진행 상태이므로 통째로 버린다.
    if !reg.check_in(checkout) {
        return;
    }
    let lines = match poll {
        Ok(TailPoll::Lines { lines, resynced }) => {
            if resynced {
                tracing::debug!(
                    "agent-stream: transcript for surface {surface_id} was truncated or replaced — re-read from the start (duplicates are absorbed by uuid dedupe)"
                );
            }
            lines
        }
        Ok(TailPoll::Missing) => return,
        Err(e) => {
            tracing::warn!("agent-stream: reading transcript for surface {surface_id} failed: {e}");
            return;
        }
    };
    if lines.is_empty() {
        return;
    }
    let events = collect_events(&mut reg, surface_id, &lines);
    for event in events {
        reg.push_event(surface_id, &session_id, event);
    }
    reg.mark_dirty();
}

/// 아직 파일을 못 찾은 대상의 경로를 다시 해석해 본다(세션 시작 직후 race / 세션 교체 직후).
///
/// `is_file` 검사와 디렉토리 탐색은 **락 밖에서** 한다 — 위 `pump_one` 과 같은 이유다.
fn resolve_pending_path(registry: &Shared, surface_id: u32, root: Option<&Path>) {
    let Some(root) = root else {
        return;
    };
    let Some((session_id, transcript)) = target_of(registry, surface_id) else {
        return;
    };

    // ── 락 없음: 파일 존재 확인 + 디렉토리 탐색 ──────────────────────────
    if transcript.is_file() {
        return;
    }
    let Some(path) = resolve::find_transcript(root, &session_id) else {
        return;
    };
    // ───────────────────────────────────────────────────────────────────

    if let Ok(mut reg) = registry.lock() {
        // 그 사이 세션이 바뀌었으면 반영하지 않는다 — 방금 찾은 경로는 옛 세션 것이다.
        reg.set_transcript(surface_id, &session_id, path);
    }
}

/// 대상의 (session_id, transcript) 스냅샷. 락은 이 함수 안에서만 잡는다.
fn target_of(registry: &Shared, surface_id: u32) -> Option<(String, std::path::PathBuf)> {
    let reg = registry.lock().ok()?;
    reg.targets()
        .into_iter()
        .find(|(id, _, _)| *id == surface_id)
        .map(|(_, session, path)| (session, path))
}

/// 라인들을 파싱해 이벤트로 바꾼다. 레코드 `uuid` 로 중복을 접는다.
fn collect_events(reg: &mut StreamRegistry, surface_id: u32, lines: &[String]) -> Vec<StreamEvent> {
    let Some(watch) = reg.watch_mut(surface_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in lines {
        match record::parse_line(line) {
            Ok(parsed) => {
                if !watch.accept_record(parsed.uuid.as_deref()) {
                    continue;
                }
                out.extend(parsed.events);
            }
            Err(e) => {
                // 재동기화 직후의 반쪽 라인이거나 우리가 모르는 포맷이다. 그 한 줄만
                // 버리고 계속 간다 — 한 줄 때문에 스트림 전체를 끊지 않는다.
                tracing::debug!("agent-stream: skipping unparsable transcript line: {e}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::new_watch;
    use serde_json::{Value, json};
    use std::io::Write;
    use tasty_plugin_sdk::PluginError;

    struct StubHost {
        exists: bool,
        session: Option<String>,
    }

    impl HostCall for StubHost {
        fn call(&self, method: &str, _params: Value) -> Result<Value, PluginError> {
            match method {
                "surface.locate" => Ok(json!({ "exists": self.exists })),
                "surface.meta.get" => Ok(json!({ "value": self.session })),
                other => panic!("unexpected host call {other}"),
            }
        }
    }

    fn append(path: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open");
        f.write_all(text.as_bytes()).expect("write");
    }

    fn assistant_line(uuid: &str, text: &str, stop: &str) -> String {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","message":{{"stop_reason":"{stop}","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    }

    fn shared_with(watch_path: &Path) -> Shared {
        let reg = Arc::new(Mutex::new(StreamRegistry::new(None)));
        reg.lock().expect("lock").insert(new_watch(
            1,
            "sess".into(),
            watch_path.to_path_buf(),
            false,
        ));
        reg
    }

    fn events(registry: &Shared) -> Vec<Value> {
        let reg = registry.lock().expect("lock");
        reg.poll_json(None, 0, 1000)["events"]
            .as_array()
            .expect("array")
            .clone()
    }

    #[test]
    fn each_append_produces_its_events_exactly_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"").expect("create");
        let registry = shared_with(&path);

        append(
            &path,
            &format!("{}\n", assistant_line("u1", "one", "tool_use")),
        );
        pump_all(&registry, None);
        pump_all(&registry, None); // 두 번 돌려도 다시 방출되지 않는다.
        assert_eq!(events(&registry).len(), 1);

        append(
            &path,
            &format!("{}\n", assistant_line("u2", "two", "end_turn")),
        );
        pump_all(&registry, None);
        let collected = events(&registry);
        assert_eq!(collected.len(), 3, "text + text + turn_end");
        assert_eq!(collected[1]["text"], "two");
        assert_eq!(collected[2]["kind"], "turn_end");
        assert_eq!(collected[2]["reason"], "stop:end_turn");
    }

    #[test]
    fn a_partial_line_emits_once_when_it_completes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"").expect("create");
        let registry = shared_with(&path);

        let line = assistant_line("u1", "hello", "end_turn");
        let (head, tail) = line.split_at(line.len() / 2);
        append(&path, head);
        pump_all(&registry, None);
        assert!(events(&registry).is_empty(), "half a line emits nothing");

        append(&path, &format!("{tail}\n"));
        pump_all(&registry, None);
        assert_eq!(events(&registry).len(), 2, "text + turn_end, emitted once");
        pump_all(&registry, None);
        assert_eq!(events(&registry).len(), 2, "and not again");
    }

    #[test]
    fn a_rewritten_transcript_is_re_read_without_duplicating_events() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"").expect("create");
        let registry = shared_with(&path);

        append(
            &path,
            &format!("{}\n", assistant_line("u1", "a", "end_turn")),
        );
        pump_all(&registry, None);
        assert_eq!(events(&registry).len(), 2);

        // 파일이 통째로 짧게 다시 쓰였다 — 같은 레코드가 다시 관측된다.
        std::fs::write(
            &path,
            format!("{}\n", assistant_line("u1", "a", "end_turn")),
        )
        .expect("rewrite");
        pump_all(&registry, None);
        assert_eq!(
            events(&registry).len(),
            2,
            "uuid dedupe absorbs the re-read records"
        );
    }

    #[test]
    fn unparsable_lines_do_not_stop_the_stream() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"").expect("create");
        let registry = shared_with(&path);

        append(
            &path,
            &format!(
                "garbage not json\n{}\n",
                assistant_line("u1", "still here", "end_turn")
            ),
        );
        pump_all(&registry, None);
        let collected = events(&registry);
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0]["text"], "still here");
    }

    #[test]
    fn a_transcript_that_appears_later_is_picked_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 등록 시점에는 경로가 미해결(빈 경로)이다 — 세션 시작 직후 race.
        let registry = shared_with(Path::new(""));
        pump_all(&registry, Some(dir.path()));
        assert!(events(&registry).is_empty());

        let project = dir.path().join("-some-project");
        std::fs::create_dir_all(&project).expect("mkdir");
        std::fs::write(
            project.join("sess.jsonl"),
            format!("{}\n", assistant_line("u1", "late", "end_turn")),
        )
        .expect("write");

        pump_all(&registry, Some(dir.path()));
        let collected = events(&registry);
        assert_eq!(collected.len(), 2, "the late transcript is read in full");
        assert_eq!(collected[0]["text"], "late");
    }

    #[test]
    fn a_closed_surface_is_dropped_with_a_terminal_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"").expect("create");
        let registry = shared_with(&path);

        verify_targets(
            &registry,
            &StubHost {
                exists: false,
                session: Some("sess".into()),
            },
            None,
        );
        assert!(!registry.lock().expect("lock").is_watched(1));
        let collected = events(&registry);
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0]["kind"], "turn_end");
        assert_eq!(collected[0]["reason"], record::REASON_SESSION_ENDED);
    }

    #[test]
    fn a_new_session_on_the_same_surface_rebinds_the_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = dir.path().join("old.jsonl");
        std::fs::write(&old, b"").expect("create");
        let project = dir.path().join("-proj");
        std::fs::create_dir_all(&project).expect("mkdir");
        std::fs::write(project.join("brand-new.jsonl"), b"").expect("write");
        let registry = shared_with(&old);

        verify_targets(
            &registry,
            &StubHost {
                exists: true,
                session: Some("brand-new".into()),
            },
            Some(dir.path()),
        );
        {
            let mut reg = registry.lock().expect("lock");
            let watch = reg.watch_mut(1).expect("still watched");
            assert_eq!(watch.session_id, "brand-new");
            assert_eq!(watch.transcript, project.join("brand-new.jsonl"));
        }
        let collected = events(&registry);
        assert_eq!(collected[0]["reason"], record::REASON_SESSION_ENDED);
        assert_eq!(collected[0]["session_id"], "sess", "the old turn is closed");
    }

    #[test]
    fn a_session_switch_whose_file_is_not_written_yet_leaves_the_path_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = dir.path().join("old.jsonl");
        std::fs::write(&old, b"").expect("create");
        let registry = shared_with(&old);

        verify_targets(
            &registry,
            &StubHost {
                exists: true,
                session: Some("not-on-disk-yet".into()),
            },
            Some(dir.path()),
        );
        let mut reg = registry.lock().expect("lock");
        let watch = reg.watch_mut(1).expect("still watched");
        assert_eq!(watch.session_id, "not-on-disk-yet");
        assert_eq!(watch.transcript, std::path::PathBuf::new());
    }

    #[test]
    fn an_unchanged_session_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"").expect("create");
        let registry = shared_with(&path);

        verify_targets(
            &registry,
            &StubHost {
                exists: true,
                session: Some("sess".into()),
            },
            None,
        );
        assert!(registry.lock().expect("lock").is_watched(1));
        assert!(events(&registry).is_empty());
    }

    #[test]
    fn a_meta_lookup_failure_does_not_drop_the_watch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"").expect("create");
        let registry = shared_with(&path);

        verify_targets(
            &registry,
            &StubHost {
                exists: true,
                session: None,
            },
            None,
        );
        assert!(
            registry.lock().expect("lock").is_watched(1),
            "a transient meta read failure must not tear the stream down"
        );
    }
}
