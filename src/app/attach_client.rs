//! attach/detach 작업 J (B2) — 호스트측 in-process attach-client.
//!
//! step4/6 은 서버(피점유) plane 과 CLI demux-dump 만 만들었고, GUI 가 원격 grid 를
//! mirror 해 그리는 경로는 step6 R2("후속")로 남았다. 이 모듈이 그 마지막 통합이다:
//!
//! - `dispatch_pending_gui_attach`: IPC `attach.into_gui` 가 쌓은 `(port, workspace)`
//!   요청을 `about_to_wait` 에서 drain.
//! - `start_gui_attach`: 원격 tasty(loopback port)에 attach 연결 → `attached_workspace`
//!   디스크립터로 **로컬 mirror Workspace 트리 재구성**(mirror `Terminal::new_detached`
//!   를 `TerminalStore` 삽입, remote↔local id 재매핑) → 기존 렌더러 재사용(신규 셰이더
//!   0). 입력은 `set_input_sink` 로 forward(keyboard.rs 무변경).
//! - `apply_attach_client_output`: reader thread 가 `AttachClientData` 로 깨울 때마다
//!   누적된 원격 출력을 mirror 에 적용하고 화면을 repaint. 끊긴(force-detach/EOF) 세션의
//!   mirror 를 정리. (`Tick::AttachView` 3초 tick 도 backstop 으로 같은 함수를 호출한다.)
//!   적용 대상은 창 있는 engine 뿐 아니라 **창 없는 parked engine** 도 포함한다 — 창을
//!   최소화한 동안 도착한 출력도 유실되지 않는다(ADR-0110).
//!
//! client mirror 는 내가 직접 다루는 대상이라 로컬 워크스페이스처럼 **데이터가 오는 즉시**
//! 갱신한다(로컬 PTY 의 TerminalOutput wake 와 동형). 서버측 readonly 뷰(`attach_poll` ①)만
//! 3초 cadence 로 게이트한다(plan §4). 범위는 작업 J — 자동 매핑(ssh-profiles/
//! workspace.attach_mapping)은 단계 7. 이 모듈의 `start_gui_attach` 가 단계 7 Phase B2 의
//! 호출 진입점이다.

use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use winit::event_loop::EventLoopProxy;

use crate::ipc::client::StreamConnection;
use tasty_terminal::Terminal;

use crate::AppEvent;
use crate::app::App;
use crate::ipc::stream::{self, STREAM_PROTO, StreamControl, StreamTag, StructuralOp};
use crate::model::{
    EmptySurface, ExplorerPanel, Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab,
    TerminalSurface, Workspace,
};
use crate::view::ui::View as _;

/// (ADR-0056 참고) git-viewer plugin id — `crates/tasty-plugin-git-viewer/tasty-plugin.toml`
/// 의 `id` 와 정합해야 한다. 별도 프로세스(plugin)라 상수를 공유할 crate 가 없어
/// 문자열 리터럴로 중복(이 파일 + `adapters::ipc::handler::git_viewer`).
const GIT_VIEWER_PLUGIN_ID: &str = "com.tasty.git-viewer";
/// host → git-viewer plugin unicast event key(`emit_host_event_to_plugin`). plugin
/// 의 `on_event` 가 이 key 로 매칭한다.
const GIT_VIEWER_QUERY_RESULT_EVENT: &str = "git_viewer.query_result";

/// reader thread 가 원격에서 받은 mirror 갱신 이벤트. 출력 바이트와 resize 통지를
/// **한 버퍼에 순서대로** 담아 프레임 도착 순서(원격의 apply 순서)를 보존한다 —
/// resize 앞뒤 출력이 올바른 그리드에서 재생되도록.
pub(crate) enum MirrorEvent {
    /// 원격 출력 바이트 `(remote_surface_id, bytes)`.
    Data(u32, Vec<u8>),
    /// 원격 grid resize `(remote_surface_id, cols, rows)` — mirror 크기 갱신.
    Resize(u32, usize, usize),
    /// 원격 surface 의 busy/idle 활동 상태 `(remote_surface_id, busy)`. mirror 터미널은
    /// 로컬 PTY 가 없어 스스로 활동 상태를 계산할 수 없으므로(`process_id()` 가 항상
    /// `None`), 이 push 가 mirror 워크스페이스 사이드바 status dot 의 유일한 데이터
    /// 소스다(`CoreState::set_mirror_surface_busy`).
    Activity(u32, bool),
    /// 원격 surface 의 attention 상태 `(remote_surface_id, kind)` — `None` 은 해제.
    /// attention 의 진실 원천은 surface 를 소유한 서버이고 producer(완료 IPC/CLI,
    /// Claude 훅, OSC 133, toast)가 전부 그쪽에서 돌므로, 특히 `NeedsInput` 은 mirror
    /// 가 스스로 만들 수 없다 — 이 push 가 mirror attention 의 유일한 소스다
    /// (`CoreState::set_mirror_surface_attention`).
    Attention(u32, Option<tasty_ipc::stream::AttentionKindWire>),
    /// forward 한 구조 op 가 원격에서 실패했다(2단계). `reason`(예: 미등록 kind)을 담아
    /// 메인루프가 실패 toast 를 띄운다.
    StructuralFailed(String),
    /// forward 한 구조 op 가 원격에서 **성공**했다(2단계) — 페이로드는 `op_id`. 08/09
    /// client-only focus 보정 대상 op(`user_triggered`)만 correlate 할 필요가 있어
    /// 세션의 `pending_op_focus`에서 이 id 를 찾아 `next_delta_focus`로 옮겨두는 데
    /// 쓰인다(찾지 못하면 이 op 은 focus 보정 대상이 아니었다는 뜻 — 조용히 무시).
    /// 구조 자체의 반영은 뒤따르는 `StructuralDelta` 가 담당(성공은 그 외엔 무음).
    StructuralSucceeded(u64),
    /// 원격 워크스페이스 구조가 바뀌었다(3단계 역반영). 원격 ws 전체 트리+surfaces 를
    /// 담아 메인루프가 mirror 트리를 증분 재구성한다(survivor 터미널 local id 유지 →
    /// scrollback 보존, 신규만 새 mirror, 사라진 것 제거).
    StructuralDelta {
        workspace_id: u32,
        tree: Value,
        surfaces: Vec<Value>,
    },
    /// (03 screenshot→remote-clipboard) 원격이 이 mirror 세션이 업로드한 캡처를
    /// 처리한 결과(`capture_result` 커스텀 이벤트 — `StreamControl` enum 밖, 그
    /// enum 이 인식 못 하는 별도 "event" 값으로 같은 Control 채널을 탄다). 성공 시
    /// `path` 가 원격 파일시스템 경로, 실패 시 `reason`.
    CaptureResult {
        ok: bool,
        path: Option<String>,
        reason: Option<String>,
    },
    /// (04) file picker — 원격이 이 mirror 세션의 `list_dir_request` 를 처리한 결과
    /// (`list_dir_result` 커스텀 이벤트 — capture_result 와 동일하게 `StreamControl`
    /// enum 밖). 성공 시 `dir`(echo 된 절대경로)과 `entries`, 실패 시 `reason`.
    ListDirResult {
        request_id: u64,
        ok: bool,
        dir: Option<String>,
        entries: Option<Vec<crate::core::fs_list::DirEntryInfo>>,
        /// 서버가 프레임 크기 상한(`attach_runtime::LIST_DIR_ENTRIES_BYTE_BUDGET`)
        /// 때문에 entries 를 잘랐는지 — client 는 toast 로 알린다.
        truncated: bool,
        reason: Option<String>,
    },
    /// 원격 attach mesh surface 의 완전 재조립된 frame(attach-behavior.md#mesh-mirror-채널 참고). `(remote_surface_id,
    /// generation, frame_seq, full_textures, bytes)` — `bytes` 는 이미 footer 없는 순수
    /// payload(서버 `headless_plugins::forward_mesh_frames`가 footer 를 벗겨 보낸다).
    Mesh(u32, u64, u64, bool, Vec<u8>),
    /// (ADR-0056 참고) git-viewer — 원격이 이 mirror 세션의 `git_query_request` 를 처리한
    /// 결과(`git_query_result` 커스텀 이벤트 — `ListDirResult`/`CaptureResult` 와
    /// 동일하게 `StreamControl` enum 밖). `data` 는 성공 시 kind 별 페이로드
    /// (snapshot: worktrees/status/log, diff: hunks)를 그대로 담은 JSON, 실패 시
    /// `None`(그때 `reason` 이 채워짐). 파싱을 최소화해 host 는 이 값을 그대로
    /// `emit_host_event_to_plugin` 으로 plugin 에 전달한다(host 가 스키마를 해석할
    /// 필요가 없다 — plugin 의 wire DTO 가 유일한 소비자).
    GitQueryResult {
        request_id: u64,
        ok: bool,
        kind: String,
        data: Option<Value>,
        truncated: bool,
        reason: Option<String>,
    },
}

/// [`MirrorOutbox`] 를 담는 **모듈 경계**.
///
/// 이 모듈이 없으면 봉인이 성립하지 않는다 — Rust 의 private 은 **모듈 범위**라, 같은
/// 파일(4,600 줄) 안의 다른 함수는 `sess.output.events.lock()` 으로 필드에 그대로 닿는다.
/// 결함이 살던 함수가 바로 그 파일 안에 있으므로, 모듈로 감싸 필드를 실제로 닫는다.
/// 밖으로 내보내는 것은 아래 메서드 넷뿐이다.
mod outbox {
    use std::sync::{Arc, Mutex};

    use super::{MirrorEvent, MirrorHost};

    /// reader thread 가 누적하고 메인 스레드의 apply 가 비우는 원격 mirror 이벤트 버퍼.
    ///
    /// 버퍼를 비우는 경로가 [`MirrorOutbox::take_for`] 하나뿐이고 그것이 적용 대상
    /// ([`MirrorHost`])을 인자로 요구하므로, "꺼냈는데 적용 대상이 없다" 는 상태를 **쓸
    /// 수가 없다**. 그 봉인을 지탱하는 것은 두 가지가 함께다 — 필드를 닫는 **모듈
    /// 경계**(위 `mod outbox` 참고)와 host 를 요구하는 **시그니처**. 둘 중 하나만으로는
    /// 부족하다: 모듈이 없으면 필드로 우회할 수 있고, 시그니처가 느슨하면 host 없이
    /// 비울 수 있다(ADR-0110 "무엇이 무엇을 지탱하는가").
    #[derive(Clone)]
    pub(crate) struct MirrorOutbox {
        events: Arc<Mutex<Vec<MirrorEvent>>>,
    }

    impl MirrorOutbox {
        pub(super) fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// reader thread 전용 — 도착 순서대로 쌓는다. 쌓았으면 `true`(메인 루프를 깨울지
        /// 판단하는 데 쓴다). lock 이 오염됐으면 쌓지 않고 `false` — 오염은 메인 스레드가
        /// 적용 중 패닉했다는 뜻이라 어차피 그 세션은 정리된다.
        pub(super) fn push(&self, ev: MirrorEvent) -> bool {
            match self.events.lock() {
                Ok(mut buf) => {
                    buf.push(ev);
                    true
                }
                Err(_) => false,
            }
        }

        /// 쌓인 이벤트를 **도착 순서대로** 통째로 꺼낸다.
        ///
        /// 적용 대상을 **인자로 요구**하는 것이 요점이다 — host 없이 비울 방법이 없다.
        /// mutex 오염(reader thread panic)은 lock 을 무효화할 이유가 없어 안쪽 값을
        /// 그대로 회수한다(`send_frame` 과 같은 복구 방침) — 이미 도착한 출력을 버리지
        /// 않는다.
        pub(super) fn take_for(&self, _host: &MirrorHost<'_>) -> Vec<MirrorEvent> {
            std::mem::take(&mut *crate::poison::recover_mutex(
                self.events.lock(),
                super::MIRROR_OUTBOX_WHAT,
                &super::MIRROR_OUTBOX_POISONED,
            ))
        }

        /// 테스트에서 버퍼를 채우고 들여다보는 유일한 창구 — 프로덕션 경로는
        /// [`MirrorOutbox::push`] 와 [`MirrorOutbox::take_for`] 뿐이다.
        ///
        /// 즉 위 "host 없이는 비울 수 없다" 는 봉인의 범위는 프로덕션 빌드다. 테스트에서는
        /// 이 창구로 버퍼를 직접 만질 수 있다(ADR-0110 "무엇이 무엇을 지탱하는가").
        #[cfg(test)]
        pub(super) fn peek(&self) -> std::sync::MutexGuard<'_, Vec<MirrorEvent>> {
            self.events.lock().unwrap_or_else(|p| p.into_inner())
        }
    }
}

use outbox::MirrorOutbox;

/// write 전용 스레드로 보내는 한 프레임. forwarder(Data)/heartbeat(Ping)/
/// resize·structural(Control)/detach(Detach)가 모두 이 큐에 push 만 하고, 단일 write
/// 스레드가 순차로 소켓에 `write_frame` 한다 — 여러 스레드가 writer 를 각자 lock 후
/// 직접 쓰던 구조(락 경합·heartbeat 굶김)를 대체한다.
struct OutFrame {
    tag: StreamTag,
    payload: Vec<u8>,
}

/// [`OutFrame`] 을 write 스레드로 보내는 큐 sender. 세션·heartbeat·forwarder 가 공유한다.
type FrameSender = std::sync::mpsc::Sender<OutFrame>;

/// 재연결(attach-behavior.md#gui-자동-재연결-스코프 / #재연결-시-세션-상태-보존 참고) 시 write 스레드/소켓이 통째로 교체돼도, 그보다 수명이 긴 입력
/// forwarder 스레드(터미널 생존 기간 내내 삶)가 최신 `FrameSender` 를 계속 가리킬 수
/// 있게 하는 교체 가능한 핸들. `AttachClientSession::frame_tx` 와 각 forwarder 가 같은
/// `Arc` 를 공유(clone)하고, 재연결 성공 시 `reconnect_session` 이 안쪽 값만 새
/// sender 로 교체한다(Arc 자체는 그대로 — forwarder 는 clone 을 들고 있을 뿐이라 자동
/// 반영). heartbeat/write 스레드는 연결 1 회 수명에 스코프돼 있어 이 간접 계층이
/// 필요 없다 — 자신만의 raw `FrameSender` 를 직접 캡처한다.
type SharedFrameSender = Arc<Mutex<FrameSender>>;

/// attach 연결 락들의 poison 복구 공용 보고 좌표(첫-1 회). frame sender 는 mpsc
/// `Sender`(원자적 send — FORBIDDEN 인 소켓 writer 와 다르다)이고 mirror 출력 버퍼는
/// `Vec` 라, 둘 다 복구가 안전하다. 틀린 것은 흔적이 없다는 것이라 여기로 모은다.
const FRAME_TX_WHAT: &str = "attach frame sender";
static FRAME_TX_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const MIRROR_OUTBOX_WHAT: &str = "attach mirror outbox";
static MIRROR_OUTBOX_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// mirror 세션의 transport 상태(attach-behavior.md#재연결-시-세션-상태-보존 참고). `Connected` 만 실제 소켓 IO 가 살아있다 —
/// `Reconnecting` 은 mirror workspace/터미널(scrollback 포함)을 살려둔 채 transport 만
/// 끊긴 상태로, `auto_attach.rs` 의 backoff 스케줄러가 `reconnect_session` 재시도를
/// 담당한다. 세션이 완전히 닫히면(사용자 close 또는 anchor 없는 disconnect) 이 열거형
/// 값을 두지 않고 `attach_client_sessions` 에서 바로 제거한다 — "Closed" 는 곧 그
/// 세션이 vec 에 더 이상 없는 상태와 동치라 별도 variant 를 두지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionState {
    Connected,
    Reconnecting,
}

/// attach mesh mirror(attach-behavior.md#mesh-mirror-채널 참고) leaf 를 `AttachMeshSurface`로 재구성하는 데 필요한
/// 표시용 메타. `build_layout`이 `term`(터미널 local id 집합)과 나란히 받아, 로컬
/// surface_id 가 mesh role 이면 이 정보로 `AttachMeshSurface`를 만든다.
#[derive(Debug, Clone)]
struct MirrorMeshInfo {
    kind: String,
    plugin_id: String,
    display_name: String,
}

/// client 가 점유한 원격 워크스페이스의 로컬 mirror 세션(작업 J).
pub(crate) struct AttachClientSession {
    /// 로컬에 추가된 mirror Workspace 의 id.
    local_workspace: u32,
    /// 원격 surface_id → 로컬 mirror surface_id. 출력 demux 적용에 사용.
    remote_to_local: HashMap<u32, u32>,
    /// reader thread 가 누적하는 원격 출력 `(remote_surface_id, bytes)`.
    output: MirrorOutbox,
    /// reader thread 가 EOF/force-detach 를 만나면 set. apply 가 보고 mirror 정리.
    disconnected: Arc<AtomicBool>,
    /// 원격으로 나가는 모든 프레임(입력 Data / resize·structural Control / Detach)을
    /// 단일 write 스레드로 보내는 큐 sender. 여러 forwarder/heartbeat 가 각자
    /// writer 를 lock 후 직접 쓰던 구조를 대체 — 락 경합/heartbeat 굶김 제거.
    /// attach-behavior.md#재연결-시-세션-상태-보존 참고 —
    /// 재연결 시 안쪽 sender 만 교체 가능하도록 `Arc<Mutex<_>>` 로 감쌌다(교체 이유는
    /// [`SharedFrameSender`] 문서 참고).
    frame_tx: SharedFrameSender,
    /// attach-behavior.md#재연결-시-세션-상태-보존 참고 — 이 세션의 transport 상태. `apply_attach_client_output` 이 disconnect
    /// 를 감지하면(anchor 있는 세션 한정) mirror 를 지우는 대신 여기를 `Reconnecting`
    /// 으로 전이시키고, `auto_attach.rs` 의 backoff 스케줄러가 `reconnect_session` 으로
    /// `Connected` 복귀를 시도한다.
    state: SessionState,
    // 이유: 서버가 할당한 mirror 세션 식별자 — 현재 read 경로 없음(진단/향후 프레임 라우팅용 보관).
    #[allow(dead_code)]
    client_id: u32,
    /// (06) bulk 파일 전송(ADR-0054)이 결속할 **원격** workspace id. 대화형 attach 가
    /// `open_attach_workspace` 에 넘긴 그 값 — 전용 bulk 연결의 `open_bulk` 가 이 값을
    /// 서버에 실어 "이 ws 의 holder 가 존재하는가" 인가의 근거로 삼는다(06-α 서버 검증).
    remote_workspace: u32,
    /// (06) bulk 전용 연결이 두 번째 `TcpStream::connect` 를 걸 로컬 포트. 대화형 attach
    /// 가 쓴 포트와 동일(자동 attach 는 `tunnel.local_port`, 수동/loopback 은 직접 포트) —
    /// 같은 `ssh -L` 터널/포워딩을 재사용하므로 별도 인프라가 필요 없다.
    bulk_port: u16,
    /// 단계 7 — 자동 attach 의 SSH 터널 핸들. 세션이 살아있는 동안 보관해 Drop(자식
    /// ssh kill)을 막는다. 수동 트리거(`attach.into_gui`)·loopback 은 None.
    #[allow(dead_code)]
    tunnel: Option<tasty_ssh::SshTunnel>,
    /// 단계 7 — 이 mirror 를 띄운 매핑된(anchor) 로컬 워크스페이스 id. 세션 정리 시
    /// `auto_attach_active` 에서 제거해 재활성 시 재attach 가능하게 한다. 수동 None.
    anchor_ws_id: Option<u32>,
    /// forward 한 구조 op 의 op_id 시퀀스(2단계). 회신 correlate/로그용 — 단조 증가.
    op_seq: u64,
    /// 08/09 — `user_triggered` op 중 focus 보정이 필요한 것의 `op_id → 의도`.
    /// `forward_one_structural_op` 이 전송 시 채우고, 그 op 의 성공 회신
    /// (`StructuralResult{ok:true}`)이 오면 `next_delta_focus`로 옮겨지며 제거된다.
    /// 실패 회신은 그냥 버려짐(딜타가 안 오므로 여기 남아도 다음 op 와 섞이지 않게
    /// 반드시 제거해야 한다 — 실패 시엔 애초에 삽입되지 않는 성공 전용 슬롯이라
    /// 자연히 문제없다).
    pending_op_focus: HashMap<u64, PendingOpFocus>,
    /// 방금 성공한 op 의 focus 의도 — 다음 `StructuralDelta` 적용에 1회 소비(take)된다.
    next_delta_focus: Option<PendingOpFocus>,
    /// client-driven resize(ADR-0045) 중복 전송 억제. **원격 surface_id →
    /// 마지막으로 forward 한 (cols, rows)**. 로컬 레이아웃 스윕은 매 프레임 돌고
    /// mirror grid 는 server echo 로만 갱신되므로, echo 왕복(약 1 RTT) 동안 같은
    /// 목표가 매 프레임 재계산된다 — 여기서 직전 전송값과 같으면 재전송을 생략해
    /// 네트워크 프레임 폭주를 막는다(서버측 동일값 no-op 이 2차 방어). TCP 는 신뢰
    /// 전송이라 한 번 보낸 값은 도달이 보장돼 재전송이 불필요하다.
    last_forwarded_resize: HashMap<u32, (usize, usize)>,
    /// (04) 파일 피커 원격 host 배지에 쓰이는 표시 문자열. attach 확립 시점의
    /// loopback 엔드포인트(`127.0.0.1:<port>`)로 채운다 — SSH 프로필의 실제
    /// `user@host` 는 이 세션까지 threading 되어 있지 않아(auto_attach/remote_attach
    /// 팝업 모두 `port` 만 넘김) 후속 개선 대상으로 남긴다.
    remote_label: String,
    /// (ADR-0059 참고) 아직 응답을 못 받은 `list_dir_request` 의 `request_id →
    /// 소비자 태그`(`None`=File Picker, `Some(surface_id)`=explorer). 서버 응답
    /// (`list_dir_result`)엔 이 태그가 안 실려 오므로(wire 스키마 변경 없음, ADR
    /// Decision 5), 보낼 때 여기 기록해뒀다가 응답 도착 시 꺼내 라우팅을 분기한다.
    /// 응답을 소비하면(성공/실패 무관) 제거 — stale 재사용 없음.
    pending_list_dir_consumers: HashMap<u64, Option<u32>>,
}

impl AttachClientSession {
    /// attach-behavior.md#gui-자동-재연결-스코프 참고 — `auto_attach.rs` 의 backoff 스케줄러가 재연결 후보(anchor 매핑 +
    /// `Reconnecting` 상태)를 찾는 데 쓴다. 필드가 모듈 비공개라 sibling 모듈
    /// (`auto_attach.rs`)에서 직접 접근할 수 없어 최소 getter 로 노출한다.
    pub(crate) fn state(&self) -> SessionState {
        self.state
    }

    /// attach-behavior.md#gui-자동-재연결-스코프 참고 — 이 세션이 자동 attach 매핑(anchor)에서 만들어졌는지.
    pub(crate) fn anchor_ws_id(&self) -> Option<u32> {
        self.anchor_ws_id
    }

    /// `frame_tx` 공유 핸들을 lock 해 프레임 하나를 write 큐에 넣는다. 호출부가 매번
    /// lock/에러 처리를 반복하지 않도록 모은 헬퍼(attach-behavior.md#재연결-시-세션-상태-보존 참고 — `frame_tx` 가
    /// `Arc<Mutex<_>>` 로 바뀌며 추가). mutex 오염(다른 스레드 panic)은 lock 자체를
    /// 무효화할 이유가 없어 `into_inner`로 복구해 계속 진행한다.
    fn send_frame(
        &self,
        tag: StreamTag,
        payload: Vec<u8>,
    ) -> Result<(), std::sync::mpsc::SendError<OutFrame>> {
        crate::poison::recover_mutex(self.frame_tx.lock(), FRAME_TX_WHAT, &FRAME_TX_POISONED)
            .send(OutFrame { tag, payload })
    }
}

impl App {
    /// `about_to_wait` 에서 호출 — IPC 가 쌓은 GUI attach 요청을 drain 해 실행한다.
    pub(crate) fn dispatch_pending_gui_attach(&mut self) {
        let mut reqs: Vec<(u16, u32)> = Vec::new();
        for main in self.main_windows_iter_mut() {
            reqs.append(&mut main.core_state.pending_gui_attach);
        }
        if let Some(e) = self.core_state.as_mut() {
            reqs.append(&mut e.pending_gui_attach);
        }
        for (port, workspace) in reqs {
            self.try_dispatch_one_gui_attach_ipc(port, workspace);
        }

        // 사용자 경로(remote_attach 팝업 Connect) — 조회 터널을 재사용해 attach 하고,
        // 성공 시 새 mirror ws 로 **focus 이동**(사용자 확정 동작 — 원칙 1②). IPC 경로와
        // 분리된 별도 큐라 release IPC/에이전트가 이 focus 이동 경로를 탈 수 없다.
        let mut user_reqs: Vec<crate::core::GuiAttachUserReq> = Vec::new();
        for main in self.main_windows_iter_mut() {
            user_reqs.append(&mut main.core_state.pending_gui_attach_user);
        }
        if let Some(e) = self.core_state.as_mut() {
            user_reqs.append(&mut e.pending_gui_attach_user);
        }
        for req in user_reqs {
            self.try_dispatch_one_gui_attach_user(req);
        }
    }

