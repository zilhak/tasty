#![forbid(unsafe_code)]

//! attach 스트림의 Control 메시지, capability 이름, workspace attach descriptor의 wire 타입.
//!
//! serde 타입과 상수만 둔다. 프레임 코덱·소켓·handshake·세션은 tasty-ipc와 호스트가 맡는다.
//! 데스크톱 의존(tokio·저장소·GUI) 없이 다른 client가 같은 타입을 쓰도록 의존을 serde 수준으로 묶고,
//! 그 범위는 `crates/tasty-attach-protocol/tests/dependency_closure.rs`가 지킨다.

mod descriptor;
mod inherit_cwd;

pub use descriptor::AttachedWorkspaceDescriptor;
pub use inherit_cwd::InheritCwd;
use serde::{Deserialize, Serialize};

/// 서버가 모든 ClientResize에 Resize 또는 ResizeRejected로 응답한다는 capability 이름.
/// system.info에 알리고 workspace attach descriptor의 [`DESCRIPTOR_CAPABILITIES`]에도 싣는다.
/// client는 연결마다 descriptor를 보고 응답을 기다릴지 정한다.
pub const RESIZE_ACK_CAPABILITY: &str = "ipc.stream.resize-ack";
/// [`RESIZE_ACK_CAPABILITY`]의 버전.
pub const RESIZE_ACK_VERSION: u32 = 1;
/// Main-loop receipt of all Data/MeshInput frames preceding an input checkpoint.
/// This does not acknowledge shell consumption, command completion, or rendering.
pub const INPUT_ACK_CAPABILITY: &str = "ipc.stream.input-ack";
/// 서버가 [`StructuralOp::CloseWorkspace`]로 점유한 workspace 전체를 닫고 결과를 회신한다는 capability 이름.
/// 이 기능이 없는 구 서버는 모르는 op를 회신 없이 버리므로 client는 보내지 않고 미지원으로 알린다.
pub const WORKSPACE_CLOSE_CAPABILITY: &str = "ipc.stream.workspace-close";
/// 서버가 원격 생성 요청의 `inherit_cwd`(상속 원본 surface 또는 상속 안 함)를 따른다는 capability 이름.
/// 이 기능이 없는 구 서버는 필드를 무시하고 자기 View 선택과 자기 설정으로 상속한다. client는 그래도
/// 요청을 보낸다. 결과는 필드가 없을 때의 규칙과 같지만 "상속 안 함"은 지켜지지 않는다.
pub const INHERIT_CWD_CAPABILITY: &str = "ipc.stream.inherit-cwd";
/// 서버가 터미널 바이트에서 알림을 직접 만들고, 그 새 알림의 소리를 `ClientTerminalEffects`를 선언한
/// holder에게 [`StreamControl::NotificationSound`]로 넘긴다는 capability 이름. GUI 서버만 싣는다.
/// 이 기능이 있는 연결의 mirror는 자기가 같은 바이트로 만든 터미널 유래 알림의 소리를 내지 않는다.
/// 없는 서버(headless, 구 서버)면 mirror가 자기 알림으로 소리를 낸다.
pub const NOTIFICATION_SOUND_CAPABILITY: &str = "ipc.stream.notification-sound";
/// workspace attach descriptor에서 이 연결의 서버 기능 이름 목록을 담는 키.
/// 구 서버는 이 키를 보내지 않으므로 없으면 빈 목록으로 읽는다.
pub const DESCRIPTOR_CAPABILITIES: &str = "stream_capabilities";
/// workspace attach descriptor에서 서버 PTY가 읽는 키 입력 형식을 담는 키.
/// 값은 [`PTY_INPUT_WIN32_CONSOLE`] 또는 [`PTY_INPUT_VT`]다. 구 서버는 이 키를 보내지 않으므로
/// 없으면 VT로 읽는다.
pub const DESCRIPTOR_PTY_INPUT: &str = "pty_input";
/// ConPTY: 수식키가 붙은 줄바꿈 키를 win32-input 시퀀스로 받아야 하는 Windows 콘솔.
pub const PTY_INPUT_WIN32_CONSOLE: &str = "win32-console";
/// Unix PTY: CSI-u와 제어문자를 그대로 응용 프로그램에 전달한다.
pub const PTY_INPUT_VT: &str = "vt";

/// 이 빌드의 PTY 입력 형식. 서버가 descriptor에 싣는다.
pub fn local_pty_input() -> &'static str {
    if cfg!(windows) {
        PTY_INPUT_WIN32_CONSOLE
    } else {
        PTY_INPUT_VT
    }
}

