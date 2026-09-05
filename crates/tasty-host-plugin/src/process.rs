//! Plugin 자식 프로세스 + 호스트와의 양방향 채널.
//!
//! `PluginProcess::spawn(...)`는:
//! 1. 토큰 생성
//! 2. 자식 프로세스 spawn (env로 host port + token + plugin id 전달, stdout/stderr는 로그 파일)
//! 3. listener에서 token 매칭된 connection 수신 (timeout 10s)
//! 4. 송신/수신 스레드 가동 → mpsc 채널로 호스트 메인 루프에 노출
//!
//! plugin이 응답할 때마다 `last_pong`이 갱신된다. 헬스체크는 `since_last_pong()` 비교.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use tasty_plugin_protocol::{HandleChannelMessage, PixelRect, SharedBufferId};

use crate::handle_channel::{HandleListener, HandleStream, HandleStreamReader};
use crate::listener::HostListener;
use crate::protocol::{PluginEvent, PluginRequest, PluginResponse};
use tasty_plugin_manifest::{HOST_API_VERSION, PluginPackage};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

// plugin 프로세스 상태를 지키는 내부 락들의 poison 보고 플래그(각 첫 1 회만).
//
// 셋 다 임계구역이 자료구조/값 조작뿐이라(타임스탬프 · 상태 enum · dirty 맵) 락을 든 채
// 죽은 스레드가 불변식을 깨지 않는다 — 복구가 맞다. 조용히 삼키면: pong 갱신이 유실돼
// plugin 이 죽은 것으로 오판돼 kill 되고(last_pong), aux 채널이 있는데 없는 것으로
// 보이며(handle_state), 프레임 dirty 영역이 통째로 사라진다(dirty_rects). 자유 함수에서도
// 쓰므로 모듈 static 으로 둔다.
static LAST_PONG_POISONED: AtomicBool = AtomicBool::new(false);
const LAST_PONG_WHAT: &str = "plugin last-pong timestamp";
static HANDLE_STATE_POISONED: AtomicBool = AtomicBool::new(false);
const HANDLE_STATE_WHAT: &str = "plugin aux handle stream state";
static DIRTY_RECTS_POISONED: AtomicBool = AtomicBool::new(false);
const DIRTY_RECTS_WHAT: &str = "plugin dirty-rects map";

// HandleStream 락은 위 셋과 반대다 — 임계구역이 소켓에 프레임을 쓰므로 락을 든 채 죽은
// 스레드가 반쪽 프레임을 남길 수 있다. poison 을 `into_inner` 로 복구해 이어 쓰면 프레이밍이
// 깨진다(= `writer` 가 FORBIDDEN_LOCKS 에 있는 것과 같은 이유). 그래서 복구하지 않고 이
// 연산을 건너뛰되, 조용히는 아니다 — 첫 1 회 보고한다(aux_reader_loop 의 pong 과 같은 판단).
static WITH_HANDLE_STREAM_POISONED: AtomicBool = AtomicBool::new(false);

/// 보조 채널 stream을 mailbox에서 가져올 때 첫 호출 한도. plugin SDK가 HandleClient::connect
/// 완료 → 호스트 accept thread가 stream을 우편함에 채울 때까지 ms 단위 정도면 충분하지만,
/// startup 직후 호출 가능성을 고려해 500ms 여유.
const HANDLE_STREAM_MATERIALIZE_TIMEOUT: Duration = Duration::from_millis(500);

/// 보조 핸들 채널 상태 머신. spawn 시점에 Pending(rx) 또는 Unavailable로 초기화되고,
/// 첫 사용 시 Pending → Ready(stream)으로 전이. Ready 전이 시 reader 스레드도 함께
/// 시작되어 plugin이 보내는 Dirty 메시지를 수신한다.
enum HandleStreamState {
    /// 아직 plugin이 보조 채널에 connect하지 않음. mailbox에서 try-recv 대기.
    Pending(mpsc::Receiver<HandleStream>),
    /// 한 번 materialize 완료. write 핸들은 Arc로 공유 — reader 스레드가 Pong을
    /// 응답할 때도 같은 stream을 쓴다.
    Ready(Arc<Mutex<HandleStream>>),
    /// 보조 채널 미지원 (handle_listener bind 실패 / Windows stub) 또는 reader
    /// 분리 실패. 향후 호출이 항상 None을 반환하도록 sticky.
    Unavailable,
}

pub struct PluginProcess {
    pub plugin_id: String,
    child: Option<Child>,
    pub req_tx: mpsc::Sender<PluginRequest>,
    pub resp_rx: mpsc::Receiver<PluginResponse>,
    pub event_rx: mpsc::Receiver<PluginEvent>,
    last_pong: Arc<Mutex<Instant>>,
    /// 보조 핸들 채널 상태. 첫 사용 시 Pending → Ready 전이하며 reader 스레드 시작.
    handle_state: Mutex<HandleStreamState>,
    /// reader 스레드가 누적하는 dirty rect. `Some(rect)`는 union된 영역, `None`은
    /// "전체 갱신" sticky flag. 호스트 main loop이 frame 합성 시 `take_dirty_rects`로
    /// drain한다.
    dirty_rects: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>>,
}

