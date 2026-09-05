//! Plugin 생명주기 매니저.
//!
//! 호스트의 부팅 시 한 번 만들어지고, `App`이 유일한 인스턴스를 보유한다.
//! - 부팅 시 `discover_and_start()`로 `~/.tasty/plugins/`를 스캔하여 활성 plugin 모두 spawn
//! - 매 메인 루프 tick에서 `pump()` 호출 → plugin 알림 처리 + 헬스체크 + 재시작
//! - 종료 시 `shutdown_all()`

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use tasty_plugin_protocol::SharedBufferId;
use tasty_shm::SharedMemory;

use tasty_plugin_protocol::host_port::SurfaceRegistry;

use crate::handle_channel::HandleListener;
use crate::host_cmd::{HostCmd, SurfaceHandles};
use crate::ipc_namespace::IpcNamespaceRegistry;
use crate::listener::HostListener;
use crate::process::{PluginProcess, ShutdownBatch};
use crate::protocol::PluginResponse;
use crate::registry_state::PluginsConfig;
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_plugin_manifest::{HookMode, IpcHookDecl, Permission, PluginPackage};

/// host popup(`PopupManager`)과 plugin popup(`PluginManager::popup_instances`) 사이의
/// z-order 를 판정하는 유일한 공유 기준. 두 매니저가 서로 다른 크레이트에 있어 자료구조를
/// 통합하지 않는 대신, 열리거나 클릭/포커스될 때마다 이 전역 단조증가 순번을 하나씩
/// 받아 각자의 상태(`PopupState.z_seq` / `PopupInstance.z_seq`)에 기록한다 — 값이 큰 쪽이
/// 나중에 열리거나 클릭된 것이므로 항상 위에 그려진다(`docs/design/systems/popup.md` 규칙 7).
static NEXT_POPUP_Z_SEQ: AtomicU64 = AtomicU64::new(1);

/// 다음 z-order 순번을 발급한다. host popup 오픈/포커스와 plugin popup 오픈/클릭 양쪽에서
/// 호출된다 — 값 자체의 의미는 없고, 오직 다른 값과의 대소 비교(더 큰 쪽이 위)에만 쓰인다.
pub fn next_popup_z_seq() -> u64 {
    NEXT_POPUP_Z_SEQ.fetch_add(1, Ordering::Relaxed)
}

pub(super) const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const PING_INTERVAL: Duration = Duration::from_secs(15);
pub(super) const RESTART_FAILURE_WINDOW: Duration = Duration::from_secs(10);
pub(super) const RESTART_FAILURE_LIMIT: usize = 3;
/// plugin 하나에 주는 graceful 종료 기회. 초과하면 force kill 한다. 종료 전체
/// (`shutdown_all`)는 이 값을 plugin 마다 직렬로 더하지 않고 겹쳐서 소비하므로,
/// plugin 이 몇 개든 총 대기는 이 값으로 수렴한다.
pub(super) const PLUGIN_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// H — auto-reload polling 간격. pump tick 안의 자연 debounce — 2초 내
/// 발생한 연속 mtime 변경은 한 번의 swap 으로 흡수된다.
pub(super) const AUTO_RELOAD_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// RssSurge 이상탐지(`docs/features/telemetry/index.md`) — plugin RSS sampling
/// 주기. 너무 짧으면
/// sysinfo 호출 비용이 매 tick 마다 누적되고, 너무 길면 5-샘플 sliding
/// window(`RSS_SURGE_MIN_SAMPLES`)가 실제 급증을 늦게 잡는다.
pub(super) const RSS_SAMPLE_INTERVAL: Duration = Duration::from_secs(30);

/// plugin manager 가 자기 [`TimerHub`](tasty_timer::TimerHub) 에 등록하는 주기 작업 키.
///
/// 이 크레이트는 호스트 `App` 을 모르므로 본체 허브에 직접 등록할 수 없다 — 대신
/// 자기 허브를 소유하고 [`PluginManager::next_deadline`] 만 노출한다. 호스트는 그
/// 값을 자기 데드라인과 `min` 으로 합성한다(`docs/dev-guide/timer-hub.md`
/// "계층을 넘는 허브 합성").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PluginTick {
    /// `PING_INTERVAL` 주기 ping 송신 + 무응답 plugin 재시작 판정.
    ///
    /// healthcheck 를 별도 tick 으로 두지 않고 여기 합승시켰다 — `HEALTHCHECK_TIMEOUT`
    /// 은 인터벌이 아니라 "마지막 pong 이후 경과" 데드라인 비교라 검사 자체는 아무
    /// tick 에서나 할 수 있고, ping 을 보내는 tick 이 곧 응답을 기대하는 tick 이라
    /// 판정 시점으로 자연스럽다. 결과적으로 **비응답 검출 상한은
    /// `HEALTHCHECK_TIMEOUT + PING_INTERVAL` = 75 초**다(프로세스가 실제로 죽는
    /// 경우는 이 경로가 아니라 event 채널 Disconnected 로 즉시 잡힌다).
    Ping,
    /// `RSS_SAMPLE_INTERVAL` 주기 RSS 샘플링.
    Rss,
    /// `AUTO_RELOAD_POLL_INTERVAL` 주기 auto-reload polling. flag off 면 **등록
    /// 자체를 하지 않는다** — 꺼진 기능이 데드라인에 기여하지 않는다.
    AutoReload,
}