    /// GUI mirror attach 의 self(loopback) 게이트 — `port` 가 **이 인스턴스 자신의 IPC
    /// 포트**면 거부하고 `true`. 원격 GUI mirror 는 `ssh -L` 터널의 local_port(자기 IPC
    /// 포트와 다름)라 그대로 통과한다.
    ///
    /// **debug/release 양쪽에서 거부한다.** 근거가 둘이다:
    ///
    /// 1. **구조적으로 성립할 수 없다.** 이 경로의 핸드셰이크(`attach_handshake`)는 GUI
    ///    **메인 스레드에서 동기 블로킹**으로 돈다. 그런데 그 응답(`attached_workspace`
    ///    디스크립터)을 만드는 것도 같은 메인 스레드다(accept 스레드는 요청을 큐에 넣을
    ///    뿐, 점유·디스크립터는 메인 루프가 적용한다). 자기 자신을 대상으로 하면 메인
    ///    스레드가 자기가 만들어야 할 응답을 기다리며 막혀 **반드시 실패**한다 —
    ///    heartbeat Ping 을 먼저 받거나 read timeout 이 터진다.
    /// 2. 실패하는 동안 **서버 쪽 점유만 잡힌다.** 그 workspace 는 실패가 정리될 때까지
    ///    정상 attach 를 `already_attached` 로 거절한다.
    ///
    /// 즉 debug 에서 열어 두어도 얻는 기능이 없고 점유 사고만 남는다. 로컬 self-mirror
    /// 검증 수단은 그대로 남는다 — `tasty debug attach` 는 **별도 프로세스**의 raw attach
    /// client(`crates/tasty-cli/src/local/debug/attach.rs`)라 이 메인 스레드 교착과 무관하다.
    /// 원칙 1 ②(사용자 입력 재현은 debug 격리) 판단은 그대로 유지되며, 이 게이트는 거기에
    /// "성립 불가 + 점유 사고" 라는 별개 근거를 더해 무조건 거부로 올린 것이다.
    /// 상세: `docs/adr/0116-attach-handshake-validated-before-occupancy.md`.
    fn reject_self_attach(&self, port: u16, label: &str) -> bool {
        if self.hub.ipc_server.as_ref().map(|s| s.port()) != Some(port) {
            return false;
        }
        tracing::warn!(
            "self(loopback) {label} (port={port}) 는 차단됩니다 — 자기 자신을 mirror 하는 \
             attach 는 메인 스레드가 자기 응답을 기다리며 교착돼 성립할 수 없고, 실패하는 \
             동안 그 workspace 점유만 잡습니다. 로컬 self-mirror 는 `tasty debug attach`."
        );
        true
    }

    /// IPC(`attach.into_gui`) 경로 한 건을 처리한다.
    fn try_dispatch_one_gui_attach_ipc(&mut self, port: u16, workspace: u32) {
        if self.reject_self_attach(port, "attach.into_gui") {
            return;
        }
        if let Err(e) = self.start_gui_attach(port, workspace, None, None) {
            tracing::warn!("gui attach failed (port={port}, ws={workspace}): {e}");
        }
    }

    /// 사용자 경로(remote_attach 팝업 Connect) 한 건을 처리한다 — 성공 시 새 mirror
    /// workspace 로 focus 이동.
    fn try_dispatch_one_gui_attach_user(&mut self, req: crate::core::GuiAttachUserReq) {
        if self.reject_self_attach(req.port, "remote-attach") {
            return;
        }
        match self.start_gui_attach(req.port, req.workspace, req.tunnel, None) {
            Ok(ws_id) => self.focus_mirror_workspace(ws_id),
            Err(e) => tracing::warn!(
                "remote-attach failed (port={}, ws={}): {e}",
                req.port,
                req.workspace
            ),
        }
    }

    /// 원격 tasty(loopback `port`)의 `workspace` 를 mirror 로 재구성해 GUI 에 띄운다.
    /// loopback 연결+핸드셰이크는 near-instant 라 동기 처리.
    ///
    /// 단계 7 자동 attach(`auto_attach.rs`)는 SSH 터널을 먼저 세워 그 `tunnel.local_port`
    /// 를 `port` 로 넘기고 `tunnel` 핸들을 세션에 실어 Drop 을 막는다. `anchor_ws_id` 는
    /// 매핑된 로컬 워크스페이스 id(세션 정리 시 재attach 게이트 해제용). 수동 트리거는
    /// 둘 다 None.
    ///
    /// 반환값은 새로 만든 로컬 mirror workspace 의 id — 사용자 경로(remote_attach 팝업)
    /// 가 이 id 로 focus 를 옮기는 데 쓴다(IPC/자동 경로는 반환값을 무시해 focus 중립).
    pub(crate) fn start_gui_attach(
        &mut self,
        port: u16,
        workspace: u32,
        tunnel: Option<tasty_ssh::SshTunnel>,
        anchor_ws_id: Option<u32>,
    ) -> anyhow::Result<u32> {
        // 1. 연결 + 핸드셰이크 + 디스크립터 수신.
        let (conn, client_id, write_half, name, surfaces, tree) =
            attach_handshake(port, workspace, "gui attach")?;

        // 원격으로 나가는 모든 프레임을 단일 write 스레드로 직렬화하는 큐.
        // forwarder(Data)/heartbeat(Ping)/resize·structural(Control)/detach 가 이 큐에
        // push 만 하고, write 스레드 하나가 순차로 소켓에 write_frame 한다 — writer 락
        // 경합/heartbeat 굶김 원천 제거. write half 는 그 스레드가 단독 소유한다.
        let (frame_tx, frame_rx) = std::sync::mpsc::channel::<OutFrame>();
        // attach-behavior.md#재연결-시-세션-상태-보존 참고 — forwarder 가 재연결 후에도 최신 sender 를 찾을 수 있도록 공유 핸들로
        // 감싼다(교체는 `reconnect_session` 이 담당, 이 Arc 자체는 세션 수명 내내 불변).
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(frame_tx));

        // 2. focus 엔진에 mirror 구성(스코프 borrow). survivor 매핑(신규 attach 라
        //    old_map 은 빈 맵 — 전부 신규 취급)으로 로컬 id 발급 + mirror terminal +
        //    입력 sink forwarder 를 만든다(재연결과 로직 공유, `merge_survivor_mapping`).
        let local_ws_id;
        // client mirror reader thread 가 원격 출력 수신 즉시 메인 루프를 깨우는 데 쓴다
        // (실시간 갱신 — 서버 readonly 의 3초 cadence 와 분리).
        let proxy;
        let remote_to_local: HashMap<u32, u32>;
        {
            let Some(main) = self.focused_window_mut() else {
                anyhow::bail!("no focused window to host mirror workspace");
            };
            proxy = main.proxy.clone();
            let engine = &mut main.core_state;
            let ids = engine.next_ids.clone();

            let (new_map, terminal_locals, mesh_locals, explorer_locals, _newly_created) =
                merge_survivor_mapping(&HashMap::new(), &surfaces, &ids, &frame_tx, engine);
            remote_to_local = new_map;

            local_ws_id = ids.next_workspace();
            let mut ws = build_mirror_workspace(
                local_ws_id,
                &name,
                &tree,
                &ids,
                &remote_to_local,
                &terminal_locals,
                &mesh_locals,
                &explorer_locals,
            );
            // client mirror 표식 — 사이드바 이름 앞 하늘색 glyph(레일=우하단 chip)로 표시
            // (로컬 ws 와 구분; status dot 은 실행상태 전용). 상세 view.rs draw_workspace_card.
            ws.mirror = true;
            engine.workspaces.push(ws);
            main.mark_dirty();
        }

        // 3. reader thread: 원격 출력 → 버퍼(remote_id 키). EOF/force → disconnected.
        let output = MirrorOutbox::new();
        let disconnected = Arc::new(AtomicBool::new(false));

        // 3.5. write 전용 스레드: frame_rx 를 순차 소비해 소켓에 write_frame.
        spawn_attach_write_thread(
            write_half,
            frame_rx,
            disconnected.clone(),
            proxy.clone(),
            "",
        );

        spawn_attach_reader_thread(
            conn,
            output.clone(),
            disconnected.clone(),
            proxy.clone(),
            local_ws_id,
            "",
        );

        // 4. heartbeat thread: 서버측 read timeout 갱신용으로 주기적으로 Ping 송신.
        // heartbeat 는 이 연결 1 회 수명에만 스코프된다(attach-behavior.md#재연결-시-세션-상태-보존 참고) — forwarder 와
        // 달리 재연결을 가로질러 살아남지 않으므로 공유 핸들이 아닌 이 연결의 raw
        // sender 를 직접 잡는다. `disconnected` 도 이 연결 전용 Arc — 재연결 후
        // 새 연결은 별도의 새 heartbeat 스레드(새 raw sender/새 disconnected)를 띈다.
        let raw_frame_tx =
            crate::poison::recover_mutex(frame_tx.lock(), FRAME_TX_WHAT, &FRAME_TX_POISONED)
                .clone();
        spawn_attach_heartbeat_thread(raw_frame_tx, disconnected.clone());

