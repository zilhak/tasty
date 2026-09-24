//! surface가 없는 headless PTY의 메타데이터와 watcher가 보고한 결과를 보관한다.
//! 실제 Terminal은 TerminalStore에 있으며 등록 개수 제한과 유휴 정리는 이 레지스트리가 맡는다.
//! 세션 권한·task runner·자식 surface 목록과는 별개이며 재시작 뒤 복원하지 않는다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const DEFAULT_MAX_CONCURRENT: usize = 8;

/// 마지막 활동에서 이 시간 이상 지난 항목을 sweep_idle이 반환한다.
pub const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);

/// surface와 PTY가 같은 TerminalStore를 쓰므로 PTY 카운터를 이 경계에서 시작한다.
/// IPC 입력·메타데이터·복원 floor도 같은 경계를 사용한다. 카운터 overflow나 surface 범위 소진을 막는 상수는 아니다.
pub const PTY_ID_BASE: u32 = 0x8000_0000;

/// ID가 PTY 경계 아래인지 본다. 0이나 실제 존재 여부는 별도로 확인해야 한다.
pub const fn is_surface_id_space(id: u32) -> bool {
    id < PTY_ID_BASE
}

/// watcher의 wait_fn이 반환한 결과. 실제 child.wait 오류도 호출자가 None·false로 전달할 수 있다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExit {
    /// 종료 코드를 얻지 못했으면 None이다.
    pub code: Option<i32>,
    pub success: bool,
}

impl PtyExit {
    pub fn from_status(code: Option<i32>, success: bool) -> Self {
        Self { code, success }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtySpawnError {
    LimitReached { current: usize, max: usize },
}

impl std::fmt::Display for PtySpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PtySpawnError::LimitReached { current, max } => write!(
                f,
                "headless PTY concurrency limit reached ({current}/{max})"
            ),
        }
    }
}

impl std::error::Error for PtySpawnError {}

/// watcher가 마지막으로 기록한 진행 상태. 검사 진단용이며 자식 프로세스의 생사를 직접 측정하지 않는다.
/// 상태 기록은 제품 빌드에서도 하고 읽는 API만 test 전용이다.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchPhase {
    /// 아직 등록하지 않았거나 마지막 스레드 생성 시도가 실패했다.
    NotAttached,
    /// 스레드 생성 시도를 시작했으며 wait_fn 진입 표시는 아직 관측하지 못했다.
    Spawned,
    /// wait_fn 호출 직전 기록한다. 결과 게시를 위한 락 대기 중에도 이 상태일 수 있다.
    Waiting,
    /// wait_fn 결과를 게시하고 대기자에게 알린 뒤 기록한다.
    Reaped,
}

#[cfg(test)]
impl WatchPhase {
    fn from_raw(v: u8) -> Self {
        match v {
            1 => Self::Spawned,
            2 => Self::Waiting,
            3 => Self::Reaped,
            _ => Self::NotAttached,
        }
    }
}

const PHASE_NOT_ATTACHED: u8 = 0;
const PHASE_SPAWNED: u8 = 1;
const PHASE_WAITING: u8 = 2;
const PHASE_REAPED: u8 = 3;

/// PTY 메타데이터와 결과 저장 공간. Terminal 인스턴스는 같은 ID로 별도 store가 보관한다.
pub struct PtyEntry {
    pub id: u32,
    /// 통계에 사용할 요청자의 AgentId. 이 문자열 자체가 인증된 신원을 증명하지는 않는다.
    pub owner_agent_id: String,
    pub cwd: Option<String>,
    pub command: Vec<String>,
    created_at: Instant,
    /// touch가 기록한 마지막 활동 시각. idle TTL의 기준이다.
    last_activity: Instant,
    /// watcher가 결과를 저장하고 Condvar로 알린다. 제품 IPC는 대기하지 않고 snapshot을 읽는다.
    exit_result: Arc<(Mutex<Option<PtyExit>>, Condvar)>,
    /// 엔트리를 제거해 핸들이 drop돼도 watcher 스레드는 계속 실행된다. 여기서는 join하지 않는다.
    _watcher: Option<JoinHandle<()>>,
    watch_phase: Arc<AtomicU8>,
}