/// descriptor가 Windows 콘솔 입력을 알리는지. 키가 없거나 모르는 값이면 false(VT)다.
pub fn descriptor_wants_win32_console_input(descriptor: &serde_json::Value) -> bool {
    descriptor
        .get(DESCRIPTOR_PTY_INPUT)
        .and_then(|v| v.as_str())
        == Some(PTY_INPUT_WIN32_CONSOLE)
}

/// 서버가 점유를 끊을 때 Detach보다 먼저 보내는 Control의 `event` 값.
pub const FORCE_DETACHED_EVENT: &str = "force_detached";

/// `force_detached` Control의 `reason` 가운데 서버가 점유한 workspace를 잃어 점유를 정리한 경우.
/// 사용자·에이전트의 강제 끊기가 아니므로 client는 연결 끊김처럼 다시 붙어 보고 그 workspace가
/// 없다는 것을 확인한다.
pub const FORCE_DETACH_WORKSPACE_GONE: &str = "workspace_gone";

/// `force_detached` Control이 알리는 끊김의 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForceDetachNotice {
    /// 사용자·에이전트의 강제 끊기. client는 명시적 연결 전까지 다시 붙지 않는다.
    Final,
    /// 점유한 workspace가 사라져 생긴 정리([`FORCE_DETACH_WORKSPACE_GONE`]).
    WorkspaceGone,
}

/// Control payload를 JSON으로 읽어 `event`가 [`FORCE_DETACHED_EVENT`]일 때만 통지로 본다.
/// 다른 Control의 값에 같은 문자열이 들어 있어도(예: 파일 이름) 통지가 아니다.
pub fn force_detach_notice(payload: &[u8]) -> Option<ForceDetachNotice> {
    serde_json::from_slice::<ForceDetachedControl>(payload)
        .ok()
        .map(|control| control.notice())
}

/// 서버가 점유를 끊을 때 Detach보다 먼저 보내는 `force_detached` Control. 서버는 사유를 늘 싣는다.
/// 읽을 때 `event`가 [`FORCE_DETACHED_EVENT`]가 아니면 거절하고, 사유가 없거나 문자열이 아니면 빈 사유로 읽는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", tag = "event", rename = "force_detached")]
pub struct ForceDetachedControl {
    /// 끊은 사유. [`FORCE_DETACH_WORKSPACE_GONE`]만 client 동작을 바꾼다.
    #[serde(default, deserialize_with = "string_or_empty")]
    pub reason: String,
}

impl ForceDetachedControl {
    /// 이 통지가 알리는 끊김의 종류. [`FORCE_DETACH_WORKSPACE_GONE`] 밖의 사유는 모두 강제 끊기다.
    pub fn notice(&self) -> ForceDetachNotice {
        if self.reason == FORCE_DETACH_WORKSPACE_GONE {
            ForceDetachNotice::WorkspaceGone
        } else {
            ForceDetachNotice::Final
        }
    }
}

// 파생 Deserialize는 struct의 tag를 확인하지 않으므로 `event`를 먼저 본 뒤 파생 본문으로 읽는다.
impl Serialize for ForceDetachedControl {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ForceDetachedControl::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for ForceDetachedControl {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("event").and_then(serde_json::Value::as_str) != Some(FORCE_DETACHED_EVENT) {
            return Err(serde::de::Error::custom("not a force_detached control"));
        }
        ForceDetachedControl::deserialize(value).map_err(serde::de::Error::custom)
    }
}

fn string_or_empty<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value.as_str().unwrap_or_default().to_string())
}

