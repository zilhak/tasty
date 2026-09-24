//! 플러그인의 연결 대기와 송수신 스레드 시작을 별도 스레드에 맡긴다.
//! 연결 전 요청은 큐에 쌓이고 결과는 ConnectSlot에 남겨 pump가 처리한다.
//! 대기 스레드를 만들지 못하면 호출한 스레드에서 기다린다.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::{
    LAST_PONG_POISONED, LAST_PONG_WHAT, MeteredReceiver, MeteredSender, PluginEvent,
    PluginResponse, sanitize_id, spawn_rx_thread, spawn_tx_thread,
};
use crate::listener::PendingConnection;

/// plugin 이 호스트에 연결하기를 기다리는 한도. 넘기면 기동 실패로 친다.
pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

static SLOT_POISONED: AtomicBool = AtomicBool::new(false);
const SLOT_WHAT: &str = "plugin connect slot";

/// 연결 대기의 결과. 매니저가 한 번 거두면 [`ConnectState::Reported`] 가 된다.
enum ConnectState {
    Connecting,
    /// 연결했고 송수신 스레드가 돈다. `at` 은 연결이 성사된 시각, `waited` 는 spawn 부터
    /// 거기까지 걸린 시간이다.
    Connected {
        at: Instant,
        waited: Duration,
    },
    /// 시한 안에 연결이 안 왔거나 송수신 스레드를 못 띄웠다. 값은 사유 문장이다.
    Failed(String),
    Reported,
}

/// 매니저가 거둔 연결 결과 한 건.
#[derive(Debug)]
pub(crate) enum ConnectOutcome {
    Connected { at: Instant, waited: Duration },
    Failed(String),
}

/// 연결 대기 결과를 공유한다. poison 상태여도 마지막 저장값을 재사용한다.
pub(crate) struct ConnectSlot {
    state: Mutex<ConnectState>,
    settled: Condvar,
}

impl ConnectSlot {
    fn connecting() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ConnectState::Connecting),
            settled: Condvar::new(),
        })
    }

    /// 이미 거둔 상태로 만든다 — 연결 대기가 없는 시험용 stub 이 쓴다.
    #[cfg(test)]
    pub(super) fn reported() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ConnectState::Reported),
            settled: Condvar::new(),
        })
    }

    fn settle(&self, state: ConnectState) {
        *tasty_utils::poison::recover_mutex(self.state.lock(), SLOT_WHAT, &SLOT_POISONED) = state;
        self.settled.notify_all();
    }

    /// 결과가 났으면 한 번만 꺼낸다. 아직 연결 중이거나 이미 거뒀으면 `None`.
    pub(super) fn take(&self) -> Option<ConnectOutcome> {
        let mut st =
            tasty_utils::poison::recover_mutex(self.state.lock(), SLOT_WHAT, &SLOT_POISONED);
        let outcome = match &*st {
            ConnectState::Connected { at, waited } => ConnectOutcome::Connected {
                at: *at,
                waited: *waited,
            },
            ConnectState::Failed(reason) => ConnectOutcome::Failed(reason.clone()),
            ConnectState::Connecting | ConnectState::Reported => return None,
        };
        *st = ConnectState::Reported;
        Some(outcome)
    }

    /// 결과가 날 때까지 `deadline` 까지 기다린다. 결과를 꺼내지는 않는다.
    pub(super) fn wait_settled(&self, deadline: Instant) {
        let mut st =
            tasty_utils::poison::recover_mutex(self.state.lock(), SLOT_WHAT, &SLOT_POISONED);
        while matches!(*st, ConnectState::Connecting) {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return;
            };
            st = match self.settled.wait_timeout(st, left) {
                Ok((g, _)) => g,
                Err(poisoned) => {
                    tasty_utils::poison::recover_poisoned(poisoned, SLOT_WHAT, &SLOT_POISONED).0
                }
            };
        }
    }
}