        self.attach_client_sessions.push(AttachClientSession {
            local_workspace: local_ws_id,
            remote_to_local,
            output,
            disconnected,
            frame_tx,
            state: SessionState::Connected,
            client_id,
            remote_workspace: workspace,
            bulk_port: port,
            tunnel,
            anchor_ws_id,
            op_seq: 0,
            pending_op_focus: HashMap::new(),
            next_delta_focus: None,
            last_forwarded_resize: HashMap::new(),
            remote_label: format!("127.0.0.1:{port}"),
            pending_list_dir_consumers: HashMap::new(),
        });
        tracing::info!(
            "gui attach: mirror workspace {local_ws_id} from 127.0.0.1:{port} (remote ws {workspace})"
        );
        Ok(local_ws_id)
    }

    /// attach-behavior.md#gui-자동-재연결-스코프 / #재연결-시-세션-상태-보존 참고 — `auto_attach.rs` 의 backoff 스케줄러가 재연결 엔드포인트 해석에
    /// 성공했을 때 호출. `sess_idx` 의 `Reconnecting` 세션을 **새 연결로 재개**한다.
    /// `start_gui_attach` 와 달리 로컬 mirror workspace/터미널을 새로 만들지 않고,
    /// `merge_survivor_mapping`(survivor local id/scrollback 보존) + 이전 focus 복원을
    /// 적용한 뒤 reader/writer/heartbeat 스레드만 새로 띄운다. 입력 forwarder 는
    /// `sess.frame_tx`(교체 가능 공유 핸들)의 내용물만 갈아끼우면 재결선 없이 새 연결을
    /// 향하게 된다.
    pub(crate) fn reconnect_session(
        &mut self,
        sess_idx: usize,
        port: u16,
        tunnel: Option<tasty_ssh::SshTunnel>,
    ) -> anyhow::Result<()> {
        let (workspace, local_workspace) = {
            let sess = &self.attach_client_sessions[sess_idx];
            (sess.remote_workspace, sess.local_workspace)
        };

        // 1. 연결 + 핸드셰이크(신규 attach 와 동일 계약).
        let (conn, client_id, write_half, name, surfaces, tree) =
            attach_handshake(port, workspace, "gui reconnect")?;

        let (new_frame_tx, frame_rx) = std::sync::mpsc::channel::<OutFrame>();
        // 기존 세션이 들고 있던 **같은 Arc** 를 재사용 — 안쪽 sender 만 새 것으로 교체한다
        // (Arc 를 새로 만들면 survivor 터미널의 입력 forwarder 가 여전히 옛 Arc 를
        // 바라봐 갱신을 못 본다 — `SharedFrameSender` 문서 참고).
        let shared_frame_tx: SharedFrameSender =
            self.attach_client_sessions[sess_idx].frame_tx.clone();
        *crate::poison::recover_mutex(shared_frame_tx.lock(), FRAME_TX_WHAT, &FRAME_TX_POISONED) =
            new_frame_tx;

        // 2. survivor 매핑 + mirror 트리 in-place 교체(같은 local_ws_id → scrollback/
        //    local id 보존) + focus 복원(구조 delta 와 동일 패턴).
        let Some(wid) = self.find_main_with_workspace(local_workspace) else {
            anyhow::bail!(
                "mirror workspace {local_workspace} 가 더 이상 어느 창에도 없음 — 재연결 취소"
            );
        };
        let proxy;
        {
            // `sess`/`main` 을 같은 스코프에서 직접 field projection 으로 각각 얻는다
            // (disjoint borrow — `apply_attach_client_output` 과 동일 패턴). 이후 클로저가
            // `self` 대신 이미 로컬인 `sess` 를 캡처하게 해 자기참조 대여 문제를 피한다.
            let sess = &mut self.attach_client_sessions[sess_idx];
            let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                anyhow::bail!("window {wid:?} 가 더 이상 MainView 가 아님 — 재연결 취소");
            };
            proxy = main.proxy.clone();
            let engine = &mut main.core_state;
            let ids = engine.next_ids.clone();

            // focus 캡처(교체 전) — `apply_mirror_structural_delta` 와 동일 패턴.
            let old_focused_remote: Option<u32> = engine
                .workspaces
                .iter()
                .find(|w| w.id == local_workspace)
                .and_then(|ws| capture_focused_remote(ws, &sess.remote_to_local));

            let (new_map, terminal_locals, mesh_locals, explorer_locals, _newly_created) =
                merge_survivor_mapping(
                    &sess.remote_to_local,
                    &surfaces,
                    &ids,
                    &shared_frame_tx,
                    engine,
                );
            sess.remote_to_local = new_map;

            let Some(pos) = engine
                .workspaces
                .iter()
                .position(|w| w.id == local_workspace)
            else {
                anyhow::bail!(
                    "mirror workspace {local_workspace} 를 engine 에서 못 찾음 — 재연결 취소"
                );
            };
            let mut ws = build_mirror_workspace(
                local_workspace,
                &name,
                &tree,
                &ids,
                &sess.remote_to_local,
                &terminal_locals,
                &mesh_locals,
                &explorer_locals,
            );
            ws.mirror = true;
            if !restore_focus_after_delta(&mut ws, old_focused_remote, &sess.remote_to_local) {
                tracing::info!(
                    "gui reconnect: 이전 focus surface 를 재연결 후 트리에서 찾지 못함 — 원격 기본 focus 유지"
                );
            }
            engine.workspaces[pos] = ws;
            main.state.toasts.push(
                crate::i18n::t("attach.toast.mirror_reconnected").to_string(),
                crate::adapters::ui::ToastKind::Success,
                crate::adapters::ui::ToastScope::Window,
            );
            main.mark_dirty();
        }

        // 3. reader thread — 신규 attach 와 동일 계약(새 output 버퍼/disconnected).
        let output = MirrorOutbox::new();
        let disconnected = Arc::new(AtomicBool::new(false));

        // 3.5. write 전용 스레드 — 이 연결 1 회 수명.
        spawn_attach_write_thread(
            write_half,
            frame_rx,
            disconnected.clone(),
            proxy.clone(),
            "(재연결)",
        );

        spawn_attach_reader_thread(
            conn,
            output.clone(),
            disconnected.clone(),
            proxy.clone(),
            local_workspace,
            "(재연결)",
        );

        // 4. heartbeat thread — 이 연결 전용 raw sender(attach-behavior.md#재연결-시-세션-상태-보존 참고 — `make_mirror_surface`
        //    문서 참고, heartbeat 는 재연결을 가로질러 살아남지 않는다).
        let raw_frame_tx =
            crate::poison::recover_mutex(shared_frame_tx.lock(), FRAME_TX_WHAT, &FRAME_TX_POISONED)
                .clone();
        spawn_attach_heartbeat_thread(raw_frame_tx, disconnected.clone());

        let sess = &mut self.attach_client_sessions[sess_idx];
        sess.output = output;
        sess.disconnected = disconnected;
        sess.client_id = client_id;
        sess.bulk_port = port;
        sess.tunnel = tunnel;
        sess.op_seq = 0;
        sess.pending_op_focus.clear();
        sess.next_delta_focus = None;
        sess.last_forwarded_resize.clear();
        // ADR-0059 Decision 6 — 재연결 시 pending list_dir 요청 폐기(끊긴 연결에
        // 물려 있던 request_id 는 다시 응답이 안 온다). explorer/File Picker 쪽의
        // Loading 상태는 각자의 soft timeout 으로 알아서 ErrorConn 전이한다.
        sess.pending_list_dir_consumers.clear();
        sess.state = SessionState::Connected;
        sess.remote_label = format!("127.0.0.1:{port}");
        tracing::info!(
            "gui attach: mirror workspace {local_workspace} 재연결 성공 (remote ws {workspace})"
        );
        Ok(())
    }

    /// 사용자 경로 전용 — 새 mirror workspace 로 focus 를 옮긴다(원격 워크스페이스 추가
    /// 팝업의 Connect 확정). mirror 를 호스팅한 창의 `active_workspace` 를 그 ws 인덱스로
    /// 설정한다. IPC/자동 attach 경로는 이 함수를 호출하지 않아 focus 중립을 유지한다.
    fn focus_mirror_workspace(&mut self, ws_id: u32) {
        for main in self.main_windows_iter_mut() {
            if let Some(idx) = main
                .core_state
                .workspaces
                .iter()
                .position(|ws| ws.id == ws_id)
            {
                main.state.active_workspace = idx;
                main.mark_dirty();
                break;
            }
        }
    }

    /// `AttachClientData`(reader wake)마다 — 누적 원격 출력을 mirror Terminal 에
    /// 적용(repaint) + 끊긴 세션 정리. client mirror 는 데이터가 오는 즉시 갱신한다
    /// (로컬 워크스페이스와 동일한 반응성). `Tick::AttachView` 3초 tick 도 backstop 으로 호출.
    ///
    /// 적용 대상은 **창 있는 engine → parked engine** 순으로 찾고(`mirror_output_host`),
    /// 대상을 찾은 **뒤에야** 버퍼를 비운다 — 비우는 경로가 [`MirrorOutbox::take_for`]
    /// 하나뿐이고 그것이 `MirrorHost` 를 인자로 요구하므로, 이 함수가 순서를 지키는지와
    /// 무관하게 host 없이 꺼내는 코드는 **쓸 수가 없다**. 창이 없는 parked 상태에서도 mirror
    /// 터미널·매핑은 그 engine 안에 그대로 살아 있으므로 로컬 PTY 출력
    /// (`handle_terminal_output` 의 parked 순회)과 똑같이 즉시 적용한다 — 창 복원 시
    /// 새 창이 그 engine 을 그대로 그리므로 최소화 동안의 출력이 남아 있다. 어느
    /// engine 에도 없으면(= 고아, 같은 프레임의 `detach_orphaned_mirror_sessions` 가
    /// 세션째 정리) 버퍼를 그대로 둔다 — 먼저 꺼내면 적용 대상이 없을 때 되돌릴 수 없다. 순회 범위는 고아 판정
    /// (`mirror_workspace_engine_alive`)·정리(`cleanup_mirror_workspace`)와 같아야 한다
    /// (ADR-0110).
    pub(crate) fn apply_attach_client_output(&mut self) {
        if self.attach_client_sessions.is_empty() {
            return;
        }
        let mut dead: Vec<usize> = Vec::new();
        let mut reconnecting: Vec<usize> = Vec::new();
        for idx in 0..self.attach_client_sessions.len() {
            let (local_ws, disconnected, state, anchor_ws_id) = {
                let sess = &self.attach_client_sessions[idx];
                (
                    sess.local_workspace,
                    sess.disconnected.load(Ordering::SeqCst),
                    sess.state,
                    sess.anchor_ws_id,
                )
            };

            // 적용 대상 탐색이 버퍼를 비우는 것보다 **앞** — 대상이 없으면 건드리지 않는다.
            let host = mirror_output_host(
                self.find_main_with_workspace(local_ws),
                &self.parked_states,
                local_ws,
            );
            // 세션(remote→local 매핑)과 그 mirror 를 호스팅하는 engine 을 **분리
            // 대여**(self 의 서로 다른 필드 → disjoint borrow). delta 가 매핑을
            // 갱신하므로 clone 이 아닌 **라이브 매핑**을 써야 같은 drain 안의 이후
            // Data 가 새 surface 로 라우팅된다.
            //
            // 아래 `as_main_mut()` 같은 2차 조회가 실패해도 이미 꺼낸 이벤트가 버려지는
            // 일은 없다 — 그 시점엔 아직 꺼내지 않았고, 꺼내려면 host 값이 있어야 하기
            // 때문이다(`MirrorOutbox::take_for`). 유실이 생기면 조용히 생긴다(ADR-0110).
            match host {
                Some(MirrorOutputHost::Window(wid)) => {
                    let sess = &mut self.attach_client_sessions[idx];
                    let mut main = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut());
                    let mirror_host = main
                        .as_mut()
                        .map(|m| MirrorHost::windowed(&mut m.state, &mut m.core_state));
                    let applied =
                        apply_pending_mirror_output(sess, mirror_host, &mut self.plugin_manager);
                    if applied && let Some(main) = main {
                        main.mark_dirty_from(crate::view::RepaintSource::AttachMirror);
                    }
                }
                Some(MirrorOutputHost::Parked(pidx)) => {
                    // 창이 없으니 repaint 대상도 없다 — 복원 시 새 창이 이 engine 의
                    // 터미널 grid 를 그대로 그린다.
                    let sess = &mut self.attach_client_sessions[idx];
                    let (state, engine) = &mut self.parked_states[pidx];
                    apply_pending_mirror_output(
                        sess,
                        Some(MirrorHost::parked(state, engine)),
                        &mut self.plugin_manager,
                    );
                }
                None => {}
            }
            // `state` 를 같이 확인하는 이유: `disconnected` atomic 은 한 번 true 가 되면
            // 다음 성공적 재연결 전까지 계속 true 로 남는다(리더/write 스레드가 리셋하지
            // 않음) — `Connected` 일 때만 "방금 처음 감지"로 보고 1 회 반응, 이미
            // `Reconnecting` 이면 매 프레임 재처리하지 않는다(attach-behavior.md#재연결-시-세션-상태-보존 참고).
            if disconnected && state == SessionState::Connected {
                // anchor(자동 attach 매핑) 가 있으면 재연결 가능 후보 — mirror 를 지우지
                // 않고 Reconnecting 으로 전이(attach-behavior.md#gui-자동-재연결-스코프 / #재연결-시-세션-상태-보존 참고). 없으면(수동/IPC attach) 재연결
                // 트리거 소스가 없으므로 기존처럼 완전 정리.
                if anchor_ws_id.is_some() {
                    reconnecting.push(idx);
                } else {
                    dead.push(idx);
                }
            }
        }
        for &idx in &reconnecting {
            self.enter_reconnecting(idx);
        }
        for &idx in dead.iter().rev() {
            let sess = self.attach_client_sessions.remove(idx);
            self.cleanup_mirror_workspace(&sess, true);
        }
    }

    /// attach-behavior.md#gui-자동-재연결-스코프 / #재연결-시-세션-상태-보존 참고 — disconnect 가 처음 감지된 anchor-매핑 세션을 mirror workspace/
    /// 터미널을 살려둔 채 `Reconnecting` 으로 전이시킨다(완전 정리 대신). `auto_attach.rs`
    /// 의 backoff 스케줄러가 이 상태의 세션을 찾아 `reconnect_session` 재시도를 건다.
    fn enter_reconnecting(&mut self, idx: usize) {
        let (anchor, local_workspace) = {
            let sess = &mut self.attach_client_sessions[idx];
            sess.state = SessionState::Reconnecting;
            (sess.anchor_ws_id, sess.local_workspace)
        };
        // 재진입 대기 등록 — 기존 엣지(워크스페이스 전환) 트리거와 신규 backoff 트리거가
        // 둘 다 이 집합을 게이트로 쓴다(auto_attach.rs).
        if let Some(anchor) = anchor {
            self.auto_attach_active.remove(&anchor);
            self.auto_attach_pending_reactivation.insert(anchor);
        }
        for main in self.main_windows_iter_mut() {
            if main
                .core_state
                .workspaces
                .iter()
                .any(|ws| ws.id == local_workspace)
            {
                main.state.toasts.push(
                    crate::i18n::t("attach.toast.mirror_reconnecting").to_string(),
                    crate::adapters::ui::ToastKind::Warning,
                    crate::adapters::ui::ToastScope::Window,
                );
                main.mark_dirty();
                break;
            }
        }
        tracing::info!(
            "gui attach: mirror workspace {local_workspace} 재연결 대기(Reconnecting) 진입 (anchor {anchor:?})"
        );
    }

    /// 끊긴(force-detach/EOF) 세션의 mirror workspace + mirror terminal 을 제거한다.
    /// 원칙 1①: 서버의 force-detach 가 client 의 *닫힌항목 히스토리/포커스* 를 건드리지
    /// 않게 — mirror workspace 만 제거하고 active index 만 클램프한다.
    ///
    /// `from_disconnect`: 이 정리가 **원격발 disconnect**(EOF/force-detach/heartbeat
    /// TTL — `apply_attach_client_output` 호출)로 일어났으면 `true`, **로컬 사용자가
    /// mirror workspace 자체를 닫은** 경로(`detach_orphaned_mirror_sessions` 호출)면
    /// `false`. `true`일 때만 anchor 를 `auto_attach_pending_reactivation` 에 넣어
    /// `maybe_trigger_auto_attach` 가 워크스페이스 전환(엣지) 전까지 조용한 자동
    /// 재연결을 억제하게 한다 — 사용자가 명시적으로 mirror ws 를 닫은 경우는 "재진입
    /// 대기" 의미가 없어(사용자 스스로 걷어낸 것) 게이팅 대상이 아니다.
    fn cleanup_mirror_workspace(&mut self, sess: &AttachClientSession, from_disconnect: bool) {
        log_mirror_cleanup(sess, from_disconnect);
        // 창이 있는 engine(MainView) → parked engine 순으로 찾는다. parked 도
        // 순회하는 이유: 마지막 창을 닫거나(macOS 는 최소화도) engine 이 `parked_states`
        // 로 옮겨가도 mirror 워크스페이스와 그 터미널·busy·mesh 엔트리는 그 engine 안에
        // 그대로 살아 있다. main 만 훑으면 정리가 통째로 스킵돼 나중에 그 engine 이
        // 창에 다시 실릴 때 아무 데도 연결되지 않은 mirror 워크스페이스가 되살아난다.
        // 이 순회 범위는 `mirror_workspace_engine_alive`(고아 판정)·`mirror_output_host`
        // (mirror 이벤트 적용 대상 탐색)와 **같아야** 한다 — 판정이 "살아 있다"고 본 곳을
        // 정리가 못 찾으면 잔류가 생기고, 적용이 못 찾으면 그 구간의 출력이 유실된다
        // (ADR-0110).
        let mut removed = false;
        for main in self.main_windows_iter_mut() {
            if remove_mirror_workspace_from_engine(
                &mut main.core_state,
                &mut main.state,
                sess.local_workspace,
                &sess.remote_to_local,
            ) {
                // 원격발 disconnect(EOF/force-detach/heartbeat TTL/ write 실패 승격)로 mirror
                // 가 정리될 때만 사용자에게 통지한다. 사용자가 mirror ws 를 직접
                // 닫은 경로(from_disconnect=false)는 스스로 걷어낸 것이라 toast 하지 않는다.
                if from_disconnect {
                    main.state.toasts.push(
                        crate::i18n::t("attach.toast.mirror_disconnected").to_string(),
                        crate::adapters::ui::ToastKind::Warning,
                        crate::adapters::ui::ToastScope::Window,
                    );
                }
                main.mark_dirty();
                removed = true;
                break;
            }
        }
        if !removed {
            // parked engine 에는 창이 없어 toast 를 띄울 표면이 없다. 토스트는 수명이
            // wall-clock 기준이라 창 복원 시점엔 이미 만료돼 보이지도 않으므로 쌓지
            // 않는다(`mark_dirty` 도 대상 창이 없어 불필요) — 그래서 main 루프와 달리
            // 순회 전체를 순수 헬퍼로 뺄 수 있다.
            remove_mirror_workspace_from_parked(
                &mut self.parked_states,
                sess.local_workspace,
                &sess.remote_to_local,
            );
        }
        // 원격발 disconnect 로 mirror 가 사라지면, 그 순간 진행 중이던
        // git-viewer 원격 요청은 응답이 영영 오지 않아 popup 이 "Loading…" 에 무한정
        // 멈출 수 있다 — sentinel 로 강제 abandon 을 알린다(자세한 이유는 함수 doc).
        if from_disconnect {
            self.notify_git_viewer_mirror_lost();
        }
        // heartbeat 스레드 종료 신호 — 사용자 close 경로(disconnected 가 아직 false)도
        // 포함해 여기서 항상 set. 안 하면 그 스레드가 writer(Arc) 를 계속 붙들어 세션이
        // 이미 정리된 뒤에도 소켓이 살아있고 Ping 이 무의미하게 계속 나간다.
        sess.disconnected.store(true, Ordering::SeqCst);
        // 원격에 detach 통지(best-effort). write 큐로 보내 write 스레드가 쓴다. 종료
        // 경로라 send 실패(write 스레드 이미 종료)는 의도적 무시.
        let _ = sess.send_frame(StreamTag::Detach, Vec::new()); // 종료 경로 best-effort — write 스레드가 이미 죽었으면 무시(의도적)
        // 단계 7 — 자동 attach 였다면 anchor 게이트 해제(재활성 시 재attach 가능).
        if let Some(anchor) = sess.anchor_ws_id {
            self.auto_attach_active.remove(&anchor);
            // attach-behavior.md#gui-자동-재연결-스코프 참고 — 완전 정리되는 세션은 더 이상 backoff 재시도 대상이 아니다(스케줄
            // 슬롯이 있었다면 제거). `dead`(anchor 없음) 경로에선 애초에 슬롯이 없어 no-op.
            self.auto_attach_reconnect.remove(&anchor);
            // disconnect 발 정리만 재진입 대기로 표시 — 사용자가 mirror ws 를 직접
            // 닫은 경로(from_disconnect=false, 예: Reconnecting 중인 mirror 를 사용자가
            // 스스로 닫음)는 "재진입 대기"/자동 재시도 대상이 아니다(위 함수 docstring 참고).
            if from_disconnect {
                self.auto_attach_pending_reactivation.insert(anchor);
            } else {
                self.auto_attach_pending_reactivation.remove(&anchor);
            }
        }
        // 터널 핸들(sess.tunnel)은 여기서 Drop → 자식 ssh kill(고아 터널 방지).
    }

    /// `about_to_wait` 에서 호출 — 사용자가 mirror 워크스페이스 **자체를 닫으면**
    /// (context menu / 단축키 `close_workspace`) 로컬 워크스페이스는 즉시 사라지지만
    /// 그 워크스페이스를 mirror 하던 attach 세션은 남는다. 세션 소켓이 열린 채라
    /// 원격에 `Detach` 가 전달되지 않고 원격의 hard workspace 점유가 해제되지 않아
    /// 재연결 시 "사용 중"으로 남는다. 세션의 `local_workspace` 를 들고 있는 engine 이
    /// **하나도 살아 있지 않으면**(`mirror_workspace_engine_alive`) 고아로 보고
    /// `cleanup_mirror_workspace` 로 정리한다 — `Detach` 통지 → 원격이 `Disconnected`
    /// 로 점유 해제 + anchor 게이트 해제 + 터널 kill. disconnected (EOF/force-detach)
    /// 정리와 동형이되, 트리거가 **로컬 사용자 close** 인 경로다.
    /// 세션 push 는 항상 mirror workspace 생성(같은 동기 함수) 뒤라 attach 셋업 중
    /// false-positive 고아는 발생하지 않는다.
    ///
    /// 판정 기준은 "창이 있는가"가 아니라 "engine 이 살아 있는가"다 — 창이 하나도 없는
    /// parked 상태(마지막 창 닫기 / macOS 최소화)는 engine 이 `parked_states` 에 그대로
    /// 살아 있으므로 고아가 아니다. 창 유무로 판정하면 사용자가 창을 최소화했을 뿐인데
    /// 원격 attach 점유가 조용히 풀린다.
    pub(crate) fn detach_orphaned_mirror_sessions(&mut self) {
        if self.attach_client_sessions.is_empty() {
            return;
        }
        // (idx, local_workspace) 를 먼저 수집한 뒤 존재 여부를 조회 — iter 대여를
        // 들고 mirror_workspace_engine_alive(&self) 를 부르지 않도록 분리.
        let orphaned: Vec<usize> = self
            .attach_client_sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| (idx, s.local_workspace))
            .filter(|&(_, ws)| !self.mirror_workspace_engine_alive(ws))
            .map(|(idx, _)| idx)
            .collect();
        for &idx in orphaned.iter().rev() {
            let sess = self.attach_client_sessions.remove(idx);
            self.cleanup_mirror_workspace(&sess, false);
        }
    }

    /// `about_to_wait` 에서 호출 — `Core::apply` 가 mirror 워크스페이스 구조 op 를 쌓은
    /// forward 큐를 drain 해 원격에 전송한다(2단계). 각 op 의 anchor 로컬 surface id 를
    /// 세션 매핑으로 원격 id 로 치환한 뒤 attach stream 의 `StreamTag::Control` 로 보낸다.
    /// 로컬은 이미 mutation 이 차단됐고(요청/응답), 원격 실행 결과는 reader 가 받는
    /// `StructuralResult`(실패 시 toast)로 반영된다.
    pub(crate) fn dispatch_pending_structural_forwards(&mut self) {
        let mut pending: Vec<crate::core::PendingStructuralForward> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.append(&mut main.core_state.pending_structural_forward);
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.append(&mut e.pending_structural_forward);
        }
        for local_op in pending {
            self.forward_one_structural_op(local_op);
        }
    }

    /// forward 큐의 op 하나를 담당 mirror 세션으로 전송한다. anchor 로컬 surface 를 가진
    /// 세션을 찾아 local→remote 치환 후 `StructuralOp` 프레임을 write half 로 보낸다.
    /// 세션을 못 찾으면(예상 밖) warn 후 drop.
    ///
    /// `user_triggered`(08/09)면, 이 op 의 op_id 에 대응하는 focus 의도(`PendingOpFocus`)
    /// 를 세션에 등록해둔다 — 성공 회신(`StructuralResult{ok:true}`) 이 오면 그 직후
    /// (프로토콜 보장) 도착하는 `StructuralDelta` 적용 시 소비된다. `close_focus_
    /// candidates`(로컬 id)는 여기서 anchor 와 같은 방식으로 원격 id 로 치환한다 —
    /// 매핑에 없는(예상 밖) 후보는 조용히 걸러진다.
    fn forward_one_structural_op(&mut self, pending: crate::core::PendingStructuralForward) {
        let crate::core::PendingStructuralForward {
            op: local_op,
            user_triggered,
            close_focus_candidates,
        } = pending;
        let local_anchor = local_op.anchor_surface_id();
        let Some((sess, remote_anchor)) = find_mirror_session_and_remote_id(
            &mut self.attach_client_sessions,
            local_anchor,
            "structural",
        ) else {
            return;
        };
        let wire = local_op.with_anchor_surface_id(remote_anchor);
        let op_id = sess.op_seq;
        sess.op_seq += 1;

        if user_triggered
            && let Some(intent) =
                pending_op_focus_for(&local_op, &close_focus_candidates, &sess.remote_to_local)
        {
            sess.pending_op_focus.insert(op_id, intent);
        }

        let payload = serde_json::to_vec(&StreamControl::StructuralOp { op_id, op: wire })
            .unwrap_or_default();
        // write 큐로 보내 write 스레드가 순차로 쓴다(락 직접 획득 제거).
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("structural forward: write 큐 send 실패(세션 종료 중) — drop: {e}");
        }
    }

    /// `about_to_wait` 에서 호출 — `Core::resize_all_terminals` 의 로컬 레이아웃
    /// 스윕이 mirror(detached) 터미널마다 쌓은 client-driven resize 큐를 drain 해
    /// 원격에 forward 한다(ADR-0045). 각 로컬 surface id 를 세션 매핑으로 원격 id 로
    /// 치환하고, 세션의 last-forwarded dedup 을 통과한 것만 `StreamControl::ClientResize`
    /// 로 보낸다. 로컬 mirror grid 는 여기서 건드리지 않는다 — server 의 `Resize`
    /// echo 가 유일한 갱신원(desync 방지).
    pub(crate) fn dispatch_pending_resize_forwards(&mut self) {
        let mut pending: Vec<(u32, usize, usize)> = Vec::new();
        for main in self.main_windows_iter_mut() {
            for (sid, (cols, rows)) in main.core_state.pending_resize_forward.drain() {
                pending.push((sid, cols, rows));
            }
        }
        if let Some(e) = self.core_state.as_mut() {
            for (sid, (cols, rows)) in e.pending_resize_forward.drain() {
                pending.push((sid, cols, rows));
            }
        }
        for (local_sid, cols, rows) in pending {
            self.forward_one_resize(local_sid, cols, rows);
        }
    }

    /// resize 큐의 항목 하나를 담당 mirror 세션으로 전송한다. 로컬 mirror surface 를
    /// 보유한 세션을 찾아 local→remote 치환 후, 직전 전송값과 다르면
    /// `ClientResize` 프레임을 write half 로 보낸다(같으면 생략 — coalesce).
    /// 세션/원격 id 를 못 찾으면(예상 밖) warn 후 drop.
    fn forward_one_resize(&mut self, local_sid: u32, cols: usize, rows: usize) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.attach_client_sessions,
            local_sid,
            "resize",
        ) else {
            return;
        };
        // dedup: 직전 forward 와 같은 (cols, rows)면 재전송 생략(coalesce).
        if sess.last_forwarded_resize.get(&remote_sid) == Some(&(cols, rows)) {
            return;
        }
        let payload = serde_json::to_vec(&StreamControl::ClientResize {
            surface_id: remote_sid,
            cols,
            rows,
        })
        .unwrap_or_default();
        // write 큐로 보내 write 스레드가 순차로 쓴다(락 직접 획득 제거).
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("resize forward: write 큐 send 실패(세션 종료 중) — drop: {e}");
            return;
        }
        sess.last_forwarded_resize.insert(remote_sid, (cols, rows));
    }

    /// `about_to_wait` 에서 호출 — (04) 파일 피커 popup wrapper 가 쌓은 원격
    /// 디렉토리 목록 forward 큐(`CoreState::pending_list_dir_forward`)를 drain 해
    /// 각 요청을 해당 mirror 세션의 attach 채널로 전송한다(구조 op/resize forward 와
    /// 동일한 "domain 이 큐에 push, App 이 drain 해 소켓 IO" 패턴). 세션을 못 찾으면
    /// (예: 그 사이 세션이 정리됨) warn 후 drop — 응답을 못 받으므로 popup 은 자체
    /// soft timeout 으로 `ErrorConn` 전이한다.
    pub(crate) fn dispatch_pending_list_dir_forwards(&mut self) {
        let mut pending: Vec<crate::core::PendingListDirForward> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.append(&mut main.core_state.pending_list_dir_forward);
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.append(&mut e.pending_list_dir_forward);
        }
        for req in pending {
            if let Err(e) =
                self.send_list_dir_request(req.local_ws_id, req.request_id, &req.dir, req.consumer)
            {
                tracing::warn!(
                    "list_dir_request send 실패 (mirror ws {}, request {}): {e}",
                    req.local_ws_id,
                    req.request_id
                );
            }
        }
    }

    /// `about_to_wait` 에서 호출 — `git_viewer.query` IPC 핸들러가 쌓은
    /// 원격 git 조회 forward 큐(`CoreState::pending_git_query_forward`)를 drain 해
    /// 각 요청을 해당 mirror 세션의 attach 채널로 전송한다
    /// (`dispatch_pending_list_dir_forwards` 와 동형). 세션을 못 찾으면(예: mirror
    /// workspace 소멸/재연결 중) ADR-0053 과 동일하게 **soft timeout 을 기다리지
    /// 않고 즉시** 실패 결과를 plugin 에 회신한다(무한 로딩 없음).
    pub(crate) fn dispatch_pending_git_query_forwards(&mut self) {
        let mut pending: Vec<crate::core::PendingGitQueryForward> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.append(&mut main.core_state.pending_git_query_forward);
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.append(&mut e.pending_git_query_forward);
        }
        for req in pending {
            let send_result = self.send_git_query_request(
                req.local_surface_id,
                req.request_id,
                req.kind,
                req.worktree_path.as_deref(),
                req.diff_path.as_deref(),
            );
            if let Err(e) = send_result {
                tracing::warn!(
                    "git_query_request send 실패 (local surface {}, request {}): {e}",
                    req.local_surface_id,
                    req.request_id
                );
                self.fail_pending_git_query(req.request_id, req.kind, &e.to_string());
            }
        }
    }

    /// (ADR-0056 참고) 원격 전송 자체가 실패한 git 조회 요청을 plugin 에 `ok:false` 로
    /// 즉시 회신하고, 열려 있는 git-viewer popup 인스턴스에 강제 repaint 를
    /// 예약한다(`apply_attach_client_output`의 `MirrorEvent::GitQueryResult` 성공
    /// 경로와 동형 — 여기는 attach 응답 자체가 오지 않는 케이스라 host 가 직접
    /// 합성한다).
    fn fail_pending_git_query(
        &mut self,
        request_id: u64,
        kind: crate::adapters::production::stream_hub::GitQueryKind,
        reason: &str,
    ) {
        self.broadcast_git_query_reply(serde_json::json!({
            "request_id": request_id,
            "ok": false,
            "kind": kind.as_wire_str(),
            "data": serde_json::Value::Null,
            "truncated": false,
            "reason": reason,
        }));
    }

    /// (ADR-0056 참고) mirror workspace 가 disconnect 로 정리될 때 호출 — 그 시점에 진행
    /// 중이던 git-viewer 원격 요청이 있으면 응답이 영영 오지 않아 popup 이
    /// "Loading…" 에 무한정 멈춘다. host 는 plugin 내부 pending 상태(어떤
    /// `request_id` 를 기다리는지)를 모르므로, `request_id = 0`(실제 발급은 1부터 —
    /// `next_git_query_request_id`) 를 "지금 뭔가 기다리고 있다면 무조건 버려라"
    /// sentinel 로 쓴다(plugin `apply_remote_reply` 가 해석). 여러 mirror workspace
    /// 를 동시에 쓰는 중이면 다른(살아있는) workspace 의 git-viewer popup 까지 함께
    /// 리셋될 수 있는 보수적 근사다 — git-viewer 는 단일 primary popup 인스턴스만
    /// 활성 조회를 하므로 실질적으로는 그 하나만 영향받는다.
    fn notify_git_viewer_mirror_lost(&mut self) {
        self.broadcast_git_query_reply(serde_json::json!({
            "request_id": 0,
            "ok": false,
            "kind": "",
            "data": serde_json::Value::Null,
            "truncated": false,
            "reason": "mirror workspace disconnected",
        }));
    }

    /// `fail_pending_git_query`/`notify_git_viewer_mirror_lost` 공용 — payload 를
    /// git-viewer plugin 에 unicast 하고 열려 있는 모든 인스턴스에 강제 repaint 를
    /// 예약한다.
    fn broadcast_git_query_reply(&mut self, payload: serde_json::Value) {
        let git_viewer_instances: Vec<u64> = match self.plugin_manager.as_mut() {
            Some(mgr) => {
                mgr.emit_host_event_to_plugin(
                    GIT_VIEWER_PLUGIN_ID,
                    GIT_VIEWER_QUERY_RESULT_EVENT,
                    &payload,
                    tasty_plugin_protocol::EventScope::System,
                );
                mgr.popup_instances()
                    .filter(|(_, inst)| inst.plugin_id == GIT_VIEWER_PLUGIN_ID)
                    .map(|(iid, _)| iid)
                    .collect()
            }
            None => Vec::new(),
        };
        if git_viewer_instances.is_empty() {
            return;
        }
        for main in self.main_windows_iter_mut() {
            for iid in &git_viewer_instances {
                main.state.plugin_mesh_popup_pending_repaint.insert(*iid);
            }
        }
    }

    /// `about_to_wait` 에서 호출 — attach mesh mirror pane 의 redraw 스윕
    /// (`forward_attach_mesh_context`)이 geometry/theme/focus 변경을 감지해 쌓은
    /// 로컬 surface_id 큐(attach-behavior.md#mesh-mirror-채널, "App/CoreState 경계를 건너는 forward-queue 패턴" 참고)를 drain 해 원격에 `MeshContext` 를 forward한다.
    /// `dispatch_pending_resize_forwards` 와 동형.
    pub(crate) fn dispatch_pending_mesh_context_forwards(&mut self) {
        let mut pending: Vec<(u32, crate::core::AttachMeshContextForward)> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.extend(main.core_state.pending_mesh_context_forward.drain());
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.extend(e.pending_mesh_context_forward.drain());
        }
        for (local_sid, ctx) in pending {
            self.forward_one_mesh_context(local_sid, ctx);
        }
    }

    /// context 큐의 항목 하나를 담당 mirror 세션으로 전송한다. `forward_one_resize`
    /// 와 동형 — 세션/원격 id 를 못 찾으면 warn 후 drop. dedup 은 클라이언트측
    /// `AttachMeshForwardState`(호출 이전 단계)가 이미 담당하므로 여기선 무조건 전송.
    fn forward_one_mesh_context(
        &mut self,
        local_sid: u32,
        ctx: crate::core::AttachMeshContextForward,
    ) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.attach_client_sessions,
            local_sid,
            "mesh context",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::MeshContext {
            surface_id: remote_sid,
            width_px: ctx.width_px,
            height_px: ctx.height_px,
            pixels_per_point: ctx.pixels_per_point,
            theme: ctx.theme,
            focused: ctx.focused,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("mesh context forward: write 큐 send 실패(세션 종료 중) — drop: {e}");
        }
    }

    /// `about_to_wait` 에서 호출 — attach mesh mirror pane 위 로컬 입력을 누적한
    /// 큐(attach-behavior.md#mesh-mirror-채널, "App/CoreState 경계를 건너는 forward-queue 패턴" 참고)를 drain 해 원격에 `MeshInput` 을 forward한다.
    pub(crate) fn dispatch_pending_mesh_input_forwards(&mut self) {
        let mut pending: Vec<(u32, tasty_plugin_protocol::protocol::RawInputWire)> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.extend(main.core_state.pending_mesh_input_forward.drain());
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.extend(e.pending_mesh_input_forward.drain());
        }
        for (local_sid, input) in pending {
            self.forward_one_mesh_input(local_sid, input);
        }
    }

    /// 입력 큐의 항목 하나를 담당 mirror 세션으로 전송한다. `forward_one_mesh_context`
    /// 와 동형.
    fn forward_one_mesh_input(
        &mut self,
        local_sid: u32,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    ) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.attach_client_sessions,
            local_sid,
            "mesh input",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::MeshInput {
            surface_id: remote_sid,
            input,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("mesh input forward: write 큐 send 실패(세션 종료 중) — drop: {e}");
        }
    }

    /// `about_to_wait` 에서 호출 — GPU 렌더 prepare 가 attach mesh mirror surface 의
    /// 텍스처 delta 체인 단절을 감지해 쌓은 로컬 surface_id 큐(attach-behavior.md#mesh-mirror-채널 참고)를 drain 해
    /// 원격에 `MeshFullResendRequest` 를 forward 한다. `dispatch_pending_resize_forwards`
    /// 와 동형.
    pub(crate) fn dispatch_pending_mesh_full_resend_forwards(&mut self) {
        let mut pending: Vec<u32> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.extend(main.core_state.pending_mesh_full_resend_forward.drain());
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.extend(e.pending_mesh_full_resend_forward.drain());
        }
        for local_sid in pending {
            self.forward_one_mesh_full_resend_request(local_sid);
        }
    }

    /// `about_to_wait` 에서 호출 — mirror surface 의 attention **해제 edge** 큐
    /// (`CoreState::pending_attention_clear_forward`)를 drain 해 원격에
    /// `ClientAttentionClear` 를 forward 한다. 큐에 들어가는 것은 실제로 레코드를
    /// 제거한 순간뿐이라(`CoreState::clear_attention`) 포커스를 유지해도 프레임이
    /// 반복되지 않는다. `dispatch_pending_mesh_full_resend_forwards` 와 동형.
    pub(crate) fn dispatch_pending_attention_clear_forwards(&mut self) {
        let mut pending: Vec<u32> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.extend(main.core_state.pending_attention_clear_forward.drain());
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.extend(e.pending_attention_clear_forward.drain());
        }
        for local_sid in pending {
            self.forward_one_attention_clear(local_sid);
        }
    }

    /// 해제 edge 하나를 담당 mirror 세션으로 전송한다.
    /// `forward_one_mesh_full_resend_request` 와 동형 — 세션/원격 id 를 못 찾으면
    /// warn 후 drop. 전송 실패도 drop 이다: 해제는 edge 신호라 재시도 큐를 두지
    /// 않는다(세션이 끊기는 중이면 서버측 점유도 곧 풀린다).
    fn forward_one_attention_clear(&mut self, local_sid: u32) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.attach_client_sessions,
            local_sid,
            "attention clear",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::ClientAttentionClear {
            surface_id: remote_sid,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("attention clear forward: write 큐 send 실패(세션 종료 중) — drop: {e}");
        }
    }

    /// full 재전송 요청 큐의 항목 하나를 담당 mirror 세션으로 전송한다.
    /// `forward_one_resize` 와 동형 — 세션/원격 id 를 못 찾으면 warn 후 drop.
    fn forward_one_mesh_full_resend_request(&mut self, local_sid: u32) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.attach_client_sessions,
            local_sid,
            "mesh full-resend",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::MeshFullResendRequest {
            surface_id: remote_sid,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!(
                "mesh full-resend forward: write 큐 send 실패(세션 종료 중) — drop: {e}"
            );
        }
    }
}

/// `local_sid` 를 mirror 로 보유한 세션과 그 원격 surface id 를 찾는다. 세션이
/// 없거나(로컬 surface 를 가진 세션이 없음) 원격 id 매핑이 없으면(예상 밖) `label`
/// 을 포함한 warn 로그를 남기고 `None` 을 반환한다 — `forward_one_structural_op`/
/// `forward_one_resize`/`forward_one_mesh_context`/`forward_one_mesh_input`/
/// `forward_one_mesh_full_resend_request`/`forward_one_attention_clear` 6형제가
/// 공유하는 "세션 lookup + local→remote 치환" 전처리(개별 분해 대신 공용 헬퍼로
/// 묶어 로직 drift 위험을 없앤다).
fn find_mirror_session_and_remote_id<'a>(
    sessions: &'a mut [AttachClientSession],
    local_sid: u32,
    label: &str,
) -> Option<(&'a mut AttachClientSession, u32)> {
    let Some(sess) = sessions
        .iter_mut()
        .find(|s| s.remote_to_local.values().any(|&l| l == local_sid))
    else {
        tracing::warn!(
            "{label} forward: mirror 세션이 로컬 surface {local_sid} 를 갖지 않음 — drop"
        );
        return None;
    };
    let Some(remote_sid) = sess
        .remote_to_local
        .iter()
        .find(|&(_, &l)| l == local_sid)
        .map(|(&r, _)| r)
    else {
        tracing::warn!("{label} forward: 로컬 surface {local_sid} 의 원격 id 없음 — drop");
        return None;
    };
    Some((sess, remote_sid))
}

