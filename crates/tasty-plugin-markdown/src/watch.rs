//! idle 상태에서도 markdown 파일 변경을 감지하는 감시 worker (단계 06, Stage B 갱신).
//!
//! `on_start` 이 받은 `HostHandle` 을 이 모듈의 별도 스레드로 넘겨
//! `RELOAD_CHECK_INTERVAL_SECS` 주기로 등록된 surface 들의 파일 mtime 을 stat 한다.
//! webview-kind surface 는 (egui-mesh 와 달리) `paint`/`set_context` 를 전혀 받지 않으므로
//! — 그 경로에 있던 `MdDoc::poll_reload` 도 이제 호출되지 않는다 — idle 감시가 사실상
//! 유일한 자동 갱신 경로다.
//!
//! **read 는 이 스레드에서 하지 않는다** — 변경 감지 시 이 plugin이 스스로 소유한
//! `markdown.reload` IPC 메서드를 `HostHandle::self_invoke` 로 트리거한다. host 를
//! 왕복하는 `host.call` 은 여기서 쓸 수 없다 — 호스트 dispatcher(`plugin_ipc.rs`)는
//! caller가 네임스페이스 owner 자신이면 forward하지 않고 host-native dispatch로
//! 통과시키므로(trampoline 패턴 지원 목적), plugin이 자기 네임스페이스 메서드를
//! `call()`로 부르면 host에 동명 메서드가 없어 항상 `-32601 Method not found`가
//! 떨어진다. `self_invoke` 는 host를 거치지 않고 `&mut plugin` 을 쥔 단일 worker
//! 스레드의 처리 큐에 직접 enqueue한다 — CLI/사용자가 같은 메서드를 호출하는 것과
//! 동일하게 그 worker 스레드에 직렬로 도착하므로, 실제 read(`MdDoc::force_reload`)와
//! 문서 재생성(`reload_webview`)은 항상 이 하나의 경로(`MarkdownPlugin::markdown_reload`)만
//! 탄다 — 빠른 연속 편집이 와도 "stale read 가 최신 것을 덮어쓰는" 레이스가 애초에
//! 생기지 않는다(쓰기 경로가 하나뿐).

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;
use tasty_plugin_sdk::HostHandle;

use crate::RELOAD_CHECK_INTERVAL_SECS;

/// 감시 worker 에 보내는 등록/해제 명령. `create_surface`/`destroy_surface` 가 보낸다.
/// `markdown.navigate` 제자리 이동은 같은 surface_id 로 `create_surface` 가 다시
/// 호출되므로 `Register` 가 감시 대상 경로를 자연스럽게 갱신한다(별도 케이스 불필요).
pub(crate) enum WatchCmd {
    /// surface 의 감시 대상 경로를 등록(또는 갱신). `path` 가 `None` 이면 감시 해제와
    /// 동일(파일 없는 surface).
    Register {
        surface_id: u32,
        path: Option<String>,
    },
    /// surface 소멸 — 감시 대상에서 제거.
    Unregister { surface_id: u32 },
}

/// 감시 중인 surface 1개 — worker 가 마지막으로 관측한 mtime.
struct WatchEntry {
    path: String,
    last_mtime: Option<SystemTime>,
}

/// `on_start` 에서 spawn 되는 감시 루프. `rx` 가 끊기면(plugin 종료) 반환한다.
pub(crate) fn run(host: HostHandle, rx: mpsc::Receiver<WatchCmd>) {
    let mut watched: HashMap<u32, WatchEntry> = HashMap::new();
    loop {
        if !drain_commands_until_tick(&rx, &mut watched) {
            return;
        }
        for surface_id in poll_changed(&mut watched) {
            // worker 큐에 직접 enqueue — 모듈 문서 참조(실제 read + 문서 재생성은
            // 전부 `MarkdownPlugin::markdown_reload` 하나로 수렴시켜 레이스를 없앤다).
            if let Err(e) = host.self_invoke("markdown.reload", json!({ "surface": surface_id })) {
                tracing::warn!(
                    "markdown watch: markdown.reload self-invoke failed for surface {surface_id}: {e}"
                );
            }
        }
    }
}