/// 모든 exit cell이 최초 poison 보고 플래그를 공유한다. 개별 cell의 poison 상태까지 공유하는 것은 아니다.
static EXIT_CELL_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

const EXIT_CELL_WHAT: &str = "pty exit cell";

impl PtyEntry {
    /// 게시된 결과를 읽는다. None은 결과 미관측이며 자식이 실행 중이라는 보장은 아니다.
    /// 임계구역은 Option<PtyExit> 읽기·쓰기라 poison 뒤에도 값을 복구한다. 기본 None으로 덮지 않는다.
    pub fn exit(&self) -> Option<PtyExit> {
        crate::poison::recover_mutex(
            self.exit_result.0.lock(),
            EXIT_CELL_WHAT,
            &EXIT_CELL_POISON_REPORTED,
        )
        .clone()
    }

    /// 결과가 게시됐는지 확인한다. 호출자의 wait 오류 결과도 포함될 수 있다.
    pub fn has_exited(&self) -> bool {
        self.exit().is_some()
    }

    #[cfg(test)]
    pub fn watch_phase(&self) -> WatchPhase {
        WatchPhase::from_raw(self.watch_phase.load(Ordering::Acquire))
    }

    // 이유: 생성 시각을 확인하는 검사용 접근자다.
    #[allow(dead_code)]
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    #[allow(dead_code)]
    pub fn last_activity(&self) -> Instant {
        self.last_activity
    }
}

#[derive(Debug, Clone)]
pub struct PtySpawnSpec {
    pub owner_agent_id: String,
    pub cwd: Option<String>,
    pub command: Vec<String>,
}

/// 등록된 PTY 항목을 관리한다. 실제 프로세스 종료와 Terminal store 정리는 호출자가 맡는다.
pub struct PtyRegistry {
    entries: HashMap<u32, PtyEntry>,
    /// engine이 같은 발급기를 사용해 PTY ID가 중복되지 않게 한다.
    next_id: std::sync::Arc<AtomicU32>,
    max_concurrent: usize,
    idle_ttl: Duration,
}

impl Default for PtyRegistry {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            next_id: std::sync::Arc::new(AtomicU32::new(PTY_ID_BASE)),
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            idle_ttl: DEFAULT_IDLE_TTL,
        }
    }
}