#[cfg(test)]
impl PluginProcess {
    /// 단위 테스트 전용 stub. child/last_pong 등 외부에서 접근 불가능한 필드를
    /// 합리적인 기본값으로 채운다. 송수신 채널은 dangling이라 실제로 사용하면 안 된다.
    pub(crate) fn stub_for_test(plugin_id: &str) -> Self {
        let (req_tx, _req_rx) = mpsc::channel();
        let (_resp_tx, resp_rx) = mpsc::channel();
        let (_event_tx, event_rx) = mpsc::channel();
        Self {
            plugin_id: plugin_id.into(),
            child: None,
            req_tx,
            resp_rx,
            event_rx,
            last_pong: Arc::new(Mutex::new(Instant::now())),
            handle_state: Mutex::new(HandleStreamState::Unavailable),
            dirty_rects: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl PluginProcess {
    pub fn spawn(
        package: &PluginPackage,
        listener: &HostListener,
        handle_listener: Option<&HandleListener>,
        log_dir: &Path,
        waker: tasty_terminal::waker_factory::SharedWakerFactory,
        reaper: &crate::reaper::PluginReaper,
    ) -> anyhow::Result<Self> {
        let token = generate_token();
        std::fs::create_dir_all(log_dir).ok();
        let log_path = log_dir.join(format!("{}.log", sanitize_id(&package.manifest.id)));
        let log_file = std::fs::File::create(&log_path)?;
        let log_clone = log_file.try_clone()?;
        let entry_path = package.entry_command_path();

        let (mut cmd, handle_stream_rx) = build_plugin_command(
            package,
            listener,
            handle_listener,
            reaper,
            &token,
            log_file,
            log_clone,
        );
        inject_plugin_data_env(&mut cmd, package, &log_path);

        // spawn 은 reaper 를 경유한다 — Linux 는 PDEATHSIG 가 fork 한 스레드 수명에
        // 결박되므로(단명 부트 워커에서 직접 spawn 하면 그 스레드 종료 시 plugin
        // 전원 SIGKILL) 영속 spawner 스레드에서 fork 해야 한다. 타 OS 는 직접 spawn.
        let child = reaper.spawn_bound(cmd).map_err(|e| {
            anyhow::anyhow!(
                "failed to spawn plugin '{}' ({}): {}",
                package.manifest.id,
                entry_path.display(),
                e
            )
        })?;

        // spawn 직후 자식이 살아있는 시점에 Job 에 assign(Windows). 실패해도
        // 플러그인 기능은 정상이며 수명 결박만 누락되므로 warn 후 진행한다.
        if let Err(e) = reaper.adopt(&child) {
            tracing::warn!(
                "plugin '{}' lifetime adopt failed — process not bound to host lifetime: {e}",
                package.manifest.id
            );
        }

        let stream = match listener.expect_connection(&token, HANDSHAKE_TIMEOUT) {
            Some(s) => s,
            None => {
                anyhow::bail!(
                    "plugin '{}' did not connect within {}s — log: {}",
                    package.manifest.id,
                    HANDSHAKE_TIMEOUT.as_secs(),
                    log_path.display()
                );
            }
        };

        // 보조 채널은 별도 mailbox로 받는다 — blocking하지 않는다. plugin이 connect하면
        // listener accept thread가 stream을 receiver로 넣어 둠. shared buffer 사용 시점에
        // 비로소 try_recv로 가져온다. plugin이 영영 connect 안 해도 startup 지연 0.

        let last_pong = Arc::new(Mutex::new(Instant::now()));
        let (req_tx, req_rx) = mpsc::channel::<PluginRequest>();
        let (resp_tx, resp_rx) = mpsc::channel::<PluginResponse>();
        let (event_tx, event_rx) = mpsc::channel::<PluginEvent>();

        let writer = stream.try_clone()?;
        spawn_tx_thread(&package.manifest.id, writer, req_rx)?;
        spawn_rx_thread(
            &package.manifest.id,
            stream,
            waker,
            last_pong.clone(),
            resp_tx,
            event_tx,
        )?;

        let initial_state = match handle_stream_rx {
            Some(rx) => HandleStreamState::Pending(rx),
            None => HandleStreamState::Unavailable,
        };
        Ok(Self {
            plugin_id: package.manifest.id.clone(),
            child: Some(child),
            req_tx,
            resp_rx,
            event_rx,
            last_pong,
            handle_state: Mutex::new(initial_state),
            dirty_rects: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 보조 핸들 채널 stream을 첫 호출 시 materialize한 뒤 closure로 노출한다.
    ///
    /// 첫 호출은 mailbox에서 짧은 timeout(`HANDLE_STREAM_MATERIALIZE_TIMEOUT`)으로
    /// 대기하며, 성공하면 reader 스레드를 함께 spawn해 dirty 수신을 시작한다.
    /// 이후 호출은 캐시된 stream을 lock한 뒤 closure에 넘긴다. 보조 채널이
    /// 활성화되지 않은 plugin이면 `None`.
    pub fn with_handle_stream<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut HandleStream) -> R,
    {
        let arc = self.ensure_handle_stream()?;
        let mut g = match arc.lock() {
            Ok(g) => g,
            Err(_) => {
                if !WITH_HANDLE_STREAM_POISONED.swap(true, Ordering::Relaxed) {
                    tracing::error!(
                        "plugin aux handle stream lock poisoned — skipping this shared-buffer op; \
                         a thread panicked mid-frame and continuing would corrupt framing"
                    );
                }
                return None;
            }
        };
        Some(f(&mut g))
    }

    fn ensure_handle_stream(&self) -> Option<Arc<Mutex<HandleStream>>> {
        let mut state = tasty_utils::poison::recover_mutex(
            self.handle_state.lock(),
            HANDLE_STATE_WHAT,
            &HANDLE_STATE_POISONED,
        );
        match &*state {
            HandleStreamState::Ready(arc) => return Some(arc.clone()),
            HandleStreamState::Unavailable => return None,
            HandleStreamState::Pending(_) => {}
        }
        let rx = match std::mem::replace(&mut *state, HandleStreamState::Unavailable) {
            HandleStreamState::Pending(rx) => rx,
            other => {
                *state = other;
                return None;
            }
        };
        match self.materialize_handle_stream(rx) {
            Ok(arc) => {
                *state = HandleStreamState::Ready(arc.clone());
                Some(arc)
            }
            // 재시도 가능(timeout) — rx 를 Pending 으로 되돌려 다음 호출이 이어받는다.
            Err(Some(rx)) => {
                *state = HandleStreamState::Pending(rx);
                None
            }
            // 영구 불가(disconnected / reader split 실패 / reader thread spawn 실패)
            // — state 는 이미 Unavailable 로 replace 되어 있다.
            Err(None) => None,
        }
    }

    /// Pending mailbox 에서 stream 을 꺼내 reader 스레드까지 띄운다.
    /// `Err(Some(rx))` 는 재시도 가능(timeout) — caller 가 rx 를 Pending 으로
    /// 되돌린다. `Err(None)` 은 영구 불가.
    fn materialize_handle_stream(
        &self,
        rx: mpsc::Receiver<HandleStream>,
    ) -> Result<Arc<Mutex<HandleStream>>, Option<mpsc::Receiver<HandleStream>>> {
        let stream = match rx.recv_timeout(HANDLE_STREAM_MATERIALIZE_TIMEOUT) {
            Ok(s) => s,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    "plugin '{}' handle stream not yet available",
                    self.plugin_id
                );
                return Err(Some(rx));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // accept thread가 종료됨. 영구적으로 사용 불가.
                return Err(None);
            }
        };
        let reader = match stream.reader() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "plugin '{}' handle stream reader split failed: {e}",
                    self.plugin_id
                );
                return Err(None);
            }
        };
        let arc = Arc::new(Mutex::new(stream));
        if !self.spawn_aux_reader_thread(arc.clone(), reader) {
            return Err(None);
        }
        Ok(arc)
    }

