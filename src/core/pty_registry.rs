//! 호스트 headless PTY registry (TODO 18 / `pty.*` primitive — 18-a: Registry + IO).
//!
//! 에이전트가 Surface(Tab) 없이 백그라운드에서 굴리는 **headless PTY** 의 메타데이터와
//! 실제 종료코드(exit-code)를 호스트가 단일 SoT 로 보관한다. `child_terminal.rs` 의
//! [`ChildTerminalRegistry`](crate::core::child_terminal::ChildTerminalRegistry) 와
//! 병렬 구조지만 역할이 다르다:
//!
//! - `child_terminal`: `terminal.spawn` 으로 만든 **자식 터미널 surface** 의 parent/index/
//!   idle 매핑 (ADR-0040 occupancy, GUI 에 보이는 장수명 child-agent). Surface 가 있다.
//! - `pty_registry`(본 모듈): Surface 가 아예 없는 1 회성 자동화 PTY. `child.wait()` 로
//!   **진짜 exit-code** 를 잡고, GUI 안전망(닫기 버튼)이 없으므로 **동시 개수 상한 +
//!   idle TTL** 로 좀비 누적을 스스로 막는다.
//!
//! **경계 (레지스트리 파편화 방지)**: `session.rs` 의 `SessionStore`(권한 토큰),
//! `runner_host.rs` 의 `shell_children`(DAG 러너 subprocess), `child_terminal.rs`(자식
//! 터미널 surface) 와 모두 다른 서브시스템이며 통합 대상이 아니다.
//!
//! host-IPC-free — 단위 테스트 가능. 시간 의존 연산([`register`](PtyRegistry::register)/
//! [`touch`](PtyRegistry::touch)/[`sweep_idle`](PtyRegistry::sweep_idle))은 `now: Instant`
//! 를 주입받아 테스트가 5 분 경과를 sleep 없이 재현한다.
//!
//! **비영속**: headless PTY 자식 프로세스는 호스트와 수명을 같이하므로(재부팅 후 살아있는
//! PTY 는 없다) `child_terminal` 과 달리 JSON 영속화하지 않는다.

// 18-a(Registry + IO)는 데이터 모델·exit-code 캡처만 만든다 — 이 API 의 실제 호출자
// (IPC 핸들러 `pty.spawn`/`write`/`read`/`wait`/`kill`/`list`, 상태바 카운트, sweep
// 배선)는 18-b/18-c 에서 붙는다. soft_occupancy.rs 와 동일하게 배선 전까지 dead_code
// 를 억제한다(18-b 소비 시점에 제거).
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// 동시 headless PTY 개수 기본 상한 (사용자 확정, 2026-07-14). `rate_limit.rs` 철학대로
/// 코드에 기본값을 박아두되 [`PtyRegistry::with_limits`] 로 override 가능하다.
pub const DEFAULT_MAX_CONCURRENT: usize = 8;

/// idle(무 IO 활동) 상태가 이 시간을 넘으면 [`sweep_idle`](PtyRegistry::sweep_idle) 이
/// 정리 대상으로 반환한다. 기본 5 분 (사용자 확정, 2026-07-14).
pub const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);

/// PTY id 시작값. Surface id 카운터(1 부터 증가)와 **완전히 겹치지 않는 disjoint 고범위**
/// 에서 발급한다. task 결정("별도 카운터, 충돌 위험 없음")을 구체화한 것 — 18-b 가
/// headless `Terminal` 을 surface id 와 같은 u32 공간의 `TerminalStore` 에 재사용 등록해도
/// 실 surface id 와 절대 충돌하지 않는다(surface id 가 2^31 까지 자랄 일은 없다).
pub const PTY_ID_BASE: u32 = 0x8000_0000;

/// headless PTY 자식의 실제 종료 결과. `runner_host.rs` 의
/// `shell_outcome_from_status(pid, code, success)` 와 동형(pid 는 registry 가 이미 id 로
/// 귀속하므로 생략).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExit {
    /// OS exit code. 시그널 종료 등 code 가 없는 경우 `None`.
    pub code: Option<i32>,
    /// `ExitStatus::success()` — code == 0.
    pub success: bool,
}

