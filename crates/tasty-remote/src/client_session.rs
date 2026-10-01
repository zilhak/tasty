//! Outbound mirror session ownership. App applies received values to explicit engine/View targets.
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tasty_ipc::stream::StreamTag;
use tasty_model::Workspace;
/// Host-independent remote directory values; the application chooses its local picker cache.
#[derive(Clone)]
pub struct RemoteDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
    pub ext: String,
}
/// 출력과 resize를 같은 버퍼에 도착 순서대로 담아 올바른 크기의 그리드에 적용한다.
pub enum MirrorEvent {
    Data(u32, Vec<u8>),
    Resize(u32, usize, usize),
    /// 로컬 PTY가 없는 mirror의 busy 상태는 서버에서 받는다.
    Activity(u32, bool),
    /// None이면 원격 attention 해제다. 로컬 완료 감지와 별도로 반영한다.
    Attention(u32, Option<tasty_ipc::stream::AttentionKindWire>),
    /// 원격 cwd. 로컬 파일시스템 경로로 사용하지 않으며 None이면 값을 지운다.
    Cwd(u32, Option<String>),
    /// 구조 변경 실패. 에이전트 요청은 로그, 사용자 요청은 toast로 알린다.
    StructuralFailed(u64, Option<String>),
    /// 사용자 요청의 포커스 의도를 뒤따르는 StructuralDelta에 전달할 op_id.
    StructuralSucceeded(u64),
    /// 원격의 전체 구조. 기존 surface의 로컬 ID·터미널은 가능한 한 재사용한다.
    StructuralDelta {
        workspace_id: u32,
        tree: Value,
        surfaces: Vec<Value>,
    },
    /// capture_result 커스텀 이벤트. 성공 경로는 원격 파일시스템의 경로다.
    CaptureResult {
        ok: bool,
        path: Option<String>,
        reason: Option<String>,
    },
    /// list_dir_result 커스텀 이벤트. dir은 서버가 반환한 절대경로다.
    ListDirResult {
        request_id: u64,
        ok: bool,
        dir: Option<String>,
        entries: Option<Vec<RemoteDirEntry>>,
        /// 서버가 프레임 크기 때문에 목록을 잘랐으면 사용자에게 알린다.
        truncated: bool,
        reason: Option<String>,
    },
    /// 재조립된 mesh frame. bytes는 서버가 footer를 제거한 payload다.
    Mesh(u32, u64, u64, bool, Vec<u8>),
    /// git_query_result의 kind별 JSON은 호스트가 해석하지 않고 플러그인으로 전달한다.
    GitQueryResult {
        request_id: u64,
        ok: bool,
        kind: String,
        data: Option<Value>,
        truncated: bool,
        reason: Option<String>,
    },
    /// markdown_content_result의 surface_id는 원격 ID다. 성공은 file/source, 실패는 reason을 담는다.
    MarkdownContentResult {
        request_id: u64,
        surface_id: u32,
        ok: bool,
        file: Option<String>,
        source: Option<String>,
        truncated: bool,
        reason: Option<String>,
    },
    /// 다른 점유 대상의 문서 신호도 올 수 있어 이 세션의 원격 surface ID인지 확인해야 한다.
    MarkdownChanged {
        surface_id: u32,
    },
    /// 서버 전송 손실. 데이터가 연속이라는 가정을 버리고 재동기화를 요청한다.
    Desynced {
        frames: u64,
    },
}