/// 다음 폴링 tick 까지 명령을 즉시 반영하며 대기한다(등록/해제가 다음 tick 을 기다리지
/// 않고 바로 감시 목록에 반영됨). 채널이 끊기면(plugin 종료) `false`.
fn drain_commands_until_tick(
    rx: &mpsc::Receiver<WatchCmd>,
    watched: &mut HashMap<u32, WatchEntry>,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs_f64(RELOAD_CHECK_INTERVAL_SECS);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        match rx.recv_timeout(remaining) {
            Ok(WatchCmd::Register { surface_id, path }) => {
                apply_register(watched, surface_id, path)
            }
            Ok(WatchCmd::Unregister { surface_id }) => {
                watched.remove(&surface_id);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => return true,
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn apply_register(watched: &mut HashMap<u32, WatchEntry>, surface_id: u32, path: Option<String>) {
    match path {
        Some(p) => {
            let last_mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
            watched.insert(
                surface_id,
                WatchEntry {
                    path: p,
                    last_mtime,
                },
            );
        }
        None => {
            watched.remove(&surface_id);
        }
    }
}

/// 등록된 모든 surface 의 mtime 을 재stat 해 변경된 surface_id 목록을 반환(마지막
/// 관측값도 갱신) — host 호출(`notify`) 없이 순수 로직만 테스트 가능하도록 분리했다.
/// 삭제(metadata 실패=None)도 변경으로 취급해 host 재forward → `poll_reload` 의 기존
/// 삭제-감지 규약(main.rs)으로 흡수시킨다.
fn poll_changed(watched: &mut HashMap<u32, WatchEntry>) -> Vec<u32> {
    let mut changed = Vec::new();
    for (surface_id, entry) in watched.iter_mut() {
        let current = std::fs::metadata(&entry.path)
            .and_then(|m| m.modified())
            .ok();
        if current == entry.last_mtime {
            continue;
        }
        entry.last_mtime = current;
        changed.push(*surface_id);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_register_sets_and_clears_watch() {
        let mut watched = HashMap::new();
        let path =
            std::env::temp_dir().join(format!("tasty-md-watch-reg-{}.md", std::process::id()));
        std::fs::write(&path, b"# hi").unwrap();
        apply_register(&mut watched, 1, Some(path.to_string_lossy().into_owned()));
        assert!(watched.contains_key(&1));
        apply_register(&mut watched, 1, None);
        assert!(!watched.contains_key(&1));
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    #[test]
    fn apply_register_missing_file_has_no_mtime_but_is_tracked() {
        let mut watched = HashMap::new();
        apply_register(&mut watched, 2, Some("\0nonexistent-watch".to_string()));
        let entry = watched.get(&2).expect("still tracked despite stat failure");
        assert!(entry.last_mtime.is_none());
    }

    /// 삭제(mtime None) 를 변경으로 감지하되 지속 상태(None==None)는 반복 emit 하지
    /// 않는다 — main.rs 의 `poll_reload_detects_external_deletion_as_error` 와 동형
    /// 규약. mtime 해상도 낮은 파일시스템에서도 안정적이도록 "수정" 대신 "삭제"로
    /// 변경을 유발한다.
    #[test]
    fn poll_changed_detects_deletion_once_then_quiesces() {
        let path =
            std::env::temp_dir().join(format!("tasty-md-watch-del-{}.md", std::process::id()));
        std::fs::write(&path, b"v1").unwrap();
        let mut watched = HashMap::new();
        apply_register(&mut watched, 9, Some(path.to_string_lossy().into_owned()));
        assert!(poll_changed(&mut watched).is_empty(), "변경 전에는 빈 목록");

        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            poll_changed(&mut watched),
            vec![9],
            "삭제가 1회 감지되어야 한다"
        );
        assert!(
            poll_changed(&mut watched).is_empty(),
            "삭제 지속 시(None==None) 반복 emit 없음"
        );
    }

    #[test]
    fn poll_changed_ignores_untracked_surfaces() {
        let mut watched = HashMap::new();
        assert!(poll_changed(&mut watched).is_empty());
    }
}