    /// aux 채널 reader 스레드를 띄운다. thread spawn 실패 시 false(caller 는
    /// 영구 Unavailable 로 처리).
    fn spawn_aux_reader_thread(
        &self,
        writer: Arc<Mutex<HandleStream>>,
        reader: HandleStreamReader,
    ) -> bool {
        let dirty = self.dirty_rects.clone();
        let plugin_id = self.plugin_id.clone();
        if let Err(e) = std::thread::Builder::new()
            .name(format!("plugin-aux-rx-{}", sanitize_id(&plugin_id)))
            .spawn(move || aux_reader_loop(reader, dirty, writer, plugin_id))
        {
            tracing::warn!(
                "plugin '{}' aux reader thread spawn failed: {e}",
                self.plugin_id
            );
            return false;
        }
        true
    }

    /// reader 스레드가 누적한 dirty rect를 drain. 호스트 main loop이 frame 합성 직전에
    /// 호출. 반환된 map의 value가 `None`이면 "전체 갱신".
    pub fn take_dirty_rects(&self) -> HashMap<SharedBufferId, Option<PixelRect>> {
        let mut guard = tasty_utils::poison::recover_mutex(
            self.dirty_rects.lock(),
            DIRTY_RECTS_WHAT,
            &DIRTY_RECTS_POISONED,
        );
        std::mem::take(&mut *guard)
    }

    /// 자식 프로세스의 OS PID. Windows의 `DuplicateHandle` 대상 식별에 필요.
    /// `shutdown` 이후나 stub 인스턴스에서는 `None`.
    pub fn child_pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    pub fn ping(&self, next_id: u64) {
        if let Err(e) = self.req_tx.send(PluginRequest {
            method: "ping".into(),
            params: serde_json::json!({}),
            id: next_id,
        }) {
            tracing::trace!("plugin ping send dropped (writer exited): {e}");
        }
    }

    pub fn since_last_pong(&self) -> Duration {
        tasty_utils::poison::recover_mutex(
            self.last_pong.lock(),
            LAST_PONG_WHAT,
            &LAST_PONG_POISONED,
        )
        .elapsed()
    }

    /// shutdown 요청만 보내고 **대기하지 않고** 즉시 반환한다. 반환된
    /// [`PendingShutdown`] 이 자식 소유권을 가져가므로(`child.take()`), 남은
    /// `PluginProcess` 가 이 자리에서 drop 돼도 [`PluginProcess::drop`] 의 즉시
    /// kill 이 graceful 대기를 앞지르지 않는다.
    ///
    /// 요청 전송과 대기를 분리해 두면 호출자가 여러 plugin 의 요청을 먼저 전부
    /// 뿌린 뒤 대기 구간만 겹칠 수 있다 — 총 소요가 Σ(개별 대기) 가 아니라
    /// max(개별 대기) 로 수렴한다.
    pub fn begin_shutdown(mut self, deadline: Instant) -> PendingShutdown {
        if let Err(e) = self.req_tx.send(PluginRequest {
            method: "shutdown".into(),
            params: serde_json::json!({}),
            id: u64::MAX,
        }) {
            tracing::trace!("plugin shutdown send dropped (writer exited): {e}");
        }
        PendingShutdown {
            plugin_id: std::mem::take(&mut self.plugin_id),
            child: self.child.take(),
            deadline,
            started: Instant::now(),
        }
    }

    /// 요청 전송 + 종료 대기를 한 번에 하는 블로킹 형태 — 단건 경로(plugin
    /// disable / 재시작 / swap)용. 반환 시점에 자식은 회수(exit 관측 또는
    /// kill+wait 완료)돼 있다.
    ///
    /// 반환값은 종료 계측(`S4a plugin_shutdown_one`)의 `reason` 필드용이다 —
    /// 어느 plugin 이 graceful 시간 안에 못 빠졌는지 가리려면 소요 ms 만으로는
    /// 부족하고 사유가 함께 있어야 한다.
    pub fn shutdown(self, timeout: Duration) -> ShutdownOutcome {
        self.begin_shutdown(Instant::now() + timeout).wait()
    }
}

/// 자식 종료를 관측하는 폴링 간격. `try_wait` 는 논블로킹이라 간격이 그대로
/// 관측 해상도가 된다 — 짧게 하면 종료 감지가 빨라지지만 대기 스레드의 busy
/// 비율이 오른다.
pub const CHILD_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// [`PluginProcess::begin_shutdown`] 이 반환하는 종료 대기 핸들.
///
/// shutdown 요청 전송은 이미 끝났고 남은 것은 자식 종료 관측뿐이다. `poll` 은
/// 논블로킹이라 여러 핸들을 번갈아 폴링하면 대기 구간이 서로 겹친다.
pub struct PendingShutdown {
    plugin_id: String,
    /// 아직 회수하지 않은 자식. 종료를 관측했거나 kill 을 마친 순간 `None` 이 된다.
    child: Option<Child>,
    /// graceful 종료를 기다려 주는 한계 시각. 초과하면 force kill.
    deadline: Instant,
    started: Instant,
}