/// Incoming values have one arrival order and a bounded retained-byte budget. On overflow the
/// connection is explicitly desynchronized; dropping content never masquerades as a continuous cut.
mod outbox {
    use super::MirrorEvent;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Pending {
        events: Vec<MirrorEvent>,
        bytes: usize,
    }
    #[derive(Clone)]
    pub struct MirrorOutbox {
        epoch: crate::connection::ConnectionEpoch,
        pending: Arc<Mutex<Pending>>,
    }
    impl MirrorOutbox {
        pub fn new(epoch: crate::connection::ConnectionEpoch) -> Self {
            Self {
                epoch,
                pending: Arc::new(Mutex::new(Pending::default())),
            }
        }
        pub fn push(&self, event: MirrorEvent) -> bool {
            if !self.epoch.is_active() {
                return false;
            }
            let Ok(mut pending) = self.pending.lock() else {
                if !super::MIRROR_OUTBOX_PUSH_DROPPED
                    .swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    tracing::error!("remote inbox poisoned; arrival discarded");
                }
                return false;
            };
            if !self.epoch.is_active() {
                return false;
            }
            let bytes = event.retained_bytes();
            if pending.events.len() >= 1024
                || pending.bytes.saturating_add(bytes) > tasty_ipc::admission::QUEUED_BYTES_LIMIT
            {
                let frames = pending.events.len() as u64 + 1;
                pending.events.clear();
                pending.bytes = std::mem::size_of::<MirrorEvent>();
                pending.events.push(MirrorEvent::Desynced { frames });
            } else {
                pending.bytes += bytes;
                pending.events.push(event);
            }
            true
        }
        /// A blocked structural delta and its complete tail precede arrivals received meanwhile.
        /// If preserving the combined tail exceeds the same budget, report loss explicitly.
        pub fn restore_front(&self, mut events: Vec<MirrorEvent>) {
            if !self.epoch.is_active() {
                return;
            }
            let mut pending = tasty_utils::poison::recover_mutex(
                self.pending.lock(),
                super::MIRROR_OUTBOX_WHAT,
                &super::MIRROR_OUTBOX_POISONED,
            );
            let bytes = events.iter().fold(0usize, |sum, event| {
                sum.saturating_add(event.retained_bytes())
            });
            if events.len().saturating_add(pending.events.len()) > 1024
                || bytes.saturating_add(pending.bytes) > tasty_ipc::admission::QUEUED_BYTES_LIMIT
            {
                let frames = events.len().saturating_add(pending.events.len()) as u64;
                pending.events.clear();
                pending.events.push(MirrorEvent::Desynced { frames });
                pending.bytes = std::mem::size_of::<MirrorEvent>();
            } else {
                events.append(&mut pending.events);
                pending.events = events;
                pending.bytes += bytes;
            }
        }
        pub fn drain(&self) -> Vec<MirrorEvent> {
            let mut pending = tasty_utils::poison::recover_mutex(
                self.pending.lock(),
                super::MIRROR_OUTBOX_WHAT,
                &super::MIRROR_OUTBOX_POISONED,
            );
            pending.bytes = 0;
            if !self.epoch.is_active() {
                pending.events.clear();
                return Vec::new();
            }
            std::mem::take(&mut pending.events)
        }
    }
}

pub use outbox::MirrorOutbox;

/// 입력·heartbeat·제어 프레임을 단일 writer 스레드로 전달한다.
pub struct OutFrame {
    pub tag: StreamTag,
    pub payload: Vec<u8>,
}

pub type FrameSender = Arc<super::connection::ConnectionSender>;
pub type SharedFrameSender = FrameSender;

const MIRROR_OUTBOX_WHAT: &str = "attach mirror outbox";
static MIRROR_OUTBOX_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// poison 복구와 이벤트 폐기는 서로 다른 진단이므로 각각 한 번씩 기록한다.
static MIRROR_OUTBOX_PUSH_DROPPED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Reconnecting에서는 mirror와 scrollback을 남기고 연결만 교체한다.
/// 완전히 닫힌 세션은 목록에서 제거하므로 Closed 상태는 두지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Reconnecting,
}

/// 성공한 사용자 요청의 다음 delta에 한 번 적용할 로컬 포커스 의도.
#[derive(Debug, Clone)]
pub enum PendingOpFocus {
    NewResource,
    /// 옛 포커스가 사라졌으면 우선순위 순서의 원격 ID 후보 중 남은 surface를 선택한다.
    Close {
        candidates: Vec<u32>,
    },
}

/// 회신을 기다리는 에이전트 요청 ID. 회신이나 재연결 때 제거한다.
#[derive(Debug, Default)]
pub struct AgentRequests {
    /// forward 한 구조 op 의 op_id(`PendingStructuralForward::silent_failure`).
    pub structural: HashSet<u64>,
    /// 원격 markdown 원문 요청의 request_id(`PendingMarkdownContentForward::agent_origin`).
    markdown: HashSet<u64>,
}