/// 세션 중 전달하는 event 태그 기반 Control 메시지. handshake descriptor는 별도 JSON이다.
/// 모르는 variant는 역직렬화에 실패하고 소비자가 무시한다. Loss는 출력의 연속성을 바꾸므로
/// ClientLossNotify를 선언한 클라이언트에만 보낸다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamControl {
    /// Client→server: an ordered boundary after previously queued Data/MeshInput.
    InputCheckpoint { sequence: u64 },
    /// Server→client: the main loop dispatched the input preceding this boundary.
    /// Rejected input still follows its existing error path; receipt is not success.
    InputProcessed { sequence: u64 },
    /// 서버→클라이언트: 원격 PTY에 적용된 크기. 클라이언트는 로컬 요청값이 아니라
    /// 이 응답으로 mirror 크기를 바꿔 원격 reflow 결과와 맞춘다.
    Resize {
        /// Remote surface id. The client maps it to its local mirror surface id
        /// (workspace attach) or applies it to its sole mirror (surface attach).
        surface_id: u32,
        cols: usize,
        rows: usize,
    },
    /// 서버→클라이언트: 원격 surface의 busy/idle 상태. mirror의 활동 표시에 사용한다.
    /// 서버는 1Hz 조회에서 바뀐 값을 보낸다. 송신 전에 변화 캐시가 갱신되므로
    /// 버려진 프레임을 다음 tick이 같은 값으로 다시 보내지는 않는다.
    Activity {
        /// Remote surface id, resolved the same way as [`StreamControl::Resize`].
        surface_id: u32,
        busy: bool,
    },
    /// 클라이언트→서버: 로컬 mirror pane에 맞는 PTY 크기를 요청한다.
    /// 서버가 holder를 확인하고 resize한 뒤 Resize로 확정 크기를 보낸다.
    /// [`RESIZE_ACK_CAPABILITY`]를 알리는 서버는 모든 요청에 응답한다. 크기가 이미 같으면 출력과 같은
    /// 순서로 현재 크기의 Resize를, 대상·점유가 맞지 않으면 ResizeRejected를 보낸다.
    /// 그 capability가 없는 구 서버는 크기가 같을 때나 거절할 때 응답하지 않는다.
    ClientResize {
        /// Remote surface id (the client maps its local mirror id to this before
        /// sending). The server resolves the enclosing workspace and verifies the
        /// requesting client is its attach holder before applying.
        surface_id: u32,
        cols: usize,
        rows: usize,
    },
    /// 서버→클라이언트: ClientResize를 적용하지 않았다. 대상이 terminal이 아니거나 없거나,
    /// 요청한 연결이 그 workspace의 holder가 아닐 때다. 크기를 바꾸지 않으므로 출력 순서와 무관하게
    /// 바로 보낸다. 값은 거절한 요청의 것이며 client는 이것으로 기다리던 요청을 찾는다.
    /// 구 client는 이 variant를 모르므로 역직렬화에 실패해 무시한다.
    ResizeRejected {
        /// Remote surface id named by the rejected request.
        surface_id: u32,
        cols: usize,
        rows: usize,
    },
    /// 클라이언트→서버: 사용자가 mirror surface를 확인해 attention을 지웠다는 통지.
    /// 실제 레코드가 제거됐을 때만 보내며 holder를 확인한 서버가 원격 상태도 지운다.
    /// 서버의 Attention(None)을 이미 빈 mirror에 적용해도 제거가 없어 다시 통지하지 않는다.
    /// 별도 응답은 없다.
    ClientAttentionClear {
        /// Remote surface id whose attention record should be dropped. The server
        /// resolves the enclosing workspace and verifies the requesting client is
        /// its attach holder before applying.
        surface_id: u32,
    },
    /// 클라이언트→서버: mirror의 구조 변경을 원격에서 실행하도록 요청한다.
    /// anchor는 원격 surface ID다. 서버가 자신의 트리에서 pane/tab/workspace를 찾고
    /// holder를 확인한 뒤 같은 op_id의 StructuralResult를 보낸다.
    StructuralOp {
        /// Client-assigned monotonic id, echoed back in the result so the client
        /// can correlate the reply (and toast on failure).
        op_id: u64,
        op: StructuralOp,
        /// 사용자 또는 에이전트 요청인지 구분한다. 서버의 복원 기록과 탭 선택에 사용한다.
        /// 이전 client와의 호환을 위해 필드 생략은 User다. 새 client는 항상 보낸다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<ForwardOrigin>,
    },
    /// 서버→클라이언트: 구조 변경 결과. 거절 시 reason을 보내며 해당 작업은 적용하지 않는다.
    /// 예를 들어 원격에 등록되지 않은 surface 종류는 만들 수 없다.
    StructuralResult {
        op_id: u64,
        /// Exact resource created by this operation, when the server knows it.
        /// Old peers omit/ignore this field; never infer an arbitrary ID from a merged snapshot.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_surface_id: Option<u32>,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// 성공한 원격 사용자 닫기에서 큐 예산을 넘어 닫은 항목 기록에 내용 없이 남긴 terminal 수.
        /// 요청한 사용자가 경고를 받는다. idempotent replay 회신과 옛 서버는 보내지 않는다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history_omitted: Option<u32>,
    },
    /// 서버→클라이언트: 구조 변경 후 workspace 전체 트리와 surface descriptor.
    /// handshake와 같은 형식이며, 클라이언트는 살아남은 터미널의 로컬 ID·스크롤백을 유지한다.
    /// 전달된 op의 결과라면 성공 StructuralResult 다음에 보내고, 서버 자체 구조 변화도 이 형식을 쓴다.
    StructuralDelta {
        /// Remote workspace id (the client maps it to its local mirror
        /// workspace; a mirror session hosts exactly one workspace so this is
        /// mainly for validation/diagnostics).
        workspace_id: u32,
        /// Post-op full workspace tree (`to_attach_tree_json`, same shape as the
        /// handshake `tree`).
        tree: serde_json::Value,
        /// Post-op per-surface descriptors (same shape as the handshake
        /// `surfaces`: `{remote_id, role, cols, rows}` for terminals /
        /// `{remote_id, role, kind}` for placeholders).
        surfaces: Vec<serde_json::Value>,
    },
    /// bulk 파일 전송(docs/dev-guide/attach-behavior.md#커스텀-이벤트-확장-streamcontrol-밖-raw-json-event-태그)의 control-plane 시작 메시지. 전용 bulk 연결에서
    /// 실제 파일 바이트(`Data` 프레임, `tasty_ipc::stream::encode_bulk_chunk`)에 앞서 파일명·총 크기를
    /// 알린다. 서버는 `total_size` 를 사전 용량 승인의 입력으로 쓰고, `transfer_id`
    /// 단위로 청크를 누적한다. 저장 dir 결정·경로 회신은 `commit` 에서 확정.
    ///
    /// Direction: **client→server**.
    BulkBegin {
        transfer_id: u64,
        filename: String,
        total_size: u64,
    },
    /// bulk 전송 완료 신호. 서버는 누적 바이트를 파일로 저장 확정하고
    /// [`StreamControl::BulkResult`] 로 원격 절대경로(또는 실패사유)를 회신한다.
    ///
    /// Direction: **client→server**.
    BulkCommit { transfer_id: u64 },
    /// bulk 전송 결과. `ok=true` 면 `path` 에 원격 파일시스템 절대경로, `ok=false` 면
    /// `reason` 에 실패사유(용량 초과·미인가·저장 실패 등). 소비자(mirror 터미널 이미지 붙여넣기 업로드 · 전송 진행/실패 팝업)가 이 경로를
    /// 대화형 스트림에 삽입하거나 진행 UI 에 표시한다.
    ///
    /// Direction: **server→client**.
    BulkResult {
        transfer_id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// 클라이언트→서버: mirror pane의 크기·배율·테마·포커스.
    /// 첫 메시지가 mesh 구독을 시작하고 이후 변경은 원격 플러그인의 set_context에 반영한다.
    /// 입력 묶음은 별도 MeshInput으로 전달한다.
    MeshContext {
        /// Remote surface id (client-mapped, like every other mirror message).
        surface_id: u32,
        /// Physical pixel width of the client's local mirror pane.
        width_px: u32,
        /// Physical pixel height of the client's local mirror pane.
        height_px: u32,
        /// Logical→physical scale (egui `ScreenDescriptor.pixels_per_point`).
        pixels_per_point: f32,
        /// The client's own resolved theme — the mirror should visually match
        /// what the *attach client* is displaying, not the (possibly headless,
        /// possibly differently-themed) server.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
        /// Whether the local mirror pane currently has keyboard focus.
        #[serde(default)]
        focused: bool,
    },
    /// 클라이언트→서버: RawInputWire 입력 묶음. PointerGone·포커스·modifier도 포함한다.
    /// 별도 응답 없이 원격 repaint의 MeshData로 화면 결과를 받는다.
    MeshInput {
        surface_id: u32,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    },
    /// 클라이언트→서버: frame_seq가 끊겼을 때 전체 텍스처를 다시 요청한다.
    /// 서버는 다음 set_context에 need_full_textures를 켠다. 별도 ack 대신
    /// 해당 surface의 full_textures=true인 MeshData가 도착했는지 확인한다.
    MeshFullResendRequest { surface_id: u32 },
    /// 서버→클라이언트: surface 소유 인스턴스의 attention 상태. None은 해제를 뜻한다.
    /// mirror는 이 값을 AttentionStore에 반영한다. 서버는 바뀐 값만 보내므로
    /// 유실된 프레임이 다음 tick에 자동 재전송되거나 client의 임의 상태 변경이 복구되지는 않는다.
    Attention {
        /// Remote surface id, resolved the same way as [`StreamControl::Resize`].
        surface_id: u32,
        /// The attention kind now recorded on the remote, or `None` if cleared.
        kind: Option<AttentionKindWire>,
    },
    /// 서버→클라이언트: 점유한 대상에 새 알림이 생겼으니 client 설정대로 알림음을 한 번 낸다.
    /// 출처와 무관하게 서버가 새로 저장한 알림마다 보낸다(GUI 서버는 터미널 유래 알림 포함).
    /// 알림을 만든 때 한 번만 보내며 재연결·유실 프레임을 다시 보내지 않는다.
    /// 구 client는 이 variant를 모르므로 역직렬화에 실패해 무시한다.
    NotificationSound {
        /// Remote surface id the notification belongs to, or 0 for a workspace-level one.
        surface_id: u32,
    },
    /// 서버→클라이언트: 원격이 확인한 cwd. None이면 mirror의 저장값도 지운다.
    /// 원격 경로이므로 로컬 파일 작업에 사용하지 않는다. 서버의 inherit_cwd와 무관하게
    /// 관측값을 보내며 실행 시 그 설정의 적용 여부는 소비자가 판단한다.
    /// 값이나 holder가 바뀔 때 보내며 유실 후 다음 tick의 재전송은 보장하지 않는다.
    Cwd {
        /// Remote surface id, resolved the same way as [`StreamControl::Resize`].
        surface_id: u32,
        /// The remote path, or `None` if the remote cwd is unknown.
        cwd: Option<String>,
    },
    /// 서버→클라이언트: 원격 PTY의 전경 프로세스 이름. None이면 mirror의 저장값도 지운다.
    /// mirror에는 조회할 로컬 PTY가 없어 앱별 입력 규칙·마우스 캡처 목록·상태바가 이 값을 쓴다.
    /// 값이나 holder가 바뀔 때 보내며 유실 후 다음 tick의 재전송은 보장하지 않는다.
    Foreground {
        /// Remote surface id, resolved the same way as [`StreamControl::Resize`].
        surface_id: u32,
        /// The remote foreground process name, or `None` if it is unknown.
        name: Option<String>,
    },
    /// 서버→클라이언트: mesh mirror가 불가능한 surface의 오류.
    /// 대상 부재·지원 kind 아님·플러그인 미실행 등의 reason을 보낸다.
    MeshError { surface_id: u32, reason: String },
    /// 서버→클라이언트: 이 연결의 push 큐에서 프레임이 유실됐다.
    /// ClientLossNotify를 선언한 연결에만 보내며 새 출력보다 먼저 큐에 넣는다.
    /// 유실 뒤에 선언하면 그전의 누적 손실도 알린다. 여러 surface가 같은 연결을 쓰므로 surface ID는 없다.
    /// 복구는 PTY·상태·mesh·bulk 소비자가 각자의 규칙으로 처리한다.
    Loss {
        /// Frames dropped for this connection since the previous `Loss` frame (or
        /// since the connection opened, if this is the first). Counts frames, not
        /// bytes — the sink is a frame queue and the bytes behind a dropped frame
        /// are gone before anyone counts them.
        frames: u64,
    },
    /// 클라이언트→서버: Loss 통지를 받을 수 있다고 선언한다. 반복해도 같고 해제 기능은 없다.
    /// 선언 전에는 손실을 조용히 처리한다. 지원하지 않는 구 서버는 이 variant를 무시한다.
    /// 서버 지원 여부는 system.info의 ipc.stream.loss-notify로 확인한다.
    /// STREAM_PROTO는 동등 비교이므로 이 추가 기능 때문에 버전을 올리지 않는다.
    ClientLossNotify {},
    /// 클라이언트→서버: 이 연결이 받는 터미널 바이트의 사용자 쪽 효과(OSC 52 클립보드 쓰기·읽기)를
    /// 스스로 처리하고, 점유한 대상의 서버 알림음을 `NotificationSound`로 넘겨받아 자기 설정대로
    /// 재생하겠다고 선언한다. 선언한 연결이 받는 동안 서버는 그 효과와 소리를 내지 않는다. 선언하지
    /// 않은 연결(CLI attach, 구 client)의 대상은 서버가 지금처럼 처리한다. 구 서버는 이 variant를
    /// 무시한다.
    ClientTerminalEffects {},
}