/// IPC 응답을 최종적으로 어디로 회신해야 하는지를 식별. 호스트 외부 caller(CLI/사용자)는
/// `Local`, 다른 plugin이면 `Plugin`.
pub(super) enum FinalCaller {
    Local {
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        original_id: serde_json::Value,
    },
    Plugin {
        caller_plugin_id: String,
        call_id: u64,
    },
}

/// pending host→plugin request의 종류. 응답 수신 시 어떤 후처리를 할지 식별.
pub(super) enum PendingRequestKind {
    SurfaceCreate {
        surface_id: u32,
    },
    SurfaceRestore {
        surface_id: u32,
    },
    /// 단계 G: 단축키 매칭으로 plugin command가 트리거된 경우. 응답은
    /// SurfaceResult 형태로 surface display_name 을 갱신할 수 있다.
    CommandInvoke {
        surface_id: u32,
    },
    /// popup.open IPC 응답 대기. 응답은 [`PopupOpenResult`].
    PopupOpen {
        instance_id: u64,
    },
    /// 그 외 (host.hello / ping / 등) — 응답 무시.
    Other,
    /// Client IPC 요청을 plugin namespace로 forward한 경우. plugin이 응답을 주면
    /// 보관한 response_tx로 client에 회신한다.
    NamespaceInvoke {
        plugin_id: String,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        original_id: serde_json::Value,
    },
    /// 다른 plugin이 보낸 IpcCall이 namespace 메서드인 경우. target plugin이 응답을
    /// 주면 caller plugin에 `ipc.result`로 회신한다.
    PluginToPluginNamespace {
        /// forward 받은 target plugin (응답을 주는 쪽).
        plugin_id: String,
        /// 호출한 plugin (응답을 받을 쪽).
        caller_plugin_id: String,
        /// caller plugin이 ipc.call 시점에 발급한 call_id.
        call_id: u64,
    },
    /// extension의 pre-IPC hook을 dispatch한 뒤 응답 대기. extension이 응답을 주면
    /// (transform이면 payload 교체, filter면 차단 결정) 그 결과로 target plugin에
    /// 실제 ipc.invoke를 보낸다. post-hook이 있으면 함께 전달해 두 번째 phase에 사용.
    ExtensionPreIpcHook {
        target_plugin_id: String,
        extension_plugin_id: String,
        method: String,
        params: serde_json::Value,
        pre_hook_mode: HookMode,
        final_caller: FinalCaller,
        post_hook: Option<IpcHookDecl>,
        /// hook 응답이 도착해야 하는 시각. 지나면 timeout으로 처리.
        deadline: Instant,
    },
    /// extension의 post-IPC hook을 dispatch한 뒤 응답 대기. extension이 응답을 주면
    /// (transform이면 payload 교체, filter면 ignored — post는 차단 무의미)
    /// 그 결과로 caller에 최종 응답.
    ExtensionPostIpcHook {
        extension_plugin_id: String,
        method: String,
        post_hook_mode: HookMode,
        /// target plugin의 응답을 그대로 들고 온 것. Ok면 result, Err면 (msg, code).
        target_outcome: TargetOutcome,
        final_caller: FinalCaller,
        deadline: Instant,
    },
    /// pre-hook 없이 target에 직접 ipc.invoke한 뒤, 매칭 post-hook이 있어
    /// 응답이 오면 post-hook으로 chain해야 하는 경우의 pending.
    NamespaceInvokeWithPostHook {
        target_plugin_id: String,
        method: String,
        extension_plugin_id: String,
        post_hook_decl: IpcHookDecl,
        final_caller: FinalCaller,
    },
    /// debug 빌드 한정 — `debug.extension.invoke_hook`이 보낸 hook 응답을 그대로
    /// caller(local CLI)에 회신.
    #[cfg(debug_assertions)]
    DebugExtensionInvokeHook {
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        original_id: serde_json::Value,
    },
    /// extension의 pre-event hook을 dispatch한 뒤 응답 대기. 응답이 오면
    /// (transform이면 envelope.payload 교체, filter면 fan-out 차단)
    /// event_bus.fan_out으로 진행. post_event가 있으면 함께 둔다.
    ExtensionPreEventHook {
        publisher_plugin_id: String,
        extension_plugin_id: String,
        envelope: tasty_plugin_protocol::EventEnvelope,
        pre_hook_mode: HookMode,
        post_hook: Option<tasty_plugin_manifest::EventHookDecl>,
        deadline: Instant,
    },
    /// extension의 post-event hook을 dispatch한 뒤 응답 대기. event는 이미 fan-out 됐으므로
    /// post 응답은 observe로만 의미가 있다 (transform/filter는 ignore).
    ExtensionPostEventHook {
        extension_plugin_id: String,
        event_key: String,
        deadline: Instant,
    },
}