impl PendingShutdown {
    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// `begin_shutdown` 이후 경과 — 계측의 개별 plugin 소요(`S4a`)용.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// 논블로킹 폴링. 자식이 아직 살아 있고 deadline 전이면 `None`.
    /// `Some` 을 한 번 반환한 시점에 자식은 회수 완료 상태다.
    pub fn poll(&mut self) -> Option<ShutdownOutcome> {
        let Some(child) = self.child.as_mut() else {
            return Some(ShutdownOutcome::NoChild);
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                self.child = None;
                Some(ShutdownOutcome::Graceful)
            }
            Ok(None) if Instant::now() <= self.deadline => None,
            // deadline 초과. 강제 종료로 넘어간다.
            Ok(None) => Some(self.force_kill()),
            // try_wait 자체가 실패하면 더 기다려도 관측할 방법이 없다 — 기존
            // `wait_for_child_exit` 와 같이 즉시 kill 경로로 보낸다.
            Err(e) => {
                tracing::trace!("plugin child try_wait failed: {e}");
                Some(self.force_kill())
            }
        }
    }

    /// 자식이 회수될 때까지 블로킹. 단건 경로용.
    pub fn wait(mut self) -> ShutdownOutcome {
        loop {
            if let Some(outcome) = self.poll() {
                return outcome;
            }
            std::thread::sleep(CHILD_EXIT_POLL_INTERVAL);
        }
    }

    /// kill 실패는 이미 죽은 프로세스(`ESRCH`)거나 OS 권한 문제이며, 어느 쪽이든
    /// 호스트가 추가로 할 수 있는 일이 없으므로 trace 로만 흔적을 남긴다.
    fn force_kill(&mut self) -> ShutdownOutcome {
        if let Some(mut child) = self.child.take() {
            if let Err(e) = child.kill() {
                tracing::trace!("plugin child kill failed (already exited?): {e}");
            }
            if let Err(e) = child.wait() {
                tracing::trace!("plugin child wait failed: {e}");
            }
        }
        ShutdownOutcome::Killed
    }
}

impl Drop for PendingShutdown {
    fn drop(&mut self) {
        // 폴링을 끝내기 전에 핸들이 버려진 경우에만 남아 있다 — 좀비를 만들지
        // 않기 위해 여기서 회수한다.
        if self.child.is_some() {
            self.force_kill();
        }
    }
}

/// 여러 plugin 의 종료 대기를 **겹쳐서** 진행하는 집합 핸들.
///
/// 생성 시점에 이미 모든 대상에 shutdown 요청이 나가 있어야 한다
/// ([`PluginProcess::begin_shutdown`]). 이후 `poll` 을 반복 호출하면 각 자식의
/// 대기가 서로 독립적으로 진행되므로 전체 소요는 개별 deadline 의 max 로
/// 수렴한다.
///
/// 스레드를 쓰지 않는 것은 의도다 — 호출자가 프레임 루프 안에서 논블로킹으로
/// 돌릴 수 있어야 종료 화면 같은 것을 그리면서 대기할 수 있다.
pub struct ShutdownBatch {
    pending: Vec<PendingShutdown>,
    total: usize,
    started: Instant,
}

/// plugin 한 개의 종료 결과 — 계측 로그 한 줄에 필요한 값 묶음.
pub struct ShutdownReport {
    pub plugin_id: String,
    pub elapsed: Duration,
    pub outcome: ShutdownOutcome,
}

impl ShutdownBatch {
    pub fn new(pending: Vec<PendingShutdown>) -> Self {
        let total = pending.len();
        Self {
            pending,
            total,
            started: Instant::now(),
        }
    }

    /// 대기 시작 시점의 대상 수 (완료된 것 포함).
    pub fn total(&self) -> usize {
        self.total
    }

    /// batch 생성 이후 경과 — 계측의 합계(`S4`)용.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn is_done(&self) -> bool {
        self.pending.is_empty()
    }

    /// 논블로킹 — 이번 라운드에 종료가 관측된 plugin 들의 결과만 반환한다.
    pub fn poll(&mut self) -> Vec<ShutdownReport> {
        let mut done = Vec::new();
        self.pending.retain_mut(|p| match p.poll() {
            Some(outcome) => {
                done.push(ShutdownReport {
                    plugin_id: p.plugin_id.clone(),
                    elapsed: p.elapsed(),
                    outcome,
                });
                false
            }
            None => true,
        });
        done
    }

    /// 전부 회수될 때까지 블로킹. 반환 시점에 잔존 자식은 없다.
    pub fn wait(&mut self) -> Vec<ShutdownReport> {
        let mut all = Vec::new();
        loop {
            all.extend(self.poll());
            if self.is_done() {
                return all;
            }
            std::thread::sleep(CHILD_EXIT_POLL_INTERVAL);
        }
    }
}

/// `PluginProcess::shutdown` 의 결말 — 종료 계측의 `reason` 필드.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// 자식이 deadline 안에 스스로 종료했다.
    Graceful,
    /// deadline 초과 → `child.kill()` 로 강제 종료했다.
    Killed,
    /// 회수할 자식 핸들이 없었다 (이미 이관/종료됨).
    NoChild,
}

impl ShutdownOutcome {
    /// 로그 필드용 표기 — **맨 소문자 토큰**(`[a-z][a-z0-9_]*`)이고 닫힌 집합이다.
    /// 부팅 계측의 `reason = satisfied|deadline` 이 같은 모양을 쓴다.
    ///
    /// ★ 그 짝은 **저장소 전역 관례가 아니다.** 같은 이름의 필드가 이 저장소에 셋 있고
    /// 값의 모양이 서로 다르다 — `agent-stream` 은 `stream:` 을 앞에 붙인 이름공간 토큰
    /// (`turn_end{reason=stream:turn_timeout}`)을 쓰고, `hook-failures.log` 의 `reason`
    /// 은 애초에 **산문**이라 언어까지 갈린다(`docs/adr/0164-…`). 그래서 "reason 은 늘
    /// 맨 토큰" 으로 일반화한 관측자는 다른 두 곳에서 조용히 0 을 센다.
    ///
    /// 이쪽 절반(값 셋의 모양·구별)은 아래 단정이 잡는다. 반대쪽 절반(부팅의 인라인
    /// 리터럴)은 **아무것도 안 잡는다** — 크레이트가 갈려 부를 수도, 타입으로 묶을 수도
    /// 없어서 이 문장은 주석에 머문다. 부팅 쪽 표기가 바뀌면 여기는 조용히 낡는다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graceful => "graceful",
            Self::Killed => "killed",
            Self::NoChild => "no_child",
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Err(e) = child.kill() {
                tracing::trace!("plugin child kill on drop failed: {e}");
            }
            if let Err(e) = child.wait() {
                tracing::trace!("plugin child wait on drop failed: {e}");
            }
        }
    }
}