impl PtyExit {
    pub fn from_status(code: Option<i32>, success: bool) -> Self {
        Self { code, success }
    }
}

/// [`PtyRegistry::register`] 실패 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtySpawnError {
    /// 동시 개수 상한 초과. spawn 요청을 실패시켜야 하며 panic 하지 않는다.
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

/// headless PTY 하나의 메타데이터 + exit-code 캡처 cell. `Terminal` 인스턴스 자체는
/// 담지 않는다 — 18-b 에서 동일 id 로 `engine.terminals`(`TerminalStore`)가 보관한다.
pub struct PtyEntry {
    pub id: u32,
    /// AgentId — cap/telemetry 귀속용(위조 가능한 잠정 모델, agent-identification.md).
    pub owner_agent_id: String,
    pub cwd: Option<String>,
    pub command: Vec<String>,
    /// 생성 시각(monotonic). age 계산·정렬용.
    created_at: Instant,
    /// 마지막 IO 활동 시각(monotonic). idle TTL 판정 기준 — read/write 시 `touch`.
    last_activity: Instant,
    /// watcher-thread 가 `child.wait()` 완료 시 채우는 cell(`runner_host.rs` 패턴 이식).
    exit_result: Arc<Mutex<Option<PtyExit>>>,
    /// exit watcher 스레드 핸들. 살려두기만 하면 되므로 join 하지 않는다(detached).
    _watcher: Option<JoinHandle<()>>,
}

impl PtyEntry {
    /// 캡처된 종료 결과(있으면). watcher 가 아직 안 채웠으면 `None`(=실행 중).
    pub fn exit(&self) -> Option<PtyExit> {
        self.exit_result.lock().ok().and_then(|g| g.clone())
    }

    /// 자식이 종료돼 exit-code 가 잡혔는가.
    pub fn has_exited(&self) -> bool {
        self.exit().is_some()
    }

    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    pub fn last_activity(&self) -> Instant {
        self.last_activity
    }
}

/// headless PTY spawn 시 registry 에 넘기는 메타데이터.
#[derive(Debug, Clone)]
pub struct PtySpawnSpec {
    pub owner_agent_id: String,
    pub cwd: Option<String>,
    pub command: Vec<String>,
}

/// headless PTY 메타데이터 registry. 동시 개수 상한 + idle TTL 로 좀비 누적을 막는다.
pub struct PtyRegistry {
    entries: HashMap<u32, PtyEntry>,
    /// Surface id 와 disjoint 한 별도 카운터([`PTY_ID_BASE`] 부터).
    next_id: AtomicU32,
    max_concurrent: usize,
    idle_ttl: Duration,
}

impl Default for PtyRegistry {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            next_id: AtomicU32::new(PTY_ID_BASE),
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            idle_ttl: DEFAULT_IDLE_TTL,
        }
    }
}