/// 원격 tasty(loopback `port`)에 연결해 workspace attach 핸드셰이크를 수행하고,
/// write half + 파싱된 디스크립터(client_id/name/surfaces/tree)를 반환한다.
/// `start_gui_attach`/`reconnect_session` 이 공유 — 두 곳의 유일한 차이(로그 문구
/// "gui attach" vs "gui reconnect")는 `log_prefix` 로 흡수한다.
fn attach_handshake(
    port: u16,
    workspace: u32,
    log_prefix: &str,
) -> anyhow::Result<(StreamConnection, u32, TcpStream, String, Vec<Value>, Value)> {
    let sock = TcpStream::connect(("127.0.0.1", port))?;
    // 조용한 네트워크 단절 감지용 read timeout(핸드셰이크 ack/디스크립터 대기에도
    // 적용됨 — 이 함수 내 이후 `conn.recv()` 호출들이 그 대상). heartbeat 스레드
    // 가 이 주기 이내에 Ping 을 보내 idle 세션에서도 서버측 read timeout 을 갱신한다.
    if let Err(e) = sock.set_read_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("{log_prefix}: failed to set read timeout: {e}");
    }
    // write 방향 백스톱: forwarder/heartbeat 프레임 write 가 백프레셔로
    // 무기한 막히지 않도록 write timeout 을 건다. 만료(WouldBlock)는 write 스레드
    // 에서 세션 disconnect 로 승격된다. read timeout 은 silent disconnect 감지
    // 계약이라 건드리지 않는다. (try_clone_writer 로 뜬 write half 는 같은 소켓
    // fd 를 공유하므로 이 timeout 을 그대로 물려받는다.)
    if let Err(e) = sock.set_write_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("{log_prefix}: failed to set write timeout: {e}");
    }
    let (mut conn, client_id) =
        StreamConnection::open_attach_workspace(sock, STREAM_PROTO, workspace)?;
    let first = conn.recv()?;
    if first.tag != StreamTag::Control {
        anyhow::bail!("expected attach Control frame, got {:?}", first.tag);
    }
    let ctrl: Value = serde_json::from_slice(&first.payload)?;
    match ctrl.get("event").and_then(|v| v.as_str()) {
        Some("attached_workspace") => {}
        Some("attach_error") => {
            let reason = ctrl
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("workspace attach rejected: {reason}");
        }
        other => anyhow::bail!("unexpected attach control event: {other:?}"),
    }
    let name = ctrl
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("remote")
        .to_string();
    let surfaces = ctrl
        .get("surfaces")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let tree = ctrl.get("tree").cloned().unwrap_or(Value::Null);
    let write_half = conn.try_clone_writer()?;
    Ok((conn, client_id, write_half, name, surfaces, tree))
}

/// 한 engine 에서 mirror 워크스페이스의 흔적을 통째로 걷어낸다 — mirror 터미널,
/// mirror busy 엔트리, mesh 프레임 캐시, 그리고 워크스페이스 행. 워크스페이스를
/// 제거했으면 짝인 `AppState.active_workspace` 인덱스도 클램프한다(제거로
/// out-of-range 가 되는 것을 막는다).
///
/// 이 engine 이 그 워크스페이스를 들고 있지 않으면 아무것도 하지 않고 `false`.
/// `cleanup_mirror_workspace` 가 창 있는 engine 과 parked engine 양쪽에 **같은**
/// 정리를 적용하기 위해 쓰는 공용 본문이라, 소켓을 들고 있는 `AttachClientSession`
/// 의존 없이 단위 테스트할 수 있도록 원시 값만 받는다.
fn remove_mirror_workspace_from_engine(
    engine: &mut crate::core::CoreState,
    state: &mut crate::state::AppState,
    local_workspace: u32,
    remote_to_local: &HashMap<u32, u32>,
) -> bool {
    let Some(pos) = engine
        .workspaces
        .iter()
        .position(|ws| ws.id == local_workspace)
    else {
        return false;
    };
    for &local in remote_to_local.values() {
        engine.terminals.remove(local);
        engine.forget_mirror_surface_busy(local);
        engine.forget_mirror_surface_attention(local);
        engine.attach_mesh_frames.remove(local);
    }
    engine.workspaces.remove(pos);
    // 활성 포인터를 대상 기준으로 보정(제거로 인한 밀림 + out-of-range 방지).
    state.fix_workspace_pointers_after_removal(pos, engine.workspaces.len());
    true
}

/// `cleanup_mirror_workspace` 의 **parked 순회** — `App.parked_states` 를 앞에서부터
/// 훑어 그 mirror 워크스페이스를 들고 있는 첫 engine 에서 정리를 수행하고 `true`.
/// 어느 parked engine 에도 없으면 `false`(아무것도 건드리지 않는다).
///
/// 창을 여럿 닫으면 parked engine 도 여럿 쌓이므로 첫 항목에서 멈추면 안 된다.
/// `App`(GUI 의존) 없이 `parked_states` 와 같은 타입을 그대로 받아, 이 순회 자체가
/// 단위 테스트로 덮이게 한다.
fn remove_mirror_workspace_from_parked(
    parked: &mut [(crate::state::AppState, crate::core::CoreState)],
    local_workspace: u32,
    remote_to_local: &HashMap<u32, u32>,
) -> bool {
    for (state, engine) in parked.iter_mut() {
        if remove_mirror_workspace_from_engine(engine, state, local_workspace, remote_to_local) {
            return true;
        }
    }
    false
}

/// `apply_attach_client_output` 이 mirror 이벤트를 적용할 engine 의 위치 — 창 있는
/// engine(`MainView` 의 `WindowId`) 또는 창 없는 parked engine(`App.parked_states`
/// 인덱스). 어느 쪽이든 적용되는 상태는 같은 `(AppState, CoreState)` 쌍이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MirrorOutputHost {
    Window(winit::window::WindowId),
    Parked(usize),
}

/// mirror 이벤트의 적용 대상을 고른다 — **창 있는 engine 이 우선**, 없으면 parked
/// engine. 순회 범위는 고아 판정(`mirror_workspace_engine_alive`)·정리
/// (`cleanup_mirror_workspace`)와 **같다**: 판정이 살아 있다고 본 engine 에 적용이 닿지
/// 않으면 그 구간의 출력이 유실된다. `None` 은 어느 engine 에도 없다는 뜻(= 고아) —
/// 호출부는 이때 버퍼를 drain 하지 않는다. `App`(GUI 의존) 없이 순수 함수로 두어
/// 단위 테스트가 순서(창 → parked)와 부재(None)를 직접 검증한다.
fn mirror_output_host(
    windowed: Option<winit::window::WindowId>,
    parked: &[(crate::state::AppState, crate::core::CoreState)],
    local_workspace: u32,
) -> Option<MirrorOutputHost> {
    windowed.map(MirrorOutputHost::Window).or_else(|| {
        find_parked_with_workspace(parked, local_workspace).map(MirrorOutputHost::Parked)
    })
}

/// `apply_attach_client_output` 의 **parked 순회** — 그 mirror 워크스페이스를 들고 있는
/// 첫 parked engine 의 인덱스. 창을 여럿 닫으면 parked engine 도 여럿 쌓이므로 첫
/// 항목만 보면 안 된다(`remove_mirror_workspace_from_parked` 와 동형).
fn find_parked_with_workspace(
    parked: &[(crate::state::AppState, crate::core::CoreState)],
    local_workspace: u32,
) -> Option<usize> {
    parked
        .iter()
        .position(|(_, engine)| engine.has_workspace(local_workspace))
}

/// mirror 이벤트를 적용할 대상 engine — 창이 있든(`MainView` 의 `state`/`core_state`)
/// 없든(parked 튜플) 같은 `(AppState, CoreState)` 쌍이다. 창 유무는 상태 적용에는
/// 영향이 없고, toast 처럼 **창 표면이 있어야 의미 있는 부수효과**만 게이트한다.
struct MirrorHost<'a> {
    state: &'a mut crate::state::AppState,
    engine: &'a mut crate::core::CoreState,
    /// 이 engine 을 그리는 창이 지금 있는가(`MainView` 경유면 true, parked 면 false).
    windowed: bool,
}

impl<'a> MirrorHost<'a> {
    fn windowed(
        state: &'a mut crate::state::AppState,
        engine: &'a mut crate::core::CoreState,
    ) -> Self {
        Self {
            state,
            engine,
            windowed: true,
        }
    }

    fn parked(
        state: &'a mut crate::state::AppState,
        engine: &'a mut crate::core::CoreState,
    ) -> Self {
        Self {
            state,
            engine,
            windowed: false,
        }
    }

    /// 창이 있으면 window-scope toast, 없으면(parked) 로그만 남긴다 — parked engine
    /// 에는 toast 를 띄울 표면이 없고 토스트 수명이 wall-clock 기준이라 창 복원
    /// 시점엔 이미 만료돼 보이지도 않으므로 쌓지 않는다(`cleanup_mirror_workspace`
    /// 의 parked 분기와 같은 이유). 상태 변경(터미널·매핑·트리)은 이 게이트와 무관하게
    /// 항상 적용된다.
    /// 이 host 로 갈 이벤트를 버퍼에서 꺼내 **그 자리에서** 적용한다. 적용한 이벤트가
    /// 있었으면 `true`(호출부의 repaint 판단용).
    ///
    /// 꺼내는 일과 적용하는 일을 한 메서드가 쥐고, 그 메서드를 부르려면 `MirrorHost`
    /// 값이 있어야 한다 — "적용 대상 없이 꺼낸다" 가 호출 순서 약속이 아니라 **타입**
    /// 으로 불가능해지는 지점이다(ADR-0110).
    fn drain_and_apply(
        &mut self,
        sess: &mut AttachClientSession,
        plugin_manager: &mut Option<crate::plugin::PluginManager>,
    ) -> bool {
        let drained = sess.output.take_for(self);
        if drained.is_empty() {
            return false;
        }
        apply_mirror_events(sess, self, plugin_manager, drained);
        true
    }

    fn toast(&mut self, message: String, kind: crate::adapters::ui::ToastKind) {
        if self.windowed {
            self.state
                .toasts
                .push(message, kind, crate::adapters::ui::ToastScope::Window);
        } else {
            tracing::info!("attach mirror: parked engine 이라 toast 생략 — {message}");
        }
    }
}

/// 적용 대상이 **확보된 경우에만** 버퍼를 비우고 적용한다. 적용된 이벤트가 있었으면
/// `true`(호출부의 repaint 판단용).
///
/// `host` 가 `None` 이면 **아무것도 꺼내지 않는다** — 꺼낸 뒤 적용에 실패하면 되돌릴
/// 방법이 없고, mirror 이벤트의 유실은 조용히 일어난다(`Data` 는 복원 뒤 화면 결손,
/// `StructuralDelta` 는 매핑 desync). 버퍼를 그대로 두면 다음 호출(`AttachClientData`
/// wake 또는 `Tick::AttachView`)이 다시 시도한다.
///
/// 이 함수는 `Option` 을 [`MirrorHost::drain_and_apply`] 로 넘기는 얇은 어댑터일 뿐이다 —
/// 유실을 막는 것은 이 함수의 순서가 아니라 버퍼를 비우는 유일한 경로
/// ([`MirrorOutbox::take_for`])가 host 를 요구한다는 사실이다(ADR-0110).
fn apply_pending_mirror_output(
    sess: &mut AttachClientSession,
    host: Option<MirrorHost<'_>>,
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
) -> bool {
    let Some(mut host) = host else {
        return false;
    };
    host.drain_and_apply(sess, plugin_manager)
}

/// drain 한 mirror 이벤트들을 **도착 순서대로** 한 engine 에 적용한다. 창 있는 engine 과
/// parked engine 이 같은 본문을 쓴다 — 두 경로의 적용 규칙이 갈라지지 않게 하는 단일
/// 지점이며, `App` 없이 호출 가능해 parked 적용이 단위 테스트로 덮인다.
fn apply_mirror_events(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_>,
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    events: Vec<MirrorEvent>,
) {
    for ev in events {
        apply_one_mirror_event(sess, host, plugin_manager, ev);
    }
}

/// `cleanup_mirror_workspace` 진입 시 세션 식별 정보를 로깅한다 — anchor 없는
/// 세션(수동/IPC attach)의 disconnect 정리는 `enter_reconnecting` 의 info 로그로
/// 이어지지 않는 유일한 경로라 여기가 유일한 관측 지점이다. `from_disconnect`
/// 여부에 따라 원인(원격발 disconnect vs 로컬 사용자 close)이 갈리므로 레벨도
/// 그에 맞춘다(write 스레드 disconnect 승격 warn 과 일관).
fn log_mirror_cleanup(sess: &AttachClientSession, from_disconnect: bool) {
    if from_disconnect {
        tracing::warn!(
            "attach mirror cleanup: local ws {} (remote ws {}, anchor {:?}) — 원격발 disconnect 로 정리",
            sess.local_workspace,
            sess.remote_workspace,
            sess.anchor_ws_id
        );
    } else {
        tracing::info!(
            "attach mirror cleanup: local ws {} (remote ws {}, anchor {:?}) — 사용자 close 로 정리",
            sess.local_workspace,
            sess.remote_workspace,
            sess.anchor_ws_id
        );
    }
}

/// 원격으로 나가는 프레임을 직렬화해 소켓에 쓰는 write 전용 스레드를 띄운다.
/// `frame_rx`(모든 sender drop 시 자연 EOF)를 순차 소비해 `write_frame`, 실패(write
/// timeout=WouldBlock 포함, BrokenPipe 등)는 세션 disconnect 로 승격한다 —
/// 부분전송 프레임으로 서버가 프레임 경계를 잃으므로(desync) 같은 소켓 재시도
/// 없이 세션 정리로만 귀결한다. 여러 스레드가 writer 를 각자 lock 후 직접 쓰던
/// 구조를 단일화 — forwarder/heartbeat/forward 는 큐에 push 만 하므로 락 경합·
/// heartbeat 굶김이 사라진다. `start_gui_attach`/`reconnect_session` 이 공유
/// (`log_suffix` 로 로그 문구 차이만 흡수: "" vs "(재연결)").
fn spawn_attach_write_thread(
    write_half: TcpStream,
    frame_rx: std::sync::mpsc::Receiver<OutFrame>,
    disconnected: Arc<AtomicBool>,
    proxy: EventLoopProxy<AppEvent>,
    log_suffix: &'static str,
) {
    std::thread::spawn(move || {
        let mut write_half = write_half;
        for item in frame_rx {
            if let Err(e) = stream::write_frame(&mut write_half, item.tag, &item.payload) {
                tracing::warn!(
                    "attach write thread{log_suffix}: 프레임 write 실패 — 세션 disconnect 승격: {e}"
                );
                disconnected.store(true, Ordering::SeqCst);
                let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                break;
            }
        }
    });
}

/// 원격 출력을 읽어 `output` 버퍼에 쌓고 메인 루프를 깨우는 reader 스레드를
/// 띄운다. Data/Resize/Activity/Attention/StructuralFailed/StructuralSucceeded/
/// StructuralDelta/Mesh 이벤트를 `MirrorEvent` 로 변환, Detach/force_detached/
/// recv 실패는 세션 disconnect 로 승격. `start_gui_attach`/`reconnect_session`
/// 이 공유(스레드 본문이 두 곳에서 100% 동일) — `log_suffix` 로 로그 문구 차이만
/// 흡수(write 스레드와 동일 패턴: "" vs "(재연결)").
fn spawn_attach_reader_thread(
    mut conn: StreamConnection,
    output: MirrorOutbox,
    disconnected: Arc<AtomicBool>,
    proxy: EventLoopProxy<AppEvent>,
    local_workspace: u32,
    log_suffix: &'static str,
) {
    std::thread::spawn(move || {
        let mut mesh_assembler = tasty_ipc::mesh_stream::MeshFrameAssembler::new();
        loop {
            match conn.recv() {
                Ok(frame) => match frame.tag {
                    StreamTag::Data => {
                        if let Some((sid, payload)) = stream::decode_mux(&frame.payload) {
                            output.push(MirrorEvent::Data(sid, payload.to_vec()));
                        }
                        // 실시간 갱신: 데이터가 오는 즉시 메인 루프를 깨워 mirror 에
                        // 적용한다(로컬 PTY 의 TerminalOutput wake 와 동형).
                        let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                    }
                    StreamTag::Detach => {
                        disconnected.store(true, Ordering::SeqCst);
                        let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                        break;
                    }
                    StreamTag::Control => {
                        if String::from_utf8_lossy(&frame.payload).contains("force_detached") {
                            disconnected.store(true, Ordering::SeqCst);
                            let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                            break;
                        }
                        // mid-session Control: 원격 resize 통지 / forward 회신.
                        // 알 수 없는 event(구/신 스키마)는 파싱 실패 → 무시(전방 호환).
                        let mirror_ev =
                            match serde_json::from_slice::<StreamControl>(&frame.payload) {
                                Ok(StreamControl::Resize {
                                    surface_id,
                                    cols,
                                    rows,
                                }) => Some(MirrorEvent::Resize(surface_id, cols, rows)),
                                Ok(StreamControl::Activity { surface_id, busy }) => {
                                    Some(MirrorEvent::Activity(surface_id, busy))
                                }
                                Ok(StreamControl::Attention { surface_id, kind }) => {
                                    Some(MirrorEvent::Attention(surface_id, kind))
                                }
                                // 2단계: forward 실패 회신 → 실패 toast.
                                Ok(StreamControl::StructuralResult {
                                    ok: false, reason, ..
                                }) => {
                                    Some(MirrorEvent::StructuralFailed(reason.unwrap_or_default()))
                                }
                                // 성공 회신 — UX 로는 무음이지만(구조 반영은
                                // 뒤따르는 StructuralDelta), 08/09 focus 보정
                                // op 를 correlate 하려면 op_id 가 필요하다.
                                Ok(StreamControl::StructuralResult {
                                    ok: true, op_id, ..
                                }) => Some(MirrorEvent::StructuralSucceeded(op_id)),
                                // 3단계: 원격 구조 변경 역반영 → mirror 트리 재구성.
                                Ok(StreamControl::StructuralDelta {
                                    workspace_id,
                                    tree,
                                    surfaces,
                                }) => Some(MirrorEvent::StructuralDelta {
                                    workspace_id,
                                    tree,
                                    surfaces,
                                }),
                                // StreamControl 이 인식 못 하는 payload — (03)
                                // capture_result 또는 (04) list_dir_result
                                // 커스텀 이벤트인지 확인(별도 enum, StreamControl
                                // 비수정 — parse_capture_result/parse_list_dir_result 참조).
                                Ok(_) | Err(_) => parse_capture_result(&frame.payload)
                                    .or_else(|| parse_list_dir_result(&frame.payload))
                                    .or_else(|| parse_git_query_result(&frame.payload)),
                            };
                        if let Some(ev) = mirror_ev
                            && output.push(ev)
                        {
                            let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                        }
                    }
                    // heartbeat — read 자체가 이미 소켓 read timeout 을 리셋
                    // 하므로 별도 처리 불필요.
                    StreamTag::Ping => {}
                    // attach mesh mirror 청크 — frame_id 완성 시에만
                    // MirrorEvent::Mesh 를 push. 손상 청크는 조용히 버린다(다음
                    // full 재전송이 self-heal — GPU 측 chain_ok 게이트가 이미
                    // 이런 유실을 전제로 설계됨).
                    StreamTag::MeshData => {
                        if let Ok(Some((meta, bytes))) = mesh_assembler.push_chunk(&frame.payload)
                            && output.push(MirrorEvent::Mesh(
                                meta.surface_id,
                                meta.generation,
                                meta.frame_seq,
                                meta.full_textures,
                                bytes,
                            ))
                        {
                            let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                        }
                    }
                },
                Err(e) => {
                    // 조용한 네트워크 단절(케이블 단절/NAT 타임아웃 등 FIN/RST 없는 끊김)의
                    // 실질적 감지 진입점 — heartbeat TTL(HEARTBEAT_TIMEOUT) 만료로 인한
                    // read timeout 이 여기로 들어온다. write 스레드의 대칭 로그(위 참고)와
                    // 일관되게 원인(`e`)을 남긴다.
                    tracing::warn!(
                        "attach reader thread{log_suffix}: mirror workspace {local_workspace} 원격 recv 실패 — 세션 disconnect 승격: {e}"
                    );
                    disconnected.store(true, Ordering::SeqCst);
                    let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                    break;
                }
            }
        }
    });
}

/// 서버측 read timeout 갱신용으로 주기적으로 Ping 을 보내는 heartbeat 스레드를
/// 띄운다(반대 방향은 서버 write thread 의 동일 로직이 이 소켓의 read timeout 을
/// 갱신한다). 세션 정리(`cleanup_mirror_workspace`, disconnect 든 사용자 close 든)
/// 시 `disconnected` 가 set 되므로 다음 tick 에 자연 종료 — writer/소켓을 무기한
/// 붙들지 않는다. 활성 입력 트래픽과 무관하게 고정 주기로 보낸다 — Ping 프레임은
/// 5바이트라 오버헤드가 무시할 만하고, 여러 forwarder 스레드의 "마지막 전송
/// 시각"을 공유 상태로 조율하는 비용이 더 크다. 이 연결 1 회 수명에만 스코프된다
/// (attach-behavior.md#재연결-시-세션-상태-보존 참고 — heartbeat 는 재연결을
/// 가로질러 살아남지 않으므로 호출자가 공유 핸들이 아닌 이 연결의 raw sender 를
/// 직접 잡아 넘긴다). `start_gui_attach`/`reconnect_session` 이 공유.
fn spawn_attach_heartbeat_thread(raw_frame_tx: FrameSender, disconnected: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(stream::HEARTBEAT_INTERVAL);
            if disconnected.load(Ordering::SeqCst) {
                break;
            }
            // 큐에 push 만 하므로 백프레셔로 write 가 막혀도 heartbeat 는 굶지
            // 않는다. write 스레드가 사망(receiver drop)했으면 send 가
            // Err → 종료.
            if raw_frame_tx
                .send(OutFrame {
                    tag: StreamTag::Ping,
                    payload: Vec::new(),
                })
                .is_err()
            {
                break;
            }
        }
    });
}

/// 원격 surface 하나에 대응하는 mirror 터미널을 만들어 `engine` 에 삽입한다.
/// `Terminal::new_detached`(로컬 PTY 없음) + 입력 sink forwarder(로컬 키 입력 →
/// `encode_mux(remote_id)` → writer → 원격 PTY, 서버 holder+workspace 검증) + 옵저버
/// 게이트 초기화. 핸드셰이크(`start_gui_attach`)와 역반영(`merge_survivor_mapping` 을 통해
/// `apply_mirror_structural_delta`/`reconnect_session`)이 공유한다. 입력 forwarder 는
/// mirror drop(세션 정리/역반영 remove) 시 sink 채널이 끊겨 자연 종료한다.
fn make_mirror_surface(
    remote_id: u32,
    local_id: u32,
    cols: usize,
    rows: usize,
    frame_tx: &SharedFrameSender,
    engine: &mut crate::core::CoreState,
) {
    let mut mirror = Terminal::new_detached(cols, rows);
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    mirror.set_input_sink(tx);
    let frame_tx = frame_tx.clone();
    // 입력 forwarder: mpsc 로 온 각 chunk 를 MAX_FRAME_LEN-4(mux prefix 4byte) 미만
    // 조각으로 분할(paste 가 1 MiB 캡을 넘겨 write_frame 이 거부·스레드
    // 사망하던 결함)해 순차로 write 큐에 push. 단일 forwarder 스레드가 rx 를 FIFO
    // 소비하므로 bracketed paste(\x1b[200~ → text → \x1b[201~) 순서가 보존된다.
    //
    // attach-behavior.md#재연결-시-세션-상태-보존 참고 — `frame_tx` 는 공유 핸들(`SharedFrameSender`)이라 매 전송마다 lock 해
    // **그 순간의 최신** sender 를 읽는다. send 실패(transport disconnect 중 — 옛
    // write 스레드가 이미 죽었거나 아직 재연결 전)는 이 청크만 버리고 루프를
    // 계속한다 — 예전엔 `return`(스레드 종료)했지만, 그러면 재연결로 `frame_tx` 내부가
    // 새 sender 로 교체돼도 이 forwarder 가 이미 죽어 있어 survivor 터미널의 입력이
    // 영구히 원격에 닿지 못했다(Codex 크로스체크 지적). 이 스레드는 오직 `rx`(터미널
    // 자체가 drop 될 때 sink 채널이 끊김)로만 종료한다.
    std::thread::spawn(move || {
        const MAX_BODY: usize = (stream::MAX_FRAME_LEN as usize) - 4;
        for chunk in rx {
            for part in chunk.chunks(MAX_BODY) {
                let framed = stream::encode_mux(remote_id, part);
                let current = crate::poison::recover_mutex(
                    frame_tx.lock(),
                    FRAME_TX_WHAT,
                    &FRAME_TX_POISONED,
                )
                .clone();
                if current
                    .send(OutFrame {
                        tag: StreamTag::Data,
                        payload: framed,
                    })
                    .is_err()
                {
                    // 이 청크는 유실(disconnect 구간) — 다음 청크에서 재시도(재연결
                    // 되면 그때는 최신 sender 로 성공한다).
                    continue;
                }
            }
        }
    });
    // Mirror emit 은 process() 밖(feed_bytes)이라 process 진입의 lazy 게이트 동기화가
    // 닿지 않는다 — 옵저버가 먼저 등록된 경우를 위해 insert 시점에 게이트를 직접 초기화.
    mirror.set_output_events_enabled(engine.observer_router.wants(local_id));
    engine.terminals.insert(local_id, mirror);
}

/// forward 한 `user_triggered` op 하나에 대해, 성공 시 어떤 client-only focus 보정을
/// 해야 하는지(08/09). `op_id`로 세션에 등록해뒀다가 그 op 의 성공 회신 직후 도착하는
/// `StructuralDelta` 적용에서 1회 소비된다.
#[derive(Debug, Clone)]
enum PendingOpFocus {
    /// new-tab/split(08): 결과 delta 에서 새로 생긴 surface 로 focus 를 옮긴다.
    NewResource,
    /// close(09): 캡처해둔 이전 focus 가 이번 op 로 사라지면(=이번 op 이 바로 그
    /// surface/tab 을 닫은 것), 아래 후보(**remote** id, 우선순위 순) 중 delta 이후에도
    /// 살아남은 첫번째로 focus 를 옮긴다. 후보가 다 없으면 기존 동작(원격 고정값) 유지.
    Close { candidates: Vec<u32> },
}