/// 연속 실패 hook의 backoff 윈도우. 3회 연속 실패 후 60초 동안 hook 우회.
pub(super) const HOOK_FAIL_BACKOFF: Duration = Duration::from_secs(60);
pub(super) const HOOK_FAIL_LIMIT: u8 = 3;

/// (ext_id, method) 단위 hook 실패 추적. consecutive_failures가 HOOK_FAIL_LIMIT에 도달하면
/// backoff_until로 설정해 그 동안 hook을 우회한다.
#[derive(Debug, Clone, Default)]
pub(super) struct HookFailureState {
    pub(super) consecutive_failures: u8,
    pub(super) backoff_until: Option<Instant>,
}

/// target plugin의 ipc.invoke 응답 결과. post-hook 진입 시 보존해 둔다.
pub(super) enum TargetOutcome {
    Ok(serde_json::Value),
    Err { message: String, code: i32 },
}

/// extension hook 응답에서 추출한 결정.
pub(super) enum HookOutcome {
    /// payload를 새 값으로 교체 (transform).
    Modified(serde_json::Value),
    /// 차단 (filter).
    Block,
    /// 그 외 — observe, filter pass, transform no-op, 응답 누락, 파싱 실패.
    Pass,
}

/// `ExtensionHookResult` JSON에서 outcome 추출. 에러/누락은 fail-open(`Pass`).
pub(super) fn parse_hook_result(resp: &PluginResponse) -> HookOutcome {
    if resp.error.is_some() {
        return HookOutcome::Pass;
    }
    let result = match &resp.result {
        Some(v) => v,
        None => return HookOutcome::Pass,
    };
    if let Some(v) = result.get("modified_payload")
        && !v.is_null()
    {
        return HookOutcome::Modified(v.clone());
    }
    if let Some(pass) = result.get("pass").and_then(|p| p.as_bool())
        && !pass
    {
        return HookOutcome::Block;
    }
    HookOutcome::Pass
}

pub(super) struct RemoteSurfaceEntry {
    /// 소유 plugin id — surface 닫힘 시 `surface.destroy` 를 이 plugin 에 보낸다.
    pub(super) plugin_id: String,
    pub(super) handles: SurfaceHandles,
}

/// egui-mesh surface 의 최근 수신 mesh frame 메타 (A1-S3 수신 라우팅 골격).
///
/// plugin 이 [`tasty_plugin_protocol::PluginEvent::PaintFrame`] 를 보낼 때마다
/// `pump` 가 갱신한다. 렌더 prepare(A1-S5)가 `buffer_id` 로 [`PluginManager::plugin_buffer`]
/// 를 lookup → footer Acquire-load → `mesh_wire::decode_paint` 의 출발점으로 읽는다.
/// 본체(mesh 바이트)는 shared buffer 안에 있고, 이 구조체는 메타만 운반한다.
#[derive(Debug, Clone)]
pub struct EguiMeshFrame {
    /// buffer lookup 에 필요한 소유 plugin id.
    pub plugin_id: String,
    /// mesh POD 바이트가 들어있는 shared buffer.
    pub buffer_id: SharedBufferId,
    /// plugin 이 commit 한 footer generation. host 는 마지막 합성 generation 과
    /// 비교해 변하지 않았으면 재합성을 건너뛴다.
    pub generation: u64,
    /// plugin 렌더 코어의 송신 frame 단조 시퀀스(1부터, buffer 재생성과 무관).
    /// 렌더 prepare 가 `frame_seq == last + 1` 로 textures_delta 체인 연속성을 검증한다.
    /// 구버전 plugin 은 0 → 항상 체인 단절로 취급된다.
    pub frame_seq: u64,
    /// 이 frame 의 textures_delta 가 plugin 의 전체 텍스처 상태를 full image 로
    /// 담고 있는가. true 면 체인 연속성과 무관하게 수락하고 텍스처 상태를 리셋한다.
    pub full_textures: bool,
    /// `mesh_wire::encode_paint` 가 실제로 만든 바이트 길이(shared buffer 의
    /// power-of-two capacity 가 아니라). attach mesh mirror 가 네트워크로
    /// 정확한 payload 만 내보내는 데 필요. 0 이면 구버전 plugin — consumer 는 버퍼
    /// capacity 전체를 fallback 으로 쓴다.
    pub byte_len: u32,
}