/// `PluginProcess::spawn` 의 `Command` 조립 스텝 — entry/args/필수 env 설정 후
/// 보조 채널 endpoint 를 알려주고 mailbox 를 등록한다. mailbox 등록은 *child
/// spawn 전*에 일어나야 SDK 가 빠르게 connect 해도 accept thread 가 매핑할
/// sender 를 찾을 수 있다. `log_file`/`log_clone` 은 stdout/stderr 로 소비된다.
fn build_plugin_command(
    package: &PluginPackage,
    listener: &HostListener,
    handle_listener: Option<&HandleListener>,
    reaper: &crate::reaper::PluginReaper,
    token: &str,
    log_file: std::fs::File,
    log_clone: std::fs::File,
) -> (Command, Option<mpsc::Receiver<HandleStream>>) {
    let entry_path = package.entry_command_path();
    let mut cmd = Command::new(&entry_path);
    // Windows GUI 서브시스템 호스트가 콘솔 서브시스템 플러그인 바이너리를
    // spawn 할 때 빈 콘솔 창이 뜨는 것을 막는다 (비-Windows 에서는 no-op).
    tasty_utils::process::hide_console(&mut cmd);
    // 플러그인 수명을 호스트에 결박: spawn *전* 준비(Linux PDEATHSIG pre_exec /
    // macOS TASTY_HOST_PID env 주입). Windows assign 은 spawn *후*(adopt).
    reaper.prepare(&mut cmd);
    inject_locale_env(&mut cmd);
    cmd.args(package.entry_args())
        .env("TASTY_PLUGIN_ID", &package.manifest.id)
        .env("TASTY_HOST_API_VERSION", HOST_API_VERSION)
        .env("TASTY_HOST_IPC_PORT", listener.port().to_string())
        .env("TASTY_PLUGIN_TOKEN", token)
        .env("TASTY_PLUGIN_DIR", &package.dir)
        .current_dir(&package.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_clone));

    let handle_stream_rx = if let Some(hl) = handle_listener {
        cmd.env("TASTY_PLUGIN_HANDLE_ENDPOINT", hl.endpoint());
        Some(hl.register_token(token))
    } else {
        None
    };
    (cmd, handle_stream_rx)
}

/// 활성 로케일 env 주입. 이 크레이트는 `tasty-i18n` 에 의존하지 않는다 — 활성 언어는
/// host 본 바이너리가 부팅 시(`src/boot/locale.rs`, 스레드 생성 전 단일 스레드 구간)
/// 자기 프로세스 env 에 set 한 `TASTY_LOCALE` / `TASTY_LOCALE_FONT` 를 그대로 자식에
/// propagate 한다. `Command` 는 host env 를 상속하므로 두 값은 명시하지 않아도
/// 흘러가지만, 계약을 코드에 드러내고(host 본 바이너리 밖 — 테스트 · 다른 호스트 — 에서
/// 쓰일 때의 `en` 폴백) 빈 폰트 값이 자식에 남지 않게 여기서 확정한다. 값은 spawn 시점에
/// 고정된다 — 근거 `docs/adr/0103-plugin-locale-via-host-process-env.md`.
fn inject_locale_env(cmd: &mut Command) {
    let (locale, font) = locale_env_for_child(
        std::env::var_os("TASTY_LOCALE"),
        std::env::var_os("TASTY_LOCALE_FONT"),
    );
    cmd.env("TASTY_LOCALE", locale);
    match font {
        Some(font) => {
            cmd.env("TASTY_LOCALE_FONT", font);
        }
        None => {
            cmd.env_remove("TASTY_LOCALE_FONT");
        }
    }
}

/// host env 의 로케일 값을 자식에 넘길 형태로 정리한다 — `TASTY_LOCALE` 은 항상
/// (미설정 · 빈 값이면 `en`), `TASTY_LOCALE_FONT` 는 비어 있지 않을 때만.
fn locale_env_for_child(
    locale: Option<OsString>,
    font: Option<OsString>,
) -> (OsString, Option<OsString>) {
    let locale = locale
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OsString::from("en"));
    (locale, font.filter(|v| !v.is_empty()))
}

/// plugin별 격리 디렉터리 env 주입. 디렉터리 생성은 호스트가 미리 보장한다 —
/// plugin이 fs.write 권한 없이도 자기 영역만은 자유롭게 쓸 수 있도록.
fn inject_plugin_data_env(cmd: &mut Command, package: &PluginPackage, log_path: &Path) {
    let Some(home) = tasty_utils::path::tasty_home() else {
        return;
    };
    let data_dir = home.join("plugin-data").join(&package.manifest.id);
    let config_path = home
        .join("plugin-config")
        .join(format!("{}.toml", &package.manifest.id));
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::warn!("plugin data dir {} create failed: {e}", data_dir.display());
    }
    if let Some(parent) = config_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("plugin config dir {} create failed: {e}", parent.display());
    }
    cmd.env("TASTY_PLUGIN_DATA_DIR", &data_dir);
    cmd.env("TASTY_PLUGIN_CONFIG_PATH", &config_path);
    cmd.env("TASTY_PLUGIN_LOG_PATH", log_path);
    // host 가 부팅 시 확정한 데이터 루트를 자식에 정보성으로 내려준다
    // (completion-log 경로 판별용). **`TASTY_HOME` 이 아니라
    // `TASTY_PARENT_HOME`** 으로 주입한다 — `TASTY_HOME` 은 tasty_home()
    // (self-determination, override 전용)의 1순위라, 정보성 값을 그 이름으로
    // 주입하면 자식이 그걸 자기 데이터 루트 override 로 오인한다(release 안에서
    // debug 실행 시 격리 붕괴). notify_log_path() 가 `TASTY_PARENT_HOME` 을
    // 최우선으로 보므로 writer(plugin)/reader(conductor) 경로는 계속 일치한다.
    cmd.env("TASTY_PARENT_HOME", &home);
}