/// `forward_one_structural_op` 이 op 하나를 세션에 실어 보내기 직전, 이 op 이 08/09
/// focus 보정 대상인지 판정한다. new-tab/split 계열은 항상 `NewResource`. close 계열은
/// `close_focus_candidates`(로컬 id, `AppState` 가 닫히기 **전** 트리에서 계산해둔 것)를
/// `remote_to_local`(전송 시점 기준 — anchor 치환과 동일 스냅샷)로 원격 id 로 치환해
/// 담는다. 치환 결과가 전부 비면(매핑에 없는 후보뿐이었으면) `None` — split/move 등
/// 대상이 아닌 op 도 `None`. `remote_to_local` 을 직접 받아(세션 전체가 아니라) 순수
/// 함수로 유지 — 테스트가 TCP 연결을 갖춘 `AttachClientSession` 없이도 검증 가능하다.
fn pending_op_focus_for(
    op: &StructuralOp,
    close_focus_candidates: &[u32],
    remote_to_local: &HashMap<u32, u32>,
) -> Option<PendingOpFocus> {
    match op {
        StructuralOp::NewTab { .. }
        | StructuralOp::SplitSurface { .. }
        | StructuralOp::SplitPane { .. } => Some(PendingOpFocus::NewResource),
        StructuralOp::CloseSurface { .. }
        | StructuralOp::CloseTab { .. }
        | StructuralOp::ClosePane { .. } => {
            let candidates: Vec<u32> = close_focus_candidates
                .iter()
                .filter_map(|local_sid| {
                    remote_to_local
                        .iter()
                        .find(|&(_, l)| l == local_sid)
                        .map(|(&r, _)| r)
                })
                .collect();
            if candidates.is_empty() {
                None
            } else {
                Some(PendingOpFocus::Close { candidates })
            }
        }
        _ => None,
    }
}

/// remote surfaces 디스크립터(구조 delta 또는 재연결 handshake)를 `old_map`(재구성
/// 전의 remote→local 매핑)과 병합한다 — survivor(= `old_map` 에 이미 있던 remote_id)는
/// **기존 local id 를 재사용**(터미널을 재생성하지 않아 scrollback/grid 보존), 신규는
/// 로컬 id 발급 + `make_mirror_surface`(터미널만), `old_map` 에는 있었지만 이번
/// surfaces 에 없는 것은 mirror 터미널을 제거한다. survivor 라도 convert 로 kind 자체가
/// 바뀌었으면(`Surface::kind()` 로 옛 kind 대조) 옛 kind 전용 로컬 리소스(Terminal
/// 객체·busy state·mesh frame 캐시)를 즉시 정리하고 새 kind 가 terminal 이면
/// `make_mirror_surface` 로 새로 만든다 — local id 는 그대로 유지한 채 리소스만 새
/// kind 에 맞춘다. `apply_mirror_structural_delta`(구조 변경 역반영)와 `reconnect_session`
/// (재연결 — attach-behavior.md#gui-자동-재연결-스코프 / #재연결-시-세션-상태-보존 참고)이 공유하는 핵심 로직 — 두
/// 시나리오 모두 "새 handshake/delta 를 기존 세션 상태에 diff 적용"이라는 점에서
/// 구조적으로 동일하다.
fn merge_survivor_mapping(
    old_map: &HashMap<u32, u32>,
    surfaces: &[Value],
    ids: &crate::core::state::IdGenerator,
    frame_tx: &SharedFrameSender,
    engine: &mut crate::core::CoreState,
) -> (
    HashMap<u32, u32>,
    HashSet<u32>,
    HashMap<u32, MirrorMeshInfo>,
    HashMap<u32, std::path::PathBuf>,
    Vec<u32>,
) {
    let mut new_map: HashMap<u32, u32> = HashMap::new();
    let mut terminal_locals: HashSet<u32> = HashSet::new();
    let mut mesh_locals: HashMap<u32, MirrorMeshInfo> = HashMap::new();
    let mut explorer_locals: HashMap<u32, std::path::PathBuf> = HashMap::new();
    let mut newly_created_remote_ids: Vec<u32> = Vec::new();
    for s in surfaces {
        let remote_id = s.get("remote_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let role = s.get("role").and_then(|v| v.as_str());
        let is_terminal = role == Some("terminal");
        // 이번 delta 가 실어보낸 실제 kind — client 가 그 role 에 대해 실제로 구성할
        // `Surface::kind()` 값과 1:1 대응(아래 survivor 분기의 "바뀌었는가" 판정 기준).
        // terminal/explorer 는 role 자체가 kind, mesh 는 서버가 함께 보낸 kind 필드,
        // 나머지(placeholder — 비-whitelist mesh 포함)는 client 가 `EmptySurface`
        // ("empty")로 구성한다.
        let new_kind: &str = if is_terminal {
            "terminal"
        } else if role == Some("mesh") {
            s.get("kind").and_then(|v| v.as_str()).unwrap_or("mesh")
        } else if role == Some("explorer") {
            "explorer"
        } else {
            "empty"
        };
        let local_id = match old_map.get(&remote_id) {
            Some(&l) => {
                // survivor — local id 는 그대로 재사용(터미널/mesh 프레임 캐시 유지).
                // 단, convert 로 kind 자체가 바뀐 survivor 는 옛 kind 에 종속된 로컬
                // 리소스가 새 kind 와 안 맞게 된다 — 즉시 정리/생성하지 않으면 이 surface
                // 가 나중에 닫힐 때까지 orphan Terminal 객체(입력 forwarder 스레드 포함)
                // 나 stale mesh frame 캐시, busy state 가 그대로 남는다.
                let old_kind = engine.find_surface_by_id(l).map(|s| s.kind());
                if old_kind != Some(new_kind) {
                    if old_kind == Some("terminal") {
                        // terminal → 다른 kind: 옛 Terminal(+ 입력 forwarder) 과 busy
                        // state 는 새 kind 와 무관해졌으니 제거.
                        engine.terminals.remove(l);
                        engine.forget_mirror_surface_busy(l);
                    }
                    // 옛 kind 가 뭐였든, 캐시된 mesh frame 은 새 kind 의 것이 아니므로
                    // 버린다 — 새 frame 이 도착하기 전까지 옛 kind 의 화면이 잠깐이라도
                    // 그려지는 걸 막는다.
                    engine.attach_mesh_frames.remove(l);
                    if is_terminal {
                        // 다른 kind → terminal: 이 local_id 는 지금까지 Terminal 객체가
                        // 없었다(mesh/explorer/placeholder 였으므로) — 새로 만들어야
                        // 입력 forwarding 이 동작한다(신규 survivor 와 동일한 생성 경로).
                        let cols = s.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
                        let rows = s.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
                        make_mirror_surface(remote_id, l, cols, rows, frame_tx, engine);
                    }
                }
                l
            }
            None => {
                let l = ids.next_surface();
                if is_terminal {
                    let cols = s.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
                    let rows = s.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
                    make_mirror_surface(remote_id, l, cols, rows, frame_tx, engine);
                }
                newly_created_remote_ids.push(remote_id);
                l
            }
        };
        if is_terminal {
            terminal_locals.insert(local_id);
        } else if role == Some("mesh") {
            let kind = s
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("mesh")
                .to_string();
            let plugin_id = s
                .get("plugin_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // 서버가 실제 display_name(예: markdown 파일명)을 보내주면 그걸 쓴다.
            // 필드 자체가 없으면(구버전 서버 등) 기존처럼 kind 로 fallback.
            let display_name = s
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| kind.clone());
            mesh_locals.insert(
                local_id,
                MirrorMeshInfo {
                    display_name,
                    kind,
                    plugin_id,
                },
            );
        } else if role == Some("explorer") {
            let root = s
                .get("root")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            explorer_locals.insert(local_id, root);
        }
        new_map.insert(remote_id, local_id);
    }

    // removed — `old_map` 에 있었지만 이번 surfaces 에 없는 것: mirror 터미널 제거(입력
    // forwarder 는 sink drop 으로 자연 종료) + attach mesh frame 캐시 정리.
    for (&remote_id, &local_id) in old_map.iter() {
        if !new_map.contains_key(&remote_id) {
            engine.terminals.remove(local_id);
            engine.forget_mirror_surface_busy(local_id);
            engine.forget_mirror_surface_attention(local_id);
            engine.attach_mesh_frames.remove(local_id);
        }
    }

    (
        new_map,
        terminal_locals,
        mesh_locals,
        explorer_locals,
        newly_created_remote_ids,
    )
}

/// 드레인된 `MirrorEvent` 한 건을 mirror 세션/engine 상태에 적용한다. 대상은
/// `MirrorHost` — 창 있는 engine 이든 parked engine 이든 같은 분기 로직을 탄다.
fn apply_one_mirror_event(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_>,
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    ev: MirrorEvent,
) {
    match ev {
        MirrorEvent::Data(remote_id, bytes) => {
            if let Some(&local) = sess.remote_to_local.get(&remote_id)
                && let Some(t) = host.engine.terminals.get_mut(local)
            {
                t.feed_bytes(&bytes);
            }
        }
        MirrorEvent::Resize(remote_id, cols, rows) => {
            // mirror 그리드를 원격 새 크기로 갱신. 로컬 resize
            // 스윕은 detached mirror 를 건너뛰므로, 이 경로가
            // mirror 를 리사이즈하는 유일한 지점이다.
            if let Some(&local) = sess.remote_to_local.get(&remote_id)
                && let Some(t) = host.engine.terminals.get_mut(local)
            {
                t.resize(cols, rows);
            }
        }
        MirrorEvent::Activity(remote_id, busy) => {
            if let Some(&local) = sess.remote_to_local.get(&remote_id) {
                host.engine.set_mirror_surface_busy(local, busy);
            }
        }
        MirrorEvent::Attention(remote_id, kind) => {
            // 원격 적용 전용 진입점 — 로컬 producer 의 `raise_attention`/
            // `clear_attention` 을 타지 않는다(억제 게이트·해제 forward 우회).
            if let Some(&local) = sess.remote_to_local.get(&remote_id) {
                host.engine.set_mirror_surface_attention(
                    local,
                    kind.map(crate::core::AttentionKind::from_wire),
                );
            }
        }
        MirrorEvent::StructuralFailed(reason) => {
            // forward 한 구조 op 가 원격에서 실패(예: 미등록 kind).
            // 사용자에게 실패 toast. 로컬/원격 어느 쪽도 구조 변경
            // 없음(요청/응답).
            let base = crate::i18n::t("attach.toast.mirror_structural_forward_failed");
            let msg: String = if reason.is_empty() {
                base.to_string()
            } else {
                format!("{base} ({reason})")
            };
            host.toast(msg, crate::adapters::ui::ToastKind::Warning);
        }
        MirrorEvent::StructuralSucceeded(op_id) => {
            // 08/09 — 이 op 이 focus 보정 대상(user_triggered)으로
            // 등록돼 있었으면, 뒤따르는(프로토콜 보장) 다음
            // StructuralDelta 적용 시 1회 소비할 의도로 옮겨둔다.
            // 등록돼 있지 않았으면(에이전트/IPC 유래 등) no-op.
            if let Some(intent) = sess.pending_op_focus.remove(&op_id) {
                sess.next_delta_focus = Some(intent);
            }
        }
        MirrorEvent::StructuralDelta {
            workspace_id,
            tree,
            surfaces,
        } => {
            // 원격 구조 변경 역반영: survivor 터미널 local id 를
            // 유지하며 mirror 트리를 재구성(신규 추가/사라진 것 제거).
            let pending_focus = sess.next_delta_focus.take();
            apply_mirror_structural_delta(
                sess,
                host.engine,
                workspace_id,
                &tree,
                &surfaces,
                pending_focus,
            );
        }
        MirrorEvent::CaptureResult { ok, path, reason } => {
            // (03) 원격이 이 세션의 캡처 업로드를 처리한 결과.
            let msg = if ok {
                format!(
                    "{} ({})",
                    crate::i18n::t("attach.toast.mirror_capture_saved"),
                    path.unwrap_or_default()
                )
            } else {
                let base = crate::i18n::t("attach.toast.mirror_capture_failed");
                match reason {
                    Some(r) if !r.is_empty() => format!("{base} ({r})"),
                    _ => base.to_string(),
                }
            };
            let kind = if ok {
                crate::adapters::ui::ToastKind::Success
            } else {
                crate::adapters::ui::ToastKind::Warning
            };
            host.toast(msg, kind);
        }
        MirrorEvent::ListDirResult {
            request_id,
            ok,
            dir,
            entries,
            truncated,
            reason,
        } => {
            apply_list_dir_result_event(
                sess, host, request_id, ok, dir, entries, truncated, reason,
            );
        }
        MirrorEvent::GitQueryResult {
            request_id,
            ok,
            kind,
            data,
            truncated,
            reason,
        } => {
            apply_git_query_result_event(
                plugin_manager,
                host.state,
                request_id,
                ok,
                kind,
                data,
                truncated,
                reason,
            );
        }
        MirrorEvent::Mesh(remote_id, generation, frame_seq, full, bytes) => {
            // attach mesh mirror: GPU 렌더은 다음 프레임
            // `AttachMeshFrameStore` 를 읽는다 — 여기선 저장만.
            if let Some(&local) = sess.remote_to_local.get(&remote_id) {
                host.engine
                    .attach_mesh_frames
                    .update(local, bytes, generation, frame_seq, full);
            }
        }
    }
}

/// (04, ADR-0059) `MirrorEvent::ListDirResult` 한 건을 적용한다. 이 요청의 소비자
/// 태그로 분기 — `None` = File Picker(기존 로직), `Some(surface_id)` = explorer(그
/// surface 의 `ExplorerView` 로 라우팅). 태그가 없으면(세션이 이미 재연결로
/// 지워졌거나 stale) 조용히 무시한다.
fn apply_list_dir_result_event(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_>,
    request_id: u64,
    ok: bool,
    dir: Option<String>,
    entries: Option<Vec<crate::core::fs_list::DirEntryInfo>>,
    truncated: bool,
    reason: Option<String>,
) {
    let consumer = sess
        .pending_list_dir_consumers
        .remove(&request_id)
        .flatten();
    if let Some(surface_id) = consumer {
        let result = if ok {
            Ok(entries.unwrap_or_default())
        } else {
            Err(reason.unwrap_or_default())
        };
        if let Some(panel) = host
            .engine
            .find_surface_by_id(surface_id)
            .and_then(|s| s.as_any().downcast_ref::<crate::model::ExplorerPanel>())
        {
            let is_err = result.is_err();
            host.state
                .explorer_views
                .apply_remote_list_dir_result(surface_id, request_id, panel, result);
            if !is_err && truncated {
                host.toast(
                    crate::i18n::t("explorer.state.remote_listing_truncated").to_string(),
                    crate::adapters::ui::ToastKind::Warning,
                );
            }
        }
        return;
    }
    // (04) 원격이 이 세션의 list_dir_request 를 처리한 결과 — popup 이 열려 있고 그
    // 요청을 아직 기다리는 중일 때만 반영(다른 요청/이미 닫힌 popup 응답은 조용히
    // 무시 — stale reply).
    let Some(picker) = host.state.dialogs.file_picker.as_mut() else {
        return;
    };
    let is_pending = matches!(
        &picker.load,
        crate::state::FpLoadState::Loading { request_id: rid, .. } if *rid == request_id
    );
    if !is_pending {
        return;
    }
    if ok {
        let es = entries.unwrap_or_default();
        if let Some(d) = dir {
            picker.current_dir = d;
        }
        picker.load = if es.is_empty() {
            crate::state::FpLoadState::Empty
        } else {
            crate::state::FpLoadState::Loaded
        };
        picker.entries = es;
        // host 배지 라벨 — App 이 소유한 attach_client_sessions 에만 있어(popup
        // wrapper 도달 불가) 첫 성공 응답에 실어온다.
        if picker.remote_host.is_none() {
            picker.remote_host = Some(sess.remote_label.clone());
        }
        if truncated {
            host.toast(
                crate::i18n::t("filepicker.remote_listing_truncated").to_string(),
                crate::adapters::ui::ToastKind::Warning,
            );
        }
    } else {
        let reason_str = reason.unwrap_or_default();
        picker.load = if reason_str == "permission denied" {
            crate::state::FpLoadState::ErrorPerm(reason_str)
        } else {
            crate::state::FpLoadState::ErrorConn(reason_str)
        };
    }
}

/// (ADR-0056 참고) `MirrorEvent::GitQueryResult` 한 건을 적용한다. host 는 페이로드를
/// 해석하지 않고 그대로 plugin(별도 프로세스)에 unicast 이벤트로 전달한다 — plugin
/// 의 wire DTO 가 유일한 소비자. 인가/mirror workspace 소멸 관측은 이미 send
/// 단계(dispatch_pending_git_query_forwards)에서 처리됐으므로 여기선 무조건 forward.
fn apply_git_query_result_event(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    state: &mut crate::state::AppState,
    request_id: u64,
    ok: bool,
    kind: String,
    data: Option<Value>,
    truncated: bool,
    reason: Option<String>,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    let payload = serde_json::json!({
        "request_id": request_id,
        "ok": ok,
        "kind": kind,
        "data": data,
        "truncated": truncated,
        "reason": reason,
    });
    mgr.emit_host_event_to_plugin(
        GIT_VIEWER_PLUGIN_ID,
        GIT_VIEWER_QUERY_RESULT_EVENT,
        &payload,
        tasty_plugin_protocol::EventScope::System,
    );
    // set_context 는 geom/input/theme 변경시에만 나가(popup_render.rs dirty 판정) 이
    // push 만으론 다음 frame 에 plugin 이 다시 그려지지 않는다 — 열려 있는 git-viewer
    // popup 인스턴스 전부에 강제 repaint 를 예약(단일 primary 인스턴스 모델이라
    // 보통 최대 1개).
    for (iid, inst) in mgr.popup_instances() {
        if inst.plugin_id == GIT_VIEWER_PLUGIN_ID {
            state.plugin_mesh_popup_pending_repaint.insert(iid);
        }
    }
}

/// 원격 구조 변경 delta(3단계 역반영)를 mirror 트리에 적용한다. 원격 ws 의 실행 후 전체
/// 트리+surfaces 를 받아:
/// 1. survivor(기존 매핑에 있는 remote_id)는 **기존 local id 를 재사용**(터미널을
///    재생성하지 않아 scrollback/grid 보존),
/// 2. 신규 remote surface 는 로컬 id 발급 + `make_mirror_surface`(터미널만),
/// 3. 사라진 것은 mirror 터미널 제거,
/// 4. 갱신된 매핑으로 `build_mirror_workspace` 재실행 → 같은 local ws id 로 교체.
///
/// pane 상위 배치는 `build_mirror_workspace` 의 기존 horizontal-chain 근사를 그대로
/// 승계한다(핸드셰이크와 동일 수준 — 3단계가 악화시키지 않음).
///
/// **focus 보존(수정 방향 B)**: 순수 pane/tab 전환(클릭·키보드 이동)은 forward 되는
/// StructuralOp 가 없어 원격의 `Workspace.focused_pane`/`Pane.active_tab` 은 갱신되지
/// 않는다(대개 워크스페이스 생성 시점의 첫 pane/첫 탭에 고정). 아래 4단계가 그 값을
/// 그대로 담은 delta 로 로컬 트리를 통째로 교체하면, 사용자가 로컬에서만 이동해둔
/// focus 가 매번 그 고정값으로 되돌아간다 — 이를 막기 위해 교체 **전** 로컬에서 실제로
/// focus 돼 있던 surface 를 remote id 기준으로 캡처해뒀다가, 교체 **후** 새 트리에서
/// 그 surface 를 찾아 focus 를 복원한다(서버 상태는 건드리지 않음 — client-only 보정).
fn apply_mirror_structural_delta(
    sess: &mut AttachClientSession,
    engine: &mut crate::core::CoreState,
    workspace_id: u32,
    tree: &Value,
    surfaces: &[Value],
    pending_focus: Option<PendingOpFocus>,
) {
    let ids = engine.next_ids.clone();

    // focus 캡처(교체 전) — 로컬에서 실제로 focus 돼 있던 surface 를, 재구성마다 바뀌는
    // local id 대신 안정적인 **remote id** 로 기억한다(옛 remote_to_local 기준).
    let old_focused_remote: Option<u32> = engine
        .workspaces
        .iter()
        .find(|w| w.id == sess.local_workspace)
        .and_then(|ws| capture_focused_remote(ws, &sess.remote_to_local));

    // 1·2·3. survivor 유지 + 신규 할당 + 사라진 것 제거(재연결 `reconnect_session` 과
    // 공유하는 `merge_survivor_mapping`). 이 op 으로 새로 생긴 remote surface(=이전
    // 매핑에 없던 것)도 순서대로 받아둔다(08 — new-tab/split 성공 시 focus 를 옮길 대상
    // 후보).
    let (new_map, terminal_locals, mesh_locals, explorer_locals, newly_created_remote_ids) =
        merge_survivor_mapping(
            &sess.remote_to_local,
            surfaces,
            &ids,
            &sess.frame_tx,
            engine,
        );

    // 매핑 교체(이후 같은 drain 의 Data 는 갱신된 매핑으로 라우팅된다).
    sess.remote_to_local = new_map;

    // 4. 트리 재구성 → 같은 local ws id 로 in-place 교체(survivor local id 유지 →
    //    위치·구성만 갱신, active_workspace 인덱스 불변).
    if let Some(pos) = engine
        .workspaces
        .iter()
        .position(|w| w.id == sess.local_workspace)
    {
        let name = engine.workspaces[pos].name.clone();
        let mut ws = build_mirror_workspace(
            sess.local_workspace,
            &name,
            tree,
            &ids,
            &sess.remote_to_local,
            &terminal_locals,
            &mesh_locals,
            &explorer_locals,
        );
        ws.mirror = true;

        // 08 — 이번 op 이 user_triggered new-tab/split 이면, 옛 focus 를 복원하는 대신
        // 새로 생긴 surface 로 focus 를 옮긴다(옛 focus 는 새 리소스를 만든 op 으로는
        // 거의 항상 살아남으므로, restore 를 먼저 태우면 08 의 목적과 반대로 옛 위치에
        // 눌러앉는다 — 그래서 NewResource 는 restore 를 아예 건너뛴다).
        let mut focus_handled = false;
        if matches!(pending_focus, Some(PendingOpFocus::NewResource))
            && let Some(&new_local) = newly_created_remote_ids
                .first()
                .and_then(|rid| sess.remote_to_local.get(rid))
        {
            focus_handled = set_focus_to_surface(&mut ws, new_local);
        }
        if !focus_handled {
            let restored =
                restore_focus_after_delta(&mut ws, old_focused_remote, &sess.remote_to_local);
            // 09 — 옛 focus 복원이 실패했다(=캡처해둔 surface 가 이번 op 으로 사라짐,
            // 전형적으로 그 surface/tab 자체를 닫은 경우) — user_triggered close 로 미리
            // 계산해둔 인접 후보(remote id, 우선순위 순) 중 delta 이후에도 살아있는
            // 첫번째로 fallback 한다. 후보가 다 사라졌으면(예상 밖) 기존 동작대로 원격의
            // 고정 focused_pane/active_tab 값 그대로 남는다.
            if !restored && let Some(PendingOpFocus::Close { candidates }) = &pending_focus {
                for &remote_cand in candidates {
                    if let Some(&local_cand) = sess.remote_to_local.get(&remote_cand)
                        && set_focus_to_surface(&mut ws, local_cand)
                    {
                        break;
                    }
                }
            }
        }

        engine.workspaces[pos] = ws;
    } else {
        tracing::warn!(
            "structural delta: mirror workspace {} (remote {workspace_id}) 를 못 찾음 — drop",
            sess.local_workspace
        );
    }
}

/// delta 로 새로 만들어진 `ws` 에 `old_focused_remote`(교체 전 캡처한 remote surface
/// id)가 가리키던 위치로 focus 를 되돌린다. 캡처해둔 surface 가 새 트리에도 살아있으면
/// (이번 op 로 사라지지 않았으면) `ws.focused_pane`/해당 pane 의 `active_tab`/그 tab 의
/// `focused_surface` 를 그 위치로 맞추고 `true`. surface 자체가 이번 op 로 없어졌으면
/// (예: 그 surface 를 닫은 CloseSurface) 억지로 복원하지 않고 `false` — 호출부(09)가
/// 인접 후보 fallback 을 시도할지 판단하는 신호로 쓴다.
fn restore_focus_after_delta(
    ws: &mut Workspace,
    old_focused_remote: Option<u32>,
    remote_to_local: &HashMap<u32, u32>,
) -> bool {
    let Some(remote_sid) = old_focused_remote else {
        return false;
    };
    let Some(&new_local_sid) = remote_to_local.get(&remote_sid) else {
        return false;
    };
    set_focus_to_surface(ws, new_local_sid)
}

/// `ws` 안에서 `local_sid` 를 포함하는 (pane, tab) 을 찾아 그 위치로
/// `focused_pane`/`active_tab`/`focused_surface` 를 맞춘다. 찾지 못하면(surface 가
/// 이번 delta 에 없음) 아무것도 바꾸지 않고 `false`.
fn set_focus_to_surface(ws: &mut Workspace, local_sid: u32) -> bool {
    let Some((pane_id, tab_id)) = find_pane_and_tab_for_surface(ws, local_sid) else {
        return false;
    };
    ws.focused_pane = pane_id;
    if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id)
        && let Some(tab_index) = pane.tabs.iter().position(|t| t.id == tab_id)
    {
        pane.active_tab = tab_index;
        pane.tabs[tab_index].focused_surface = local_sid;
        true
    } else {
        false
    }
}

/// 현재 `ws`(교체되기 전의 mirror workspace)에서 실제로 focus 돼 있는 surface 를
/// **remote surface id** 로 찾아 반환한다(`remote_to_local` 역조회). local pane/tab/
/// surface id 는 매 delta 마다 재발급되어 안정적이지 않으므로, 여러 delta 를 거쳐도
/// 불변인 remote id 를 캡처의 기준으로 삼는다. focus 가 가리키는 surface 가 아직 이
/// 세션에 매핑되지 않았으면(예상 밖) `None`.
fn capture_focused_remote(ws: &Workspace, remote_to_local: &HashMap<u32, u32>) -> Option<u32> {
    let pane = ws.pane_layout().find_pane(ws.focused_pane)?;
    let tab = pane.tabs.get(pane.active_tab)?;
    let local_sid = tab.focused_surface_id()?;
    remote_to_local
        .iter()
        .find(|&(_, &l)| l == local_sid)
        .map(|(&r, _)| r)
}