pub struct PluginManager {
    pub packages: Vec<PluginPackage>,
    /// trust gate 에서 거부된 plugin 들 (서명 미신뢰/검증 실패/권한 변경).
    /// `refresh_packages` 가 `packages` 와 함께 갱신한다. UI "확인 필요" 탭 +
    /// 사이드바 경고 배지가 소비. debug 빌드는 trust gate 우회라 항상 비어 있다.
    pub rejected: Vec<crate::discovery::RejectedPlugin>,
    pub processes: HashMap<String, PluginProcess>,
    pub config: PluginsConfig,
    pub(super) waker: tasty_terminal::waker_factory::SharedWakerFactory,
    pub(super) listener: Option<HostListener>,
    /// 보조 핸들 채널 listener. shared buffer 핸들 전송에 사용. Unix/Windows 양쪽에서
    /// `HandleListener::bind`가 채널을 연다 (Unix=`AF_UNIX` socket, Windows=Named Pipe).
    /// bind 실패 시에만 `None`이 된다.
    pub(super) handle_listener: Option<HandleListener>,
    pub log_dir: PathBuf,
    pub(super) next_request_id: AtomicU64,
    /// plugin 주기 작업 스케줄. `pump(now)` 가 `drain_due` 로 실행하고, 호스트는
    /// [`PluginManager::next_deadline`] 로 자기 대기 계산에 합성한다.
    pub(super) timers: tasty_timer::TimerHub<PluginTick>,
    /// plugin id → 최근 spawn 실패 timestamps. 짧은 시간 내 반복 실패하면 자동 disable.
    pub(super) spawn_failures: HashMap<String, Vec<Instant>>,
    /// 자동 disable되어 사용자가 수동 enable하기 전까지 더 이상 spawn 시도 안 함.
    pub(super) auto_disabled: std::collections::HashSet<String>,
    /// H — auto-reload: plugin id → 마지막 관측한 entry binary mtime.
    pub(super) plugin_binary_mtimes: HashMap<String, SystemTime>,
    /// H — auto-reload: plugin id → 마지막 관측한 manifest version.
    pub(super) plugin_manifest_versions: HashMap<String, String>,
    /// H — auto-reload 활성 여부. `TASTY_PLUGIN_AUTO_RELOAD` env 로 결정.
    /// false 이면 `PluginTick::AutoReload` 이 등록되지 않아 cost 0.
    pub(super) auto_reload_enabled: bool,
    /// hello 받은 plugin의 surface_kinds를 등록하기 위한 registry 핸들. None이면
    /// registry 등록 동작이 비활성 (헤드리스/테스트).
    pub surface_registry: Option<Arc<dyn SurfaceRegistry>>,
    /// 이미 registry에 등록된 plugin id (hello를 여러 번 받아도 1회만 등록).
    pub registered_plugins: std::collections::HashSet<String>,
    /// registry create/restore closure가 새 RemoteSurface 등록을 보내는 채널.
    pub host_cmd_tx: Sender<HostCmd>,
    pub(super) host_cmd_rx: Receiver<HostCmd>,
    /// surface_id → RemoteSurface handle. 라이프사이클 동안 유지.
    pub(super) surfaces: HashMap<u32, RemoteSurfaceEntry>,
    /// host → plugin 요청 ID → 종류. 응답 수신 시 후처리 dispatch용.
    pub(super) pending_requests: HashMap<u64, PendingRequestKind>,
    /// 각 plugin에 grant된 권한. 매니페스트 + plugins.toml의 granted를 교집합한 결과.
    /// `Arc`로 공유하여 CallerContext가 동시 호출 시 안전.
    pub(super) plugin_permissions: HashMap<String, Arc<HashSet<Permission>>>,
    /// plugin이 보낸 IpcCall을 호스트의 main loop에서 라우팅 처리하기 위해 모으는 큐.
    /// `App::process_plugin_ipc_calls()`가 매 tick에 비운다.
    pub(super) pending_plugin_calls: Vec<PendingPluginCall>,
    /// plugin이 매니페스트로 선언한 단축키 command 일람. plugin
    /// enable/disable/install/remove 시 갱신됨.
    pub command_registry: super::command_registry::PluginCommandRegistry,
    /// plugin 이 매니페스트로 선언한 `[[contributes.settings_pages]]` sub-page 일람.
    /// plugin hello/manifest 수신 시 등록되고, disable / 재시작 시 정리된다.
    /// 설정 모달의 sub-tab 합성은 본 registry 를 순회 (Step 5).
    pub settings_pages: crate::settings_registry::SettingsPageRegistry,
    /// plugin이 매니페스트로 선언한 IPC namespace prefix 일람. **설치된 매니페스트에서
    /// 유도되며**(ADR-0173) 실행 여부와 무관하다 — 호스트 IPC dispatcher가 namespace
    /// 메서드를 어느 plugin에 forward할지 해결할 때 조회한다. "누가 그 이름의 주인인가"
    /// 와 "지금 떠 있는가" 는 다른 물음이고, 뒤엣것은 `processes` 가 답한다(안 떠 있으면
    /// `-32002`). 이 표는 `packages` 에서 유도되므로 `packages` 를 바꾸는 자리는
    /// [`PluginManager::refresh_packages`] 를 거쳐야 한다.
    pub ipc_namespaces: IpcNamespaceRegistry,
    /// plugin id → (buffer id → 매핑 영역). 호스트가 `host.shared_buffer.create`로
    /// 발급한 영역의 매핑 유지(=OS region keep-alive)와 dirty 수신 시 lookup용.
    /// plugin process가 종료/재시작되면 해당 plugin 슬롯이 통째로 drop되어
    /// 매핑이 해제된다.
    pub(super) plugin_buffers: HashMap<String, HashMap<SharedBufferId, SharedMemory>>,
    /// 호스트 전체에서 단조 증가하는 shared buffer id. plugin 간 충돌 회피 + 디버그
    /// 추적을 단순화하기 위해 글로벌 카운터로 둔다.
    pub(super) next_buffer_id: AtomicU64,
    /// egui-mesh surface_id → 최근 paint_frame 메타 (A1-S3). plugin 의 `PaintFrame`
    /// 알림마다 갱신되고, 렌더 prepare(A1-S5)가 buffer lookup + 디코드 출발점으로 읽는다.
    /// plugin process 가 종료/재시작되면 해당 plugin 의 엔트리를 정리한다 (stale buffer 참조 방지).
    pub(super) egui_mesh_frames: HashMap<u32, EguiMeshFrame>,
    /// egui-mesh popup instance_id → 최근 paint_frame 메타 (A2). plugin 의
    /// `PopupPaintFrame` 알림마다 갱신되고, 호스트 popup 합성기가 instance_id 로
    /// lookup 한다. popup 이 닫히거나 plugin 이 종료되면 해당 엔트리를 정리한다.
    pub(super) popup_mesh_frames: HashMap<u64, EguiMeshFrame>,
    /// Plugin extension 상태 추적. `[extends]` 블록을 선언한 plugin들의
    /// active/pending/disabled/conflict 상태를 보관한다. PR 4/5에서 event/IPC
    /// hook dispatch 시 `active_extension_for_target`을 조회한다.
    pub extensions: super::extension_registry::ExtensionRegistry,
    /// (ext_id, method) 단위 hook 실패 추적. 3회 연속 실패하면 60초간 backoff.
    pub(super) hook_failures: HashMap<(String, String), HookFailureState>,
    /// Event Bus 1.0 라우터. 호스트 본문과 plugin 간 broadcast 이벤트를 fan-out.
    pub event_bus: super::event_bus::EventBus,
    /// 호스트가 발화하는 envelope의 `meta.trace_id` 카운터.
    pub(super) event_trace_seq: AtomicU64,
    /// 현재 열려 있는 popup 인스턴스. host가 발급한 `instance_id`를 키로 사용.
    pub(super) popup_instances: HashMap<u64, PopupInstance>,
    /// 다음 popup `instance_id`. 1부터 시작해 단조 증가.
    pub(super) next_popup_instance_id: u64,
    /// 현재 열려 있는 banner 인스턴스(A3). host가 발급한 `instance_id`를 키로 사용.
    pub(super) banner_instances: HashMap<u64, BannerInstance>,
    /// 다음 banner `instance_id`. popup 과 별도 카운터, 1부터 단조 증가.
    pub(super) next_banner_instance_id: u64,
    /// egui-mesh banner instance_id → 최근 paint_frame 메타(A3). plugin 의
    /// `BannerPaintFrame` 알림마다 갱신되고, 호스트 banner 합성기가 instance_id 로
    /// lookup 한다. banner 가 닫히거나 plugin 이 종료되면 해당 엔트리를 정리한다.
    pub(super) banner_mesh_frames: HashMap<u64, EguiMeshFrame>,
    /// `SurfaceInvalidated`(단계 06) 로 알려진 surface_id 누적 — idle 상태(입력 무)에서
    /// 파일이 바뀐 egui-mesh surface(markdown 등). `pump()` 가 채우고
    /// `take_invalidated_surfaces` 가 드레인한다.
    pub(super) invalidated_surfaces: Vec<u32>,
    /// `PopupInvalidated`(`docs/dev-guide/egui-mesh-channel.md` "popup 대응") 로
    /// 알려진 popup instance_id 누적 — egui
    /// `viewport_output` self-repaint 요청(스크롤 스무딩 등) 처럼 무입력 상태에서
    /// plugin 이 재-forward 를 요청한 egui-mesh popup(git-viewer/clipboard-viewer 등).
    /// `pump()` 가 채우고 `take_invalidated_popups` 가 드레인한다.
    pub(super) invalidated_popups: Vec<u64>,
    /// sysinfo 측정 핸들 — tick 마다 새로 만들지 않고 재사용(할당 비용 절감).
    pub(super) sys: sysinfo::System,
    /// 이번 sampling tick 에서 모인 (plugin_id, rss_bytes). `pump()` 가 채우고
    /// `take_rss_samples` 가 드레인한다 — `App::about_to_wait` 이 host 가 직접 가진
    /// `CoreState`/`AnomalyDetector` 로 넘겨 검출·영속·알림을 처리한다(본 크레이트는
    /// telemetry anomaly 판정 로직을 모른다, plain data 만 반환).
    pub(super) pending_rss_samples: Vec<(String, u64)>,
    /// 파일 형식 식별 시스템. plugin enable/disable 시 detector 추가/제거.
    /// 호스트 본문이 CoreState 와 같은 Arc 를 공유 (trait object 로 의존성 격리).
    pub file_format: Arc<dyn tasty_plugin_protocol::host_port::FileFormatRegistryPort>,
    /// 파일 핸들러 시스템. plugin enable/disable 시 handler 추가/제거.
    pub file_handler: Arc<dyn tasty_plugin_protocol::host_port::FileHandlerRegistryPort>,
    /// 공유 훅 핸들러 레지스트리(webhook/hook). plugin enable/disable 시
    /// `[[contributes.hook_handler]]` 등록/제거. 호스트가 setter 로 주입하며 None
    /// 이면 skip (headless 부팅 전/test — 훅 핸들러 없이도 코어 동작).
    pub hook_handler: Option<Arc<dyn tasty_plugin_protocol::host_port::HookHandlerRegistryPort>>,
    /// `[[contributes.completion_strategy]]` 등록/제거. 호스트가
    /// setter 로 주입하며 None 이면 skip — hook_handler 와 동일 지위(독립
    /// 레지스트리, 미주입 시 완료 판정 전략 없이도 코어 동작).
    pub completion_strategy:
        Option<Arc<dyn tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort>>,
    /// i18n namespace 등록 trait. None 이면 등록 skip (headless/test).
    pub i18n_registrar: Option<Arc<dyn tasty_plugin_protocol::host_port::I18nNamespaceRegistrar>>,
    /// 플러그인 자식 프로세스 수명을 호스트에 결박하는 크로스 플랫폼 reaper.
    /// Windows 는 Job Object 핸들을 여기 보유해야 tasty 수명과 KILL_ON_JOB_CLOSE
    /// 가 연동된다. spawn 경로가 prepare/adopt 를 호출. 상세 [`crate::reaper`].
    pub(super) plugin_reaper: crate::reaper::PluginReaper,
    /// 진행 중인 종료 대기. `begin_shutdown_all()` 이 채우고 `poll_shutdown_all()`
    /// 이 비운다. `None` 이면 종료 대기 중이 아니다.
    pub(super) shutdown_batch: Option<ShutdownBatch>,
}