/// 송신 스레드 — `req_rx` 로 들어오는 요청을 NDJSON 한 줄씩 `writer` 에 기록.
fn spawn_tx_thread(
    plugin_id: &str,
    mut writer: std::net::TcpStream,
    req_rx: mpsc::Receiver<PluginRequest>,
) -> io::Result<()> {
    let plugin_id_tx = plugin_id.to_string();
    std::thread::Builder::new()
        .name(format!("plugin-tx-{}", sanitize_id(&plugin_id_tx)))
        .spawn(move || {
            for req in req_rx.iter() {
                let line = match serde_json::to_string(&req) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("plugin '{}' request encode error: {}", plugin_id_tx, e);
                        continue;
                    }
                };
                if writeln!(writer, "{line}").is_err() {
                    break;
                }
                if writer.flush().is_err() {
                    break;
                }
            }
        })?;
    Ok(())
}

/// 수신 스레드 — `stream` 에서 한 줄씩 읽어 `handle_incoming_line` 으로 분류.
fn spawn_rx_thread(
    plugin_id: &str,
    stream: std::net::TcpStream,
    waker: tasty_terminal::waker_factory::SharedWakerFactory,
    last_pong: Arc<Mutex<Instant>>,
    resp_tx: mpsc::Sender<PluginResponse>,
    event_tx: mpsc::Sender<PluginEvent>,
) -> io::Result<()> {
    let plugin_id_rx = plugin_id.to_string();
    std::thread::Builder::new()
        .name(format!("plugin-rx-{}", sanitize_id(&plugin_id_rx)))
        .spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                let trim = line.trim();
                if trim.is_empty() {
                    continue;
                }
                handle_incoming_line(trim, &resp_tx, &event_tx, &last_pong, &plugin_id_rx);
                waker.make_default_waker()();
            }
        })?;
    Ok(())
}

fn handle_incoming_line(
    line: &str,
    resp_tx: &mpsc::Sender<PluginResponse>,
    event_tx: &mpsc::Sender<PluginEvent>,
    last_pong: &Arc<Mutex<Instant>>,
    plugin_id: &str,
) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("plugin '{plugin_id}' sent unparseable line: {e}");
            return;
        }
    };
    if v.get("id").and_then(|x| x.as_u64()).is_some() {
        handle_incoming_response(v, resp_tx, last_pong, plugin_id);
        return;
    }
    if let Some(ev_value) = v.get("event") {
        handle_incoming_event(ev_value.clone(), event_tx, plugin_id);
    }
}

fn handle_incoming_response(
    v: serde_json::Value,
    resp_tx: &mpsc::Sender<PluginResponse>,
    last_pong: &Arc<Mutex<Instant>>,
    plugin_id: &str,
) {
    match serde_json::from_value::<PluginResponse>(v) {
        Ok(resp) => {
            *tasty_utils::poison::recover_mutex(
                last_pong.lock(),
                LAST_PONG_WHAT,
                &LAST_PONG_POISONED,
            ) = Instant::now();
            if let Err(e) = resp_tx.send(resp) {
                tracing::trace!("plugin response forward dropped (consumer exited): {e}");
            }
        }
        Err(e) => {
            tracing::warn!("plugin '{plugin_id}' response decode error: {e}");
        }
    }
}

fn handle_incoming_event(
    ev_value: serde_json::Value,
    event_tx: &mpsc::Sender<PluginEvent>,
    plugin_id: &str,
) {
    match serde_json::from_value::<PluginEvent>(ev_value) {
        Ok(ev) => {
            if let Err(e) = event_tx.send(ev) {
                tracing::trace!("plugin event forward dropped (consumer exited): {e}");
            }
        }
        Err(e) => {
            tracing::warn!("plugin '{plugin_id}' event decode error: {e}");
        }
    }
}