impl AgentRequests {
    pub fn note_structural(&mut self, agent_origin: bool, op_id: u64) {
        if agent_origin {
            self.structural.insert(op_id);
        }
    }

    pub fn forget_structural(&mut self, op_id: u64) {
        self.structural.remove(&op_id);
    }

    pub fn note_markdown(&mut self, agent_origin: bool, request_id: u64) {
        if agent_origin {
            self.markdown.insert(request_id);
        }
    }

    pub fn take_markdown(&mut self, request_id: u64) -> bool {
        self.markdown.remove(&request_id)
    }

    pub fn clear(&mut self) {
        self.structural.clear();
        self.markdown.clear();
    }
}

#[derive(Default)]
pub struct MirrorStructureIds {
    pub panes: HashMap<u32, u32>,
    pub remote_tabs: HashMap<u32, u32>,
}

impl MirrorStructureIds {
    pub fn retain_workspace(&mut self, workspace: &Workspace) {
        let pane_ids = workspace.pane_layout().all_pane_ids();
        self.panes.retain(|_, local| pane_ids.contains(local));
        self.remote_tabs.retain(|_, local| {
            pane_ids.iter().any(|id| {
                workspace
                    .pane_layout()
                    .find_pane(*id)
                    .is_some_and(|pane| pane.tabs.iter().any(|tab| tab.id == *local))
            })
        });
    }
}

pub struct AttachClientSession {
    pub state: ClientSessionState,
    pub transport: ClientTransport,
}
pub struct ClientTransport {
    pub workers: crate::transport::ConnectionWorkers,
    pub output: MirrorOutbox,
    pub disconnected: Arc<AtomicBool>,
    pub frame_tx: SharedFrameSender,
    pub tunnel: Option<tasty_ssh::SshTunnel>,
}
pub struct ClientSessionState {
    pub structure_ids: MirrorStructureIds,
    pub local_workspace: u32,
    /// 원격 surface ID를 로컬 mirror ID로 바꾼다.
    pub remote_to_local: HashMap<u32, u32>,
    /// 읽기·쓰기 실패나 종료 통지를 메인 루프의 정리 경로에 전달한다.
    /// 재연결 때 sender를 교체하며 입력 forwarder는 같은 공유 핸들을 유지한다.
    pub phase: SessionState,
    // 이유: 서버 세션 ID를 보관한다. 현재 이 필드를 읽는 경로는 없다.
    #[allow(dead_code)]
    pub client_id: u32,
    /// bulk 연결이 기존 점유에 연결할 원격 workspace ID.
    pub remote_workspace: u32,
    /// bulk 연결도 대화형 attach의 로컬 포트와 SSH 터널을 사용한다.
    pub bulk_port: u16,
    /// 세션이 살아 있는 동안 터널을 유지한다. 직접 loopback 연결은 None.
    #[allow(dead_code)]
    /// 자동 연결을 시작한 로컬 anchor ID. 수동 연결은 None.
    pub anchor_ws_id: Option<u32>,
    pub op_seq: u64,
    /// 사용자 요청별 포커스 의도. 성공 회신에서 꺼내 다음 delta에 적용한다.
    /// 현재 실패 회신에서는 제거하지 않으며 재연결이나 세션 제거 때 정리된다.
    pub pending_op_focus: HashMap<u64, PendingOpFocus>,
    /// 에이전트 요청의 회신은 사용자 toast 대신 로그로 알린다.
    pub agent_requests: AgentRequests,
    /// 성공 회신 뒤 다음 StructuralDelta가 한 번 소비할 포커스 의도.
    pub next_delta_focus: Option<PendingOpFocus>,
    /// 원격 surface별 마지막 전송 크기. 응답 전 반복 전송을 줄이며 재연결 때 비운다.
    /// 큐 전송 성공은 원격 적용 확인이 아니다.
    pub last_forwarded_resize: HashMap<u32, (usize, usize)>,
    /// 현재 배지는 loopback 엔드포인트다. 실제 SSH host 정보는 이 세션에 전달되지 않는다.
    pub remote_label: String,
    /// 응답에 소비자 정보가 없어 요청별로 기록한다. None은 File Picker, Some은 explorer surface ID다.
    pub pending_list_dir_consumers: HashMap<u64, Option<u32>>,
    /// 로컬 markdown surface ID. mirror 교체·삭제는 일반 lifecycle 큐를 거치지 않아 직접 destroy를 보낸다.
    pub markdown_locals: HashSet<u32>,
    /// 손실 뒤 재attach를 기다리는 동안 통지된 프레임 수.
    /// 서버가 점유 해제를 큐에 넣은 뒤 소켓을 닫으므로 Detach 후 EOF를 기다려 새 attach를 보낸다.
    pub resync_pending: Option<u64>,
    /// parked 상태에서는 재attach할 창이 없어 Detach 전송을 미룬다. 창이 생기면 이어서 처리한다.
    pub resync_awaiting_window: bool,
}

