//! idle 상태에서도 열려 있는 파일의 외부 변경을 감지하는 감시 worker(plugin 공용).
//!
//! `on_start` 이 받은 [`HostHandle`] 을 별도 스레드로 넘겨 [`RELOAD_CHECK_INTERVAL_SECS`]
//! 주기로 등록된 surface 들의 파일을 견준다. host 는 plugin surface 에 무조건 tick 을 주지
//! 않는다 — webview kind 는 `paint`/`set_context` 를 아예 안 받고, egui-mesh kind 도
//! 입력·geom·theme·focus·invalidated 중 하나가 있어야 forward 된다. 그래서 **idle 자동
//! 갱신의 유일한 경로가 이 감시 스레드다.**
//!
//! **read 는 이 스레드에서 하지 않는다** — 변경 감지 시 plugin 이 스스로 소유한 reload
//! 메서드를 [`HostHandle::self_invoke`] 로 트리거한다. host 를 왕복하는 `host.call` 은
//! 여기서 쓸 수 없다 — 호스트 dispatcher 는 caller 가 네임스페이스 owner 자신이면
//! forward 하지 않고 host-native dispatch 로 통과시키므로(trampoline 패턴 지원 목적),
//! plugin 이 자기 네임스페이스 메서드를 `call()` 로 부르면 host 에 동명 메서드가 없어 항상
//! `-32601 Method not found` 가 떨어진다. `self_invoke` 는 host 를 거치지 않고
//! `&mut plugin` 을 쥔 단일 worker 스레드의 처리 큐에 직접 enqueue 한다 — CLI/사용자가
//! 같은 메서드를 호출하는 것과 동일하게 그 worker 스레드에 직렬로 도착하므로, 실제 read 와
//! 문서/이미지 재생성은 항상 그 reload 메서드 하나로 수렴한다 — 빠른 연속 편집이 와도
//! "stale read 가 최신 것을 덮어쓰는" 레이스가 애초에 생기지 않는다(쓰기 경로가 하나뿐).
//!
//! ## 무엇이 plugin 마다 갈리나 — 둘뿐이다
//!
//! 1. **판정자**([`EntryProbe`]) — 무엇을 견줘 "바뀌었다" 로 볼 것인가.
//! 2. **reload 메서드 이름** — 변경을 알릴 자기 네임스페이스 메서드.
//!
//! 나머지(주기·채널·등록/해제·루프·self_invoke 규약)는 전부 공용이다. 특히 주기는
//! "외부 편집이 얼마 만에 보이나" 라는 **사용자에게 하나인 물음**이라 답도 하나여야 한다.
//!
//! ## 왜 판정자가 `fn(&str) -> Option<값>` 이 아니라 trait 인가
//!
//! 값을 돌려받아 견주는 형태로 두면 그 값이 매 폴 계산돼야 한다. 그러면 **읽지 않고
//! 답하는 단계를 가진 판정자를 표현할 수 없다** — 읽기 비용에 상한이 없는 입력(임의
//! 크기의 이미지 등)에서는 싼 게이트(`stat`)로 먼저 거르고 움직였을 때만 읽는 2 단
//! 판정이 필요한데, 그 "안 읽고 통과" 를 반환값으로는 말할 수 없다. 그래서 판정자가
//! 자기 상태를 들고 `bool` 을 답한다. 제네릭이라 할당도 간접호출도 없다.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::host::HostHandle;

/// 감시 폴링 주기(초). 사용자에게 보이는 뜻은 "밖에서 고친 것이 늦어도 이만큼 뒤에 보인다".
///
/// 이 값이 파일시스템 mtime 눈금과 어떤 관계인지는 [`ContentDigest`] 의 문서 참조 —
/// 눈금이 이 주기보다 거친 파일시스템이 실재한다.
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