/// 보조 채널 reader 스레드의 메시지 처리 루프.
///
/// - `Dirty`: `dirty_rects`에 union(coalesce)해 누적.
/// - `Ping`: 동일 `seq`로 `Pong` 응답.
/// - `Pong`: 호스트는 ping을 보내지 않으므로 무시(트레이스 로그만).
/// - `HandleAttach`: plugin→host로 오는 일은 없어야 함. 받으면 fd 즉시 close 후 경고.
///
/// EOF가 도착하면 (plugin 종료/재시작 또는 정상 shutdown) 조용히 종료.
#[allow(clippy::cognitive_complexity)] // complexity-exempt: 4-arm 평면 메시지
// dispatch 루프 — HandleAttach arm 의 fd 소유권 정리만 플랫폼별 cfg 분기다.
// `aux: Option<RawFd>` (unix) / `Option<u64>` (windows) 로 타입이 cfg 에 따라
// 달라서 arm 을 별 함수로 뽑으려면 cfg 게이트를 그대로 복제해야 하고, fd
// close 책임 소재가 흐려질 위험이 이득보다 크다 — 이 자리에 두는 편이 더 안전.
fn aux_reader_loop(
    mut reader: HandleStreamReader,
    dirty: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>>,
    writer: Arc<Mutex<HandleStream>>,
    plugin_id: String,
) {
    loop {
        match reader.recv_message() {
            Ok((HandleChannelMessage::Dirty { id, rect }, _)) => {
                merge_dirty(&dirty, id, rect);
            }
            Ok((HandleChannelMessage::Ping { seq }, _)) => {
                // poison 이면 보내지 않는다 — 임계구역이 소켓 쓰기라 반쯤 쓰인 메시지
                // 위에 이어 쓰면 프레이밍이 깨진다(plugin SDK 쪽 pong 과 같은 판단).
                // 다만 **조용히** 건너뛰지 않는다: pong 이 끊기면 상대가 이 채널을 죽은
                // 것으로 보는데, 그 원인이 어디에도 안 남으면 추적이 불가능하다.
                match writer.lock() {
                    Ok(mut w) => {
                        if let Err(e) = w.send_message(&HandleChannelMessage::Pong { seq }) {
                            tracing::warn!("plugin '{plugin_id}' aux Pong send failed: {e}");
                        }
                    }
                    Err(_) => tracing::error!(
                        "plugin '{plugin_id}' aux writer lock poisoned — skipping pong; the \
                         plugin will see this channel go quiet"
                    ),
                }
            }
            Ok((HandleChannelMessage::Pong { .. }, _)) => {
                // 호스트가 Ping을 보내지 않으므로 정상 시나리오에서는 도착하지 않는다.
            }
            Ok((HandleChannelMessage::HandleAttach { .. }, aux)) => {
                tracing::warn!("plugin '{plugin_id}' sent unexpected HandleAttach on aux channel");
                #[cfg(unix)]
                if let Some(fd) = aux {
                    // SAFETY: 동행 fd가 있다면 우리가 SCM_RIGHTS로 받은 새 fd 소유권.
                    // 사용처가 없으므로 leak 방지를 위해 close.
                    unsafe { libc::close(fd) };
                }
                // Windows: aux는 in-band HANDLE u64 값일 뿐, 우리 프로세스 핸들 테이블에
                // 복제된 게 아니므로 CloseHandle 대상이 아니다(plugin→host 로 HandleAttach 가
                // 오는 것 자체가 비정상). 그냥 버린다.
                #[cfg(windows)]
                let _ = aux;
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                tracing::warn!("plugin '{plugin_id}' aux channel reader error: {e}");
                break;
            }
        }
    }
}

/// 한 buffer의 dirty 상태를 incoming rect와 union한다. value가 `None`이면 "전체 갱신"
/// sticky flag — 더 이상 좁히지 않는다.
fn merge_dirty(
    map: &Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>>,
    id: SharedBufferId,
    incoming: Option<PixelRect>,
) {
    let Ok(mut m) = map.lock() else {
        return;
    };
    match (m.get(&id).copied(), incoming) {
        (Some(None), _) => {} // 이미 full — 무시.
        (_, None) => {
            m.insert(id, None);
        }
        (None, Some(r)) => {
            m.insert(id, Some(r));
        }
        (Some(Some(existing)), Some(r)) => {
            m.insert(id, Some(union_rect(existing, r)));
        }
    }
}

/// 두 정수 rect의 bounding union. tasty-plugin-protocol의 PixelRect는 (x, y, w, h)이고
/// w/h=0은 invalid 취급이지만 reader는 wire 그대로 union한다 (필터링은 호출자).
fn union_rect(a: PixelRect, b: PixelRect) -> PixelRect {
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = a.x.saturating_add(a.w).max(b.x.saturating_add(b.w));
    let y2 = a.y.saturating_add(a.h).max(b.y.saturating_add(b.h));
    PixelRect {
        x: x1,
        y: y1,
        w: x2.saturating_sub(x1),
        h: y2.saturating_sub(y1),
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn generate_token() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // 단순한 의사 랜덤 — 단계 07에서 강화 가능 (rand 크레이트 등).
    let a = (nanos as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b = ((nanos >> 64) as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    format!("{a:016x}{b:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 종료 계측의 `reason` 값은 **맨 소문자 토큰**이고 서로 구별된다.
    ///
    /// 이 로그는 사람이 아니라 기계가 읽는다 — 값에 공백·대문자·구분자가 섞이면 그것을
    /// 세던 관측자가 **실패가 아니라 0** 을 낸다(안 보인다). 그래서 모양을 단정으로 박는다.
    /// 값 자체는 `docs/architecture/shutdown-sequence.md` 가 인용한다(`reason="killed"`).
    #[test]
    fn shutdown_reasons_are_distinct_bare_lowercase_tokens() {
        let all = [
            ShutdownOutcome::Graceful,
            ShutdownOutcome::Killed,
            ShutdownOutcome::NoChild,
        ];
        let mut seen: Vec<&str> = Vec::new();
        for o in all {
            let v = o.as_str();
            assert!(!v.is_empty(), "빈 표기: {o:?}");
            assert!(
                v.starts_with(|c: char| c.is_ascii_lowercase()),
                "소문자로 시작해야 한다: {v}"
            );
            assert!(
                v.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "맨 소문자 토큰이어야 한다(공백·대문자·구분자 금지): {v}"
            );
            assert!(!seen.contains(&v), "두 결말이 같은 표기를 쓴다: {v}");
            seen.push(v);
        }
    }

    #[test]
    fn token_is_32_hex_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn locale_env_falls_back_to_en_and_drops_empty_font() {
        assert_eq!(
            locale_env_for_child(None, None),
            (OsString::from("en"), None)
        );
        assert_eq!(
            locale_env_for_child(Some(OsString::new()), Some(OsString::new())),
            (OsString::from("en"), None)
        );
    }

    #[test]
    fn locale_env_propagates_host_values() {
        assert_eq!(
            locale_env_for_child(
                Some(OsString::from("ko")),
                Some(OsString::from("/x/lang/ko/fonts/a.ttf"))
            ),
            (
                OsString::from("ko"),
                Some(OsString::from("/x/lang/ko/fonts/a.ttf"))
            )
        );
    }

    #[test]
    fn sanitize_strips_special() {
        assert_eq!(sanitize_id("com.foo/bar:baz"), "com.foo_bar_baz");
        assert_eq!(sanitize_id("com.example-x"), "com.example-x");
    }

    #[test]
    fn union_rect_combines_bbox() {
        let a = PixelRect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let b = PixelRect {
            x: 5,
            y: 5,
            w: 10,
            h: 10,
        };
        let u = union_rect(a, b);
        assert_eq!(
            u,
            PixelRect {
                x: 0,
                y: 0,
                w: 15,
                h: 15
            }
        );
    }

    #[test]
    fn union_rect_disjoint_gives_outer_bbox() {
        let a = PixelRect {
            x: 0,
            y: 0,
            w: 4,
            h: 4,
        };
        let b = PixelRect {
            x: 10,
            y: 10,
            w: 5,
            h: 5,
        };
        let u = union_rect(a, b);
        assert_eq!(
            u,
            PixelRect {
                x: 0,
                y: 0,
                w: 15,
                h: 15
            }
        );
    }

    #[test]
    fn merge_dirty_full_is_sticky() {
        let map: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let id = SharedBufferId(1);
        merge_dirty(&map, id, None);
        // 이후 Some이 와도 None 유지.
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 2,
                h: 2,
            }),
        );
        assert_eq!(map.lock().unwrap().get(&id).copied(), Some(None));
    }

    #[test]
    fn merge_dirty_some_unions_with_existing() {
        let map: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let id = SharedBufferId(2);
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 5,
                h: 5,
            }),
        );
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 10,
                y: 10,
                w: 5,
                h: 5,
            }),
        );
        let got = map.lock().unwrap().get(&id).copied().flatten();
        assert_eq!(
            got,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 15,
                h: 15
            })
        );
    }

    #[test]
    fn merge_dirty_some_then_full_becomes_full() {
        let map: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let id = SharedBufferId(3);
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 5,
                h: 5,
            }),
        );
        merge_dirty(&map, id, None);
        assert_eq!(map.lock().unwrap().get(&id).copied(), Some(None));
    }
}