impl AttachClientSession {
    /// 손실 복구를 위해 옛 연결에 Detach를 보냈다면 다음 EOF는 재attach로 처리한다.
    pub fn resync_released(&self) -> bool {
        self.state.resync_pending.is_some() && !self.state.resync_awaiting_window
    }

    pub fn state(&self) -> SessionState {
        self.state.phase
    }

    pub fn anchor_ws_id(&self) -> Option<u32> {
        self.state.anchor_ws_id
    }

    /// 현재 sender로 큐에 넣는다. poison 상태에서는 sender를 회수해 사용한다.
    pub fn send_frame(
        &self,
        tag: StreamTag,
        payload: Vec<u8>,
    ) -> Result<(), std::sync::mpsc::SendError<OutFrame>> {
        self.transport.frame_tx.send(OutFrame { tag, payload })
    }
}

impl Drop for ClientTransport {
    fn drop(&mut self) {
        // Closing the old socket is distinct from replaying input into another connection.
        if let Err(error) = self.frame_tx.send(OutFrame {
            tag: StreamTag::Detach,
            payload: Vec::new(),
        }) {
            tracing::debug!("remote detach queue already closed: {error}");
        }
        self.workers.retire_tunnel(self.tunnel.take());
        self.frame_tx.retire();
        self.disconnected
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl MirrorEvent {
    fn retained_bytes(&self) -> usize {
        fn text(value: &Option<String>) -> usize {
            value.as_ref().map_or(0, String::capacity)
        }
        fn json(value: &Value) -> usize {
            std::mem::size_of::<Value>().saturating_add(match value {
                Value::String(value) => value.capacity(),
                Value::Array(values) => values.iter().fold(
                    values
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Value>()),
                    |sum, value| sum.saturating_add(json(value)),
                ),
                Value::Object(values) => {
                    values
                        .iter()
                        .fold(values.len().saturating_mul(128), |sum, (key, value)| {
                            sum.saturating_add(key.capacity())
                                .saturating_add(json(value))
                        })
                }
                _ => 0,
            })
        }
        std::mem::size_of::<Self>().saturating_add(match self {
            Self::Data(_, bytes) | Self::Mesh(_, _, _, _, bytes) => bytes.capacity(),
            Self::Cwd(_, value) | Self::StructuralFailed(_, value) => text(value),
            Self::StructuralDelta { tree, surfaces, .. } => surfaces.iter().fold(
                json(tree).saturating_add(
                    surfaces
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Value>()),
                ),
                |sum, value| sum.saturating_add(json(value)),
            ),
            Self::CaptureResult { path, reason, .. } => text(path).saturating_add(text(reason)),
            Self::ListDirResult {
                dir,
                entries,
                reason,
                ..
            } => text(dir)
                .saturating_add(text(reason))
                .saturating_add(entries.as_ref().map_or(0, |entries| {
                    entries.iter().fold(
                        entries
                            .capacity()
                            .saturating_mul(std::mem::size_of::<RemoteDirEntry>()),
                        |sum, entry| {
                            sum.saturating_add(entry.name.capacity())
                                .saturating_add(entry.ext.capacity())
                        },
                    )
                })),
            Self::GitQueryResult {
                kind, data, reason, ..
            } => kind
                .capacity()
                .saturating_add(data.as_ref().map_or(0, json))
                .saturating_add(text(reason)),
            Self::MarkdownContentResult {
                file,
                source,
                reason,
                ..
            } => text(file)
                .saturating_add(text(source))
                .saturating_add(text(reason)),
            _ => 0,
        })
    }
}
