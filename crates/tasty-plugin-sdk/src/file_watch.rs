//! 열린 파일의 변경을 주기적으로 확인하는 공용 워커.
//! EntryProbe가 파일 상태를 비교하고 변경을 감지하면 self_invoke로 플러그인의
//! reload 메서드를 호출한다. 감지 과정에서도 판정 방식에 따라 파일을 읽는다.
//! 문서·이미지 상태 갱신은 플러그인의 단일 워커에서 처리한다.
//!
//! 플러그인마다 판정 방식과 reload 메서드 이름을 지정한다. EntryProbe가 상태를
//! 보관하므로 metadata가 같으면 내용 읽기를 생략하는 방식도 사용할 수 있다.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::host::HostHandle;

/// 변경 검사 사이에 기다리는 시간. 파일 조회·읽기·렌더링 시간을 포함한
/// 화면 갱신 지연의 상한은 아니다.
pub const RELOAD_CHECK_INTERVAL_SECS: f64 = 1.0;

/// 감시 worker 에 보내는 등록/해제 명령. `create_surface`/`destroy_surface` 가 보낸다.
///
/// 제자리 이동(같은 surface_id 로 다른 파일을 여는 것)은 `create_surface` 가 다시
/// 호출되므로 `Register` 가 감시 대상 경로를 자연스럽게 갱신한다(별도 케이스 불필요).
pub enum WatchCmd {
    /// surface 의 감시 대상 경로를 등록(또는 갱신). `path` 가 `None` 이면 감시 해제와
    /// 동일(파일 없는 surface).
    Register {
        surface_id: u32,
        path: Option<String>,
    },
    /// surface 소멸 — 감시 대상에서 제거.
    Unregister { surface_id: u32 },
}

/// 감시 대상 하나에 대해 "마지막 관측 이후 바뀌었나" 를 답하는 판정자.
///
/// 자기 기준선을 자기가 들고 있다 — 모듈 문서의 "왜 trait 인가" 참조.
pub trait EntryProbe: Send + 'static {
    /// 등록 시점의 기준선을 만든다. 이 시점은 변경으로 보고되지 않는다.
    ///
    /// 읽을 수 없는 경로(파일 없음·권한)도 **추적은 된다** — 그 상태가 이어지는 동안은
    /// 변경이 아니고, 나중에 생기면 그때 변경으로 잡힌다.
    fn baseline(path: &str) -> Self;

    /// 마지막 관측 이후 바뀌었으면 `true`. 내부 기준선을 현재 상태로 갱신한다.
    fn changed(&mut self, path: &str) -> bool;
}

