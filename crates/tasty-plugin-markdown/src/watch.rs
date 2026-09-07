//! idle 상태에서도 markdown 파일 변경을 감지하는 감시 worker (단계 06, Stage B 갱신).
//!
//! `on_start` 이 받은 `HostHandle` 을 이 모듈의 별도 스레드로 넘겨
//! `RELOAD_CHECK_INTERVAL_SECS` 주기로 등록된 surface 들의 파일 **내용**을 읽어 견준다.
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
use std::time::{Duration, Instant};

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

/// 감시 중인 surface 1개 — worker 가 마지막으로 관측한 **내용 지문**.
///
/// `None` 은 "읽을 수 없다"(파일 없음·권한)이고, 그 상태가 이어지는 동안은 변경이 아니다.
struct WatchEntry {
    path: String,
    last_digest: Option<u64>,
}

/// 파일 **내용**의 지문. 읽을 수 없으면 `None`.
///
/// ## 왜 mtime 이 아닌가
///
/// 전에는 `metadata().modified()` 를 견줬다. 그러면 **두 쓰기 사이에 폴이 끼고 그 둘의
/// mtime 이 같은 값으로 찍힐 때** 뒤엣것을 놓치고, 그 누락은 **다음 저장 전까지 영구다**
/// (사용자에게는 "저장했는데 미리보기가 안 바뀐다" 로 보인다). 그 창의 크기는 파일시스템
/// 눈금이 정한다 — ext4·NTFS·APFS 는 사실상 0 이지만 **exFAT 은 2 초, FAT 은 1 초**이고,
/// 여기서 감시하는 것은 사용자가 여는 **아무 경로**라 USB·네트워크 마운트가 배제되지 않는다.
/// 폴 주기가 1 초라 그 눈금과 같은 자릿수다.
///
/// ## 비용
///
/// 폴마다 전량을 읽는다. 실측(이 레포 `.md` 429 개, 중앙 8.0 KB · 최대 131 KB):
/// `stat` 1.0 us 대 읽기+해시 **11.3 us**(8 KB) / **117 us**(131 KB). 감시 surface 10 개를
/// 열어도 초당 1.2 ms — 코어의 0.12 % 다. **비용이 문서 크기로 유계**라서 이 교환이 성립한다.
/// (같은 교환이 성립하지 않는 자리도 있다 — `src/host_api/hooks/global.rs` 의 파일 조건은
/// 감시 대상이 자라는 로그라 상한이 없고, 그래서 그쪽은 시계를 그대로 쓴다.)
///
/// 지문은 64 비트다. 서로 다른 내용이 같은 값을 낼 확률이 남지만, 그 확률은 위 눈금 창보다
/// 몇 자릿수 작다 — 길이를 함께 섞어 같은 길이가 아닌 내용은 값이 갈리게 한다.
fn digest(path: &str) -> Option<u64> {
    use std::hash::Hasher;
    let bytes = std::fs::read(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_usize(bytes.len());
    h.write(&bytes);
    Some(h.finish())
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
            let last_digest = digest(&p);
            watched.insert(
                surface_id,
                WatchEntry {
                    path: p,
                    last_digest,
                },
            );
        }
        None => {
            watched.remove(&surface_id);
        }
    }
}

/// 등록된 모든 surface 의 **내용 지문**을 다시 구해 변경된 surface_id 목록을 반환(마지막
/// 관측값도 갱신) — host 호출(`notify`) 없이 순수 로직만 테스트 가능하도록 분리했다.
/// 삭제(읽기 실패=None)도 변경으로 취급해 host 재forward → `poll_reload` 의 기존
/// 삭제-감지 규약(main.rs)으로 흡수시킨다.
fn poll_changed(watched: &mut HashMap<u32, WatchEntry>) -> Vec<u32> {
    let mut changed = Vec::new();
    for (surface_id, entry) in watched.iter_mut() {
        let current = digest(&entry.path);
        if current == entry.last_digest {
            continue;
        }
        entry.last_digest = current;
        changed.push(*surface_id);
    }
    changed
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    fn probe_path(what: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tasty-md-watch-{what}-{}-{:?}.md",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    fn set_mtime(path: &std::path::Path, t: std::time::SystemTime) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(t).unwrap();
    }

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
        let entry = watched.get(&2).expect("still tracked despite read failure");
        assert!(entry.last_digest.is_none());
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

    /// ⓪ **대조군** — 아무것도 안 바뀌면 아무것도 안 나와야 한다.
    ///
    /// 이 칸이 없으면 아래 시험이 "잡았다" 를 낼 때 그것이 **판정이 옳아서인지 하네스가
    /// 무엇이든 변경으로 부르기 때문인지** 못 가른다. 두 시험은 짝으로만 뜻이 있다.
    #[test]
    fn poll_changed_is_quiet_when_nothing_changes() {
        let path = probe_path("quiet");
        std::fs::write(&path, b"v1").unwrap();
        let mut watched = HashMap::new();
        apply_register(&mut watched, 1, Some(path.to_string_lossy().into_owned()));
        assert!(poll_changed(&mut watched).is_empty(), "1 회차");
        assert!(poll_changed(&mut watched).is_empty(), "2 회차");
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// **시각이 같고 내용이 다르면 잡아야 한다.**
    ///
    /// 시계를 흉내 내지 않는다 — 눈금이 거친 파일시스템을 재현하려 들면 우리 디스크가
    /// 안 주는 것을 "결함 없음" 으로 세게 된다. 대신 그 눈금이 만드는 **상태를 직접
    /// 세운다**: 쓰기 전후의 mtime 을 같은 값으로 찍는다. 그 상태에서 옛 판정(mtime 비교)은
    /// 반드시 놓치고, 지금 판정은 반드시 잡는다 — 어느 기계에서든 같다.
    #[test]
    fn a_rewrite_with_the_same_mtime_is_still_seen() {
        let path = probe_path("tie");
        std::fs::write(&path, b"v1").unwrap();
        let stamp = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        set_mtime(&path, stamp);

        let mut watched = HashMap::new();
        apply_register(&mut watched, 7, Some(path.to_string_lossy().into_owned()));

        std::fs::write(&path, b"v2").unwrap();
        set_mtime(&path, stamp); // 쓰기가 올린 mtime 을 되돌린다 — 같은 눈금에 떨어진 상태.
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            stamp,
            "전제 확인: 두 관측의 mtime 이 실제로 같아야 이 시험이 뜻이 있다"
        );

        assert_eq!(
            poll_changed(&mut watched),
            vec![7],
            "mtime 이 같아도 내용이 바뀌었으면 잡아야 한다"
        );
        assert!(
            poll_changed(&mut watched).is_empty(),
            "같은 내용을 다시 보면 조용해야 한다"
        );
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    #[test]
    fn poll_changed_ignores_untracked_surfaces() {
        let mut watched = HashMap::new();
        assert!(poll_changed(&mut watched).is_empty());
    }
}