/// 종료 대기 겹침 검증. 실제 plugin SDK 없이 "종료가 늦는 자식" 을 직접
/// spawn 해서, 대기가 직렬이 아니라 겹치는지를 시간으로 관측한다.
#[cfg(test)]
#[cfg(any(unix, windows))]
mod shutdown_tests {
    use super::*;

    /// `long=true` 면 deadline 을 넘길 만큼 오래 사는 자식, false 면 즉시 종료.
    /// tasty 는 Unix/Windows 만 지원하므로 두 분기로 충분하다.
    fn spawn_test_child(long: bool) -> Child {
        #[cfg(unix)]
        {
            Command::new("sleep")
                .arg(if long { "30" } else { "0" })
                .spawn()
                .expect("test child spawn")
        }
        #[cfg(windows)]
        {
            Command::new("cmd")
                .args([
                    "/C",
                    if long {
                        "ping -n 30 127.0.0.1 > nul"
                    } else {
                        "exit 0"
                    },
                ])
                .spawn()
                .expect("test child spawn")
        }
    }

    fn pending(plugin_id: &str, child: Child, deadline: Instant) -> PendingShutdown {
        PendingShutdown {
            plugin_id: plugin_id.to_string(),
            child: Some(child),
            deadline,
            started: Instant::now(),
        }
    }

    /// 응답 없는 자식 3개의 deadline 이 겹쳐야 한다 — 직렬이면 3×300ms 이상
    /// 걸린다. 개별 타임아웃 의미론(각자 deadline 까지 기다린 뒤 force kill)은
    /// `reason = killed` 로 유지되는지 함께 본다.
    #[test]
    fn shutdown_batch_waits_concurrently() {
        let children: Vec<Child> = (0..3).map(|_| spawn_test_child(true)).collect();
        let deadline = Instant::now() + Duration::from_millis(300);
        let handles = children
            .into_iter()
            .enumerate()
            .map(|(i, c)| pending(&format!("slow-{i}"), c, deadline))
            .collect();

        let mut batch = ShutdownBatch::new(handles);
        assert_eq!(batch.total(), 3);

        let t = Instant::now();
        let reports = batch.wait();
        let elapsed = t.elapsed();

        assert!(batch.is_done());
        assert_eq!(reports.len(), 3);
        assert!(
            reports.iter().all(|r| r.outcome == ShutdownOutcome::Killed),
            "deadline 을 넘긴 자식은 force kill 이어야 한다"
        );
        assert!(
            elapsed < Duration::from_millis(700),
            "대기가 겹치지 않았다 (직렬이면 900ms 이상): {elapsed:?}"
        );
    }

    /// 스스로 종료하는 자식은 deadline 을 소모하지 않고 graceful 로 빠진다.
    #[test]
    fn shutdown_batch_reports_graceful_exit() {
        let children: Vec<Child> = (0..2).map(|_| spawn_test_child(false)).collect();
        let deadline = Instant::now() + Duration::from_secs(2);
        let handles = children
            .into_iter()
            .enumerate()
            .map(|(i, c)| pending(&format!("quick-{i}"), c, deadline))
            .collect();

        let mut batch = ShutdownBatch::new(handles);
        let t = Instant::now();
        let reports = batch.wait();

        assert_eq!(reports.len(), 2);
        assert!(
            reports
                .iter()
                .all(|r| r.outcome == ShutdownOutcome::Graceful),
            "스스로 종료한 자식은 graceful 이어야 한다"
        );
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "graceful 종료가 deadline 을 기다렸다: {:?}",
            t.elapsed()
        );
    }

    /// 폴링을 끝내지 않고 핸들을 버려도 자식은 회수돼야 한다 (좀비 방지).
    #[test]
    fn dropping_pending_shutdown_reaps_child() {
        let child = spawn_test_child(true);
        #[cfg(unix)]
        let pid = child.id();
        let handle = pending("dropped", child, Instant::now() + Duration::from_secs(60));
        drop(handle);

        // kill + wait 이 Drop 안에서 끝나므로, 반환 시점에 자식은 이미 회수됐다.
        #[cfg(unix)]
        {
            // 이미 wait 했으므로 같은 pid 로 다시 신호를 보내면 실패해야 한다
            // (좀비로 남아 있다면 신호가 성공한다).
            // SAFETY: signal 0 은 프로세스를 건드리지 않고 존재 여부만 조회하는
            // POSIX 표준 용법이다. 인자는 정수 두 개뿐이라 포인터 유효성 요건이 없다.
            let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
            assert!(!alive, "자식 {pid} 이 회수되지 않았다");
        }
    }
}