/// 파일 전체를 읽어 길이와 내용의 64비트 해시를 구한다. 읽기 실패는 None이다.
pub fn content_digest(path: &str) -> Option<u64> {
    use std::hash::Hasher;
    let bytes = std::fs::read(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_usize(bytes.len());
    h.write(&bytes);
    Some(h.finish())
}

/// 검사할 때마다 파일 전체의 해시를 비교한다. mtime이 같아도 내용 변경을
/// 찾을 수 있지만 해시 충돌과 검사 사이에 사라진 변경까지 검출하지는 못한다.
/// 읽기 비용은 파일 크기에 따라 늘어나며 이 타입은 크기를 제한하지 않는다.
pub struct ContentDigest {
    last: Option<u64>,
}

impl EntryProbe for ContentDigest {
    fn baseline(path: &str) -> Self {
        Self {
            last: content_digest(path),
        }
    }

    fn changed(&mut self, path: &str) -> bool {
        let current = content_digest(path);
        if current == self.last {
            return false;
        }
        self.last = current;
        true
    }
}

/// 추가 내용 검사를 유지할 시간. FAT32에서 관측한 2초 mtime 간격을 사용한다.
/// 모든 파일 시스템의 최대 mtime 간격을 뜻하지는 않는다.
pub const COARSEST_OBSERVED_MTIME_TICK: Duration = Duration::from_secs(2);

/// 파일 내용은 읽지 않고 길이와 mtime을 조회한다.
fn stat_key(path: &str) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// 길이·mtime이 바뀌었을 때 내용을 읽어 해시를 비교한다. metadata만 바뀌고
/// 내용이 같으면 비싼 디코딩을 반복하지 않도록 변경으로 보고하지 않는다.
///
/// 최근에 수정한 파일은 COARSEST_OBSERVED_MTIME_TICK 동안 metadata가 같아도
/// 내용을 다시 확인한다. 이 기간이 지난 뒤 길이·mtime이 같은 변경은 놓칠 수 있다.
/// 64비트 해시 충돌의 한계도 있다.
pub struct StatGatedDigest {
    /// 마지막으로 본 (길이, mtime). `None` 은 `stat` 불가(파일 없음·권한).
    stat: Option<(u64, std::time::SystemTime)>,
    /// 마지막으로 읽은 내용 지문. `stat` 이 움직였거나 창이 열려 있을 때만 갱신된다.
    digest: Option<u64>,
    /// metadata가 같아도 내용을 다시 확인할 기한.
    settle_until: Option<Instant>,
}

impl StatGatedDigest {
    /// mtime이 최근 값이거나 미래 값이면 추가 확인 기한을 설정한다.
    /// stat 조회 결과가 없으면 기한도 만들지 않는다.
    fn arm_settle(stat: Option<(u64, std::time::SystemTime)>) -> Option<Instant> {
        let (_, mtime) = stat?;
        match std::time::SystemTime::now().duration_since(mtime) {
            Ok(age) if age >= COARSEST_OBSERVED_MTIME_TICK => None,
            _ => Some(Instant::now() + COARSEST_OBSERVED_MTIME_TICK),
        }
    }
}

impl EntryProbe for StatGatedDigest {
    fn baseline(path: &str) -> Self {
        let stat = stat_key(path);
        Self {
            stat,
            digest: content_digest(path),
            settle_until: Self::arm_settle(stat),
        }
    }

    fn changed(&mut self, path: &str) -> bool {
        let now_stat = stat_key(path);
        let moved = now_stat != self.stat;
        let unsettled = self.settle_until.is_some_and(|t| Instant::now() < t);
        if !moved && !unsettled {
            // 정상상태 — 여기서 끝난다. 파일을 읽지 않는다.
            return false;
        }
        self.stat = now_stat;
        if moved {
            self.settle_until = Self::arm_settle(now_stat);
        }
        // `moved` 가 아니면 여기 온 이유가 창이 열려 있어서다(위 이른 반환) — 창은 그대로
        // 두어 남은 시간만큼 더 본다.
        let current = content_digest(path);
        if current == self.digest {
            // `touch`·`rsync`·같은 내용 재저장 — 리로드하지 않는다.
            return false;
        }
        self.digest = current;
        true
    }
}

/// 감시 중인 surface 1 개 — 경로와 그 경로 전용 판정자.
struct WatchEntry<P> {
    path: String,
    probe: P,
}

/// `on_start` 에서 spawn 되는 감시 루프. `rx` 가 끊기면(plugin 종료) 반환한다.
///
/// `reload_method` 는 변경 감지 시 `self_invoke` 할 자기 네임스페이스 메서드
/// (예: `"markdown.reload"`). 그 메서드는 `{"surface": <id>}` 를 params 로 받는다.
pub fn run<P: EntryProbe>(
    host: HostHandle,
    rx: mpsc::Receiver<WatchCmd>,
    reload_method: &'static str,
) {
    let mut watched: HashMap<u32, WatchEntry<P>> = HashMap::new();
    loop {
        if !drain_commands_until_tick(&rx, &mut watched) {
            return;
        }
        for surface_id in poll_changed(&mut watched) {
            // 상태 갱신은 플러그인 워커의 reload 메서드에 맡긴다.
            if let Err(e) = host.self_invoke(reload_method, json!({ "surface": surface_id })) {
                tracing::warn!(
                    "file watch: {reload_method} self-invoke failed for surface {surface_id}: {e}"
                );
            }
        }
    }
}

/// 다음 검사 시각까지 등록·해제 명령을 받는다. 채널이 닫히면 false를 반환한다.
fn drain_commands_until_tick<P: EntryProbe>(
    rx: &mpsc::Receiver<WatchCmd>,
    watched: &mut HashMap<u32, WatchEntry<P>>,
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

fn apply_register<P: EntryProbe>(
    watched: &mut HashMap<u32, WatchEntry<P>>,
    surface_id: u32,
    path: Option<String>,
) {
    match path {
        Some(p) => {
            let probe = P::baseline(&p);
            watched.insert(surface_id, WatchEntry { path: p, probe });
        }
        None => {
            watched.remove(&surface_id);
        }
    }
}

/// 등록된 파일을 검사하고 변경된 surface ID를 반환한다.
/// 읽기 가능 상태에서 읽기 실패로 바뀐 경우도 변경으로 보고한다.
fn poll_changed<P: EntryProbe>(watched: &mut HashMap<u32, WatchEntry<P>>) -> Vec<u32> {
    let mut changed = Vec::new();
    for (surface_id, entry) in watched.iter_mut() {
        if entry.probe.changed(&entry.path) {
            changed.push(*surface_id);
        }
    }
    changed
}

#[cfg(test)]
// 테스트의 let _ = 사용은 제품 코드의 오류 무시 목록에서 제외한다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    /// 감시 대상 후보 하나. `ContentDigest` 로 고정한다 — 판정자 교체는 그 판정자를
    /// 가진 쪽에서 시험한다.
    type Watched = HashMap<u32, WatchEntry<ContentDigest>>;

    fn probe_path(what: &str) -> std::path::PathBuf {
        // 시계 해상도에 의존하지 않도록 파일명에 단조 증가 카운터를 넣는다.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "tasty-watch-{what}-{}-{:?}.md",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    fn set_mtime(path: &std::path::Path, t: std::time::SystemTime) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(t).unwrap();
    }

    #[test]
    fn apply_register_sets_and_clears_watch() {
        let mut watched: Watched = HashMap::new();
        let path = probe_path("reg");
        std::fs::write(&path, b"# hi").unwrap();
        apply_register(&mut watched, 1, Some(path.to_string_lossy().into_owned()));
        assert!(watched.contains_key(&1));
        apply_register(&mut watched, 1, None);
        assert!(!watched.contains_key(&1));
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    #[test]
    fn apply_register_missing_file_has_no_digest_but_is_tracked() {
        let mut watched: Watched = HashMap::new();
        apply_register(&mut watched, 2, Some("\0nonexistent-watch".to_string()));
        let entry = watched.get(&2).expect("still tracked despite read failure");
        assert!(entry.probe.last.is_none());
    }

    /// 삭제를 변경으로 감지하되 지속 상태(None==None)는 반복 emit 하지 않는다 — plugin 의
    /// reload 메서드가 가진 삭제-감지 규약과 동형. mtime 해상도 낮은 파일시스템에서도
    /// 안정적이도록 "수정" 대신 "삭제"로 변경을 유발한다.
    #[test]
    fn poll_changed_detects_deletion_once_then_quiesces() {
        let path = probe_path("del");
        std::fs::write(&path, b"v1").unwrap();
        let mut watched: Watched = HashMap::new();
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

    /// 변경이 없는 파일은 반복 검사해도 보고하지 않는다.
    #[test]
    fn poll_changed_is_quiet_when_nothing_changes() {
        let path = probe_path("quiet");
        std::fs::write(&path, b"v1").unwrap();
        let mut watched: Watched = HashMap::new();
        apply_register(&mut watched, 1, Some(path.to_string_lossy().into_owned()));
        assert!(poll_changed(&mut watched).is_empty(), "1 회차");
        assert!(poll_changed(&mut watched).is_empty(), "2 회차");
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// 내용을 바꾼 뒤 mtime을 되돌려도 ContentDigest가 변경을 찾는지 확인한다.
    #[test]
    fn a_rewrite_with_the_same_mtime_is_still_seen() {
        let path = probe_path("tie");
        std::fs::write(&path, b"v1").unwrap();
        let stamp = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        set_mtime(&path, stamp);

        let mut watched: Watched = HashMap::new();
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

    // StatGatedDigest: 파일 시각을 직접 설정해 추가 확인 기간의 안팎을 검사한다.

    /// 추가 확인 기간보다 오래된 mtime을 만든다.
    fn stale_stamp() -> std::time::SystemTime {
        std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)
    }

    #[test]
    fn arm_settle_opens_for_a_fresh_write_and_stays_shut_for_an_old_one() {
        let fresh = Some((1, std::time::SystemTime::now()));
        assert!(
            StatGatedDigest::arm_settle(fresh).is_some(),
            "최근 수정된 파일은 추가 내용 검사를 예약해야 한다"
        );
        let old = Some((1, stale_stamp()));
        assert!(
            StatGatedDigest::arm_settle(old).is_none(),
            "mtime이 오래된 파일은 추가 내용 검사를 예약하지 않아야 한다"
        );
        assert!(
            StatGatedDigest::arm_settle(None).is_none(),
            "stat 조회에 실패하면 추가 검사 기한을 만들지 않아야 한다"
        );
    }

    /// 변경이 없으면 보고하지 않는다.
    #[test]
    fn stat_gated_is_quiet_when_nothing_changes() {
        let path = probe_path("sg-quiet");
        std::fs::write(&path, b"v1").unwrap();
        set_mtime(&path, stale_stamp());
        let p = path.to_string_lossy().into_owned();
        let mut probe = StatGatedDigest::baseline(&p);
        assert!(!probe.changed(&p), "1 회차");
        assert!(!probe.changed(&p), "2 회차");
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// 내용이 같고 mtime만 바뀌면 다시 읽기를 요청하지 않는다.
    #[test]
    fn a_touch_that_keeps_the_content_is_not_a_change() {
        let path = probe_path("sg-touch");
        std::fs::write(&path, b"same bytes").unwrap();
        set_mtime(&path, stale_stamp());
        let p = path.to_string_lossy().into_owned();
        let mut probe = StatGatedDigest::baseline(&p);

        // 내용은 그대로 두고 시계만 움직인다 = `touch`.
        set_mtime(&path, stale_stamp() + Duration::from_secs(500));
        assert!(
            !probe.changed(&p),
            "시계가 움직여도 내용이 같으면 리로드하지 않는다"
        );
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// 내용이 진짜 바뀌면 잡는다(게이트가 열리는 평범한 경로).
    #[test]
    fn a_real_edit_is_seen() {
        let path = probe_path("sg-edit");
        std::fs::write(&path, b"v1").unwrap();
        set_mtime(&path, stale_stamp());
        let p = path.to_string_lossy().into_owned();
        let mut probe = StatGatedDigest::baseline(&p);

        std::fs::write(&path, b"v2 which is longer").unwrap();
        assert!(probe.changed(&p), "내용이 바뀌었으면 잡아야 한다");
        assert!(!probe.changed(&p), "같은 내용을 다시 보면 조용해야 한다");
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// 추가 확인 기간에는 길이와 mtime이 같은 내용 변경도 찾는지 확인한다.
    #[test]
    fn a_second_write_in_the_same_tick_is_caught_while_the_window_is_open() {
        let path = probe_path("sg-tick");
        std::fs::write(&path, b"aaaa").unwrap();
        // 최근 mtime을 주어 baseline에서 추가 내용 검사를 예약한다.
        let p = path.to_string_lossy().into_owned();
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut probe = StatGatedDigest::baseline(&p);

        // 같은 눈금에 떨어진 두 번째 쓰기를 흉내 낸다: 길이가 같고 mtime 도 같다.
        std::fs::write(&path, b"bbbb").unwrap();
        set_mtime(&path, stamp);
        let m = std::fs::metadata(&path).unwrap();
        assert_eq!(m.len(), 4, "전제: 길이가 같아야 이 시험이 뜻이 있다");
        assert_eq!(
            m.modified().unwrap(),
            stamp,
            "전제: mtime 이 같아야 stat 게이트가 못 가른다"
        );

        assert!(
            probe.changed(&p),
            "길이와 mtime이 같아도 추가 확인 기간에는 내용 변경을 찾아야 한다"
        );
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// 추가 확인 기간 밖에서는 길이와 mtime이 같은 내용 변경을 놓치는 한계를 확인한다.
    #[test]
    fn the_same_write_is_missed_once_the_window_has_shut() {
        let path = probe_path("sg-shut");
        std::fs::write(&path, b"aaaa").unwrap();
        set_mtime(&path, stale_stamp()); // 추가 내용 검사 기간 밖.
        let p = path.to_string_lossy().into_owned();
        let mut probe = StatGatedDigest::baseline(&p);

        std::fs::write(&path, b"bbbb").unwrap();
        set_mtime(&path, stale_stamp()); // 길이도 mtime 도 그대로.
        assert!(
            !probe.changed(&p),
            "추가 확인 기간 밖에서는 길이와 mtime이 같은 변경을 놓친다"
        );
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// 삭제도 변경이고, 지속되는 동안은 반복 emit 하지 않는다.
    #[test]
    fn stat_gated_detects_deletion_once_then_quiesces() {
        let path = probe_path("sg-del");
        std::fs::write(&path, b"v1").unwrap();
        set_mtime(&path, stale_stamp());
        let p = path.to_string_lossy().into_owned();
        let mut probe = StatGatedDigest::baseline(&p);

        std::fs::remove_file(&path).unwrap();
        assert!(probe.changed(&p), "삭제가 1회 감지되어야 한다");
        assert!(!probe.changed(&p), "삭제 지속 시 반복 emit 없음");
    }

    #[test]
    fn poll_changed_ignores_untracked_surfaces() {
        let mut watched: Watched = HashMap::new();
        assert!(poll_changed(&mut watched).is_empty());
    }
}