/// The concrete structural operation carried by [`StreamControl::StructuralOp`].
/// Every variant is anchored on remote surface id(s); the server resolves the
/// enclosing pane/tab/workspace from its authoritative tree, so the client never
/// needs to track remote pane/tab ids (it only maps surfaces).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StructuralOp {
    /// Split a surface within its tab. Anchor = the surface being split.
    SplitSurface {
        surface_id: u32,
        direction: SplitAxis,
        /// New surface kind (`"terminal"` or a registered plugin kind).
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
        /// cwd 상속 원본. 생략하면 서버가 자기 View 선택과 자기 설정을 쓴다([`InheritCwd`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inherit_cwd: Option<InheritCwd>,
    },
    /// Split the pane containing the anchor surface.
    SplitPane {
        anchor_surface_id: u32,
        direction: SplitAxis,
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
        /// cwd 상속 원본. 생략하면 서버가 자기 View 선택과 자기 설정을 쓴다([`InheritCwd`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inherit_cwd: Option<InheritCwd>,
    },
    /// Add a new tab to the pane containing the anchor surface.
    NewTab {
        anchor_surface_id: u32,
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
        /// cwd 상속 원본. 생략하면 서버가 자기 View 선택과 자기 설정을 쓴다([`InheritCwd`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inherit_cwd: Option<InheritCwd>,
    },
    /// Close a surface (cascading up to tab/pane/workspace as usual).
    CloseSurface { surface_id: u32 },
    /// Close the tab containing the anchor surface.
    CloseTab { anchor_surface_id: u32 },
    /// Close the pane containing the anchor surface.
    ClosePane { anchor_surface_id: u32 },
    /// Reorder a tab within the pane containing the anchor surface.
    MoveTab {
        anchor_surface_id: u32,
        from_index: usize,
        to_index: usize,
    },
    /// Convert a surface to a different kind in place.
    ConvertSurface {
        surface_id: u32,
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
        /// 변환 대상이 사용할 원격 cwd. 플랫폼에 의존하지 않는 경로 문자열이다.
        /// client가 모르면 None으로 보내고 서버가 원래 surface에서 찾는다.
        #[serde(default)]
        cwd: Option<String>,
        /// `cwd`가 없을 때의 상속 원본. 생략하면 서버가 자기 설정으로 원래 surface의 cwd를 찾는다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inherit_cwd: Option<InheritCwd>,
    },
    /// Move a live surface onto another surface's slot. On the wire both ids are
    /// remote ids of the held workspace; the server rejects a target outside it.
    MoveSurface {
        source_surface_id: u32,
        target_surface_id: u32,
    },
    /// anchor의 workspace 복원 스택에서 마지막 항목을 복원한다.
    /// 클라이언트가 종류·내용을 정하지 않으며 서버가 저장된 스크롤백과 PTY를 복원한다.
    RestoreClosedItem { anchor_surface_id: u32 },
    /// Close the whole held workspace that contains the anchor surface on the server,
    /// ending its PTYs. Sent only to a server whose descriptor carries
    /// [`WORKSPACE_CLOSE_CAPABILITY`]; an older server drops unknown ops without a reply.
    CloseWorkspace { anchor_surface_id: u32 },
}