/// 주어진 workspace 안에서 `surface_id` 를 포함하는 (pane_id, tab_id) 를 찾는다.
/// `CoreState::find_pane_for_surface`/`find_tab_for_surface` 와 동형이지만 **단일
/// workspace 로 스코프를 좁힌** 버전 — `apply_mirror_structural_delta` 가 아직
/// `engine.workspaces` 에 삽입하기 **전의** 갓 만든 `Workspace` 값에도 바로 쓸 수
/// 있어야 하기 때문(engine 전체 순회 버전은 삽입 후에만 그 워크스페이스를 찾는다).
fn find_pane_and_tab_for_surface(ws: &Workspace, surface_id: u32) -> Option<(u32, u32)> {
    for pane_id in ws.pane_layout().all_pane_ids() {
        let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
            continue;
        };
        for tab in &pane.tabs {
            if tab.contains_surface(surface_id) {
                return Some((pane_id, tab.id));
            }
        }
    }
    None
}

/// pane JSON(`{"id", "tabs":[...]}` — 평면 "panes" 원소/트리 Leaf 공용 shape)
/// → 로컬 `Pane`. 새 local pane id 발급 + 각 tab 의 layout(`build_layout`)/
/// focused_surface remote→local 매핑. `build_mirror_workspace`의 평면 fallback
/// 경로와 `build_pane_node`(트리 파서)가 공유한다.
#[allow(clippy::too_many_arguments)] // reason: mirror pane 파서 컨텍스트 전체
fn build_pane_from_json(
    p: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
) -> Pane {
    let tabs_json = p
        .get("tabs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut tabs: Vec<Tab> = Vec::new();
    let mut active_tab = 0usize;
    for (i, t) in tabs_json.iter().enumerate() {
        let layout_json = t.get("layout").cloned().unwrap_or(Value::Null);
        let layout =
            build_layout(&layout_json, ids, map, term, mesh, explorer).unwrap_or_else(|| {
                SurfaceLayout::Leaf(Box::new(EmptySurface::new(ids.next_surface())))
            });
        let remote_focus = t
            .get("focused_surface")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let focused_surface = map
            .get(&remote_focus)
            .copied()
            .or_else(|| layout.first_surface_id())
            .unwrap_or(0);
        let tab_name = t
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(crate::i18n::t("attach.tab_title_fallback"))
            .to_string();
        if t.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
            active_tab = i;
        }
        tabs.push(Tab {
            id: ids.next_tab(),
            name: tab_name,
            explicit_name: None,
            osc_title: None,
            layout_opt: Some(layout),
            focused_surface,
            cached_display_name: None,
        });
    }
    if tabs.is_empty() {
        let sid = ids.next_surface();
        tabs.push(Tab {
            id: ids.next_tab(),
            name: crate::i18n::t("attach.tab_title_fallback").to_string(),
            explicit_name: None,
            osc_title: None,
            layout_opt: Some(SurfaceLayout::Leaf(Box::new(EmptySurface::new(sid)))),
            focused_surface: sid,
            cached_display_name: None,
        });
    }
    Pane {
        id: ids.next_pane(),
        tabs,
        active_tab,
        tab_scroll_offset: 0.0,
    }
}

/// "pane_layout" JSON(`PaneNode::to_tree_json_full` shape) → `PaneNode`
/// (direction/ratio 보존). Leaf 파싱 시 (remote_pane_id → 신규 local pane id)를
/// `pane_id_map` 에 기록해, 호출부가 focused_pane remote→local 해석에 재사용한다
/// (트리 재귀 파서는 기존 `local_panes: Vec<(remote_id, Pane)>` 평면 리스트가
/// 없으므로 이 매핑이 그 대체 경로다).
#[allow(clippy::too_many_arguments)] // reason: mirror 트리 재귀 파서 컨텍스트 전체
fn build_pane_node(
    node: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
    pane_id_map: &mut HashMap<u32, u32>,
) -> Option<PaneNode> {
    match node.get("type").and_then(|v| v.as_str())? {
        "Leaf" => {
            let remote_pane = node.get("id").and_then(|v| v.as_u64())? as u32;
            let pane = build_pane_from_json(node, ids, map, term, mesh, explorer);
            pane_id_map.insert(remote_pane, pane.id);
            Some(PaneNode::Leaf(pane))
        }
        "Split" => {
            let direction = match node.get("direction").and_then(|v| v.as_str()) {
                Some("vertical") => SplitDirection::Vertical,
                _ => SplitDirection::Horizontal,
            };
            let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let first = build_pane_node(
                node.get("first")?,
                ids,
                map,
                term,
                mesh,
                explorer,
                pane_id_map,
            )?;
            let second = build_pane_node(
                node.get("second")?,
                ids,
                map,
                term,
                mesh,
                explorer,
                pane_id_map,
            )?;
            Some(PaneNode::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            })
        }
        _ => None,
    }
}

/// 디스크립터 `tree`(`to_attach_tree_json`)로 로컬 mirror Workspace 를 재구성한다.
///
/// 신버전 서버는 `"pane_layout"` 트리 필드(direction/ratio 보존, `build_pane_node`)를
/// 실어 pane 상위 배치를 정확히 재현한다. 그 필드가 없는 구버전 서버는 평면 `"panes"`
/// 리스트만 보내므로, 다중 pane 을 horizontal split chain 으로 best-effort 재구성하는
/// 기존 fallback 을 그대로 유지한다. 각 pane 의 tab 별 `SurfaceLayout`(분할 방향/비율)은
/// 두 경로 모두 `to_tree_json_full` 로 보존돼 정확히 재현된다. remote leaf id 는 `map`
/// 으로 로컬 id 치환.
#[allow(clippy::too_many_arguments)] // reason: mirror workspace 재구성 컨텍스트 전체
fn build_mirror_workspace(
    ws_id: u32,
    name: &str,
    tree: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
) -> Workspace {
    let remote_focused_pane = tree
        .get("focused_pane")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    // 신버전 서버: "pane_layout" 트리 필드로 direction/ratio 보존 파싱.
    if let Some(layout_json) = tree.get("pane_layout").filter(|v| !v.is_null()) {
        let mut pane_id_map = HashMap::new();
        if let Some(node) = build_pane_node(
            layout_json,
            ids,
            map,
            term,
            mesh,
            explorer,
            &mut pane_id_map,
        ) {
            let focused_local_pane = pane_id_map
                .get(&remote_focused_pane)
                .copied()
                .unwrap_or_else(|| node.first_pane().map(|p| p.id).unwrap_or(0));
            return Workspace::from_restored(
                ws_id,
                name.to_string(),
                String::new(),
                node,
                focused_local_pane,
            );
        }
        // "pane_layout" 이 있는데 파싱 실패(형태 불량) — 아래 구버전 fallback으로 흘려보냄.
    }

    // 구버전 fallback: 평면 "panes" 리스트 → horizontal chain(best-effort).
    let panes_json = tree
        .get("panes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut local_panes: Vec<(u32, Pane)> = Vec::new();
    for p in &panes_json {
        let remote_pane = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        local_panes.push((
            remote_pane,
            build_pane_from_json(p, ids, map, term, mesh, explorer),
        ));
    }

    if local_panes.is_empty() {
        // 빈 트리 fallback — placeholder pane 1 개.
        let sid = ids.next_surface();
        let pane = Pane::new_with_surface(
            ids.next_pane(),
            ids.next_tab(),
            crate::i18n::t("attach.tab_title_fallback").to_string(),
            Box::new(EmptySurface::new(sid)),
        );
        let fp = pane.id;
        return Workspace::from_restored(
            ws_id,
            name.to_string(),
            String::new(),
            PaneNode::Leaf(pane),
            fp,
        );
    }

    let focused_local_pane = local_panes
        .iter()
        .find(|(rp, _)| *rp == remote_focused_pane)
        .map(|(_, p)| p.id)
        .unwrap_or(local_panes[0].1.id);

    // PaneNode: 1개=Leaf, 다중=horizontal split chain(best-effort — 구버전 서버는
    // pane 배치 정보를 안 보내므로 이 근사만 가능).
    let mut iter = local_panes.into_iter().map(|(_, p)| p);
    let mut node = PaneNode::Leaf(iter.next().unwrap());
    for p in iter {
        node = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(node),
            second: Box::new(PaneNode::Leaf(p)),
        };
    }

    Workspace::from_restored(
        ws_id,
        name.to_string(),
        String::new(),
        node,
        focused_local_pane,
    )
}

/// `to_tree_json_full` JSON → `SurfaceLayout`(분할 방향/비율/focus 보존). leaf 의 remote
/// id 는 `map` 으로 로컬 치환하고, 터미널이면 `TerminalSurface`(mirror grid 가 store 에
/// 있음), attach mesh mirror(`mesh`)면 `AttachMeshSurface`(attach-behavior.md#mesh-mirror-채널 참고), 그 외엔 placeholder
/// `EmptySurface` leaf 로 만든다.
fn build_layout(
    node: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
) -> Option<SurfaceLayout> {
    match node.get("type").and_then(|v| v.as_str())? {
        "Leaf" => {
            let remote = node.get("id").and_then(|v| v.as_u64())? as u32;
            // map 에 없으면(예상 밖) 새 placeholder id 발급.
            let local = map
                .get(&remote)
                .copied()
                .unwrap_or_else(|| ids.next_surface());
            let surface: Box<dyn Surface> = if term.contains(&local) {
                Box::new(TerminalSurface { id: local })
            } else if let Some(info) = mesh.get(&local) {
                Box::new(crate::model::AttachMeshSurface::new(
                    local,
                    &info.kind,
                    info.plugin_id.clone(),
                    info.display_name.clone(),
                ))
            } else if let Some(root) = explorer.get(&local) {
                // (ADR-0059 참고) cwd == root 단순화 — wire 는 root 만 싣는다.
                Box::new(ExplorerPanel::new(local, root.clone()))
            } else {
                Box::new(EmptySurface::new(local))
            };
            Some(SurfaceLayout::Leaf(surface))
        }
        "Split" => {
            let direction = match node.get("direction").and_then(|v| v.as_str()) {
                Some("vertical") => SplitDirection::Vertical,
                _ => SplitDirection::Horizontal,
            };
            let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let focus_second = node
                .get("focus_second")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let first = build_layout(node.get("first")?, ids, map, term, mesh, explorer)?;
            let second = build_layout(node.get("second")?, ids, map, term, mesh, explorer)?;
            Some(SurfaceLayout::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
                focus_second,
            })
        }
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// (03) screenshot→remote-clipboard — mirror client 측 업로드 송신.
//
// 이 블록은 위 구조 op forward/역반영 로직(특히 `apply_mirror_structural_delta`)과
// 완전히 독립적이다 — 별도 기능(신규 03)이라 별도 impl 블록 + 전용 free fn 으로
// 분리해 둔다(병행 작업 merge 충돌 최소화).
// ─────────────────────────────────────────────────────────────────────────

/// 업로드 세션 식별자 시퀀스 — 프로세스 내 유일성만 필요(원격은 client_id 로도
/// 이미 세션이 구분되므로 재기동 간 유일성은 불필요).
static NEXT_CAPTURE_UPLOAD_ID: AtomicU64 = AtomicU64::new(1);

fn next_capture_upload_id() -> u64 {
    NEXT_CAPTURE_UPLOAD_ID.fetch_add(1, Ordering::Relaxed)
}

/// (06) bulk 파일 전송의 transfer_id 발급기. 프로세스 내 단조 증가(원격은 client_id
/// 로 연결이 구분되므로 재기동 간 유일성 불필요 — capture 와 동일 근거).
static NEXT_BULK_TRANSFER_ID: AtomicU64 = AtomicU64::new(1);

fn next_bulk_transfer_id() -> u64 {
    NEXT_BULK_TRANSFER_ID.fetch_add(1, Ordering::Relaxed)
}

/// (06) 한 bulk `Data` 청크의 raw payload 크기 상한 = `MAX_FRAME_LEN - BULK_CHUNK_HEADER_LEN`.
/// binary sub-header(`[transfer_id u64][seq u32]`) 를 얹어도 프레임이 1 MiB 를 넘지
/// 않게 한다. base64 를 쓰지 않으므로 capture(700 KiB)보다 크게 잡을 수 있다.
const BULK_CHUNK_RAW_LEN: usize = stream::MAX_FRAME_LEN as usize - stream::BULK_CHUNK_HEADER_LEN;

/// (09) 원격이 begin/commit 을 거부(`BulkResult{ok:false}`)했을 때 `upload_file_over_bulk`
/// 이 반환하는 `Err` 메시지의 접두. **거부 vs 전송 에러**를 소비자(08 결과 처리)가
/// 구분하는 안정 계약이다 — 이 접두면 원격 정책 거부(예: 07 capacity exceeded)라 재시도가
/// 무의미(실패 팝업 Dismiss 단독), 아니면 전송/프로토콜 에러라 재시도 가능(Retry). 문자열
/// 매칭이지만 생산·소비가 같은 크레이트라 이 const 로 계약을 고정한다.
pub(crate) const BULK_REJECT_PREFIX: &str = "remote rejected bulk upload: ";

/// 한 청크의 raw payload 크기 상한. base64 인코딩(약 4/3 팽창) 후에도
/// `StreamTag::Control` 프레임의 `MAX_FRAME_LEN`(1MiB) 에 JSON 오버헤드를 포함해
/// 여유 있게 들어가도록 700KiB 로 잡는다(대부분의 스크린샷은 청크 1~2개).
const CAPTURE_CHUNK_RAW_LEN: usize = 700 * 1024;

/// `parse_capture_result`가 쓰는 wire shape. `StreamControl` enum 에는 없는
/// 이벤트라 별도로 직접 파싱한다.
#[derive(serde::Deserialize)]
struct CaptureResultWire {
    ok: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// `frame.payload` 가 (03) `capture_result` 커스텀 이벤트인지 확인해 `MirrorEvent`
/// 로 변환한다. `event` 필드가 다르거나 형태가 안 맞으면 `None`(다른 미지 이벤트와
/// 동일하게 조용히 무시 — 전방 호환).
fn parse_capture_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("capture_result") {
        return None;
    }
    let wire: CaptureResultWire = serde_json::from_value(value).ok()?;
    Some(MirrorEvent::CaptureResult {
        ok: wire.ok,
        path: wire.path,
        reason: wire.reason,
    })
}

/// `parse_list_dir_result`가 쓰는 wire shape — 서버(`attach_runtime::handle_list_dir_request`)
/// 의 `list_dir_entry_wire` 와 대칭. `modified_unix`(unix epoch 초) 는 여기서
/// `SystemTime` 으로 복원한다 — `DirEntryInfo` 가 로컬/원격 어디서 만들어지든
/// 동일한 `Option<SystemTime>` 셰이프를 유지하게(사람이 읽는 포맷팅은 view 렌더
/// 직전에서만 한다).
#[derive(serde::Deserialize)]
struct ListDirEntryWire {
    name: String,
    is_dir: bool,
    size: u64,
    #[serde(default)]
    modified_unix: Option<u64>,
    #[serde(default)]
    ext: String,
}

#[derive(serde::Deserialize)]
struct ListDirResultWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    entries: Option<Vec<ListDirEntryWire>>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    reason: Option<String>,
}

/// `frame.payload` 가 (04) `list_dir_result` 커스텀 이벤트인지 확인해 `MirrorEvent`
/// 로 변환한다. `event` 필드가 다르거나 형태가 안 맞으면 `None`(다른 미지 이벤트와
/// 동일하게 조용히 무시 — 전방 호환).
fn parse_list_dir_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("list_dir_result") {
        return None;
    }
    let wire: ListDirResultWire = serde_json::from_value(value).ok()?;
    let entries = wire.entries.map(|es| {
        es.into_iter()
            .map(|e| crate::core::fs_list::DirEntryInfo {
                path: wire
                    .dir
                    .as_deref()
                    .map(|d| std::path::Path::new(d).join(&e.name))
                    .unwrap_or_else(|| std::path::PathBuf::from(&e.name)),
                name: e.name,
                is_dir: e.is_dir,
                size: e.size,
                modified: e
                    .modified_unix
                    .map(|secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)),
                ext: e.ext,
            })
            .collect()
    });
    Some(MirrorEvent::ListDirResult {
        request_id: wire.request_id,
        ok: wire.ok,
        dir: wire.dir,
        entries,
        truncated: wire.truncated,
        reason: wire.reason,
    })
}

/// `parse_git_query_result` 가 쓰는 wire shape. kind 별 페이로드
/// (worktrees/status_entries/log_entries/… 또는 file_path/hunks)는 host 가 해석할
/// 필요가 없다 — 유일한 소비자(git-viewer plugin)의 wire DTO 로 그대로 넘긴다. 이
/// 필드들은 `#[serde(flatten)]` 으로 한꺼번에 캡처해 `MirrorEvent::GitQueryResult::data`
/// 에 raw JSON 으로 싣는다(서버 `attach_runtime::handle_git_query_request` 와 대칭
/// 이지만, list_dir 과 달리 host 는 이 값을 재해석하지 않고 그대로 forward 만 한다).
#[derive(serde::Deserialize)]
struct GitQueryResultWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    truncated_status: bool,
    #[serde(default)]
    truncated_log: bool,
    #[serde(default)]
    truncated_diff: bool,
    #[serde(default)]
    reason: Option<String>,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

/// `frame.payload` 가 `git_query_result` 커스텀 이벤트인지 확인해
/// `MirrorEvent` 로 변환한다. `event` 필드가 다르거나 형태가 안 맞으면 `None`(다른
/// 미지 이벤트와 동일하게 조용히 무시 — 전방 호환).
fn parse_git_query_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("git_query_result") {
        return None;
    }
    let wire: GitQueryResultWire = serde_json::from_value(value).ok()?;
    let truncated = wire.truncated_status || wire.truncated_log || wire.truncated_diff;
    let data = wire.ok.then(|| Value::Object(wire.rest));
    Some(MirrorEvent::GitQueryResult {
        request_id: wire.request_id,
        ok: wire.ok,
        kind: wire.kind,
        data,
        truncated,
        reason: wire.reason,
    })
}

impl App {
    /// (03) 캡처된 로컬 스크린샷을 `local_ws_id` mirror 세션의 attach 채널로
    /// 업로드하고, 완료 시 원격이 그 경로를 원격 클립보드에 쓰도록 요청한다.
    /// `StreamControl` enum(다른 worktree 가 동시 수정 중)은 건드리지 않고, 그
    /// enum 이 인식 못 하는 별도 "event" 값의 raw JSON 을 같은
    /// `StreamTag::Control` 채널에 실어 보낸다(파싱 실패 시 조용히 스킵되는
    /// 전방 호환 특성을 그대로 이용 — `stream_hub.rs`/서버측이 이를 받아 처리).
    pub(crate) fn forward_capture_to_remote_clipboard(
        &mut self,
        local_ws_id: u32,
        file_name: &str,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let Some(sess) = self
            .attach_client_sessions
            .iter()
            .find(|s| s.local_workspace == local_ws_id)
        else {
            anyhow::bail!("no attach session for mirror workspace {local_ws_id}");
        };
        let frame_tx = sess.frame_tx.clone();
        let upload_id = next_capture_upload_id();

        use base64::Engine as _;
        let chunks: Vec<&[u8]> = if bytes.is_empty() {
            vec![&[][..]]
        } else {
            bytes.chunks(CAPTURE_CHUNK_RAW_LEN).collect()
        };
        let total = chunks.len() as u32;
        for (seq, chunk) in chunks.into_iter().enumerate() {
            let msg = serde_json::json!({
                "event": "capture_chunk",
                "upload_id": upload_id,
                "seq": seq as u32,
                "total": total,
                "data_b64": base64::engine::general_purpose::STANDARD.encode(chunk),
            });
            send_capture_control_frame(&frame_tx, &msg)?;
        }
        let commit = serde_json::json!({
            "event": "capture_commit",
            "upload_id": upload_id,
            "file_name": file_name,
        });
        send_capture_control_frame(&frame_tx, &commit)
    }

    /// (04) file picker/explorer(ADR-0059) — `local_ws_id` mirror 세션의 attach
    /// 채널로 `list_dir_request` 를 보낸다. `consumer`(`None`=File Picker,
    /// `Some(surface_id)`=explorer)는 응답 도착 시 라우팅에 쓰도록 세션에 기록해둔다
    /// (wire 엔 안 실림 — ADR-0059 Decision 5). 응답은 reader thread 가 비동기로 받아
    /// `MirrorEvent::ListDirResult` 로 기존 이벤트 큐에 흘려보낸다(`apply_attach_client_output`
    /// 이 소비 — capture_result 와 동일한 reader-thread→큐→메인루프 drain 경로,
    /// `remote_attach.rs` 류 독자 폴링 슬롯 불필요).
    pub(crate) fn send_list_dir_request(
        &mut self,
        local_ws_id: u32,
        request_id: u64,
        dir: &str,
        consumer: Option<u32>,
    ) -> anyhow::Result<()> {
        let Some(sess) = self
            .attach_client_sessions
            .iter_mut()
            .find(|s| s.local_workspace == local_ws_id)
        else {
            anyhow::bail!("no attach session for mirror workspace {local_ws_id}");
        };
        let msg = serde_json::json!({
            "event": "list_dir_request",
            "request_id": request_id,
            "dir": dir,
        });
        let result = send_capture_control_frame(&sess.frame_tx, &msg);
        if result.is_ok() {
            sess.pending_list_dir_consumers.insert(request_id, consumer);
        }
        result
    }

    /// `local_surface_id` 를 보유한 mirror 세션의 attach 채널로
    /// `git_query_request` 를 보낸다. `local_surface_id` 는 popup 이 anchor 된
    /// **로컬** mirror surface — `forward_one_resize` 와 동일한 조회로 원격 id 로
    /// 치환한다(서버가 그 원격 surface 의 실제 cwd 로 discover 하므로, list_dir 과
    /// 달리 client 가 cwd 문자열을 미리 계산해 보낼 필요가 없다). 응답은 reader
    /// thread 가 비동기로 받아 `MirrorEvent::GitQueryResult` 로 흘려보낸다.
    pub(crate) fn send_git_query_request(
        &mut self,
        local_surface_id: u32,
        request_id: u64,
        kind: crate::adapters::production::stream_hub::GitQueryKind,
        worktree_path: Option<&str>,
        diff_path: Option<&str>,
    ) -> anyhow::Result<()> {
        let Some(sess) = self
            .attach_client_sessions
            .iter()
            .find(|s| s.remote_to_local.values().any(|&l| l == local_surface_id))
        else {
            anyhow::bail!("no attach session for mirror surface {local_surface_id}");
        };
        let Some(remote_sid) = sess
            .remote_to_local
            .iter()
            .find(|&(_, &l)| l == local_surface_id)
            .map(|(&r, _)| r)
        else {
            anyhow::bail!("no remote surface id for mirror surface {local_surface_id}");
        };
        let msg = serde_json::json!({
            "event": "git_query_request",
            "request_id": request_id,
            "surface_id": remote_sid,
            "kind": kind.as_wire_str(),
            "worktree_path": worktree_path,
            "diff_path": diff_path,
        });
        send_capture_control_frame(&sess.frame_tx, &msg)
    }
}

/// (03) capture_chunk/capture_commit JSON 하나를 `StreamTag::Control` 프레임으로
/// 직렬화해 보낸다.
fn send_capture_control_frame(
    frame_tx: &SharedFrameSender,
    msg: &serde_json::Value,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(msg)?;
    crate::poison::recover_mutex(frame_tx.lock(), FRAME_TX_WHAT, &FRAME_TX_POISONED)
        .send(OutFrame {
            tag: StreamTag::Control,
            payload,
        })
        .map_err(|_| anyhow::anyhow!("attach write queue closed (write thread gone)"))?;
    Ok(())
}

impl App {
    /// (08) `local_ws_id` mirror 세션의 bulk 업로드 대상 `(local port, remote workspace)`
    /// 를 뽑는다. 백그라운드 스레드는 `&self`(세션)를 들 수 없으므로, 메인 스레드에서
    /// 이 값만 미리 뽑아 자유 함수 [`upload_file_over_bulk`] 에 넘긴다. 세션이 없으면
    /// (정리됨) `None`.
    pub(crate) fn bulk_target_for(&self, local_ws_id: u32) -> Option<(u16, u32)> {
        self.attach_client_sessions
            .iter()
            .find(|s| s.local_workspace == local_ws_id)
            .map(|s| (s.bulk_port, s.remote_workspace))
    }
}

/// (06) 전용 bulk 연결 하나의 전 수명을 동기로 수행: `127.0.0.1:port` 에 connect →
/// `open_bulk(remote_ws)` → begin/chunk/commit 송신 → `BulkResult` 수신 → detach.
/// 성공 시 원격 절대경로, 실패 시 원격 사유(또는 전송/프로토콜 에러)를 `Err`.
///
/// 세션 상태(`&self`)에 의존하지 않으므로 호출자가 백그라운드 스레드로 오프로드하기
/// 쉽다(세션에서 `(port, remote_ws)` 만 미리 뽑으면 됨). 전용 연결도 heartbeat/TTL
/// (ADR-0052)·인가(bulk_workspace holder 결속, 06-α 서버가 검증)를 그대로 탄다.
/// (06) 파일 바이트를 bulk `Data` 프레임 payload 시퀀스로 청킹한다(각 원소는
/// `[transfer_id u64][seq u32][part]`). 순수 함수 — 네트워크 없이 청킹 경계·seq·헤더
/// 인코딩을 검증할 수 있다. 빈 입력은 청크 0개(begin→commit 만으로 0바이트 저장).
fn bulk_chunk_frames(transfer_id: u64, bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes
        .chunks(BULK_CHUNK_RAW_LEN)
        .enumerate()
        .map(|(seq, part)| stream::encode_bulk_chunk(transfer_id, seq as u32, part))
        .collect()
}