impl PtyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 상한/TTL override 생성자 — `rate_limit.rs` 철학(기본값은 박되 호출자 지정 가능).
    pub fn with_limits(max_concurrent: usize, idle_ttl: Duration) -> Self {
        Self {
            max_concurrent,
            idle_ttl,
            ..Self::default()
        }
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    pub fn idle_ttl(&self) -> Duration {
        self.idle_ttl
    }

    /// 새 headless PTY 를 등록하고 발급된 id 를 반환한다. 동시 개수가 상한에 도달했으면
    /// [`PtySpawnError::LimitReached`] 로 **실패**시킨다(panic 하지 않는다).
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
                exit_result: Arc::new(Mutex::new(None)),
                _watcher: None,
            },
        );
        Ok(id)
    }

    /// `id` 의 PTY 자식이 종료될 때까지 기다리는 watcher-thread 를 건다. `wait_fn` 은
    /// 소유한 `portable_pty::Child`(또는 임의의 waitable)를 close-over 해 `child.wait()`
    /// 로 실제 종료코드를 뽑아 [`PtyExit`] 로 돌려주면 된다 — registry 는 `portable_pty`
    /// 타입에 직접 의존하지 않는다(closure 로 decouple). 완료 시 결과를 entry 의 cell 에
    /// 채운다(`runner_host.rs:429-451` 와 동형). 미존재 id 면 `false`.
    pub fn attach_exit_watcher<F>(&mut self, id: u32, wait_fn: F) -> bool
    where
        F: FnOnce() -> PtyExit + Send + 'static,
    {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        let cell = entry.exit_result.clone();
        let handle = thread::Builder::new()
            .name(format!("pty-exit-watcher-{id}"))
            .spawn(move || {
                let outcome = wait_fn();
                if let Ok(mut g) = cell.lock() {
                    *g = Some(outcome);
                }
            });
        match handle {
            Ok(h) => {
                entry._watcher = Some(h);
                true
            }
            Err(e) => {
                tracing::warn!("pty exit watcher spawn failed for {id}: {e}");
                false
            }
        }
    }

    /// IO 활동(read/write) 발생 시 idle 타이머를 리셋한다. 미존재 id 면 `false`.
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

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 살아있는 headless PTY id 목록(`pty.list` 용, 18-b). 순서 미보장.
    pub fn ids(&self) -> Vec<u32> {
        self.entries.keys().copied().collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &PtyEntry> {
        self.entries.values()
    }

    /// 명시적 제거(`pty.kill`/`attach_surface` 승격, 18-b·18-c). 반환된 entry 의 watcher
    /// 핸들은 drop 되지만 스레드는 detached 라 자식 wait 를 계속 수행한다.
    pub fn remove(&mut self, id: u32) -> Option<PtyEntry> {
        self.entries.remove(&id)
    }

    /// idle 이 TTL 을 초과한 항목을 제거하고 그 id 들을 반환한다(정리된 순서 미보장).
    /// GUI 안전망이 없는 headless PTY 의 좀비 누적 방지 — 에이전트가 `pty.kill`/`pty.wait`
    /// 를 잊어도 호스트가 스스로 회수한다. 반환 id 로 18-b 가 실제 자식 kill/Terminal
    /// 제거를 이어서 처리한다. `reconcile_with_live_surfaces`(child_terminal) 처럼 접근
    /// 시점 동기 정리로 호출한다.
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
    fn ids_are_disjoint_from_surface_space() {
        // Surface id 는 1 부터 증가한다 — pty id 는 그 공간과 절대 겹치지 않아야 한다.
        let mut reg = PtyRegistry::new();
        let now = Instant::now();
        let a = reg.register(spec(&["a"]), now).unwrap();
        let b = reg.register(spec(&["b"]), now).unwrap();
        assert_eq!(a, PTY_ID_BASE);
        assert_eq!(b, PTY_ID_BASE + 1);
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

        // TTL 이내: 정리 안 됨.
        let within = base + Duration::from_secs(299);
        assert!(reg.sweep_idle(within).is_empty());
        assert!(reg.contains(id));

        // TTL 초과: 정리됨.
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

        // 활동 발생: idle 타이머 리셋.
        let activity = base + Duration::from_secs(250);
        assert!(reg.touch(id, activity));

        // 최초 등록으로부터 301s 지났지만 활동으로부터는 51s 뿐 — 정리 안 됨.
        let now = base + Duration::from_secs(301);
        assert!(reg.sweep_idle(now).is_empty());
        assert!(reg.contains(id));

        // 활동으로부터 TTL 초과 — 정리됨.
        let later = activity + Duration::from_secs(301);
        assert_eq!(reg.sweep_idle(later), vec![id]);
    }

    #[test]
    fn exit_watcher_captures_real_exit_code() {
        let mut reg = PtyRegistry::new();
        let now = Instant::now();
        let id = reg.register(spec(&["exit-3"]), now).unwrap();
        assert!(!reg.get(id).unwrap().has_exited());

        // 실 프로세스를 spawn 해 non-zero 종료코드를 watcher 스레드가 잡는지 검증한다.
        // (portable_pty::Child 대신 std::process::Child 로 동일 wait() 계약을 확인 —
        // registry 는 waitable 종류에 무관하다.)
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

        // watcher 스레드가 cell 을 채울 때까지 bounded poll(최대 ~3s).
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
    fn attach_exit_watcher_on_missing_id_is_false() {
        let mut reg = PtyRegistry::new();
        assert!(!reg.attach_exit_watcher(12345, || PtyExit::from_status(Some(0), true)));
    }
}