impl PtyRegistry {
    /// 검사에서 사용할 독립 카운터. 제품 engine은 with_counter로 공유 카운터를 넘긴다.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_counter(next_id: std::sync::Arc<AtomicU32>) -> Self {
        Self {
            next_id,
            ..Self::default()
        }
    }

    /// 검사에서 등록 한도와 TTL 경계를 바꾸기 위한 생성자.
    #[allow(dead_code)]
    pub fn with_limits(max_concurrent: usize, idle_ttl: Duration) -> Self {
        Self {
            max_concurrent,
            idle_ttl,
            ..Self::default()
        }
    }

    // 이유: 한도·TTL을 확인하는 검사용 접근자다.
    #[allow(dead_code)]
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    #[allow(dead_code)]
    pub fn idle_ttl(&self) -> Duration {
        self.idle_ttl
    }

    /// 등록 항목 수가 상한이면 거절한다. 이미 결과가 있는 항목도 제거 전까지 이 수에 포함된다.
    pub fn register(&mut self, spec: PtySpawnSpec, now: Instant) -> Result<u32, PtySpawnError> {
        if self.entries.len() >= self.max_concurrent {
            return Err(PtySpawnError::LimitReached {
                current: self.entries.len(),
                max: self.max_concurrent,
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.entries.insert(
            id,
            PtyEntry {
                id,
                owner_agent_id: spec.owner_agent_id,
                cwd: spec.cwd,
                command: spec.command,
                created_at: now,
                last_activity: now,
                exit_result: Arc::new((Mutex::new(None), Condvar::new())),
                _watcher: None,
                watch_phase: Arc::new(AtomicU8::new(PHASE_NOT_ATTACHED)),
            },
        );
        Ok(id)
    }

    /// wait_fn을 별도 스레드에서 실행하고 반환값을 게시한다. 대상 부재나 스레드 생성 실패면 false다.
    /// 같은 ID의 중복 watcher 등록이나 wait_fn의 panic은 이 함수에서 막지 않는다.
    pub fn attach_exit_watcher<F>(&mut self, id: u32, wait_fn: F) -> bool
    where
        F: FnOnce() -> PtyExit + Send + 'static,
    {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        let cell = entry.exit_result.clone();
        let phase = entry.watch_phase.clone();
        // worker가 먼저 기록한 상태를 덮지 않도록 spawn 시도 전에 표시한다.
        phase.store(PHASE_SPAWNED, Ordering::Release);
        let thread_phase = phase.clone();
        let handle = thread::Builder::new()
            .name(format!("pty-exit-watcher-{id}"))
            .spawn(move || {
                thread_phase.store(PHASE_WAITING, Ordering::Release);
                let outcome = wait_fn();
                // poison 때문에 이미 받은 결과를 버리지 않도록 같은 복구 정책을 사용한다.
                *crate::poison::recover_mutex(
                    cell.0.lock(),
                    EXIT_CELL_WHAT,
                    &EXIT_CELL_POISON_REPORTED,
                ) = Some(outcome);
                // 대기자는 락 안에서 값을 확인한 뒤 기다리므로 확인과 wait 사이의 통지 누락을 피한다.
                cell.1.notify_all();
                thread_phase.store(PHASE_REAPED, Ordering::Release);
            });
        match handle {
            Ok(h) => {
                entry._watcher = Some(h);
                true
            }
            Err(e) => {
                phase.store(PHASE_NOT_ATTACHED, Ordering::Release);
                tracing::warn!("pty exit watcher spawn failed for {id}: {e}");
                false
            }
        }
    }

    /// 검사에서 결과를 기다린다. IPC pty.wait는 이 함수 대신 exit snapshot을 읽는다.
    /// 통지나 timeout 뒤 값을 다시 확인한다. None은 미존재 또는 확인 시점까지 결과가 없었다는 뜻이다.
    /// mutex 획득·재획득과 스케줄 지연까지 timeout으로 제한하지는 않는다.
    #[cfg(test)]
    pub fn wait_for_exit(&self, id: u32, timeout: std::time::Duration) -> Option<PtyExit> {
        let pair = self.entries.get(&id)?.exit_result.clone();
        let (lock, cvar) = &*pair;
        let deadline = Instant::now() + timeout;
        let mut guard =
            crate::poison::recover_mutex(lock.lock(), EXIT_CELL_WHAT, &EXIT_CELL_POISON_REPORTED);
        while guard.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            // Condvar가 같은 락을 놓고 기다렸다가 다시 잡은 뒤 값을 확인한다.
            guard = match cvar.wait_timeout(guard, remaining) {
                Ok((g, _)) => g,
                // Condvar 재획득에서 만난 poison도 공용 exit cell 보고 플래그를 쓴다.
                Err(p) => {
                    crate::poison::recover_poisoned(p, EXIT_CELL_WHAT, &EXIT_CELL_POISON_REPORTED).0
                }
            };
        }
        guard.clone()
    }

    pub fn touch(&mut self, id: u32, now: Instant) -> bool {
        match self.entries.get_mut(&id) {
            Some(e) => {
                e.last_activity = now;
                true
            }
            None => false,
        }
    }

    pub fn get(&self, id: u32) -> Option<&PtyEntry> {
        self.entries.get(&id)
    }

    pub fn contains(&self, id: u32) -> bool {
        self.entries.contains_key(&id)
    }

    // 이유: 제품 목록 조회는 iter를 쓰며 이 개수 접근자는 검사에서 사용한다.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 등록된 ID 목록. 결과가 게시된 항목도 포함하며 순서는 보장하지 않는다.
    #[allow(dead_code)]
    pub fn ids(&self) -> Vec<u32> {
        self.entries.keys().copied().collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &PtyEntry> {
        self.entries.values()
    }

    /// 메타데이터만 제거한다. 반환된 핸들이 drop돼도 watcher는 wait_fn을 계속 실행한다.
    pub fn remove(&mut self, id: u32) -> Option<PtyEntry> {
        self.entries.remove(&id)
    }

    /// 마지막 활동에서 TTL 이상 지난 항목을 제거하고 ID를 반환한다. Terminal·waker 정리는 호출자가 맡는다.
    /// GUI는 주기 타이머에서도 호출한다. 헤드리스에는 이 주기 호출이 없어 새 spawn 때의 정리에 의존한다.
    pub fn sweep_idle(&mut self, now: Instant) -> Vec<u32> {
        let ttl = self.idle_ttl;
        let expired: Vec<u32> = self
            .entries
            .iter()
            .filter(|(_, e)| now.duration_since(e.last_activity) >= ttl)
            .map(|(id, _)| *id)
            .collect();
        for id in &expired {
            self.entries.remove(id);
        }
        expired
    }
}

#[cfg(test)]
// 검사에서 종료 신호·정리의 반환값을 의도적으로 무시하는 경로가 있다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    fn spec(cmd: &[&str]) -> PtySpawnSpec {
        PtySpawnSpec {
            owner_agent_id: "agent-1".into(),
            cwd: None,
            command: cmd.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = PtyRegistry::new();
        let now = Instant::now();
        let id = reg.register(spec(&["echo", "hi"]), now).unwrap();
        assert!(id >= PTY_ID_BASE, "pty id must be in disjoint high range");
        assert_eq!(reg.len(), 1);
        assert!(reg.contains(id));
        let e = reg.get(id).unwrap();
        assert_eq!(e.command, vec!["echo".to_string(), "hi".to_string()]);
        assert_eq!(e.owner_agent_id, "agent-1");
        assert!(!e.has_exited());
    }

    #[test]
    fn registries_sharing_a_counter_never_issue_the_same_id() {
        let shared = std::sync::Arc::new(AtomicU32::new(PTY_ID_BASE));
        let mut a = PtyRegistry::with_counter(std::sync::Arc::clone(&shared));
        let mut b = PtyRegistry::with_counter(std::sync::Arc::clone(&shared));
        let now = Instant::now();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..3 {
            assert!(seen.insert(a.register(spec(&["a"]), now).unwrap()));
            assert!(seen.insert(b.register(spec(&["b"]), now).unwrap()));
        }
        assert_eq!(seen.len(), 6, "공유 카운터인데 id 가 겹쳤다: {seen:?}");
        assert!(seen.iter().all(|id| *id >= PTY_ID_BASE));

        // 독립 카운터는 같은 시작값을 발급하므로 공유 카운터 검사의 반례가 된다.
        let mut c = PtyRegistry::new();
        let mut d = PtyRegistry::new();
        assert_eq!(
            c.register(spec(&["c"]), now).unwrap(),
            d.register(spec(&["d"]), now).unwrap(),
            "독립 카운터는 같은 시작 ID를 발급한다"
        );
    }

    #[test]
    fn ids_are_disjoint_from_surface_space() {
        let mut reg = PtyRegistry::new();
        let now = Instant::now();
        let a = reg.register(spec(&["a"]), now).unwrap();
        let b = reg.register(spec(&["b"]), now).unwrap();
        assert_eq!(a, PTY_ID_BASE);
        assert_eq!(b, PTY_ID_BASE + 1);
        for id in [a, b] {
            assert!(
                !is_surface_id_space(id),
                "pty id {id} 가 surface id 공간을 침범했다"
            );
        }
        assert!(!is_surface_id_space(PTY_ID_BASE));
        assert!(is_surface_id_space(PTY_ID_BASE - 1));
    }

    #[test]
    fn surface_counter_starts_inside_surface_space() {
        let ids = crate::core::state::IdGenerator::new();
        assert!(is_surface_id_space(ids.next_surface()));
    }

    #[test]
    fn register_fails_when_limit_reached() {
        let mut reg = PtyRegistry::with_limits(2, DEFAULT_IDLE_TTL);
        let now = Instant::now();
        assert!(reg.register(spec(&["a"]), now).is_ok());
        assert!(reg.register(spec(&["b"]), now).is_ok());
        let err = reg.register(spec(&["c"]), now).unwrap_err();
        assert_eq!(err, PtySpawnError::LimitReached { current: 2, max: 2 });
        assert_eq!(reg.len(), 2, "실패한 spawn 은 등록되지 않아야 한다");
    }

    #[test]
    fn removing_frees_a_concurrency_slot() {
        let mut reg = PtyRegistry::with_limits(1, DEFAULT_IDLE_TTL);
        let now = Instant::now();
        let id = reg.register(spec(&["a"]), now).unwrap();
        assert!(reg.register(spec(&["b"]), now).is_err());
        assert!(reg.remove(id).is_some());
        assert!(
            reg.register(spec(&["b"]), now).is_ok(),
            "제거 후 슬롯이 비어 재등록 가능해야 한다"
        );
    }

    #[test]
    fn sweep_removes_idle_beyond_ttl() {
        let ttl = Duration::from_secs(300);
        let mut reg = PtyRegistry::with_limits(8, ttl);
        let base = Instant::now();
        let id = reg.register(spec(&["sleep"]), base).unwrap();

        let within = base + Duration::from_secs(299);
        assert!(reg.sweep_idle(within).is_empty());
        assert!(reg.contains(id));

        let beyond = base + Duration::from_secs(301);
        let removed = reg.sweep_idle(beyond);
        assert_eq!(removed, vec![id]);
        assert!(!reg.contains(id));
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn touch_resets_idle_timer() {
        let ttl = Duration::from_secs(300);
        let mut reg = PtyRegistry::with_limits(8, ttl);
        let base = Instant::now();
        let id = reg.register(spec(&["a"]), base).unwrap();

        let activity = base + Duration::from_secs(250);
        assert!(reg.touch(id, activity));

        let now = base + Duration::from_secs(301);
        assert!(reg.sweep_idle(now).is_empty());
        assert!(reg.contains(id));

        let later = activity + Duration::from_secs(301);
        assert_eq!(reg.sweep_idle(later), vec![id]);
    }

    #[test]
    fn exit_watcher_captures_real_exit_code() {
        let mut reg = PtyRegistry::new();
        let now = Instant::now();
        let id = reg.register(spec(&["exit-3"]), now).unwrap();
        assert!(!reg.get(id).unwrap().has_exited());

        // std::process 자식으로 watcher의 wait 결과 전달을 확인한다. 실제 PTY 생성 검사는 아니다.
        let ok = reg.attach_exit_watcher(id, || {
            #[cfg(windows)]
            let mut child = std::process::Command::new("cmd")
                .args(["/C", "exit 3"])
                .spawn()
                .expect("spawn");
            #[cfg(not(windows))]
            let mut child = std::process::Command::new("sh")
                .args(["-c", "exit 3"])
                .spawn()
                .expect("spawn");
            let status = child.wait().expect("wait");
            PtyExit::from_status(status.code(), status.success())
        });
        assert!(ok);

        let mut captured = None;
        for _ in 0..600 {
            if let Some(e) = reg.get(id).unwrap().exit() {
                captured = Some(e);
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let exit = captured.expect("exit code should be captured within timeout");
        assert_eq!(exit.code, Some(3));
        assert!(!exit.success);
        assert!(reg.get(id).unwrap().has_exited());
    }

    #[test]
    fn the_watcher_phase_says_why_the_cell_is_still_empty() {
        let mut reg = PtyRegistry::new();
        let id = reg.register(spec(&["blocks"]), Instant::now()).unwrap();

        assert_eq!(reg.get(id).unwrap().watch_phase(), WatchPhase::NotAttached);

        // 신호를 기다리는 wait_fn으로 결과가 아직 게시되지 않은 상태를 만든다.
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        assert!(reg.attach_exit_watcher(id, move || {
            // 이유: 값이 아니라 대기 해제가 목적이므로 채널 종료 오류도 무시한다.
            let _ = release_rx.recv();
            PtyExit::from_status(Some(0), true)
        }));

        let mut reached = false;
        for _ in 0..600 {
            if reg.get(id).unwrap().watch_phase() == WatchPhase::Waiting {
                reached = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(reached, "watcher의 Waiting 상태를 관측하지 못했다");
        assert!(reg.get(id).unwrap().exit().is_none());

        release_tx.send(()).expect("release");
        let mut done = false;
        for _ in 0..600 {
            if reg.get(id).unwrap().watch_phase() == WatchPhase::Reaped {
                done = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(done, "결과 게시 뒤 Reaped 상태를 관측하지 못했다");
        assert!(reg.get(id).unwrap().has_exited());
    }

    /// 대기 결과 None을 받은 뒤에도 나중에 결과가 게시될 수 있음을 재현한다.
    /// 이 한 시나리오가 모든 timeout의 원인을 판별하는 것은 아니다.
    #[test]
    fn the_reaped_but_unseen_branch_is_a_late_child_not_a_missed_wakeup() {
        let mut reg = PtyRegistry::new();
        let id = reg.register(spec(&["late"]), Instant::now()).unwrap();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        assert!(reg.attach_exit_watcher(id, move || {
            // 이유: 값이 아니라 대기 해제가 목적이다.
            let _ = release_rx.recv();
            PtyExit::from_status(Some(0), true)
        }));

        assert!(reg.wait_for_exit(id, Duration::from_millis(50)).is_none());

        release_tx.send(()).expect("release");
        let mut done = false;
        for _ in 0..600 {
            if reg.get(id).unwrap().watch_phase() == WatchPhase::Reaped {
                done = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(done, "대기 해제 뒤 Reaped 상태를 관측하지 못했다");

        assert_eq!(reg.get(id).unwrap().watch_phase(), WatchPhase::Reaped);
        assert!(reg.get(id).unwrap().exit().is_some());
    }

    /// 게시된 결과를 반환하는지 반복 확인한다. 통지 지연 자체는 측정하지 않는다.
    /// 고정 대기 시간 안에 실행될지는 스케줄에 따라 달라지므로 실패만으로 결과 유실을 확정할 수 없다.
    #[test]
    fn a_fill_is_never_lost_even_if_the_wakeup_never_comes() {
        const TRIALS: usize = 100;
        let mut missed = 0usize;
        for _ in 0..TRIALS {
            let mut reg = PtyRegistry::new();
            let id = reg.register(spec(&["parked"]), Instant::now()).unwrap();
            let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
            assert!(reg.attach_exit_watcher(id, move || {
                // 이유: 값이 아니라 대기 해제가 목적이다.
                let _ = release_rx.recv();
                PtyExit::from_status(Some(0), true)
            }));
            let releaser = thread::spawn(move || {
                thread::sleep(Duration::from_millis(1));
                // 이유: 대기가 끝나 수신자가 사라졌으면 더 깨울 필요가 없다.
                let _ = release_tx.send(());
            });
            if reg.wait_for_exit(id, Duration::from_secs(5)).is_none() {
                missed += 1;
            }
            releaser.join().expect("releaser");
        }
        assert_eq!(
            missed, 0,
            "{TRIALS}회 중 {missed}회에서 대기 시간 안에 결과를 받지 못했다"
        );
    }

    #[test]
    fn attach_exit_watcher_on_missing_id_is_false() {
        let mut reg = PtyRegistry::new();
        assert!(!reg.attach_exit_watcher(12345, || PtyExit::from_status(Some(0), true)));
    }

    #[test]
    fn exit_cell_survives_a_poisoned_lock() {
        let mut reg = PtyRegistry::new();
        let id = reg
            .register(spec(&["echo", "hi"]), Instant::now())
            .expect("register");

        let cell = reg.get(id).expect("entry").exit_result.clone();
        let poisoner = cell.clone();
        // 이유: poison을 만들기 위해 panic한 스레드의 join 오류를 무시한다.
        let _ = thread::spawn(move || {
            let _guard = poisoner.0.lock().expect("fresh lock");
            panic!("poison the exit cell on purpose");
        })
        .join();
        assert!(
            cell.0.is_poisoned(),
            "검사할 exit cell 락이 poison 상태여야 한다"
        );

        assert!(reg.get(id).expect("entry").exit().is_none());
        assert!(!reg.get(id).expect("entry").has_exited());

        assert!(reg.attach_exit_watcher(id, || PtyExit::from_status(Some(7), false)));
        let mut captured = None;
        for _ in 0..600 {
            if let Some(e) = reg.get(id).expect("entry").exit() {
                captured = Some(e);
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let exit = captured.expect("poison 이후에도 종료 결과가 채워져야 한다");
        assert_eq!(exit.code, Some(7));
        assert!(reg.get(id).expect("entry").has_exited());
    }
}