/// 전달된 구조 변경의 호출 주체. 서버의 복원 기록과 사용자 탭 선택에 사용한다.
/// 생략/null은 옛 client와 같은 User다. 모르는 값은 프레임을 거절하지 않고 Agent로 읽어
/// 사용자 상태를 변경하는 권한을 주지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardOrigin {
    /// The client's user, by hand (shortcut, button, context menu).
    User,
    /// An agent driving the client over IPC/CLI — and any origin this build
    /// does not know (see the type's doc).
    Agent,
}

impl<'de> Deserialize<'de> for ForwardOrigin {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value.as_str() {
            Some("user") => ForwardOrigin::User,
            _ => ForwardOrigin::Agent,
        })
    }
}

impl ForwardOrigin {
    /// The origin a received op stands for. Absence is `User` because that is
    /// what every client before the field meant — see the field's doc on
    /// [`StreamControl::StructuralOp`].
    pub fn of_wire(field: Option<ForwardOrigin>) -> ForwardOrigin {
        field.unwrap_or(ForwardOrigin::User)
    }
}

/// 원격 복원 스택이 비었음을 구분하는 프로토콜 값. 사용자 문구가 아니므로 번역하지 않는다.
pub const STRUCTURAL_REASON_RESTORE_EMPTY: &str = "restore_empty";