/// 원격 tasty(loopback `port`)에 bulk 파일 전송 전용 연결(ADR-0054)을 연다. 대화형
/// attach 와 동일한 read/write timeout — silent 단절 감지 + write 백프레셔
/// 백스톱(ADR-0052 heartbeat). `open_bulk` 이 핸드셰이크 ack 를 읽고 돌아온다.
fn open_bulk_connection(port: u16, remote_ws: u32) -> anyhow::Result<StreamConnection> {
    let sock = TcpStream::connect(("127.0.0.1", port))?;
    if let Err(e) = sock.set_read_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("bulk upload: failed to set read timeout: {e}");
    }
    if let Err(e) = sock.set_write_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("bulk upload: failed to set write timeout: {e}");
    }
    let (conn, _client_id) = StreamConnection::open_bulk(sock, STREAM_PROTO, remote_ws)?;
    Ok(conn)
}

/// `transfer_id` 로 begin→chunk(들)→commit 를 순서대로 보낸다. `on_progress(sent,
/// total)`(09)을 각 청크 전송 직후 호출해 누적 전송 바이트를 통지 — 시작 시
/// 1회(0, total)로도 발화해 0% 프레임을 즉시 띄운다.
fn send_bulk_payload(
    conn: &mut StreamConnection,
    transfer_id: u64,
    file_name: &str,
    bytes: &[u8],
    on_progress: impl Fn(u64, u64),
) -> anyhow::Result<()> {
    // begin(파일명·총 크기) — 서버가 basename 안전화·용량 승인(07)의 입력으로 쓴다.
    let begin = StreamControl::BulkBegin {
        transfer_id,
        filename: file_name.to_string(),
        total_size: bytes.len() as u64,
    };
    conn.send(StreamTag::Control, &serde_json::to_vec(&begin)?)?;

    // chunk — raw 바이트를 (MAX_FRAME_LEN - header) 미만으로 청킹, 각 part 앞에 binary
    // sub-header(transfer_id/seq)를 얹어 Data 프레임으로. base64 미사용. 빈 파일도
    // begin→commit 만으로 0바이트 저장되도록 청크 루프를 건너뛴다.
    let total = bytes.len() as u64;
    // 09: 시작 즉시 0% 프레임을 띄우도록 초기 통지(빈 파일은 이 1회만).
    on_progress(0, total);
    let mut sent: u64 = 0;
    for framed in bulk_chunk_frames(transfer_id, bytes) {
        // raw part 길이 = framed 길이 − binary sub-header(transfer_id/seq).
        let part_len = (framed.len() - stream::BULK_CHUNK_HEADER_LEN) as u64;
        conn.send(StreamTag::Data, &framed)?;
        sent += part_len;
        // 09: 청크 전송 진행 통지(누적 바이트). 콜백이 채널/AppEvent 로 메인에 흘린다.
        on_progress(sent, total);
    }

    // commit — 전송 완료. 서버가 저장 확정 후 BulkResult 회신.
    let commit = StreamControl::BulkCommit { transfer_id };
    conn.send(StreamTag::Control, &serde_json::to_vec(&commit)?)?;
    Ok(())
}

/// 서버의 `BulkResult` 를 기다린다 — heartbeat Ping/다른 transfer 의 응답/미지
/// Control(전방 호환)은 흘리고 이 `transfer_id` 의 결과만 기다린다. read timeout 이
/// 걸려 있어 서버 무응답 시 무기한 대기하지 않는다.
fn await_bulk_result(conn: &mut StreamConnection, transfer_id: u64) -> anyhow::Result<String> {
    loop {
        let frame = conn.recv()?;
        match frame.tag {
            StreamTag::Control => {
                match serde_json::from_slice::<StreamControl>(&frame.payload) {
                    Ok(StreamControl::BulkResult {
                        transfer_id: tid,
                        ok,
                        path,
                        reason,
                    }) if tid == transfer_id => {
                        if let Err(e) = conn.detach() {
                            // graceful close 실패 — 소켓 Drop 이 정리하므로 무해, 진단만.
                            tracing::debug!("bulk upload: detach after result failed: {e}");
                        }
                        return if ok {
                            path.ok_or_else(|| {
                                anyhow::anyhow!("bulk result ok but carried no path")
                            })
                        } else {
                            Err(anyhow::anyhow!(
                                "{BULK_REJECT_PREFIX}{}",
                                reason.unwrap_or_else(|| "unknown".to_string())
                            ))
                        };
                    }
                    // 다른 transfer 의 result 나 미지 Control(전방 호환) — 무시하고 계속.
                    _ => {}
                }
            }
            StreamTag::Detach => {
                anyhow::bail!("remote detached before delivering bulk result");
            }
            // Ping(heartbeat)/기타 Data 는 result 채널에서 무의미 — 흘린다.
            _ => {}
        }
    }
}