/// 파일 **내용**의 지문. 읽을 수 없으면 `None`.
///
/// 길이를 함께 섞어 같은 길이가 아닌 내용은 값이 갈리게 한다.
pub fn content_digest(path: &str) -> Option<u64> {
    use std::hash::Hasher;
    let bytes = std::fs::read(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_usize(bytes.len());
    h.write(&bytes);
    Some(h.finish())
}

/// 매 폴 파일 전량을 읽어 내용 지문을 견주는 판정자. 오탐·미탐이 모두 없다.
///
/// ## 왜 mtime 이 아닌가
///
/// 전에는 `metadata().modified()` 를 견줬다. 그러면 **두 쓰기 사이에 폴이 끼고 그 둘의
/// mtime 이 같은 값으로 찍힐 때** 뒤엣것을 놓치고, 그 누락은 **다음 저장 전까지 영구다**
/// (사용자에게는 "저장했는데 미리보기가 안 바뀐다" 로 보인다). 그 창의 크기는 파일시스템
/// 눈금이 정한다.
///
/// 실측(루프백 이미지에 `mkfs.vfat -F 32`, 100 ms 간격 30 회 쓰기 뒤 서로 다른 mtime 수):
///
/// - **ext4: 30/30 이 전부 갈렸다.** 별도로 2000 회 연속 쓰기에서도 1999/1999 가 갈렸고,
///   가장 촘촘했던 20.7 us 간격에서도 해상됐다 — 이 창은 ext4 에서 사실상 비어 있다.
/// - **FAT32: 30 회(3.0 초)에 서로 다른 mtime 이 2 개뿐이고, 연속 차가 정확히 2.000 s 였다.**
///   즉 눈금이 [`RELOAD_CHECK_INTERVAL_SECS`] 의 **2 배**다 — 폴 한 번 건너뛰기로는 못 닫는다.
///
/// **exFAT 은 측정하지 못했다**(이 개발 환경에 `mkfs.exfat` 이 없다). 위 2.000 s 는
/// *측정된* 최대이지 *알려진* 최대가 아니다 — 더 거친 파일시스템이 없다고 말하는 것이
/// 아니라, 우리가 잰 것 중 가장 거친 값이 그렇다는 뜻이다. 그리고 여기서 감시하는 것은
/// 사용자가 여는 **아무 경로**라 USB·네트워크 마운트가 배제되지 않는다.
///
/// ## 비용 — 이 판정자가 성립하는 조건
///
/// 폴마다 전량을 읽는다. 실측(이 레포 `.md` 429 개, 중앙 8.0 KB · 최대 131 KB):
/// `stat` 1.0 us 대 읽기+해시 **11.3 us**(8 KB) / **117 us**(131 KB). 감시 surface 10 개를
/// 열어도 초당 1.2 ms — 코어의 0.12 % 다. **비용이 문서 크기로 유계**라서 이 교환이 성립한다.
///
/// **입력이 무계면 이 교환은 성립하지 않는다.** 그때는 싼 `stat` 게이트로 먼저 거르고
/// 움직였을 때만 읽는 2 단 판정자를 쓴다 — 그것이 [`EntryProbe`] 가 값 비교가 아니라
/// trait 인 이유다. (같은 교환이 성립하지 않는 자리가 plugin 밖에도 있다 — host 의 훅
/// 파일 조건은 감시 대상이 자라는 로그라 상한이 없고, 그래서 그쪽은 시계를 그대로 쓴다.)
///
/// 지문은 64 비트다. 서로 다른 내용이 같은 값을 낼 확률이 남지만, 그 확률은 위 눈금 창보다
/// 몇 자릿수 작다.
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

/// 관측된 가장 거친 mtime 눈금.
///
/// **실측값이다** — 루프백 이미지에 `mkfs.vfat -F 32` 로 FAT32 를 만들고 100 ms 간격으로
/// 30 회 쓴 뒤 서로 다른 mtime 을 셌더니 **2 개**였고, 연속 차가 정확히 2.000 s 였다.
/// 대조로 ext4 는 같은 조건에서 30/30 이 전부 갈렸다.
///
/// **이것은 *측정된* 최대이지 *알려진* 최대가 아니다.** exFAT 은 이 개발 환경에
/// `mkfs.exfat` 이 없어 **측정하지 못했다** — 더 거친 파일시스템이 없다는 뜻이 아니라,
/// 우리가 실제로 잰 것 중 가장 거친 값이 이것이라는 뜻이다. 네트워크 마운트(NFS·SMB)도
/// 미측정이다.
pub const COARSEST_OBSERVED_MTIME_TICK: Duration = Duration::from_secs(2);

/// 파일의 싼 신원 — (길이, mtime). 읽지 않는다. `stat` 은 파일 크기와 무관하게 O(1) 이다
/// (실측 0.5~1.4 us).
fn stat_key(path: &str) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// **읽기 비용에 상한이 없는 입력**을 위한 2 단 판정자: 싼 `stat` 으로 먼저 거르고,
/// 움직였을 때만 읽어 내용 지문을 견준다.
///
/// ## 왜 [`ContentDigest`] 를 그대로 쓰지 않나
///
/// 그쪽은 폴마다 전량을 읽는다. 입력 크기에 상한이 있으면 성립하지만, 사용자가 고르는
/// 아무 이미지처럼 상한이 없으면 성립하지 않는다 — 실측 19.8 MB 파일에서 읽기+해시가
/// **폴당 20.6 ms** 였다(1 Hz 로 surface 하나에 코어의 2.1 %, 10 개면 21 %).
///
/// ## 왜 `stat` 만 쓰지도 않나 — **오탐이 여기서는 비싸다**
///
/// 이 판정자를 쓰는 자리에서는 **리로드가 읽기보다 훨씬 비싸다.** 이미지 실측:
///
/// | 파일 | 읽기+지문 | 디코드 |
/// |------|-----------|--------|
/// | 44.8 KB | 42 us | 3.4 ms |
/// | 3.74 MB | 3.1 ms | 433 ms |
/// | 7.27 MB (11210x4992) | 7.2 ms | **1.84 s** |
///
/// 그리고 그 디코드는 감시 스레드가 아니라 **plugin 의 단일 worker 스레드**에서 돈다
/// (모듈 문서의 `self_invoke` 규약). 즉 오탐 한 번이 plugin 을 그만큼 얼린다. `touch`·
/// `rsync`·같은 내용 재저장·빌드 재생성은 전부 mtime 을 올리므로 시계만 보면 그때마다
/// 얼어붙는다. 지문이 그것을 **읽기 비용(ms)** 으로 막는다.
///
/// **길이는 여기에 값을 거의 못 보탠다** — 오탐(`touch`·`rsync`)에서는 길이가 그대로고,
/// 미탐 쪽에서도 무압축 포맷이면 무력하다(실측: 내용이 완전히 다른 640x480 BMP 두 장이
/// 둘 다 921,654 B). 그래도 `stat` 한 번에 딸려오는 공짜 값이라 게이트에 함께 넣는다.
///
/// ## 닫개 — [`COARSEST_OBSERVED_MTIME_TICK`] 창
///
/// `stat` 게이트만으로는 **같은 mtime 눈금에 떨어진 두 쓰기 중 뒤엣것**을 영구히 놓친다
/// (그 눈금이 실재한다는 근거는 그 상수의 실측). 그래서 움직임을 본 뒤 그 창이 지날
/// 때까지는 `stat` 이 안 움직여도 지문을 다시 본다.
///
/// ★ **창 안에서 도는 것은 지문이지 리로드가 아니다.** 지문이 같으면 아무 일도 안 일어난다
/// — 창이 열려 있는 동안 디코드가 반복되지 않는다. 창의 비용은 폴 두세 번의 읽기뿐이고,
/// 그것도 **쓰기 직후에만** 열린다(사건 구동이라 정상상태 비용은 `stat` 하나다).
pub struct StatGatedDigest {
    /// 마지막으로 본 (길이, mtime). `None` 은 `stat` 불가(파일 없음·권한).
    stat: Option<(u64, std::time::SystemTime)>,
    /// 마지막으로 읽은 내용 지문. `stat` 이 움직였거나 창이 열려 있을 때만 갱신된다.
    digest: Option<u64>,
    /// 닫개 창의 끝. `Some` 이면 `stat` 이 안 움직여도 지문을 본다.
    settle_until: Option<Instant>,
}

impl StatGatedDigest {
    /// 방금 쓰인 파일이면(= mtime 이 눈금 창 안이면) 닫개 창을 무장한다.
    ///
    /// mtime 을 못 읽거나 미래로 찍혀 있으면(시계 어긋남) **무장하는 쪽**으로 간다 —
    /// 그쪽이 안전한 방향이다(더 자주 읽을 뿐, 놓치지 않는다).
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
            // worker 큐에 직접 enqueue — 모듈 문서 참조(실제 read + 재생성은 전부 이
            // 메서드 하나로 수렴시켜 레이스를 없앤다).
            if let Err(e) = host.self_invoke(reload_method, json!({ "surface": surface_id })) {
                tracing::warn!(
                    "file watch: {reload_method} self-invoke failed for surface {surface_id}: {e}"
                );
            }
        }
    }
}