/// 원격에 없는 파일로 markdown surface 를 바꾸려 할 때 거절 사유의 앞부분. 뒤에 원격 경로가 붙는다.
/// 로컬 `markdown.navigate` 의 거절 문구와 같고, client 는 이 앞부분으로 "파일 없음" 실패를 구분한다.
pub const STRUCTURAL_REASON_PATH_NOT_FOUND_PREFIX: &str = "path not found: ";

impl StructuralOp {
    /// The remote surface id this op is anchored on. The server uses it to locate
    /// the enclosing workspace and verify the requesting client is its attach
    /// holder before executing (only the occupier may mutate the workspace).
    pub fn anchor_surface_id(&self) -> u32 {
        match self {
            StructuralOp::SplitSurface { surface_id, .. }
            | StructuralOp::CloseSurface { surface_id }
            | StructuralOp::ConvertSurface { surface_id, .. } => *surface_id,
            StructuralOp::SplitPane {
                anchor_surface_id, ..
            }
            | StructuralOp::NewTab {
                anchor_surface_id, ..
            }
            | StructuralOp::CloseTab { anchor_surface_id }
            | StructuralOp::ClosePane { anchor_surface_id }
            | StructuralOp::RestoreClosedItem { anchor_surface_id }
            | StructuralOp::CloseWorkspace { anchor_surface_id }
            | StructuralOp::MoveTab {
                anchor_surface_id, ..
            } => *anchor_surface_id,
            StructuralOp::MoveSurface {
                source_surface_id, ..
            } => *source_surface_id,
        }
    }