/// 호스트가 추적 중인 popup 인스턴스 한 건. plugin process가 죽으면 함께 제거된다.
#[derive(Debug, Clone)]
pub struct PopupInstance {
    pub plugin_id: String,
    pub popup_id: String,
    pub contribute: tasty_plugin_manifest::PopupContribute,
    /// host↔plugin popup 통합 z-order 순번(`next_popup_z_seq`). open 시 발급되고, 콘텐츠
    /// 영역 클릭 시(host 쪽 `touch_popup_instance_z`) 갱신된다.
    pub z_seq: u64,
}

/// 호스트가 추적 중인 banner 인스턴스 한 건(A3). plugin process가 죽으면 함께 제거된다.
///
/// popup 과 달리 초기 tree 가 없다 — egui-mesh 채널로만 콘텐츠를 그린다. `surface_id`
/// 는 banner 가 도킹된 스코프 surface(D1: plugin 이 소유한 surface 로만 host 가 허용).
#[derive(Debug, Clone)]
pub struct BannerInstance {
    pub plugin_id: String,
    pub banner_id: String,
    pub contribute: tasty_plugin_manifest::BannerContribute,
    /// banner 가 도킹된 surface scope 의 host surface id.
    pub surface_id: u32,
}

/// plugin → host IPC 호출 한 건. 라우팅 후 결과를 plugin에 회신해야 함.
#[derive(Debug, Clone)]
pub struct PendingPluginCall {
    pub plugin_id: String,
    pub call_id: u64,
    pub method: String,
    pub params: serde_json::Value,
    pub permissions: Arc<HashSet<Permission>>,
}