/// 다음 폴링 tick 까지 명령을 즉시 반영하며 대기한다(등록/해제가 다음 tick 을 기다리지
/// 않고 바로 감시 목록에 반영됨). 채널이 끊기면(plugin 종료) `false`.
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

/// 등록된 모든 surface 를 판정자에게 물어 변경된 surface_id 목록을 반환(기준선도 갱신) —
/// host 호출 없이 순수 로직만 테스트 가능하도록 분리했다. 삭제(읽기 실패)도 변경으로
/// 취급해 plugin 의 reload 메서드가 가진 기존 삭제-감지 규약으로 흡수시킨다.
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
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    /// 감시 대상 후보 하나. `ContentDigest` 로 고정한다 — 판정자 교체는 그 판정자를
    /// 가진 쪽에서 시험한다.
    type Watched = HashMap<u32, WatchEntry<ContentDigest>>;

    fn probe_path(what: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tasty-watch-{what}-{}-{:?}.md",
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

    /// ⓪ **대조군** — 아무것도 안 바뀌면 아무것도 안 나와야 한다.
    ///
    /// 이 칸이 없으면 아래 시험이 "잡았다" 를 낼 때 그것이 **판정이 옳아서인지 하네스가
    /// 무엇이든 변경으로 부르기 때문인지** 못 가른다. 두 시험은 짝으로만 뜻이 있다.
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

    /// **시각이 같고 내용이 다르면 잡아야 한다.**
    ///
    /// 시계를 흉내 내지 않는다 — 눈금이 거친 파일시스템을 재현하려 들면 우리 디스크가
    /// 안 주는 것을 "결함 없음" 으로 세게 된다. 대신 그 눈금이 만드는 **상태를 직접
    /// 세운다**: 쓰기 전후의 mtime 을 같은 값으로 찍는다. 그 상태에서 옛 판정(mtime 비교)은
    /// 반드시 놓치고, `ContentDigest` 는 반드시 잡는다 — 어느 기계에서든 같다.
    ///
    /// 이 상태가 가공이 아니라는 근거는 [`ContentDigest`] 문서의 FAT32 실측이다(연속
    /// mtime 차 2.000 s — 폴 주기의 2 배).
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

    // ── StatGatedDigest (2 단 게이트 + 닫개) ──
    //
    // 시계를 흉내 내지 않는다. 눈금이 거친 파일시스템을 재현하려 들면 우리 디스크가 안 주는
    // 것을 "결함 없음" 으로 세게 된다. 대신 그 눈금이 만드는 **상태를 직접 세운다** —
    // 쓰기 전후의 mtime 을 같은 값으로 찍고, 창을 열고 닫는 것은 그 mtime 의 나이로 정한다.
    // 그래서 이 시험들은 `sleep` 이 하나도 없고 어느 기계에서든 같은 답을 낸다.

    /// 오래된 mtime(창 밖) 상태를 만든다 — 닫개가 무장하지 않는다.
    fn stale_stamp() -> std::time::SystemTime {
        std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)
    }

    #[test]
    fn arm_settle_opens_for_a_fresh_write_and_stays_shut_for_an_old_one() {
        let fresh = Some((1, std::time::SystemTime::now()));
        assert!(
            StatGatedDigest::arm_settle(fresh).is_some(),
            "방금 쓰인 파일은 같은 눈금에 두 번째 쓰기가 올 수 있다 — 창을 연다"
        );
        let old = Some((1, stale_stamp()));
        assert!(
            StatGatedDigest::arm_settle(old).is_none(),
            "눈금 창을 지난 파일은 열지 않는다 — 정상상태 비용이 stat 하나로 남아야 한다"
        );
        assert!(
            StatGatedDigest::arm_settle(None).is_none(),
            "stat 을 못 읽으면 무장할 근거가 없다"
        );
    }

    /// ⓪ **대조군** — 아무것도 안 바뀌면 조용해야 한다.
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

    /// **오탐이 없다** — `touch`·`rsync` 처럼 시계만 올라가고 내용이 같으면 리로드하지 않는다.
    ///
    /// 이것이 이 판정자가 시계만 쓰지 않는 이유다. 이 자리에서 오탐 한 번의 값은 읽기(ms)가
    /// 아니라 디코드(수백 ms ~ 초)다 — 타입 문서의 실측표.
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

    /// **닫개가 실제로 닫는다** — 길이도 mtime 도 그대로인데 내용만 바뀐 두 번째 쓰기를,
    /// 창이 열려 있는 동안에는 잡는다.
    ///
    /// 이것이 `COARSEST_OBSERVED_MTIME_TICK`(FAT32 실측 2.000 s)이 만드는 상태다:
    /// 한 눈금 안에 두 번 쓰면 `stat` 이 둘을 구분하지 못한다.
    #[test]
    fn a_second_write_in_the_same_tick_is_caught_while_the_window_is_open() {
        let path = probe_path("sg-tick");
        std::fs::write(&path, b"aaaa").unwrap();
        // 방금 쓰인 상태로 둔다 → baseline 이 닫개를 무장한다.
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
            "stat 이 못 가르는 두 번째 쓰기를 닫개가 잡아야 한다"
        );
        let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시.
    }

    /// **닫개 밖에서는 못 잡는다 — 그것이 이 판정자의 알려진 한계다.**
    ///
    /// 위 시험과 짝이다. 창이 닫힌 뒤 길이·mtime 이 모두 같은 쓰기가 오면 놓친다.
    /// 이 칸이 없으면 위 시험이 "닫개가 잡았다" 를 낼 때 그것이 **닫개 덕인지 판정자가
    /// 무엇이든 잡기 때문인지** 못 가른다. 두 시험은 짝으로만 뜻이 있다.
    ///
    /// 나중에 이 단언이 깨지면 결함을 고친 것이 아니라 **판정을 바꾼 것**이다 — 그때는
    /// 무엇이 그 창을 닫았는지 타입 문서에 적고 이 시험을 함께 고쳐라.
    #[test]
    fn the_same_write_is_missed_once_the_window_has_shut() {
        let path = probe_path("sg-shut");
        std::fs::write(&path, b"aaaa").unwrap();
        set_mtime(&path, stale_stamp()); // 창 밖 = 무장 안 함.
        let p = path.to_string_lossy().into_owned();
        let mut probe = StatGatedDigest::baseline(&p);

        std::fs::write(&path, b"bbbb").unwrap();
        set_mtime(&path, stale_stamp()); // 길이도 mtime 도 그대로.
        assert!(
            !probe.changed(&p),
            "창 밖에서는 stat 이 못 가르는 쓰기를 놓친다 — 알려진 한계"
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