    /// The op's wire tag (`kind`) — the same string serde writes. The server
    /// names a forwarded op by it when the op points at something that is no
    /// longer there, the way an IPC rejection names the request's method.
    pub fn wire_kind(&self) -> &'static str {
        match self {
            StructuralOp::SplitSurface { .. } => "split_surface",
            StructuralOp::SplitPane { .. } => "split_pane",
            StructuralOp::NewTab { .. } => "new_tab",
            StructuralOp::CloseSurface { .. } => "close_surface",
            StructuralOp::CloseTab { .. } => "close_tab",
            StructuralOp::ClosePane { .. } => "close_pane",
            StructuralOp::MoveTab { .. } => "move_tab",
            StructuralOp::ConvertSurface { .. } => "convert_surface",
            StructuralOp::MoveSurface { .. } => "move_surface",
            StructuralOp::RestoreClosedItem { .. } => "restore_closed_item",
            StructuralOp::CloseWorkspace { .. } => "close_workspace",
        }
    }

    /// Return a copy with the anchor surface id replaced. The mirror client builds
    /// an op with a **local** anchor id (the only id it knows at the block point),
    /// then swaps in the mapped **remote** id before sending. For `MoveSurface`
    /// the anchor is the source; the target is also a local id and must be
    /// replaced separately with [`with_move_target_surface_id`](Self::with_move_target_surface_id).
    pub fn with_anchor_surface_id(&self, remote: u32) -> StructuralOp {
        let mut cloned = self.clone();
        match &mut cloned {
            StructuralOp::SplitSurface { surface_id, .. }
            | StructuralOp::CloseSurface { surface_id }
            | StructuralOp::ConvertSurface { surface_id, .. } => *surface_id = remote,
            StructuralOp::SplitPane {
                anchor_surface_id, ..
            }
            | StructuralOp::NewTab {
                anchor_surface_id, ..
            }
            | StructuralOp::CloseTab { anchor_surface_id }
            | StructuralOp::ClosePane { anchor_surface_id }
            | StructuralOp::RestoreClosedItem { anchor_surface_id }
            | StructuralOp::CloseWorkspace { anchor_surface_id }
            | StructuralOp::MoveTab {
                anchor_surface_id, ..
            } => *anchor_surface_id = remote,
            StructuralOp::MoveSurface {
                source_surface_id, ..
            } => *source_surface_id = remote,
        }
        cloned
    }

    /// The second surface a `MoveSurface` names (its target). Other ops name only the anchor.
    pub fn move_target_surface_id(&self) -> Option<u32> {
        match self {
            StructuralOp::MoveSurface {
                target_surface_id, ..
            } => Some(*target_surface_id),
            _ => None,
        }
    }

    /// 생성 op가 싣는 cwd 상속 원본. 생성이 아닌 op와 생략은 None이다.
    pub fn inherit_cwd(&self) -> Option<&InheritCwd> {
        match self {
            StructuralOp::SplitSurface { inherit_cwd, .. }
            | StructuralOp::SplitPane { inherit_cwd, .. }
            | StructuralOp::NewTab { inherit_cwd, .. }
            | StructuralOp::ConvertSurface { inherit_cwd, .. } => inherit_cwd.as_ref(),
            _ => None,
        }
    }

    /// cwd 상속 원본을 바꾼 사본. 생성이 아닌 op는 그대로 돌려준다. client는 로컬 surface ID로
    /// op를 만든 뒤 이 함수로 원격 ID를 넣는다.
    pub fn with_inherit_cwd(&self, value: Option<InheritCwd>) -> StructuralOp {
        let mut cloned = self.clone();
        match &mut cloned {
            StructuralOp::SplitSurface { inherit_cwd, .. }
            | StructuralOp::SplitPane { inherit_cwd, .. }
            | StructuralOp::NewTab { inherit_cwd, .. }
            | StructuralOp::ConvertSurface { inherit_cwd, .. } => *inherit_cwd = value,
            _ => {}
        }
        cloned
    }

    /// Return a copy with the `MoveSurface` target replaced by the mapped **remote**
    /// id. Other ops are returned unchanged.
    pub fn with_move_target_surface_id(&self, remote: u32) -> StructuralOp {
        let mut cloned = self.clone();
        if let StructuralOp::MoveSurface {
            target_surface_id, ..
        } = &mut cloned
        {
            *target_surface_id = remote;
        }
        cloned
    }
}