// 06-β 가 완성한 전송 자유 함수 — 08(이미지 paste)이 백그라운드 스레드에서 호출한다.
//
// `on_progress(sent, total)` (09): 각 청크 전송 직후 누적 전송 바이트를 통지한다. 호출자
// (08 워커)가 이 콜백으로 진행 이벤트를 메인 루프에 흘려 determinate progress 팝업을
// 갱신한다. 전송 시작 시 1회(0, total) 로도 발화해 0% 프레임을 즉시 띄운다. 통지가
// 필요 없으면 `|_, _| {}` 를 넘긴다. 06 전송 로직 자체는 불변 — 콜백 호출만 추가.
pub(crate) fn upload_file_over_bulk(
    port: u16,
    remote_ws: u32,
    file_name: &str,
    bytes: &[u8],
    on_progress: impl Fn(u64, u64),
) -> anyhow::Result<String> {
    let transfer_id = next_bulk_transfer_id();
    let mut conn = open_bulk_connection(port, remote_ws)?;
    send_bulk_payload(&mut conn, transfer_id, file_name, bytes, on_progress)?;
    await_bulk_result(&mut conn, transfer_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::IdGenerator;
    use crate::ipc::stream::SplitAxis;

    /// 창이 없는(parked) engine 이든 창이 있는 engine 이든, mirror 워크스페이스
    /// 정리는 워크스페이스 행 하나만 지우는 게 아니라 그 세션이 만든 mirror
    /// 터미널·mirror busy 엔트리·mesh 프레임 캐시를 **함께** 걷어내야 한다.
    /// `cleanup_mirror_workspace` 가 main/parked 양쪽에 그대로 쓰는 공용 본문이라
    /// 이 함수만 검증하면 두 경로가 같이 커버된다.
    #[test]
    fn remove_mirror_workspace_clears_terminal_busy_and_mesh() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let ws_id = 9_000u32;
        let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
        let remote_surface = 42u32;

        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            pane_id,
            tab_id,
            local_surface,
        );
        mirror_ws.mirror = true;
        engine.workspaces.push(mirror_ws);
        engine
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24));
        engine.set_mirror_surface_busy(local_surface, true);
        engine
            .attach_mesh_frames
            .update(local_surface, vec![1, 2, 3], 0, 0, true);
        // mirror 워크스페이스가 활성인 상태에서 정리 → 인덱스 클램프까지 확인.
        state.active_workspace = engine.workspaces.len() - 1;

        let remote_to_local = HashMap::from([(remote_surface, local_surface)]);
        assert!(remove_mirror_workspace_from_engine(
            &mut engine,
            &mut state,
            ws_id,
            &remote_to_local
        ));

        assert!(!engine.has_workspace(ws_id), "mirror 워크스페이스 행 제거");
        assert!(
            !engine.terminals.contains(local_surface),
            "mirror 터미널 제거"
        );
        assert!(
            !engine.is_surface_busy(local_surface),
            "mirror busy 엔트리 제거"
        );
        assert!(
            engine.attach_mesh_frames.get(local_surface).is_none(),
            "mesh 프레임 캐시 제거"
        );
        assert_eq!(
            state.active_workspace,
            engine.workspaces.len() - 1,
            "제거로 out-of-range 가 된 active_workspace 클램프"
        );
    }

    /// 그 워크스페이스를 들고 있지 않은 engine 은 건드리지 않는다 — main → parked
    /// 순회에서 매칭되지 않은 engine 의 터미널을 잘못 지우면 안 된다.
    #[test]
    fn remove_mirror_workspace_leaves_unrelated_engine_untouched() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let local_surface = 9_003u32;
        engine
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24));
        let before = engine.workspaces.len();

        let remote_to_local = HashMap::from([(42u32, local_surface)]);
        assert!(!remove_mirror_workspace_from_engine(
            &mut engine,
            &mut state,
            9_000,
            &remote_to_local
        ));
        assert_eq!(engine.workspaces.len(), before);
        assert!(engine.terminals.contains(local_surface));
    }

    /// `parked_states` 순회 자체의 회귀 가드 — mirror 워크스페이스를 들고 있는
    /// parked engine 이 **첫 항목이 아니어도** 찾아 정리해야 하고, 무관한 parked
    /// engine 은 건드리지 않아야 한다. 창을 여럿 닫으면 parked engine 이 여럿 쌓인다.
    #[test]
    fn cleanup_scans_all_parked_engines_for_the_mirror_workspace() {
        let ws_id = 9_000u32;
        let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
        let remote_to_local = HashMap::from([(42u32, local_surface)]);

        let mut parked: Vec<(crate::state::AppState, crate::core::CoreState)> =
            (0..2).map(|_| crate::state::tests::test_state()).collect();
        let untouched_ws_count = parked[0].1.workspaces.len();

        // mirror 는 **두 번째** parked engine 에만 심는다.
        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            pane_id,
            tab_id,
            local_surface,
        );
        mirror_ws.mirror = true;
        {
            let (state, engine) = &mut parked[1];
            engine.workspaces.push(mirror_ws);
            engine
                .terminals
                .insert(local_surface, Terminal::new_detached(80, 24));
            engine.set_mirror_surface_busy(local_surface, true);
            engine
                .attach_mesh_frames
                .update(local_surface, vec![1, 2, 3], 0, 0, true);
            state.active_workspace = engine.workspaces.len() - 1;
        }

        assert!(remove_mirror_workspace_from_parked(
            &mut parked,
            ws_id,
            &remote_to_local
        ));

        let (state, engine) = &parked[1];
        assert!(!engine.has_workspace(ws_id), "mirror 워크스페이스 행 제거");
        assert!(
            !engine.terminals.contains(local_surface),
            "mirror 터미널 제거"
        );
        assert!(
            !engine.is_surface_busy(local_surface),
            "mirror busy 엔트리 제거"
        );
        assert!(
            engine.attach_mesh_frames.get(local_surface).is_none(),
            "mesh 프레임 캐시 제거"
        );
        assert_eq!(state.active_workspace, engine.workspaces.len() - 1);
        assert_eq!(
            parked[0].1.workspaces.len(),
            untouched_ws_count,
            "무관한 parked engine 은 건드리지 않는다"
        );
    }

    /// 어느 parked engine 에도 그 워크스페이스가 없으면 `false` — 호출부가 창 있는
    /// engine 에서 이미 정리했거나(2차 순회 불필요) 사용자가 직접 닫은 경우다.
    #[test]
    fn cleanup_parked_scan_reports_false_when_absent() {
        let mut parked: Vec<(crate::state::AppState, crate::core::CoreState)> =
            (0..2).map(|_| crate::state::tests::test_state()).collect();
        let remote_to_local = HashMap::from([(42u32, 9_003u32)]);
        assert!(!remove_mirror_workspace_from_parked(
            &mut parked,
            9_000,
            &remote_to_local
        ));
    }

    /// (06) bulk 청킹/시퀀스/헤더 인코딩 라운드트립: 각 프레임이 올바른
    /// transfer_id·seq 를 달고, 파트를 순서대로 이으면 원본과 바이트 동일해야 한다.
    #[test]
    fn bulk_chunk_frames_roundtrip_and_reassembly() {
        let transfer_id = 0xABCD_1234_5678_9F01u64;
        // 2.5 청크 분량(>1 MiB 를 포함해 다중 파트 경계를 밟는다).
        let total = BULK_CHUNK_RAW_LEN * 2 + 777;
        let data: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();

        let frames = bulk_chunk_frames(transfer_id, &data);
        assert_eq!(frames.len(), 3, "2.5 청크 = 3 파트");

        let mut reassembled = Vec::new();
        for (expected_seq, framed) in frames.iter().enumerate() {
            // 프레임은 MAX_FRAME_LEN 을 넘지 않아야 한다(수신측 read_frame 이 거부).
            assert!(framed.len() <= stream::MAX_FRAME_LEN as usize);
            let (tid, seq, part) =
                stream::decode_bulk_chunk(framed).expect("valid bulk chunk header");
            assert_eq!(tid, transfer_id);
            assert_eq!(seq as usize, expected_seq);
            reassembled.extend_from_slice(part);
        }
        assert_eq!(reassembled, data, "재조립 바이트가 원본과 동일");
    }

    /// (06) 빈 파일: 청크 0개(begin→commit 만으로 0바이트 저장).
    #[test]
    fn bulk_chunk_frames_empty_is_zero_chunks() {
        assert!(bulk_chunk_frames(1, &[]).is_empty());
    }

    /// (06) 정확히 한 청크 상한 크기: 파트 1개, 경계에서 분할이 새지 않는다.
    #[test]
    fn bulk_chunk_frames_exact_boundary_is_single_chunk() {
        let data = vec![7u8; BULK_CHUNK_RAW_LEN];
        let frames = bulk_chunk_frames(9, &data);
        assert_eq!(frames.len(), 1);
        let (_, seq, part) = stream::decode_bulk_chunk(&frames[0]).unwrap();
        assert_eq!(seq, 0);
        assert_eq!(part.len(), BULK_CHUNK_RAW_LEN);
    }

    /// to_tree_json_full → SurfaceLayout 재구성: 분할 방향/비율/focus 보존 +
    /// remote→local id 재매핑 + 터미널/placeholder kind 구분.
    #[test]
    fn build_layout_preserves_split_and_remaps_ids() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(100u32, 5u32); // 100 → local 5 (terminal)
        map.insert(101u32, 6u32); // 101 → local 6 (placeholder)
        let mut term = HashSet::new();
        term.insert(5u32);
        let node = serde_json::json!({
            "type": "Split",
            "direction": "vertical",
            "ratio": 0.3,
            "focus_second": true,
            "first": { "type": "Leaf", "id": 100, "kind": "terminal" },
            "second": { "type": "Leaf", "id": 101, "kind": "empty" },
        });
        let layout = build_layout(&node, &ids, &map, &term, &HashMap::new(), &HashMap::new())
            .expect("layout");
        match layout {
            SurfaceLayout::Split {
                direction,
                ratio,
                focus_second,
                first,
                second,
            } => {
                assert_eq!(direction, SplitDirection::Vertical);
                assert!((ratio - 0.3).abs() < 1e-6);
                assert!(focus_second);
                // remote id 가 로컬로 치환됐는지.
                assert_eq!(first.first_surface_id(), Some(5));
                assert_eq!(second.first_surface_id(), Some(6));
                // 터미널 vs placeholder kind.
                assert_eq!(first.find_surface(5).unwrap().kind(), "terminal");
                assert_ne!(second.find_surface(6).unwrap().kind(), "terminal");
            }
            _ => panic!("expected Split"),
        }
    }

    /// (ADR-0059 참고) `role=="explorer"` leaf 는 `explorer` 맵의 root 로
    /// `ExplorerPanel::new(local, root)`(cwd == root 단순화)를 구성해야 한다.
    #[test]
    fn build_layout_constructs_explorer_panel_from_explorer_map() {
        let ids = IdGenerator::new();
        let map = HashMap::from([(200u32, 9u32)]);
        let term = HashSet::new();
        let mesh = HashMap::new();
        let explorer = HashMap::from([(9u32, std::path::PathBuf::from("/remote/project"))]);
        let node = serde_json::json!({ "type": "Leaf", "id": 200, "kind": "explorer" });
        let layout = build_layout(&node, &ids, &map, &term, &mesh, &explorer).expect("layout");
        let SurfaceLayout::Leaf(surface) = layout else {
            panic!("expected Leaf");
        };
        assert_eq!(surface.kind(), "explorer");
        let panel = surface
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .expect("ExplorerPanel");
        assert_eq!(panel.id, 9);
        assert_eq!(
            panel.current_root(),
            std::path::Path::new("/remote/project")
        );
    }

    /// 단일 pane·tab 디스크립터 → mirror Workspace: 로컬 id 발급 + 트리 보존.
    #[test]
    fn build_mirror_workspace_single_pane_tab() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "Shell", "active": true, "focused_surface": 1,
                    "layout": { "type": "Leaf", "id": 1, "kind": "terminal" }
                } ]
            } ]
        });
        let ws = build_mirror_workspace(
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(ws.id, 99);
        // mirror surface = 로컬 50 (remote 1 재매핑).
        assert_eq!(ws.all_surface_ids(), vec![50]);
    }

    /// 3단계 역반영의 핵심 계약: survivor(기존 매핑 remote_id)는 **기존 local id 를
    /// 유지**하고 신규 remote leaf 는 새 local id 로 트리에 삽입된다. `apply_mirror_
    /// structural_delta` 가 갱신하는 매핑을 그대로 재현해 `build_mirror_workspace` 에
    /// 넘겼을 때 survivor local id 가 보존되는지 검증한다(터미널 재생성 방지의 기반).
    #[test]
    fn build_mirror_workspace_preserves_survivor_and_inserts_new_leaf() {
        let ids = IdGenerator::new();
        // survivor: remote 1 → 기존 local 50(유지). 신규: remote 2 → 새 local 발급.
        let survivor_local = 50u32;
        let mut map = HashMap::new();
        map.insert(1u32, survivor_local);
        let new_local = ids.next_surface(); // 역반영이 신규에 발급하는 것과 동형.
        map.insert(2u32, new_local);
        let mut term = HashSet::new();
        term.insert(survivor_local);
        term.insert(new_local);
        // split 트리: survivor(remote 1) + 신규(remote 2).
        let tree = serde_json::json!({
            "id": 9, "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "Shell", "active": true, "focused_surface": 1,
                    "layout": {
                        "type": "Split", "direction": "vertical", "ratio": 0.5,
                        "focus_second": false,
                        "first": { "type": "Leaf", "id": 1, "kind": "terminal" },
                        "second": { "type": "Leaf", "id": 2, "kind": "terminal" }
                    }
                } ]
            } ]
        });
        let ws = build_mirror_workspace(
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        let sids = ws.all_surface_ids();
        assert!(
            sids.contains(&survivor_local),
            "survivor local id({survivor_local}) 가 유지돼야 한다: {sids:?}"
        );
        assert!(
            sids.contains(&new_local),
            "신규 leaf local id({new_local}) 가 트리에 삽입돼야 한다: {sids:?}"
        );
        assert_eq!(sids.len(), 2, "survivor + 신규 = 2개 leaf");
    }

    /// 빈/널 트리 fallback — panic 없이 placeholder workspace.
    #[test]
    fn build_mirror_workspace_empty_tree_fallback() {
        let ids = IdGenerator::new();
        let map = HashMap::new();
        let term = HashSet::new();
        let ws = build_mirror_workspace(
            1,
            "remote",
            &serde_json::Value::Null,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(ws.id, 1);
        assert_eq!(ws.all_surface_ids().len(), 1);
    }

    /// pane_layout 필드가 있으면 direction/ratio/focused_pane 이 정확히 복원돼야 한다
    /// (이번 버그의 핵심 회귀 테스트).
    #[test]
    fn build_mirror_workspace_preserves_vertical_pane_split() {
        let ids = IdGenerator::new();
        let map = HashMap::new(); // 이 테스트는 focused_surface 매핑 불필요(pane 레벨 검증 목적)
        let term = HashSet::new();
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 8,
            "panes": [],
            "pane_layout": {
                "type": "Split",
                "direction": "vertical",
                "ratio": 0.3,
                "first": { "type": "Leaf", "id": 7, "tabs": [] },
                "second": { "type": "Leaf", "id": 8, "tabs": [] }
            }
        });
        let ws = build_mirror_workspace(
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        match ws.pane_layout() {
            PaneNode::Split {
                direction,
                ratio,
                second,
                ..
            } => {
                assert_eq!(*direction, SplitDirection::Vertical);
                assert!((*ratio - 0.3).abs() < 0.001);
                // focused_pane(remote 8) 이 second(새로 발급된 로컬 id)로 매핑됐는지.
                if let PaneNode::Leaf(p) = second.as_ref() {
                    assert_eq!(ws.focused_pane, p.id);
                } else {
                    panic!("expected second to be Leaf");
                }
            }
            _ => panic!("expected Split, got Leaf"),
        }
    }

    /// pane_layout 필드가 없으면(구버전 서버) 기존 horizontal-chain fallback 이
    /// 그대로 동작해야 한다(하위호환 회귀 검증 — 기존 3개 테스트와 별개로, "필드 부재"
    /// 그 자체를 명시적으로 검증).
    #[test]
    fn build_mirror_workspace_falls_back_to_horizontal_chain_without_pane_layout_field() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        map.insert(2u32, 51u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 2,
            "panes": [
                { "id": 1, "tabs": [ { "id": 3, "name": "Shell", "active": true,
                    "focused_surface": 1, "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } } ] },
                { "id": 2, "tabs": [ { "id": 4, "name": "Shell", "active": true,
                    "focused_surface": 2, "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } } ] }
            ]
            // "pane_layout" 필드 없음 — 구버전 서버 흉내.
        });
        let ws = build_mirror_workspace(
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        match ws.pane_layout() {
            PaneNode::Split {
                direction, ratio, ..
            } => {
                assert_eq!(*direction, SplitDirection::Horizontal);
                assert!((*ratio - 0.5).abs() < 1e-6);
            }
            _ => panic!("expected Split (2 panes → horizontal chain fallback)"),
        }
    }

    /// pane B, tab2 의 surface(local 52)를 담은 workspace 를 만들어 `capture_focused_remote`
    /// 가 **remote id 3**(local 52)을 정확히 되짚어내는지 검증한다(TODO
    /// 01-mirror-workspace-focus-jump 원인 분석의 "1. 캡처" 단계).
    #[test]
    fn capture_focused_remote_finds_remote_id_of_locally_focused_surface() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32); // pane A 의 surface
        map.insert(2u32, 51u32); // pane B, tab1 의 surface
        map.insert(3u32, 52u32); // pane B, tab2 의 surface — 사용자가 보고 있는 곳
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        term.insert(52u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 11,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                    { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                      "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
                ] }
            }
        });
        let ws = build_mirror_workspace(
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(
            capture_focused_remote(&ws, &map),
            Some(3),
            "focused_pane=pane B, active_tab=tab2(remote 3) 를 정확히 되짚어야 한다"
        );
    }

    /// 핵심 회귀 테스트(mirror-workspace-focus-jump): 클라이언트가 로컬에서만
    /// pane B 의 두 번째 탭으로 이동해둔 상태에서 구조 변경 delta 가 도착하면(원격의
    /// focused_pane 은 forward 되는 순수 focus op 가 없어 항상 최초 pane=pane A 로
    /// 고정), 패치 전에는 재구성된 트리가 pane A(첫 pane)로 focus 를 되돌렸다.
    /// `capture_focused_remote`(교체 전) → `restore_focus_after_delta`(교체 후) 조합이
    /// 실제 `apply_mirror_structural_delta` 가 쓰는 것과 동일한 복원 로직이다.
    #[test]
    fn focus_restore_keeps_client_on_pane_b_after_structural_delta_from_pane_a() {
        let ids = IdGenerator::new();

        // "before": 원격의 focused_pane 은 최초 pane A(10) 에 고정. 사용자는 로컬에서
        // pane B(11) 의 두 번째 탭(remote 3)으로 이동해 있다.
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        map.insert(2u32, 51u32);
        map.insert(3u32, 52u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        term.insert(52u32);
        let before_tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 10,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                    { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                      "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
                ] }
            }
        });
        let mut before_ws = build_mirror_workspace(
            99,
            "remote",
            &before_tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );

        // 사용자가 로컬에서 pane B, tab2 로 이동한다 — 순수 클릭/키보드 네비게이션이라
        // 원격에는 아무것도 forward 되지 않는다(버그의 1번 원인). 서버가 선언한
        // focused_pane(10, pane A)과는 별개로, 클라이언트의 실제 로컬 focus 만 바뀐다.
        let pane_b_surface3_local = *map.get(&3).unwrap();
        let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&before_ws, pane_b_surface3_local)
            .expect("pane B tab2 surface must exist");
        before_ws.focused_pane = pane_b_id;
        let pane_b = before_ws
            .pane_layout_mut()
            .find_pane_mut(pane_b_id)
            .expect("pane B exists");
        let tab_index = pane_b
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .expect("tab exists");
        pane_b.active_tab = tab_index;
        pane_b.tabs[tab_index].focused_surface = pane_b_surface3_local;

        let old_focused_remote = capture_focused_remote(&before_ws, &map);
        assert_eq!(old_focused_remote, Some(3));

        // "after": pane A 에 새 탭(remote 4)이 background 로 추가된 구조 변경 delta.
        // 원격의 focused_pane 은 여전히 pane A(10) — 버그의 근본 원인 그대로 재현.
        let mut after_map = map.clone();
        after_map.insert(4u32, 53u32);
        term.insert(53u32);
        let after_tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 10,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } },
                    { "id": 101, "name": "Shell", "active": false, "focused_surface": 4,
                      "layout": { "type": "Leaf", "id": 4, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                    { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                      "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
                ] }
            }
        });
        let mut after_ws = build_mirror_workspace(
            99,
            "remote",
            &after_tree,
            &ids,
            &after_map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );

        // 대조군 — 복원 없이 그대로 두면 pane A(원격의 고정값)에 focus 가 있다(버그 재현).
        let pane_a_local_surface = *after_map.get(&1).unwrap();
        let (pane_a_id, _) = find_pane_and_tab_for_surface(&after_ws, pane_a_local_surface)
            .expect("pane A surface must exist in rebuilt tree");
        assert_eq!(
            after_ws.focused_pane, pane_a_id,
            "패치 전이라면 재구성 직후 focus 는 항상 pane A(원격 고정값)"
        );

        // 수정된 복원 로직 적용.
        restore_focus_after_delta(&mut after_ws, old_focused_remote, &after_map);

        let pane_b_surface3_local = *after_map.get(&3).unwrap();
        let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&after_ws, pane_b_surface3_local)
            .expect("pane B tab2 surface must exist in rebuilt tree");
        assert_eq!(
            after_ws.focused_pane, pane_b_id,
            "복원 후 focus 는 pane A 가 아니라 사용자가 실제로 보던 pane B 에 있어야 한다"
        );
        let pane_b = after_ws
            .pane_layout()
            .find_pane(pane_b_id)
            .expect("pane B exists");
        assert_eq!(
            pane_b.tabs[pane_b.active_tab].id, tab_id,
            "pane B 의 active_tab 도 사용자가 보던 두 번째 탭이어야 한다"
        );
        assert_eq!(
            pane_b.tabs[pane_b.active_tab].focused_surface, pane_b_surface3_local,
            "그 탭의 focused_surface 도 정확히 그 surface 를 가리켜야 한다"
        );
    }

    /// 캡처해둔 surface 자체가 이번 op 로 사라졌으면(예: CloseSurface 로 그 surface 를
    /// 직접 닫음) 억지로 복원하지 않고 원격이 보낸 값 그대로 둬야 한다(무리한 복원 방지).
    #[test]
    fn focus_restore_is_noop_when_captured_surface_no_longer_exists() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        map.insert(2u32, 51u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 10,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": true, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } }
                ] }
            }
        });
        let mut ws = build_mirror_workspace(
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        let untouched_focused_pane = ws.focused_pane;

        // 캡처된 surface(remote 3)는 이 map/tree 어디에도 없다 — 이미 닫힌 상태를 흉내.
        restore_focus_after_delta(&mut ws, Some(3), &map);

        assert_eq!(
            ws.focused_pane, untouched_focused_pane,
            "캡처된 surface 가 없으면 원격이 보낸 focused_pane 그대로 둬야 한다"
        );
    }

    /// `set_focus_to_surface` — 존재하는 surface 로는 focused_pane/active_tab/
    /// focused_surface 를 모두 갱신하고 `true`, 없는 surface 로는 아무것도 안 바꾸고
    /// `false`(08/09 가 공유하는 핵심 primitive).
    #[test]
    fn set_focus_to_surface_updates_pane_tab_surface_or_reports_false() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        map.insert(2u32, 51u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 10,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": true, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } }
                ] }
            }
        });
        let mut ws = build_mirror_workspace(
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
        );
        let local_b = *map.get(&2).unwrap();

        assert!(set_focus_to_surface(&mut ws, local_b));
        let (pane_b_id, tab_b_id) =
            find_pane_and_tab_for_surface(&ws, local_b).expect("pane B exists");
        assert_eq!(ws.focused_pane, pane_b_id);
        let pane_b = ws.pane_layout().find_pane(pane_b_id).unwrap();
        assert_eq!(pane_b.tabs[pane_b.active_tab].id, tab_b_id);
        assert_eq!(pane_b.tabs[pane_b.active_tab].focused_surface, local_b);

        assert!(
            !set_focus_to_surface(&mut ws, 12345),
            "존재하지 않는 surface 는 false"
        );
    }

    /// 08 — new-tab/split 계열은 항상 `NewResource`(원격 id map 과 무관).
    #[test]
    fn pending_op_focus_for_new_tab_and_split_is_new_resource() {
        let map = HashMap::new();
        for op in [
            StructuralOp::NewTab {
                anchor_surface_id: 1,
                surface_kind: "terminal".to_string(),
                params: serde_json::Value::Null,
            },
            StructuralOp::SplitSurface {
                surface_id: 1,
                direction: SplitAxis::Horizontal,
                surface_kind: "terminal".to_string(),
                params: serde_json::Value::Null,
            },
            StructuralOp::SplitPane {
                anchor_surface_id: 1,
                direction: SplitAxis::Vertical,
                surface_kind: "terminal".to_string(),
                params: serde_json::Value::Null,
            },
        ] {
            assert!(matches!(
                pending_op_focus_for(&op, &[], &map),
                Some(PendingOpFocus::NewResource)
            ));
        }
    }

    /// 09 — close 계열은 `close_focus_candidates`(로컬 id)를 map 으로 원격 id 로
    /// 치환해 담는다. map 에 없는 후보만 있으면 `None`(fallback 대상 없음).
    #[test]
    fn pending_op_focus_for_close_translates_candidates_or_none() {
        let mut map = HashMap::new();
        map.insert(7u32, 70u32); // remote 7 -> local 70
        map.insert(8u32, 71u32); // remote 8 -> local 71

        let op = StructuralOp::CloseSurface { surface_id: 1 };
        match pending_op_focus_for(&op, &[70, 71], &map) {
            Some(PendingOpFocus::Close { candidates }) => {
                assert_eq!(candidates, vec![7, 8]);
            }
            other => panic!("expected Close{{candidates}}, got {other:?}"),
        }

        // 후보가 전부 매핑에 없으면(예상 밖) fallback 대상 없음 → None.
        assert!(pending_op_focus_for(&op, &[999], &map).is_none());
        // close 후보를 아예 안 준 경우(빈 슬라이스)도 None.
        assert!(pending_op_focus_for(&op, &[], &map).is_none());
    }

    /// move-tab/convert 등 08/09 대상이 아닌 op 은 후보가 있어도 항상 `None`.
    #[test]
    fn pending_op_focus_for_non_target_ops_is_none() {
        let mut map = HashMap::new();
        map.insert(7u32, 70u32);
        let op = StructuralOp::MoveTab {
            anchor_surface_id: 1,
            from_index: 0,
            to_index: 1,
        };
        assert!(pending_op_focus_for(&op, &[70], &map).is_none());
    }

    /// 서버가 mesh 디스크립터에 실제 `display_name`(예: markdown 파일명)을 실어보내면
    /// `MirrorMeshInfo.display_name` 에 그대로 반영돼야 하고, 필드 자체가 없으면
    /// (구버전 서버 등) 기존처럼 `kind` 로 fallback 해야 한다.
    #[test]
    fn merge_survivor_mapping_prefers_server_display_name_and_falls_back_to_kind() {
        let ids = IdGenerator::new();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let surfaces = vec![
            serde_json::json!({
                "remote_id": 10,
                "role": "mesh",
                "kind": "markdown",
                "plugin_id": "com.tasty.markdown",
                "display_name": "README.md",
            }),
            serde_json::json!({
                "remote_id": 11,
                "role": "mesh",
                "kind": "image",
                "plugin_id": "com.tasty.image",
                // display_name 필드 없음 — fallback 확인용.
            }),
        ];

        let (map, _term, mesh, _explorer, _new) =
            merge_survivor_mapping(&HashMap::new(), &surfaces, &ids, &frame_tx, &mut engine);

        let local_10 = map[&10];
        let local_11 = map[&11];
        assert_eq!(mesh[&local_10].display_name, "README.md");
        assert_eq!(
            mesh[&local_11].display_name, "image",
            "display_name 필드가 없으면 kind 로 fallback 해야 한다"
        );
    }

    /// convert 로 kind 가 바뀐 survivor(terminal → mesh)는 local id 를 유지하면서도
    /// 옛 kind 전용 로컬 리소스(Terminal 객체·mesh frame 캐시)를 즉시 정리해야 한다 —
    /// 안 그러면 surface 가 나중에 닫힐 때까지 orphan 으로 남는다(Gate4 리뷰 판단필요
    /// 항목).
    #[test]
    fn merge_survivor_mapping_cleans_up_stale_terminal_on_convert_to_mesh() {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
        // engine 이 이미 소비한 id 와 안 겹치도록 engine 자신의 발급기를 공유한다
        // (production 경로 `apply_mirror_structural_delta` 와 동일 — 독립된
        // `IdGenerator::new()` 는 기본 workspace 의 default surface id 와 충돌한다).
        let ids = engine.next_ids.clone();
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        // 1차: remote 10 이 terminal 로 mirror 세션에 들어온다.
        let surfaces_v1 = vec![serde_json::json!({
            "remote_id": 10, "role": "terminal", "cols": 80, "rows": 24,
        })];
        let (map1, term1, mesh1, explorer1, _new1) =
            merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine);
        let local_10 = map1[&10];
        assert!(
            engine.terminals.get(local_10).is_some(),
            "최초 terminal survivor 는 Terminal 을 만들어야 한다"
        );
        // 정리가 실제로 동작하는지 확실히 보려고 mesh frame 캐시도 하나 심어둔다.
        engine
            .attach_mesh_frames
            .update(local_10, vec![1, 2, 3], 1, 1, true);

        // production 경로(`apply_mirror_structural_delta`)와 동일하게 트리에 반영해야
        // 다음 호출의 `find_surface_by_id` 로 "옛 kind" 를 조회할 수 있다.
        let tree = serde_json::json!({
            "id": 9, "name": "mirror", "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "Shell", "active": true, "focused_surface": 10,
                    "layout": { "type": "Leaf", "id": 10, "kind": "terminal" }
                } ]
            } ]
        });
        let mut ws = build_mirror_workspace(
            999, "mirror", &tree, &ids, &map1, &term1, &mesh1, &explorer1,
        );
        ws.mirror = true;
        engine.workspaces.push(ws);

        // 2차: 같은 remote_id(10) 가 markdown 으로 convert.
        let surfaces_v2 = vec![serde_json::json!({
            "remote_id": 10,
            "role": "mesh",
            "kind": "markdown",
            "plugin_id": "com.tasty.markdown",
            "display_name": "a.md",
        })];
        let (map2, term2, mesh2, _explorer2, new2) =
            merge_survivor_mapping(&map1, &surfaces_v2, &ids, &frame_tx, &mut engine);

        assert_eq!(
            map2[&10], local_10,
            "local id 는 convert 후에도 유지돼야 한다"
        );
        assert!(new2.is_empty(), "survivor 는 신규 취급되면 안 된다");
        assert!(
            !term2.contains(&local_10),
            "markdown 으로 바뀐 뒤에는 더 이상 terminal_locals 에 없어야 한다"
        );
        assert!(
            mesh2.contains_key(&local_10),
            "mesh_locals 에는 새로 등록돼야 한다"
        );
        assert!(
            engine.terminals.get(local_10).is_none(),
            "옛 Terminal 객체는 즉시 제거돼야 한다"
        );
        assert!(
            engine.attach_mesh_frames.get(local_10).is_none(),
            "옛(terminal 시절의 무의미한) mesh frame 캐시도 제거돼야 한다"
        );
    }

    /// convert 로 kind 가 바뀐 survivor(mesh → terminal)는 지금까지 Terminal 객체가
    /// 없었으므로(mesh 였으므로) 새로 만들어야 입력 forwarding 이 동작한다 — 신규
    /// survivor 와 동일한 생성 경로(`make_mirror_surface`)를 타는지 확인.
    #[test]
    fn merge_survivor_mapping_creates_terminal_when_mesh_survivor_converts_to_terminal() {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
        // 독립된 `IdGenerator::new()` 는 기본 workspace 의 default surface id 와
        // 충돌한다 — engine 자신의 발급기를 공유한다(production 경로와 동일).
        let ids = engine.next_ids.clone();
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let surfaces_v1 = vec![serde_json::json!({
            "remote_id": 20,
            "role": "mesh",
            "kind": "markdown",
            "plugin_id": "com.tasty.markdown",
            "display_name": "a.md",
        })];
        let (map1, term1, mesh1, explorer1, _new1) =
            merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine);
        let local_20 = map1[&20];
        assert!(
            engine.terminals.get(local_20).is_none(),
            "mesh survivor 는 애초에 Terminal 이 없어야 한다"
        );

        let tree = serde_json::json!({
            "id": 9, "name": "mirror", "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "a.md", "active": true, "focused_surface": 20,
                    "layout": { "type": "Leaf", "id": 20, "kind": "markdown" }
                } ]
            } ]
        });
        let mut ws = build_mirror_workspace(
            999, "mirror", &tree, &ids, &map1, &term1, &mesh1, &explorer1,
        );
        ws.mirror = true;
        engine.workspaces.push(ws);

        let surfaces_v2 = vec![serde_json::json!({
            "remote_id": 20, "role": "terminal", "cols": 80, "rows": 24,
        })];
        let (map2, term2, _mesh2, _explorer2, new2) =
            merge_survivor_mapping(&map1, &surfaces_v2, &ids, &frame_tx, &mut engine);

        assert_eq!(
            map2[&20], local_20,
            "local id 는 convert 후에도 유지돼야 한다"
        );
        assert!(new2.is_empty(), "survivor 는 신규 취급되면 안 된다");
        assert!(term2.contains(&local_20));
        assert!(
            engine.terminals.get(local_20).is_some(),
            "mesh → terminal convert 는 새 Terminal 을 만들어야 한다(안 그러면 입력이 안 감)"
        );
    }

    /// 테스트용 mirror 세션 — transport 없이 매핑·이벤트 버퍼만 있다. write 큐의
    /// 수신측을 바로 drop 하므로 delta 가 만드는 mirror 터미널의 입력 forwarder 는
    /// 전송에 실패해도 조용히 계속된다(production 의 disconnect 구간과 동일).
    fn test_session(
        local_workspace: u32,
        remote_to_local: HashMap<u32, u32>,
    ) -> AttachClientSession {
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        AttachClientSession {
            local_workspace,
            remote_to_local,
            output: MirrorOutbox::new(),
            disconnected: Arc::new(AtomicBool::new(false)),
            frame_tx: Arc::new(Mutex::new(tx)),
            state: SessionState::Connected,
            client_id: 1,
            remote_workspace: 7,
            bulk_port: 0,
            tunnel: None,
            anchor_ws_id: None,
            op_seq: 0,
            pending_op_focus: HashMap::new(),
            next_delta_focus: None,
            last_forwarded_resize: HashMap::new(),
            remote_label: "127.0.0.1:0".to_string(),
            pending_list_dir_consumers: HashMap::new(),
        }
    }

    /// parked engine 두 개 — mirror 워크스페이스(터미널 `local_surface` 하나)는 **두
    /// 번째**에만 심는다. 창을 여럿 닫으면 parked engine 이 여럿 쌓이므로 첫 항목만
    /// 보는 순회는 여기서 걸린다.
    fn parked_with_mirror(
        ws_id: u32,
        local_surface: u32,
    ) -> Vec<(crate::state::AppState, crate::core::CoreState)> {
        let mut parked: Vec<(crate::state::AppState, crate::core::CoreState)> =
            (0..2).map(|_| crate::state::tests::test_state()).collect();
        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            9_001,
            9_002,
            local_surface,
        );
        mirror_ws.mirror = true;
        let engine = &mut parked[1].1;
        engine.workspaces.push(mirror_ws);
        engine
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24));
        parked
    }

    /// 적용 대상 선택 순서 — 창 있는 engine 이 우선, 없으면 parked engine(첫 항목이
    /// 아니어도), 어디에도 없으면 `None`(호출부가 drain 을 건너뛰는 신호).
    #[test]
    fn mirror_output_host_prefers_window_then_parked_then_none() {
        let ws_id = 9_000u32;
        let parked = parked_with_mirror(ws_id, 9_003);
        let wid = winit::window::WindowId::from(7u64);
        assert_eq!(
            mirror_output_host(Some(wid), &parked, ws_id),
            Some(MirrorOutputHost::Window(wid)),
            "창 있는 engine 이 있으면 그쪽"
        );
        assert_eq!(
            mirror_output_host(None, &parked, ws_id),
            Some(MirrorOutputHost::Parked(1)),
            "창이 없으면 mirror 를 든 parked engine(두 번째)"
        );
        assert_eq!(
            mirror_output_host(None, &parked, 424_242),
            None,
            "어느 engine 에도 없으면 None — drain 하지 않는다"
        );
    }

    /// 창이 없는 parked engine 에 mirror 이벤트가 그대로 적용된다 — `Data` 는 mirror
    /// 터미널 grid 에, `StructuralDelta` 는 매핑·트리에, 그 delta 로 생긴 새 surface 의
    /// 후속 `Data` 는 갱신된 매핑으로 라우팅된다. 이 경로가 없으면 창을 최소화한 동안
    /// 도착한 출력이 통째로 유실되고 `remote_to_local` 이 desync 된다(ADR-0110).
    #[test]
    fn parked_engine_receives_mirror_data_and_structural_delta() {
        let ws_id = 9_000u32;
        let (survivor_remote, survivor_local, new_remote) = (42u32, 9_003u32, 43u32);
        let mut parked = parked_with_mirror(ws_id, survivor_local);
        let untouched_ws_count = parked[0].1.workspaces.len();
        let mut sess = test_session(ws_id, HashMap::from([(survivor_remote, survivor_local)]));

        let tree = serde_json::json!({
            "id": 7, "name": "mirror", "focused_pane": 70,
            "panes": [ {
                "id": 70,
                "tabs": [ {
                    "id": 30, "name": "Shell", "active": true, "focused_surface": survivor_remote,
                    "layout": {
                        "type": "Split", "direction": "vertical", "ratio": 0.5,
                        "focus_second": false,
                        "first": { "type": "Leaf", "id": survivor_remote, "kind": "terminal" },
                        "second": { "type": "Leaf", "id": new_remote, "kind": "terminal" }
                    }
                } ]
            } ]
        });
        let surfaces = vec![
            serde_json::json!({ "remote_id": survivor_remote, "role": "terminal", "cols": 80, "rows": 24 }),
            serde_json::json!({ "remote_id": new_remote, "role": "terminal", "cols": 80, "rows": 24 }),
        ];
        let events = vec![
            MirrorEvent::Data(survivor_remote, b"hello-parked".to_vec()),
            MirrorEvent::StructuralDelta {
                workspace_id: 7,
                tree,
                surfaces,
            },
            MirrorEvent::Data(new_remote, b"world-new".to_vec()),
        ];

        let pidx = find_parked_with_workspace(&parked, ws_id).expect("mirror 를 든 parked engine");
        {
            let (state, engine) = &mut parked[pidx];
            let mut host = MirrorHost::parked(state, engine);
            let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
            apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, events);
        }

        let engine = &parked[pidx].1;
        let survivor = engine
            .terminals
            .get(survivor_local)
            .expect("survivor mirror 터미널은 delta 뒤에도 같은 local id 로 남는다");
        assert!(
            survivor.screen_text(false).contains("hello-parked"),
            "parked 동안 도착한 Data 가 mirror grid 에 남아야 한다: {:?}",
            survivor.screen_text(false)
        );
        let new_local = *sess
            .remote_to_local
            .get(&new_remote)
            .expect("delta 가 새 remote surface 를 매핑에 넣어야 한다(desync 방지)");
        let fresh = engine
            .terminals
            .get(new_local)
            .expect("delta 가 새 mirror 터미널을 만들어야 한다");
        assert!(
            fresh.screen_text(false).contains("world-new"),
            "delta 이후의 Data 가 갱신된 매핑으로 새 터미널에 라우팅돼야 한다: {:?}",
            fresh.screen_text(false)
        );
        let ws = engine
            .workspaces
            .iter()
            .find(|w| w.id == ws_id)
            .expect("mirror 워크스페이스는 같은 local id 로 교체된다");
        let sids = ws.all_surface_ids();
        assert!(
            sids.contains(&survivor_local) && sids.contains(&new_local),
            "{sids:?}"
        );
        assert_eq!(
            parked[0].1.workspaces.len(),
            untouched_ws_count,
            "무관한 parked engine 은 건드리지 않는다"
        );
    }

    /// 적용 대상을 확보하지 못하면 버퍼를 건드리지 않는다는 것을 **의도로 기록**한다.
    ///
    /// **집행 지점이 아니다.** 이 성질은 `MirrorOutbox::take_for` 가 host 를 요구하고
    /// 필드가 `mod outbox` 밖에서 안 보인다는 사실이 지탱한다 — 위반하는 코드는 애초에
    /// 컴파일되지 않으므로, 이 테스트가 잡을 수 있는 것은 `apply_pending_mirror_output`
    /// 이 `None` 을 받고도 `true` 를 보고하는 정도의 사소한 변이뿐이다. 남겨둔 이유는
    /// 둘이다: 아래 `a_host_…` 와의 **대칭축**(그 테스트만 있으면 "아무것도 안 하는
    /// 함수" 가 통과한다), 그리고 왜 `None` 분기가 버퍼를 남기는지를 코드 옆에 적어두는
    /// 것(꺼낸 뒤 적용에 실패하면 되돌릴 방법이 없고, mirror 이벤트 유실은 조용히
    /// 일어난다 — `Data` 는 복원 뒤 화면 결손, `StructuralDelta` 는 매핑 desync).
    #[test]
    fn no_host_leaves_the_mirror_buffer_untouched() {
        let ws_id = 9_000u32;
        let mut sess = test_session(ws_id, HashMap::new());
        sess.output.peek().extend([
            MirrorEvent::Data(1, b"a".to_vec()),
            MirrorEvent::Resize(1, 10, 5),
        ]);
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

        let applied = apply_pending_mirror_output(&mut sess, None, &mut plugin_manager);

        assert!(!applied, "적용 대상이 없으면 적용했다고 보고하지 않는다");
        let buf = sess.output.peek();
        assert_eq!(
            buf.len(),
            2,
            "host 가 없으면 버퍼는 그대로 남아 다음 호출이 다시 시도한다"
        );
        assert!(matches!(buf[0], MirrorEvent::Data(1, ref b) if b == b"a"));
        assert!(matches!(buf[1], MirrorEvent::Resize(1, 10, 5)));
    }

    /// host 가 있으면 같은 함수가 버퍼를 비우고 적용한다 — 위 테스트가 "아무것도
    /// 안 하는 함수" 를 통과시키는 것이 아님을 고정하는 대칭 축.
    #[test]
    fn a_host_drains_and_applies_the_mirror_buffer() {
        let ws_id = 9_000u32;
        let local_surface = 9_003u32;
        let remote_surface = 42u32;
        let mut parked = parked_with_mirror(ws_id, local_surface);
        let mut sess = test_session(ws_id, HashMap::from([(remote_surface, local_surface)]));
        sess.output
            .peek()
            .push(MirrorEvent::Data(remote_surface, b"applied-here".to_vec()));
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

        let pidx = find_parked_with_workspace(&parked, ws_id).expect("mirror 를 든 parked engine");
        let applied = {
            let (state, engine) = &mut parked[pidx];
            apply_pending_mirror_output(
                &mut sess,
                Some(MirrorHost::parked(state, engine)),
                &mut plugin_manager,
            )
        };

        assert!(applied);
        assert!(sess.output.peek().is_empty(), "적용했으면 버퍼는 비워진다");
        let term = parked[pidx]
            .1
            .terminals
            .get(local_surface)
            .expect("mirror 터미널");
        assert!(term.screen_text(false).contains("applied-here"));
    }

    /// 창 유무는 **부수효과만** 게이트한다(ADR-0110). parked engine 에는 toast 를
    /// 쌓지 않는다 — 표면이 없고 토스트 수명이 wall-clock 이라 복원 시점엔 이미
    /// 만료돼 보이지도 않는다. 상태 변경은 게이트와 무관하게 항상 적용된다.
    ///
    /// `MirrorHost::parked` 의 `windowed` 를 `true` 로 뒤집는 변이에서 실패해야 한다.
    #[test]
    fn parked_host_does_not_stack_toasts_but_windowed_does() {
        let ws_id = 9_000u32;
        let mut sess = test_session(ws_id, HashMap::new());
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
        let failure = || vec![MirrorEvent::StructuralFailed("nope".to_string())];

        let (mut parked_state, mut parked_engine) = crate::state::tests::test_state();
        {
            let mut host = MirrorHost::parked(&mut parked_state, &mut parked_engine);
            apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, failure());
        }
        assert_eq!(
            parked_state.toasts.len(),
            0,
            "창이 없는 engine 에는 toast 를 쌓지 않는다"
        );

        let (mut win_state, mut win_engine) = crate::state::tests::test_state();
        {
            let mut host = MirrorHost::windowed(&mut win_state, &mut win_engine);
            apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, failure());
        }
        assert_eq!(
            win_state.toasts.len(),
            1,
            "창이 있으면 같은 이벤트가 toast 를 낸다 — 게이트가 창 유무로만 갈린다"
        );
    }

    /// 버퍼는 도착 순서대로 통째로 꺼내지고 비워진다 — resize 앞뒤 출력이 올바른
    /// grid 에서 재생되려면 순서가 보존돼야 한다.
    ///
    /// 꺼내려면 `MirrorHost` 가 있어야 한다는 것 자체가 이 테스트의 형태에 드러난다 —
    /// host 없이 부르는 판은 컴파일되지 않는다.
    #[test]
    fn take_for_takes_everything_in_arrival_order() {
        let buf = MirrorOutbox::new();
        buf.peek().extend([
            MirrorEvent::Data(1, b"a".to_vec()),
            MirrorEvent::Resize(1, 10, 5),
            MirrorEvent::Data(1, b"b".to_vec()),
        ]);
        let (mut state, mut engine) = crate::state::tests::test_state();
        let host = MirrorHost::parked(&mut state, &mut engine);
        let drained = buf.take_for(&host);
        assert!(matches!(drained[0], MirrorEvent::Data(1, ref b) if b == b"a"));
        assert!(matches!(drained[1], MirrorEvent::Resize(1, 10, 5)));
        assert!(matches!(drained[2], MirrorEvent::Data(1, ref b) if b == b"b"));
        assert_eq!(drained.len(), 3);
        assert!(buf.peek().is_empty(), "꺼낸 뒤 버퍼는 비어 있다");
    }
}