/// 연결 뒤 송수신 스레드를 띄우는 데 필요한 것 — spawn 이 만들어 연결 대기 쪽에 넘긴다.
pub(super) struct ConnectJob {
    pub(super) plugin_id: String,
    pub(super) log_path: std::path::PathBuf,
    pub(super) pending: PendingConnection,
    pub(super) req_rx: MeteredReceiver<String>,
    pub(super) resp_tx: MeteredSender<PluginResponse>,
    pub(super) event_tx: MeteredSender<PluginEvent>,
    pub(super) last_pong: Arc<Mutex<Instant>>,
    pub(super) waker: tasty_terminal::waker_factory::SharedWakerFactory,
    pub(super) spawned_at: Instant,
}

/// 별도 스레드에서 연결을 기다리고 결과가 나오면 호스트를 깨운다.
/// 스레드를 만들지 못하면 호출한 스레드에서 기다린다.
pub(super) fn start(job: ConnectJob) -> Arc<ConnectSlot> {
    let slot = ConnectSlot::connecting();
    let for_thread = Arc::clone(&slot);
    let name = format!("plugin-connect-{}", sanitize_id(&job.plugin_id));
    // 스레드 생성 실패 시 호출자가 작업을 되찾을 수 있도록 공유 슬롯에 넣는다.
    let job_slot = Arc::new(Mutex::new(Some(job)));
    let job_for_thread = Arc::clone(&job_slot);
    let spawned = std::thread::Builder::new().name(name).spawn(move || {
        let job =
            tasty_utils::poison::recover_mutex(job_for_thread.lock(), SLOT_WHAT, &SLOT_POISONED)
                .take();
        if let Some(job) = job {
            run(job, &for_thread);
        }
    });
    if let Err(e) = spawned {
        let job =
            tasty_utils::poison::recover_mutex(job_slot.lock(), SLOT_WHAT, &SLOT_POISONED).take();
        if let Some(job) = job {
            tracing::warn!(
                "plugin '{}' connect thread could not start ({e}) — waiting inline",
                job.plugin_id
            );
            run(job, &slot);
        }
    }
    slot
}

/// 연결을 기다리고, 오면 송수신 스레드를 띄운다. 결과를 `slot` 에 남기고 호스트를 깨운다.
fn run(job: ConnectJob, slot: &ConnectSlot) {
    let ConnectJob {
        plugin_id,
        log_path,
        pending,
        req_rx,
        resp_tx,
        event_tx,
        last_pong,
        waker,
        spawned_at,
    } = job;
    // 한도는 등록한 listener 가 정한다 — 운영 값은 [`HANDSHAKE_TIMEOUT`].
    let timeout = pending.timeout();
    let state = match pending.wait(timeout) {
        Some(stream) => {
            // 무응답 경과 시간은 연결 성공부터 센다.
            *tasty_utils::poison::recover_mutex(
                last_pong.lock(),
                LAST_PONG_WHAT,
                &LAST_PONG_POISONED,
            ) = Instant::now();
            match start_io(
                &plugin_id, stream, req_rx, resp_tx, event_tx, last_pong, &waker,
            ) {
                Ok(()) => {
                    let at = Instant::now();
                    ConnectState::Connected {
                        at,
                        waited: at.saturating_duration_since(spawned_at),
                    }
                }
                Err(e) => ConnectState::Failed(format!(
                    "plugin '{plugin_id}' connected but its I/O threads could not start: {e}"
                )),
            }
        }
        None => ConnectState::Failed(format!(
            "plugin '{plugin_id}' did not connect within {}s — log: {}",
            timeout.as_secs(),
            log_path.display()
        )),
    };
    slot.settle(state);
    waker.make_default_waker()();
}

fn start_io(
    plugin_id: &str,
    stream: std::net::TcpStream,
    req_rx: MeteredReceiver<String>,
    resp_tx: MeteredSender<PluginResponse>,
    event_tx: MeteredSender<PluginEvent>,
    last_pong: Arc<Mutex<Instant>>,
    waker: &tasty_terminal::waker_factory::SharedWakerFactory,
) -> std::io::Result<()> {
    let writer = stream.try_clone()?;
    spawn_tx_thread(plugin_id, writer, req_rx)?;
    spawn_rx_thread(
        plugin_id,
        stream,
        waker.clone(),
        last_pong,
        resp_tx,
        event_tx,
    )
}