/// `[[contributes.popup]]` 한 항목 + 소유 plugin id. 호스트의 popup 라우터가
/// trigger 매칭에 사용하기 위한 평탄 뷰.
#[derive(Debug, Clone)]
pub struct PluginPopupEntry {
    pub plugin_id: String,
    pub contribute: tasty_plugin_manifest::PopupContribute,
}

mod banner;
mod buffer;
mod events;
mod ipc_dispatch;
mod lifecycle;
mod popup;
mod pump;
mod queries;
mod response;

// G.D.c — IpcNamespaceRegistry ↔ tasty-ipc runtime registry mirror 통합 테스트.
#[cfg(test)]
mod tests_namespace_mirror;

// H — plugin 자동 reload (baseline / check_for_updates / auto_reload_one) 테스트.
#[cfg(test)]
mod tests_auto_reload;

// plugin 주기 작업(PluginTick) 스케줄 — pump(now) 시간 주입 검증.
#[cfg(test)]
mod tests_timers;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::PluginProcess;

    fn empty_waker() -> tasty_terminal::waker_factory::SharedWakerFactory {
        // headless 환경에서 PluginManager가 사용하는 waker — 실제 wake는 no-op로 충분.
        Arc::new(tasty_terminal::waker_factory::NoopWakerFactory)
    }

    /// validate_namespace_call의 분기를 직접 검증하기 위한 mgr 초기화.
    /// process는 spawn하지 않고 ipc_namespaces와 plugin_permissions만 직접 채운다.
    fn mgr_with_namespace_owner(owner: &str, prefix: &str) -> PluginManager {
        let mut mgr = PluginManager::new(empty_waker());
        mgr.ipc_namespaces
            .register(owner, prefix)
            .expect("test prefix should be unique");
        mgr
    }

    #[test]
    fn validate_namespace_call_method_not_found() {
        let mgr = PluginManager::new(empty_waker());
        let err = mgr
            .validate_namespace_call("nope.method", None)
            .unwrap_err();
        assert_eq!(err.0, -32601);
    }

    #[test]
    fn validate_namespace_call_local_caller_allowed_when_target_running() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        // process 가짜 entry 삽입 — request 전송은 안 한다, 검증만.
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        let id = mgr
            .validate_namespace_call("codex.spawn", None)
            .expect("local caller should pass");
        assert_eq!(id, "com.example.codex");
    }

    #[test]
    fn validate_namespace_call_target_not_running() {
        let mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        let err = mgr
            .validate_namespace_call("codex.spawn", None)
            .unwrap_err();
        assert_eq!(err.0, -32002);
    }

    #[test]
    fn validate_namespace_call_self_invocation_rejected() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        let err = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.codex"))
            .unwrap_err();
        assert_eq!(err.0, -32001);
        assert!(err.1.contains("its own namespace"));
    }

    #[test]
    fn validate_namespace_call_plugin_caller_without_grant_denied() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        // caller plugin은 ipc.invoke:codex 권한 없음
        mgr.set_plugin_permissions(
            "com.example.helper",
            HashSet::from([Permission::SurfaceRead]),
        );
        let err = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.helper"))
            .unwrap_err();
        assert_eq!(err.0, -32001);
        assert!(err.1.contains("permission_denied"));
        assert!(err.1.contains("ipc.invoke:codex"));
    }

    #[test]
    fn validate_namespace_call_plugin_caller_with_grant_allowed() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        mgr.set_plugin_permissions(
            "com.example.helper",
            HashSet::from([Permission::IpcInvoke("codex".into())]),
        );
        let id = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.helper"))
            .expect("granted caller should pass");
        assert_eq!(id, "com.example.codex");
    }

    /// validate 만 보는 테스트용 stub. PluginProcess는 process.rs의 cfg(test)
    /// 헬퍼를 위임 호출한다.
    fn stub_process() -> PluginProcess {
        PluginProcess::stub_for_test("stub")
    }

    #[cfg(unix)]
    #[test]
    fn create_shared_buffer_rejects_zero_size() {
        let mut mgr = PluginManager::new(empty_waker());
        mgr.processes.insert("com.example.x".into(), stub_process());
        let err = mgr
            .create_shared_buffer_for("com.example.x", 1, 0)
            .unwrap_err();
        assert!(err.contains("size must be > 0"), "got: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn create_shared_buffer_rejects_unknown_plugin() {
        let mut mgr = PluginManager::new(empty_waker());
        let err = mgr
            .create_shared_buffer_for("com.example.ghost", 1, 4096)
            .unwrap_err();
        assert!(err.contains("not running"), "got: {err}");
    }

    fn make_response(result: Option<serde_json::Value>, error: Option<&str>) -> PluginResponse {
        PluginResponse {
            id: 1,
            result,
            error: error.map(String::from),
            error_code: None,
        }
    }

    #[test]
    fn parse_hook_result_pass_on_missing_result() {
        let resp = make_response(None, None);
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Pass));
    }

    #[test]
    fn parse_hook_result_pass_on_error() {
        let resp = make_response(Some(serde_json::json!({"pass": false})), Some("boom"));
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Pass));
    }

    #[test]
    fn parse_hook_result_block_on_pass_false() {
        let resp = make_response(Some(serde_json::json!({"pass": false})), None);
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Block));
    }

    #[test]
    fn parse_hook_result_modified_payload_takes_precedence() {
        let resp = make_response(
            Some(serde_json::json!({"modified_payload": {"x": 1}, "pass": false})),
            None,
        );
        match parse_hook_result(&resp) {
            HookOutcome::Modified(v) => assert_eq!(v, serde_json::json!({"x": 1})),
            other => panic!(
                "expected Modified, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn parse_hook_result_pass_when_modified_null() {
        let resp = make_response(Some(serde_json::json!({"modified_payload": null})), None);
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Pass));
    }

    #[test]
    fn record_hook_failure_triggers_backoff_after_limit() {
        let mut mgr = PluginManager::new(empty_waker());
        let ext = "com.example.ext";
        let method = "codex.spawn";
        assert!(!mgr.is_hook_in_backoff(ext, method));
        for _ in 0..HOOK_FAIL_LIMIT {
            mgr.record_hook_failure(ext, method);
        }
        assert!(mgr.is_hook_in_backoff(ext, method));
    }

    #[test]
    fn record_hook_success_resets_counter() {
        let mut mgr = PluginManager::new(empty_waker());
        let ext = "com.example.ext";
        let method = "codex.spawn";
        mgr.record_hook_failure(ext, method);
        mgr.record_hook_failure(ext, method);
        mgr.record_hook_success(ext, method);
        // 추가로 (HOOK_FAIL_LIMIT - 1)회 실패만으로는 backoff 진입 금지.
        for _ in 0..(HOOK_FAIL_LIMIT - 1) {
            mgr.record_hook_failure(ext, method);
        }
        assert!(!mgr.is_hook_in_backoff(ext, method));
    }

    #[test]
    fn find_active_ipc_hooks_returns_none_when_no_extension() {
        let mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        assert!(
            mgr.find_active_ipc_hooks("com.example.codex", "codex.spawn")
                .is_none()
        );
    }

    #[test]
    fn find_active_event_hooks_returns_none_when_no_extension() {
        let mgr = PluginManager::new(empty_waker());
        assert!(
            mgr.find_active_event_hooks("com.example.foo", "foo.bar")
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_shared_buffer_rejects_when_handle_listener_missing() {
        // stub mgr는 handle_listener를 bind하지 않으므로 즉시 거절되어야 한다.
        let mut mgr = PluginManager::new(empty_waker());
        mgr.processes.insert("com.example.x".into(), stub_process());
        assert!(mgr.handle_listener.is_none());
        let err = mgr
            .create_shared_buffer_for("com.example.x", 1, 4096)
            .unwrap_err();
        assert!(err.contains("handle channel not available"), "got: {err}");
    }
}