/// 호스트 AttentionKind와 변환하는 wire 타입. surface.completion의
/// completion/needs_input 문자열과 같은 이름을 사용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKindWire {
    /// Work finished — completion IPC/CLI, OSC 133 command completion, toast.
    Completion,
    /// Waiting on the user — Claude plugin `notification`/`pre-tool-use` hooks.
    NeedsInput,
}

/// Split direction carried over the wire. Maps to the host `SplitDirection` /
/// the IPC `"vertical"`/`"horizontal"` convention on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Vertical,
    Horizontal,
}

impl SplitAxis {
    /// The IPC/`SplitDirection` string form (`handle_split` parses this).
    pub fn as_ipc_str(self) -> &'static str {
        match self {
            SplitAxis::Vertical => "vertical",
            SplitAxis::Horizontal => "horizontal",
        }
    }
}

fn default_terminal_kind() -> String {
    "terminal".to_string()
}

/// 서버가 원격 explorer 파일 작업을 받는다는 capability 이름. attach descriptor 의
/// [`DESCRIPTOR_CAPABILITIES`] 와 system.info 에 싣는다.
pub const EXPLORER_WRITE_CAPABILITY: &str = "ipc.stream.explorer-write";
/// 서버가 원격 자식 에이전트 요청과 완료 통지를 지원한다는 capability 이름.
pub const AGENT_CHILDREN_CAPABILITY: &str = "ipc.stream.agent-children";
/// 서버가 holder 의 원본 터미널 읽기를 받는다는 capability 이름.
pub const TERMINAL_READ_CAPABILITY: &str = "ipc.stream.terminal-read";
/// 서버가 경로 지정 파일 읽기를 받는다는 capability 이름.
pub const FILE_DOWNLOAD_CAPABILITY: &str = "ipc.stream.file-download";
/// HTML 문서를 Control 프레임으로 보내는 전송의 capability 이름.
pub const HTML_TRANSFER_CAPABILITY: &str = "ipc.stream.html-transfer-v1";
/// HTML 전송의 viewer 진입(html-entry) capability 이름.
pub const HTML_VIEWER_CAPABILITY: &str = "ipc.stream.html-entry-v1";
/// HTML 전송의 리소스 capability 이름.
pub const HTML_RESOURCE_CAPABILITY: &str = "ipc.stream.html-resources-v1";
/// 인코딩한 HTTP 전송(opt-in)의 capability 이름.
pub const HTML_HTTP_CAPABILITY: &str = "ipc.stream.html-http-v1";
/// 인코딩한 HTTP 전송의 reload capability 이름.
pub const HTML_HTTP_RELOAD_CAPABILITY: &str = "ipc.stream.html-http-reload-v1";
/// 인코딩한 HTTP 전송의 HTTPS capability 이름.
pub const HTML_HTTPS_CAPABILITY: &str = "ipc.stream.html-https-v1";
/// JSON-RPC 요청의 `idempotency_key` 를 서버가 읽는다는 선언의 이름.
pub const IDEMPOTENCY_CAPABILITY: &str = "ipc.idempotency-key";

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
