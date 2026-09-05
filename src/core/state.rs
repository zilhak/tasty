use std::collections::HashMap;
use std::sync::Arc;

use crate::core::surface_registry::SurfaceKindRegistry;
use crate::global_hooks::GlobalHookManager;
use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::settings::Settings;
use crate::state::SurfaceMessage;
use tasty_hooks::HookManager;
use tasty_terminal::Waker;

/// ID generator for workspaces, panes, tabs, and surfaces.
///
/// 각 카운터는 `Arc<AtomicU32>` 로, 여러 CoreState 가 같은 ID 공간을 공유한다.
/// multi-window 시 두 engine 이 동일 ID 를 발급하면 IPC routing 이 불정확해지므로
/// 글로벌 유니크가 필요. `Clone` 으로 새 engine 에 같은 Arc 를 넘긴다.
#[derive(Clone)]
pub struct IdGenerator {
    workspace: Arc<std::sync::atomic::AtomicU32>,
    /// Workspace category 카운터. `normal` 은 id 0 을 예약하므로 1 부터 발급.
    category: Arc<std::sync::atomic::AtomicU32>,
    pane: Arc<std::sync::atomic::AtomicU32>,
    tab: Arc<std::sync::atomic::AtomicU32>,
    surface: Arc<std::sync::atomic::AtomicU32>,
    /// headless pty 카운터([`PTY_ID_BASE`](crate::core::pty_registry::PTY_ID_BASE) 부터).
    /// 위 doc 의 "글로벌 유니크" 는 이 둘에도 걸린다 — 라우팅이 pty id 와 observer id 를
    /// **창을 건너** 푸는데(`request_target::Kind::HeadlessPty` · `Kind::Observer`),
    /// 카운터가 engine 마다면 두 창이 같은 id 를 발급하고 그중 하나는 **어떤 요청으로도
    /// 닿을 수 없게 된다**(먼저 찾힌 engine 이 항상 이긴다).
    pty: Arc<std::sync::atomic::AtomicU32>,
    observer: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU32, AtomicU64};
        Self {
            workspace: Arc::new(AtomicU32::new(1)),
            category: Arc::new(AtomicU32::new(1)),
            pane: Arc::new(AtomicU32::new(1)),
            tab: Arc::new(AtomicU32::new(1)),
            surface: Arc::new(AtomicU32::new(1)),
            pty: Arc::new(AtomicU32::new(crate::core::pty_registry::PTY_ID_BASE)),
            observer: Arc::new(AtomicU64::new(1)),
        }
    }

    /// headless pty id 카운터 — `PtyRegistry` 가 이 Arc 를 들고 발급한다.
    pub fn pty_counter(&self) -> Arc<std::sync::atomic::AtomicU32> {
        Arc::clone(&self.pty)
    }

    /// observer id 카운터 — `ObserverRouter` 가 이 Arc 를 들고 발급한다.
    pub fn observer_counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.observer)
    }

    pub fn next_workspace(&self) -> u32 {
        self.workspace
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// 새 workspace category id 발급(1 부터). `normal`(id 0)은 발급 대상 아님.
    pub fn next_category(&self) -> u32 {
        self.category
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// category 카운터를 `min_next` 이상으로 끌어올린다(이미 크면 no-op).
    /// 복원 시 layout.json 의 최대 카테고리 id + 1 위로 floor 를 올려 재사용 차단.
    pub fn bump_category_floor(&self, min_next: u32) {
        self.category
            .fetch_max(min_next, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn next_pane(&self) -> u32 {
        self.pane.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_tab(&self) -> u32 {
        self.tab.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_surface(&self) -> u32 {
        self.surface
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// surface 카운터를 `min_next` 이상으로 끌어올린다(이미 크면 no-op).
    /// 다음 `next_surface()` 가 반환할 값이 `>= min_next` 가 되도록 보장.
    ///
    /// 재시작 시 `surface_id` 는 매 실행 1 부터 재발급되는데 surface_meta
    /// (`memory.db`)는 영속되므로, 복원이 발급하는 id 가 이전 실행의 stale
    /// `Scope::Surface(id)` 와 겹칠 수 있다. 복원 *직전* 에 floor 를 stale 최대
    /// id 위로 올려 재사용을 원천 차단한다.
    pub fn bump_surface_floor(&self, min_next: u32) {
        self.surface
            .fetch_max(min_next, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Helper to extract shell configuration from settings, avoiding boilerplate.
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
    /// 자식 셸에 추가로 심을 환경변수(docs/features/terminal-output/index.md#명령-인덱싱-osc-133
    /// — 예: zsh `ZDOTDIR` 스왑). bash 는
    /// `args`(`--rcfile`) 로 주입하므로 이 필드는 비어 있다.
    pub envs: Vec<(String, String)>,
}

impl ShellConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            shell: settings.general.shell.clone(),
            args: settings.general.effective_shell_args(),
            envs: settings.general.effective_shell_envs(),
        }
    }

    pub fn shell_ref(&self) -> Option<&str> {
        if self.shell.is_empty() {
            None
        } else {
            Some(&self.shell)
        }
    }

    pub fn args_ref(&self) -> Vec<&str> {
        self.args.iter().map(|s| s.as_str()).collect()
    }

    pub fn envs_ref(&self) -> Vec<(&str, &str)> {
        self.envs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

/// Explorer 파일 클립보드 (T11) — 복사/잘라내기한 경로 + cut 여부.
#[derive(Clone, Debug)]
pub struct ExplorerClipboard {
    pub paths: Vec<std::path::PathBuf>,
    /// true = 잘라내기(이동), false = 복사.
    pub cut: bool,
}

/// N-RA02 — 원격 워크스페이스 추가 팝업의 Connect 가 메인 루프로 넘기는 사용자-경로
/// GUI attach 요청. `port`/`workspace` 는 attach 대상, `tunnel` 은 조회에 쓴 SSH 터널을
/// 재사용(loopback 이면 None). `App::dispatch_pending_gui_attach` 가 drain 해
/// `start_gui_attach` 로 mirror 를 띄우고, 성공 시 새 mirror ws 로 focus 를 옮긴다.
/// `Clone`/`Debug` 불가(SshTunnel = 자식 process 핸들) — 큐로 단발 이동한다.
pub(crate) struct GuiAttachUserReq {
    pub(crate) port: u16,
    pub(crate) workspace: u32,
    pub(crate) tunnel: Option<tasty_ssh::SshTunnel>,
}

/// (08) mirror 터미널에 클립보드 이미지를 붙여넣을 때의 원격 업로드 요청. paste 시점에
/// mirror 판정을 끝내 두고(포커스가 업로드 완료 전에 바뀌어도 삽입 대상이 흔들리지
/// 않게), 실제 bulk 업로드(블로킹, ADR-0054)는 `App::poll_image_uploads` 가 백그라운드
/// 스레드에서 수행한다. 완료 시 원격 절대경로를 `surface_id`(=paste 시점 mirror surface)
/// 입력에 삽입한다 — mirror surface 입력은 forwarder 로 원격에 투명 전달된다.
pub(crate) struct PendingImageUpload {
    /// 업로드 대상 로컬 mirror workspace id(attach 세션 `local_workspace`).
    pub(crate) mirror_ws_id: u32,
    /// 원격 경로를 삽입할 로컬 mirror surface id(paste 시점 포커스).
    pub(crate) surface_id: u32,
    /// 삽입 시 bracketed paste 로 감쌀지(paste 시점 터미널 상태).
    pub(crate) bracketed: bool,
    /// 원격 저장 basename(`paste-<ms>.png` 규약).
    pub(crate) file_name: String,
    /// 메모리에서 인코딩한 PNG 바이트.
    pub(crate) png_bytes: Vec<u8>,
}

/// attach mesh mirror(attach-behavior.md "구독 = MeshContext" 참고) client→server
/// `MeshContext` forward 요청 하나의 payload.
/// `StreamControl::MeshContext`의 필드를 그대로 미러(surface_id 는 큐의 키라 여기 없음).
#[derive(Debug, Clone)]
pub(crate) struct AttachMeshContextForward {
    pub(crate) width_px: u32,
    pub(crate) height_px: u32,
    pub(crate) pixels_per_point: f32,
    pub(crate) theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
    pub(crate) focused: bool,
}

/// Engine-level state shared across all windows.
/// Contains all data that is not specific to a single window's UI.
///
/// struct 자체는 `pub` — pub fn 시그니처에 노출되기 때문 (e.g. `AppState::active_workspace`).
/// 다만 모든 필드는 `pub(crate)` 로 좁혀, 외부 (crate dependency 측) 가 내부
/// 도메인 데이터를 직접 두드리는 것을 막는다. Core boundary 강화.
pub struct CoreState {
    // ── Workspace / Terminal management ──
    pub(crate) workspaces: Vec<Workspace>,
    /// Workspace category(사이드바 폴더) 목록. Vec 순서 = 사이드바 섹션 표시 순서.
    /// `categories[0]` 은 항상 예약된 `normal`(id 0, 위치 고정). [`CoreState::ensure_normal_category`]
    /// 가 생성/복원 직후 이 불변식을 보장한다.
    pub(crate) categories: Vec<crate::model::WorkspaceCategory>,
    pub(crate) next_ids: IdGenerator,
    pub(crate) default_cols: usize,
    pub(crate) default_rows: usize,
    pub(crate) waker: Waker,

    // ── Settings ──
    pub(crate) settings: Settings,

    // ── Notifications / Hooks ──
    pub(crate) notifications: NotificationStore,
    pub(crate) hook_manager: HookManager,
    pub(crate) global_hook_manager: GlobalHookManager,

    // ── Closed item history ──
    pub(crate) closed_items: crate::model::ClosedItemStore,

    /// OSC 133 기반 명령 인덱서. PromptBoundary 이벤트가 도달할 때마다 호스트가
    /// 호출해 per-surface 상태를 업데이트하고, D phase 에서 memory 에 record 영속.
    pub(crate) command_index: crate::core::command_index::CommandIndex,

    /// 출력 옵저버 라우터. OutputAppended 이벤트마다 dispatch 호출.
    pub(crate) observer_router: crate::output_observer::ObserverRouter,

    /// 휴먼 핸드오프 — approval 요청/응답 큐 + 대기자 채널.
    pub(crate) approval_store: std::sync::Arc<tasty_approval::ApprovalStore>,

    /// Telemetry 이벤트 시퀀스 — 같은 ms 안에서 event_key 충돌 방지용 단조 증가 카운터.
    pub(crate) telemetry_seq: std::sync::Arc<tasty_telemetry::TelemetrySeq>,

    /// Telemetry 이상 탐지 — 호스트 singleton. in-memory sliding window 만 보관
    /// 검출된 anomaly 레코드는 호스트가 memory store 에 영속.
    pub(crate) anomaly_detector: std::sync::Arc<tasty_telemetry::AnomalyDetector>,

    /// Agent task ID 시퀀스 — 같은 ms 안에서 task_id 충돌 방지용 단조 증가 카운터.
    pub(crate) agent_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,

    /// `agent.task_await` blocking 용 waker hub. set_state 가 종결 전이 시 fire,
    /// task_await 가 등록 후 recv_timeout.
    pub(crate) task_waker_hub: std::sync::Arc<crate::core::agent::task_waker::TaskWakerHub>,

    // ── Messaging / Typing detection ──
    pub(crate) surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    pub(crate) surface_next_message_id: u32,
    pub(crate) last_key_input: HashMap<u32, std::time::Instant>,

    // ── Busy state cache (foreground process != shell). Updated by the Tick::Busy timer.
    // Set membership = busy. Surfaces missing from the set are treated as idle.
    pub(crate) busy_surfaces: std::collections::HashSet<u32>,

    /// Busy state of **mirror** (client-side attach) terminals, pushed by the
    /// remote host as `StreamControl::Activity` — mirror terminals have no local
    /// PTY/foreground process, so `refresh_busy_surfaces` can never populate
    /// `busy_surfaces` for them (see [`busy`](crate::core::state::busy)). Kept in
    /// a **separate** set (not merged into `busy_surfaces`) because
    /// `refresh_busy_surfaces` wholesale-replaces `busy_surfaces` every 1Hz tick
    /// from a fresh local poll, which would silently drop mirror entries on the
    /// very next tick if they lived in the same set. `is_surface_busy`/
    /// `busy_count`/`any_busy` read the union of both. Populated by
    /// `set_mirror_surface_busy` (`app/attach_client.rs` on `MirrorEvent::Activity`),
    /// cleared when the mirror surface/workspace is torn down.
    pub(crate) mirror_busy_surfaces: std::collections::HashSet<u32>,

    /// Server-side dedup cache for `busy_activity_forwards`: last busy value
    /// pushed to an attach client per occupied surface, so a tick only forwards
    /// an `Activity` frame when the value actually flipped. Entries for surfaces
    /// no longer hard-occupied are dropped each call (not merely on detach) so a
    /// later re-attach — possibly by a different client — always gets a fresh
    /// initial push regardless of the surface's last-seen value.
    pub(crate) last_forwarded_busy: std::collections::HashMap<u32, bool>,

    /// Server-side dedup cache for `attention_forwards`: last attention kind
    /// pushed to an attach client per occupied surface (`None` = cleared), so a
    /// tick only forwards a `StreamControl::Attention` frame when the value
    /// actually changed. Same lifecycle rule as `last_forwarded_busy` — entries
    /// for surfaces no longer hard-occupied are dropped each call, so a later
    /// re-attach always gets a fresh baseline push regardless of the surface's
    /// last-seen value.
    pub(crate) last_forwarded_attention:
        std::collections::HashMap<u32, Option<attention::AttentionKind>>,

    // ── Surface attention state. Producer-neutral shared primitive: any producer
    // (toast notification, completion IPC/CLI, OSC 133 command completion, …) may
    // raise it, and it is cleared when the surface gains real render-time focus
    // (gpu.rs). Consumers: surface border, tab title (yellow), workspace count
    // badge. Separate from `notifications` (NotificationStore) — an attention
    // record is not automatically a panel item. Helpers live in
    // `state/attention.rs`.
    pub(crate) attention: attention::AttentionStore,

    // ── Mouse-capture blacklist cache. Updated by the same 1Hz Tick::Busy using
    // the foreground names already resolved for busy detection (no extra
    // process snapshot). Set membership = that surface's foreground process
    // matches `mouse_capture_blacklist`, so its click/drag capture is disabled.
    pub(crate) mouse_capture_disabled_surfaces: std::collections::HashSet<u32>,

    // ── Mouse-capture banner suppression cache. Same 1Hz Tick::Busy, independent
    // axis from `mouse_capture_disabled_surfaces`: set membership = that
    // surface's foreground process matches `mouse_capture_banner_blacklist`, so
    // the "mouse capture active..." hint banner is suppressed while capture
    // itself stays on.
    pub(crate) mouse_capture_banner_suppressed_surfaces: std::collections::HashSet<u32>,

    // ── OSC 133 셸 통합 미설치 안내 배너 판정 상태. `shell_integration_hint.rs`
    // 참조. highlight 연결은 없음(별도 경로 — 상세 `docs/features/surface-highlight/index.md`) —
    // 순수 안내 배너 트리거용.
    /// surface 의 첫 PTY 출력 관측 시각. 이 시각 이후 일정 시간이 지나도록
    /// `PromptBoundary` 를 한 번도 못 받으면 배너 대상 후보가 된다.
    pub(crate) shell_integration_first_output_at:
        std::collections::HashMap<u32, std::time::Instant>,
    /// `PromptBoundary`(OSC 133 A/B/C/D 아무 phase)를 한 번이라도 받은 surface 집합 —
    /// 셸 통합이 설치돼 있다는 확정 증거라 이후 배너 판정에서 영구 제외한다.
    pub(crate) shell_integration_boundary_seen: std::collections::HashSet<u32>,
    /// 셸 통합 미설치 배너를 이미 1회 띄운 surface 집합 (재표시 방지).
    pub(crate) shell_integration_hint_shown: std::collections::HashSet<u32>,

    // ── Foreground process-name cache (surface_id → display name). Updated by
    // the same 1Hz Tick::Busy from the foreground programs it already resolves
    // (no extra process snapshot). The StatusBar reads this every frame instead
    // of re-snapshotting all system processes per frame. Replaced wholesale each
    // tick so names of closed surfaces never linger.
    pub(crate) foreground_names: std::collections::HashMap<u32, String>,

    /// StatusBar git-branch cache — the focused surface's branch, refreshed by the
    /// same 1Hz `Tick::Busy`. Single slot (not a per-surface map) because the
    /// StatusBar only ever shows the focused surface's branch; that also means a
    /// closed surface can never leave a stale entry behind. `gui`-only: headless
    /// never renders the StatusBar, so nothing would read it. Lifetime /
    /// invalidation rules and the "which surface refreshes" decision live in
    /// `core/state/branch.rs`.
    #[cfg(feature = "gui")]
    pub(crate) branch_cache: branch::BranchCache,

    /// Per-surface foreground "incarnation" generation counter — bumped by the
    /// same 1Hz Tick::Busy whenever the resolved foreground name changes (shell↔TUI
    /// or TUI↔TUI). See `CoreState::foreground_generation` accessor
    /// (`core/state/busy.rs`) for how banners use this to auto-close when the TUI
    /// that triggered them is no longer foreground.
    pub(crate) foreground_generation: std::collections::HashMap<u32, u64>,

    /// Surface *cut/move* slot (T9). 사용자가 우클릭 컨텍스트 메뉴에서 "잘라내기"
    /// 한 surface 의 id 를 들고 있다가, 다른 위치에서 "여기로 이동" 하면 그 surface 를
    /// 살아있는 채로 이동(replace)한다. 단일 슬롯·세션 휘발(스냅샷 아님, layout.json
    /// 영속 대상 아님). set/clear 는 사용자 우클릭 조작이라 release 경로에서 직접 갱신
    /// 한다(도메인 mutate 아님).
    pub(crate) pending_move_surface: Option<crate::model::SurfaceId>,

    /// Explorer 파일 클립보드 (T11). 우클릭 "복사"/"잘라내기"한 경로 집합 +
    /// cut 여부를 들고 있다가 "붙여넣기"에서 소비한다. OS 텍스트 클립보드와 별개의
    /// explorer 내부 파일 이동 슬롯 — 단일 슬롯·세션 휘발(layout.json 비영속).
    /// 사용자 우클릭 조작이라 release 경로에서 직접 갱신(도메인 mutate 아님).
    pub(crate) explorer_clipboard: Option<ExplorerClipboard>,

    /// Explorer 즐겨찾기 (T11). 전역(surface 무관)·영속 — 부팅 시
    /// `ExplorerFavorites::load()` 로 `~/.tasty/explorer-favorites.toml` 에서 읽고,
    /// 우클릭 추가/제거 시 메모리 갱신 + `save()` 로 즉시 디스크 반영한다. 사용자
    /// 직접 조작으로만 변경되므로 release 경로에서 직접 갱신(도메인 snapshot 비대상).
    /// (소비자가 전부 gui 어댑터라 headless 빌드에선 필드째 제외.)
    #[cfg(feature = "gui")]
    pub(crate) explorer_favorites: crate::explorer_ui::favorites::ExplorerFavorites,

    /// 포트 스캐너 즐겨찾기. 전역(surface 무관)·영속 — 부팅 시
    /// `PortFavorites::load()` 로 `~/.tasty/port-favorites.toml` 에서 읽고, 추가/제거
    /// 시 메모리 갱신 + `save()` 로 즉시 디스크 반영한다. 사용자 직접 조작으로만
    /// 변경되므로 release 경로에서 직접 갱신(도메인 snapshot 비대상).
    /// (소비자가 전부 gui 어댑터라 headless 빌드에선 필드째 제외.)
    /// 포트 스캐너 팝업(`port_scanner.rs`)의 별 토글 + 상단 즐겨찾기 섹션이 소비한다.
    #[cfg(feature = "gui")]
    pub(crate) port_favorites: crate::adapters::ui::popup::port_scanner_favorites::PortFavorites,

    /// Terminal/PTY 데이터 owner (Surface 트리와 분리). Terminal 인스턴스와
    /// 디스크 scrollback 영속 키(`scrollback_persist_ids`)를 store 가 단독
    /// 소유하며, `crate::model::TerminalSurface` 는 `{ id }` 참조만 갖는다.
    pub(crate) terminals: crate::core::terminal_store::TerminalStore,

    /// 배타적 attach 점유 lock (attach/detach 단계 3). surface_id → 점유 client.
    /// 휘발성 — 직렬화/복원 안 함(decision 2). client_id 는 단계 1 StreamClientId.
    pub(crate) attach: crate::core::attach::OccupancyRegistry,

    /// attach mesh mirror 구독 상태(`docs/dev-guide/attach-behavior.md` "mesh mirror 채널").
    /// surface_id → 최신 geometry/theme/focus + forward 진행 상태. `PluginManager`
    /// 는 `App` 소유라 여기 둘 수 없다 — 이 필드는 순수 상태만, 실제 plugin 구동은
    /// `src/boot/headless_plugins.rs` 가 이 상태를 읽어 수행한다. 휘발성(직렬화 안 함).
    pub(crate) mesh_mirror: crate::core::mesh_mirror::MeshMirrorRegistry,

    /// attach mesh mirror **클라이언트측** 최신 frame 저장소
    /// (`docs/dev-guide/attach-behavior.md` "mesh mirror 채널").
    /// `mesh_mirror`(서버측 구독 상태)와 반대편 — attach client 로 붙어있을 때, TCP 로
    /// 재조립한 원격 mesh 바이트를 `AttachMeshSurface` local id 별로 보관한다. 렌더은
    /// `gfx/gpu/egui_mesh_prepare.rs::render_attach_mesh_surfaces`가 매 frame 읽는다.
    /// 휘발성(직렬화 안 함).
    pub(crate) attach_mesh_frames: crate::core::attach_mesh_frames::AttachMeshFrameStore,

    /// child-terminal registry (ADR-0040 / occupancy-04). 에이전트가 `terminal.spawn`
    /// 으로 만든 자식 터미널 surface 의 parent/index/idle/needs_input 매핑. 부팅 시
    /// `~/.tasty/child-terminals.json` 에서 로드, 등록/제거마다 즉시 save. soft 점유
    /// (`occupy_soft`) 소비자와 짝. session.rs 의 SessionToken / runner_host 의
    /// shell_children 과는 다른 서브시스템이다(파편화 방지 — child_terminal.rs 참조).
    pub(crate) child_terminals: crate::core::child_terminal::ChildTerminalRegistry,

    /// headless PTY registry (`pty.*` primitive — ADR-0050 · features/headless-pty
    /// 참고). 에이전트가 Surface 없이
    /// 백그라운드에서 굴리는 PTY 의 메타데이터 + 진짜 exit-code 를 보관하고, 동시 개수
    /// 상한·idle TTL 로 좀비 누적을 막는다. child_terminals(자식 터미널 surface) 와는
    /// Surface 유무로 갈리는 별도 서브시스템이다(파편화 방지 — pty_registry.rs 참조).
    /// 비영속 — headless PTY 는 호스트와 수명을 같이한다.
    // 소비자: IPC `pty.*` 핸들러(18-b, `handler/pty.rs`). 상태바 카운트는 18-c.
    pub(crate) pty_registry: crate::core::pty_registry::PtyRegistry,

    /// attach/detach 작업 J — 서버측 readonly 뷰의 display-only mirror.
    /// 점유된 surface 마다 detached `Terminal`(grid 표시 전용)을 두고, 3초 `Tick::AttachView`
    /// tick 때 live grid 스냅샷을 feed 한다(plan §2.3). render_pass 가 is_hard_occupied
    /// surface 를 이 mirror 로 렌더해 "내용 보임 + 조작만 차단 + 3초 cadence" readonly
    /// 를 구현한다. live Terminal 은 PTY 소유·입력 차단 전용으로 유지. 휘발성.
    /// headless 는 렌더가 없어 읽지 않는다(gui 한정).
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub(crate) readonly_views: HashMap<u32, tasty_terminal::Terminal>,

    /// attach/detach 작업 J — GUI in-process attach-client 트리거 큐. IPC
    /// `attach.into_gui {port, workspace}` 핸들러가 `(port, workspace)` 를 push 하면
    /// App 이 `about_to_wait` 에서 drain 해 원격 워크스페이스를 mirror 로 재구성한다
    /// (focus 비의존, plan §5). headless 는 GUI 가 없어 drain 되지 않는다.
    pub(crate) pending_gui_attach: Vec<(u16, u32)>,

    /// (03) 스크린샷→클립보드 키바인딩 트리거 큐. `Some(local mirror workspace id)`
    /// 면 트리거 시점에 포커스된 surface 가 원격 mirror workspace 소속이었다는 뜻
    /// (캡처 완료 후 그 mirror 의 attach 세션으로 원격 전송), `None` 이면 로컬(캡처
    /// 후 로컬 클립보드에 직접 기록). mirror 판별은 트리거 시점에 끝내 두고(포커스가
    /// 캡처 완료 전에 바뀌어도 흔들리지 않게), 실제 OS 캡처(블로킹)는
    /// `App::poll_screenshot_captures` 가 백그라운드 스레드에서 수행한다.
    pub(crate) pending_screenshot_captures: Vec<Option<u32>>,

    /// (08) mirror 터미널 이미지 paste → 원격 업로드 트리거 큐. `MainView::paste_to_terminal`
    /// 의 이미지 분기가 focused surface 가 mirror workspace 소속일 때 push 한다. App 이
    /// `about_to_wait`(`poll_image_uploads`)에서 drain 해 백그라운드 스레드로 bulk 업로드를
    /// 수행하고, 완료 시 원격 경로를 그 mirror surface 입력에 삽입한다. mirror client 는
    /// 항상 GUI 라 headless 에서는 채워지지 않는다.
    pub(crate) pending_image_uploads: Vec<PendingImageUpload>,

    /// (03) attach 서버측 — mirror client 가 청크로 보내는 캡처 파일 바이트를
    /// upload_id 단위로 누적한다. `StreamTag::Control` 채널(기존 `StreamControl` enum
    /// 은 그대로 두고, 그 enum 이 인식 못 하는 별도 "event" 값의 raw JSON 을 실어
    /// 보낸다 — 파싱 실패 시 조용히 스킵되는 특성을 그대로 이용) 로 도착. gui/headless
    /// 양쪽 `StreamReady` 처리부가 공유한다(attach 서버는 어느 빌드든 될 수 있음).
    pub(crate) capture_uploads: crate::core::capture_upload::CaptureUploadRegistry,

    /// (06) attach 서버측 — 전용 bulk 연결(ADR-0054)이 나른 파일 청크를
    /// `(client_id, transfer_id)` 단위로 누적한다. 캡처(`capture_uploads`)의 일반화
    /// 병렬 신설이며, begin 에서 파일명·총 크기를 먼저 받고 이후 `Data` 프레임
    /// (`decode_bulk_chunk`)의 청크를 append 한 뒤 commit 에서 저장 확정한다.
    /// gui/headless 양쪽 `StreamReady` 처리부가 공유한다.
    pub(crate) bulk_transfers: crate::core::bulk_transfer::BulkTransferRegistry,

    /// mirror 워크스페이스 구조 변경 forward 큐(2단계). `Core::apply` 가 mirror
    /// 워크스페이스의 구조 op 를 로컬 실행 대신 여기 push 하고(로컬 mutation 없음),
    /// App 이 `about_to_wait` 에서 drain 해 anchor **로컬** surface id 를 원격 id 로
    /// 치환한 뒤 attach stream(`StreamTag::Control`)으로 원격에 forward 한다. 담긴
    /// [`StructuralOp`] 의 anchor 는 아직 **로컬** id(전송 직전 세션 매핑으로 원격
    /// 치환). mirror client 는 항상 GUI 라 headless 에서는 채워지지 않는다.
    ///
    /// 각 원소는 `StructuralOp` 자체 외에 08/09 client-only focus 보정용 태그
    /// (`user_triggered`/`close_focus_candidates`)를 함께 싣는다 —
    /// [`crate::core::PendingStructuralForward`] 참고.
    pub(crate) pending_structural_forward: Vec<crate::core::PendingStructuralForward>,

    /// client-driven mirror geometry(ADR-0045) forward 큐. `Core::resize_all_terminals`
    /// 의 로컬 레이아웃 스윕이 mirror(detached) 터미널의 목표 grid `(cols, rows)` 를
    /// 로컬에 적용하는 대신(로컬 grid 는 server `Resize` echo 로만 갱신 → desync 방지)
    /// 여기에 **로컬 mirror surface id → (cols, rows)** 로 넣는다. HashMap 이라 한
    /// 프레임에 여러 번 스윕돼도 surface 별 최신값만 남아 coalesce 된다. App 이
    /// `about_to_wait`(`dispatch_pending_resize_forwards`, gui)에서 drain 해 로컬 id 를
    /// 세션 매핑으로 원격 id 로 치환한 뒤 `StreamControl::ClientResize` 로 forward 한다.
    /// mirror client 는 항상 GUI 라 headless 에서는 채워지지 않는다.
    pub(crate) pending_resize_forward: std::collections::HashMap<u32, (usize, usize)>,

    /// mirror surface 의 attention **해제 edge** forward 큐. `clear_attention` 이
    /// mirror surface 에서 레코드를 **실제로 제거했을 때만** 여기에 로컬 mirror
    /// surface id 를 넣는다 — 해제 규칙(실-포커스 = 확인, 알림 읽음) 자체는 그대로
    /// 인스턴스 로컬이고, 그 판정 결과만 surface 를 소유한 인스턴스로 전달한다.
    /// 레코드가 없는 상태의 clear 는 전부 no-op 이라 edge 가 없고, 포커스를 유지해도
    /// 프레임이 반복되지 않는다(별도 last-sent 추적·주기 전송 불필요). App 이
    /// `about_to_wait`(`dispatch_pending_attention_clear_forwards`, gui)에서 drain 해
    /// 세션 매핑으로 원격 id 치환 후 `StreamControl::ClientAttentionClear` 로
    /// forward 한다. `pending_mesh_full_resend_forward` 와 동형(mirror client 는 항상
    /// GUI 라 headless 에서는 채워지지 않는다).
    pub(crate) pending_attention_clear_forward: std::collections::HashSet<u32>,

    /// (04) 파일 피커 원격 디렉토리 목록 forward 큐. popup wrapper
    /// (`adapters::ui::popup::file_picker::draw_file_picker`)가 mirror 워크스페이스에서
    /// 디렉토리 조회가 필요할 때 여기 push 하고, App 이 `about_to_wait`
    /// (`dispatch_pending_list_dir_forwards`)에서 drain 해 세션의 attach 채널로
    /// `list_dir_request` 를 전송한다. 응답은 reader thread 가 받아
    /// `MirrorEvent::ListDirResult` 로 별도 이벤트 큐를 통해 되돌아온다(이 큐는
    /// 요청 방향 전용, 응답은 여기 담기지 않음).
    pub(crate) pending_list_dir_forward: Vec<crate::core::PendingListDirForward>,
    /// git-viewer(원격) git 조회 forward 큐. `git_viewer.query` IPC 핸들러
    /// (`adapters::ipc::handler::git_viewer`)가 mirror surface 에서 git 조회가
    /// 필요할 때 여기 push 하고, App 이 `about_to_wait`
    /// (`dispatch_pending_git_query_forwards`)에서 drain 해 세션의 attach 채널로
    /// `git_query_request` 를 전송한다. 응답은 `MirrorEvent::GitQueryResult` 로
    /// 되돌아온다(`pending_list_dir_forward` 와 동형).
    pub(crate) pending_git_query_forward: Vec<crate::core::PendingGitQueryForward>,
    /// attach mesh mirror(attach-behavior.md "MeshFullResendRequest 복구" 참고) full
    /// 재전송 요청 forward 큐. GPU 렌더 prepare
    /// (`render_attach_mesh_surfaces`)가 텍스처 delta 체인 단절을 감지해
    /// `GpuState::take_attach_mesh_full_requests`로 drain된 **로컬** surface_id 를
    /// 여기 담는다. App 이 `about_to_wait`(`dispatch_pending_mesh_full_resend_forwards`,
    /// gui)에서 drain 해 세션 매핑으로 원격 id 치환 후
    /// `StreamControl::MeshFullResendRequest` 로 forward 한다. `pending_resize_forward`
    /// 와 동형(mirror client 는 항상 GUI 라 headless 에서는 채워지지 않는다).
    pub(crate) pending_mesh_full_resend_forward: std::collections::HashSet<u32>,

    /// attach mesh mirror(attach-behavior.md "구독 = MeshContext" 참고) client→server
    /// `MeshContext` forward 큐.
    /// `MainView::forward_attach_mesh_context`(redraw 스윕)가 `AttachMeshSurface`
    /// pane 의 geometry/theme/focus 변경을 감지해 **로컬** surface_id 키로 최신값만
    /// 채운다(HashMap coalesce — `pending_resize_forward`와 동형). App 이
    /// `about_to_wait`(`dispatch_pending_mesh_context_forwards`, gui)에서 drain 해
    /// 세션 매핑으로 원격 id 치환 후 `StreamControl::MeshContext` 로 forward한다.
    pub(crate) pending_mesh_context_forward:
        std::collections::HashMap<u32, AttachMeshContextForward>,

    /// attach mesh mirror(attach-behavior.md "MeshInput 누적" 참고) client→server
    /// `MeshInput` forward 큐. 로컬
    /// surface_id → 그 redraw 사이클에 누적된 입력 배치(`RawInputWire`). App 이
    /// `about_to_wait`(`dispatch_pending_mesh_input_forwards`, gui)에서 drain 해
    /// `StreamControl::MeshInput` 으로 forward한다.
    pub(crate) pending_mesh_input_forward:
        std::collections::HashMap<u32, tasty_plugin_protocol::protocol::RawInputWire>,

    /// N-RA02 — **사용자 입력 경로 전용** GUI attach 트리거 큐. 원격 워크스페이스 추가
    /// 팝업(remote_attach)의 Connect 클릭이 조회에 쓴 터널을 실어 push 한다. 위
    /// `pending_gui_attach`(IPC/에이전트 경로, focus 중립)와 분리된 이유: 이 큐 drain 은
    /// attach 성공 시 새 mirror ws 로 **focus 를 이동**하는데(사용자 확정 동작), 그 focus
    /// 이동은 사용자 입력 경로에서만 허용된다(원칙 1②). release IPC/CLI 는 이 큐에 push
    /// 하지 않는다.
    pub(crate) pending_gui_attach_user: Vec<GuiAttachUserReq>,

    /// Targeted waker creation. winit `EventLoopProxy`를 직접 들지 않고 trait 뒤로
    /// 추상화하여 헤드리스/플러그인 호스트 컨텍스트에서도 동일 인터페이스를 쓴다.
    /// `App`이 CoreState 생성 후 본체에서 `WinitWakerFactory`를 주입한다.
    pub(crate) waker_factory: Option<crate::waker::SharedWakerFactory>,

    // ── CWD polling (round-robin) ──
    // macOS/Linux 전용. Windows에서는 폴링을 돌지 않아 필드 자체가 없음.
    // ── Surface kind registry ──
    /// Surface 종류별 메타·동작 lookup. 단계 03C에서는 빈 레지스트리만 보유한다 —
    /// 03D에서 본체 7종이 등록되며, 단계 05에서 plugin이 추가될 예정.
    pub(crate) surface_registry: Arc<SurfaceKindRegistry>,

    /// Plugin 이 manifest `[[contributes.hook_events]]` 로 선언한 surface hook
    /// 이벤트 키 집계. plugin hello 시 등록, unload/remove 시 제거. `hook.set` /
    /// `surface.fire_hook` 핸들러가 (내장 ∪ 활성 plugin 선언) 검증에 사용한다.
    pub(crate) plugin_hook_events: Arc<crate::core::hook_event_registry::PluginHookEventRegistry>,

    // ── File format / handler registries (file-handler-system) ──
    /// 파일 식별기 — host default + plugin contribute + user config 통합.
    /// `PluginManager` 와 같은 Arc 를 공유한다.
    pub(crate) file_format: Arc<crate::file::format::FileFormatRegistry>,
    /// 파일 핸들러 디스패치 테이블. `PluginManager` 와 같은 Arc 를 공유한다.
    pub(crate) file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    /// 사용자가 picker 에서 직접 고른 handler 의 LRU 기록 (보조 신호).
    /// 부팅 시 디스크에서 로드, 매 선택마다 atomic save.
    pub(crate) file_handler_recent: crate::file::handler::recent::RecentPicks,
    /// 비동기 파일 식별 worker. `App` 이 EventLoopProxy 를 가진 시점에
    /// `create_app_state` 에서 주입한다 — waker_factory 와 동일 패턴.
    /// Phase C 의 mouse.rs 콜사이트가 이걸 호출해 deep identify 를 띄운다.
    #[cfg(feature = "gui")]
    pub(crate) identify_worker: Option<std::sync::Arc<crate::identify_worker::IdentifyWorker>>,

    // ── Layout persistence ──
    pub(crate) layout_dirty: crate::core::layout_persistence::LayoutDirtyTracker,
    /// Active workspace index restored from layout.json. Consumed once by AppState::new().
    pub(crate) restored_active_workspace: Option<usize>,
    /// Deferred terminal surface 의 scrollback 복원 대기 큐. 값은
    /// `scrollback_store::read` 결과(없으면 entry 자체가 생략됨). PTY 가
    /// 실제로 spawn 된 직후 (`ensure_surface_initialized` 또는 즉시 복원
    /// 경로) entry 를 꺼내 `inject_scrollback` 호출.
    pub(crate) pending_scrollback_inject: HashMap<u32, Vec<tasty_terminal::ScrollbackLine>>,
    /// 첫 plugin pump 후 적용할 layout. plugin이 제공하는 surface kind가
    /// 등록되기 전에 복원하면 사라지므로 한 번 미뤄둔다. `App::apply_pending_layout_restore`가 소비.
    pub(crate) pending_layout_restore: Option<crate::core::layout_persistence::SavedLayout>,
    /// 이 engine 이 점유한 레이아웃 슬롯. gui engine 은 항상 `Some`, headless 는
    /// `None`(복원·저장 모두 하지 않는다).
    ///
    /// **휘발성 · 직렬화 대상 아님 · 점유의 단일 진실원.** 슬롯 점유를 디스크나
    /// 별도 레지스트리로 들지 않는다 — 살아있는 engine 들의 이 필드를 모은 집합이
    /// 곧 점유 집합이다(`App::occupied_layout_slots`). `src/core/attach.rs` 의
    /// `OccupancyRegistry` 와 같은 성격이라 재시작 시 전부 free 로 환원된다.
    pub(crate) layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
    /// 점유한 슬롯을 **덮어쓰면 안 되는가.** 부팅 때 그 슬롯을 읽지 못했으면(권한·IO
    /// 오류, 이 빌드가 모르는 미래 version) 사용자의 창 구성이 디스크에 그대로 남아
    /// 있는데 옮길 수조차 없다. 그 위에 지금 상태를 쓰면 원본이 사라지므로
    /// `apply_save_layout_now` 가 저장을 건너뛴다. 읽기에 성공했거나 슬롯이 애초에
    /// 없었으면 `false`(정상 저장).
    pub(crate) layout_slot_protected: bool,
    /// 점유한 슬롯을 **덮어쓰기 전에 옮겨야 하는가.** 부팅 때 파일을 읽었지만 해석하지
    /// 못한 경우다. 원본이 그 자리에 그대로 있으므로, 저장 직전에 `NN.json.bak` 으로
    /// 옮긴 뒤 쓴다(`layout_persistence::save_slot`). 옮기고 나면 다시 `false`.
    pub(crate) layout_slot_unparsable: bool,
    /// 저장이 쓸 layouts 디렉터리 override — **테스트 전용**. 저장 경로 전체를
    /// 사용자의 실제 홈을 건드리지 않고 지나가게 한다 — 디렉터리를 해석하는 두 곳
    /// (`layout_persistence::save_slot` 과 [`CoreState::slot_preservation_is_blocked`])
    /// 이 이 값을 먼저 본다.
    #[cfg(test)]
    pub(crate) layouts_dir_override: Option<std::path::PathBuf>,
    /// 손상 슬롯을 **옮기지 못했는가.** 백업 자리(`.bak` … `.bak.9`)가 다 찼거나
    /// 그 자리를 쓸 수 없으면 참이다. 이때 저장은 계속 거부되므로(원본을 지키는
    /// 쪽이 옳다) 사용자에게 "보관했다" 가 아니라 "치워야 저장이 다시 된다" 를 알린다.
    pub(crate) layout_slot_preserve_failed: bool,

    /// Whether input simulation IPC is enabled (debug builds only, --enable-input-simulation).
    #[cfg(debug_assertions)]
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub(crate) input_simulation_enabled: bool,

    /// Memory port 의 Arc clone — Core 가 owner. 생성자에서 즉시 주입되며
    /// engine 내부 (SurfaceMetaStore, layout persistence, pty surface init 등)
    /// cascade 없이 직접 영속할 때 사용.
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,

    /// agent task runner 스레드 레지스트리의 Arc clone — Core 가 owner. 부팅이 1 회
    /// 주입한다(`set_agent_runner_registry`).
    ///
    /// `memory` 와 같은 이유의 필드다: 렌더 경로(DAG surface 의 러너 배지)는 `Core`
    /// 를 손에 쥐지 않은 채 `CoreState` 만 받으므로, 러너가 살아 있는지/죽었는지를
    /// 물으려면 여기서 같은 인스턴스에 닿아야 한다. 미주입(headless 초기·테스트)
    /// 이면 "러너 없음" 으로 읽힌다 — 조회 전용이라 그 낙하가 안전하다.
    pub(crate) agent_runner_registry:
        std::sync::OnceLock<std::sync::Arc<crate::core::agent::runner_thread::RunnerRegistry>>,
}

impl CoreState {
    /// Create a new CoreState with default settings.
    ///
    /// 테스트 / non-host 진입점용 변형. 내부에서 in-memory `MemoryStore` 를
    /// 생성해 주입한다. host 부팅 경로는 `new_with_ids` 를 사용한다.
    ///
    /// **사용자 홈의 `config.toml` 을 읽지 않는다** — `Settings::default()` 를 주입한다.
    /// 이 생성자를 쓰는 테스트가 실행하는 사람의 로컬 설정에 좌우되면 회귀 감지력이
    /// 사라진다(설정 하나로 무관한 테스트가 깨지고, CI 와 개발자 머신 결과가 갈린다).
    /// 파일 로드 자체의 검증은 `Settings` 쪽 테스트가 담당한다. 규칙·가드 사용법은
    /// `docs/dev-guide/unit-test-isolation.md`, 근거는
    /// `docs/adr/0096-unit-tests-isolated-from-user-environment.md`.
    // 이유: 현재 실제 호출처가 전부 #[cfg(test)] — 과거 engine.rs → core/ 재배치로
    // core 가 pub(crate) 로 캡슐화되며 드러남.
    #[allow(dead_code)]
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory()?,
            ));
        Self::new_with_ids_and_settings(cols, rows, waker, None, None, memory, Settings::default())
    }

    /// 새 CoreState 를 만들 때 기존 ID 공간(Arc<AtomicU32> 들)을 공유받는 변형.
    /// multi-window 시 두 번째 main 의 첫 workspace 가 첫 engine 과 ID 충돌하지
    /// 않게 한다. `shared_ids=None` 이면 새 IdGenerator 로 1부터 시작.
    ///
    /// `layout_slot` 은 이 engine 이 점유할 레이아웃 슬롯. `Some(slot)` 이고
    /// `restore_layout` 이 켜져 있을 때만 그 슬롯 파일을 읽는다. headless 는
    /// `None` — 복원 자체를 적용하지 않으므로 읽지도 않는다.
    pub fn new_with_ids(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    ) -> anyhow::Result<Self> {
        Self::new_with_ids_and_settings(
            cols,
            rows,
            waker,
            shared_ids,
            layout_slot,
            memory,
            Settings::load(),
        )
    }

    /// `new_with_ids` 의 설정 주입 변형 — 설정을 **어디서 얻을지** 를 호출자가 정한다.
    ///
    /// 부팅 경로는 `new_with_ids` 로 `Settings::load()`(사용자 `config.toml`)를 넣고,
    /// 테스트 생성자 `new` 는 `Settings::default()` 를 넣는다. 이 분리가 없으면
    /// 테스트가 사용자 홈 설정을 읽어 로컬 환경에 따라 결과가 달라진다.
    /// 부팅 때 읽은 슬롯의 판정을 engine 상태로 옮긴다.
    ///
    /// 별도 함수인 이유는 **이 배선이 유실 방지의 마지막 고리**이기 때문이다. 판정이
    /// 아무리 정확해도 여기서 플래그를 세우지 않으면 `apply_save_layout_now` 와
    /// `save_slot` 이 보호 장치를 못 보고 사용자 파일을 덮어쓴다. 부팅 전체를 세우지
    /// 않고도 이 고리만 따로 검사할 수 있게 떼어 놓았다.
    pub(crate) fn accept_slot_load(
        &mut self,
        load: crate::core::layout_persistence::SlotLoad,
        slot: crate::core::layout_persistence::LayoutSlotId,
    ) {
        use crate::core::layout_persistence::SlotLoad;
        match load {
            SlotLoad::Loaded(saved) => self.pending_layout_restore = Some(saved),
            // 쓴 적 없는 슬롯 — 호출자의 fallback 이 기본 워크스페이스를 만든다.
            SlotLoad::Absent => {}
            // 읽지 못했다. 기본 워크스페이스로 시작하되, 디스크에 남은 사용자
            // 레이아웃을 이 세션이 덮어쓰지 않도록 슬롯을 잠근다(로그는 로더가 남긴다).
            SlotLoad::Unreadable => self.layout_slot_protected = true,
            // 해석하지 못했다. 기본 워크스페이스로 시작하되, 이 슬롯을 처음 저장할 때
            // 원본을 백업으로 옮기도록 표시한다(로그는 로더가 남긴다).
            SlotLoad::Unparsable => {
                self.layout_slot_unparsable = true;
                // 백업 자리가 이미 다 찼으면 그 첫 저장이 통째로 거부된다. 그 사실은
                // 저장 시점(= `finish_boot` 이후)에야 확정되는데 부팅 알림은 그보다
                // **먼저** 뜨므로, 여기서 예산을 미리 보지 않으면 사용자는 "옆에 .bak
                // 으로 보관합니다" 라는 사실과 **반대인** 안내를 받고 원본을 지울 수 있다.
                self.layout_slot_preserve_failed = self.slot_preservation_is_blocked(slot);
            }
        }
    }

    /// 이 슬롯의 백업 예산이 이미 소진됐는가. `save_slot` 과 **같은 디렉터리 해석**을
    /// 쓴다 — 테스트는 `layouts_dir_override` 로 실제 홈을 건드리지 않고 이 판정까지
    /// 지나간다.
    fn slot_preservation_is_blocked(
        &self,
        slot: crate::core::layout_persistence::LayoutSlotId,
    ) -> bool {
        #[cfg(test)]
        if let Some(dir) = self.layouts_dir_override.as_deref() {
            return crate::core::layout_persistence::slot_preservation_is_blocked_in(dir, slot);
        }
        crate::core::layout_persistence::slot_preservation_is_blocked(slot)
    }

    fn new_with_ids_and_settings(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        settings: Settings,
    ) -> anyhow::Result<Self> {
        let restore_layout = settings.general.restore_layout;

        // Create engine with empty workspaces first; we'll fill them below.
        // 두 registry 가 같은 카운터를 들어야 하므로 먼저 확정한다.
        let next_ids = shared_ids.unwrap_or_default();
        let mut engine = Self {
            workspaces: Vec::new(),
            categories: vec![crate::model::WorkspaceCategory::normal()],
            next_ids: next_ids.clone(),
            default_cols: cols,
            default_rows: rows,
            waker: waker.clone(),
            settings,
            notifications: NotificationStore::with_coalesce_ms(500),
            hook_manager: HookManager::new(),
            global_hook_manager: GlobalHookManager::new(),
            closed_items: crate::model::ClosedItemStore::new(),
            command_index: crate::core::command_index::CommandIndex::new(),
            observer_router: crate::output_observer::ObserverRouter::with_counter(
                next_ids.observer_counter(),
            ),
            approval_store: std::sync::Arc::new(tasty_approval::ApprovalStore::new()),
            telemetry_seq: std::sync::Arc::new(tasty_telemetry::TelemetrySeq::new()),
            anomaly_detector: std::sync::Arc::new(tasty_telemetry::AnomalyDetector::new()),
            agent_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            task_waker_hub: std::sync::Arc::new(crate::core::agent::task_waker::TaskWakerHub::new()),
            surface_messages: HashMap::new(),
            surface_next_message_id: 0,
            last_key_input: HashMap::new(),
            busy_surfaces: std::collections::HashSet::new(),
            mirror_busy_surfaces: std::collections::HashSet::new(),
            last_forwarded_busy: std::collections::HashMap::new(),
            last_forwarded_attention: std::collections::HashMap::new(),
            attention: attention::AttentionStore::default(),
            mouse_capture_disabled_surfaces: std::collections::HashSet::new(),
            mouse_capture_banner_suppressed_surfaces: std::collections::HashSet::new(),
            shell_integration_first_output_at: std::collections::HashMap::new(),
            shell_integration_boundary_seen: std::collections::HashSet::new(),
            shell_integration_hint_shown: std::collections::HashSet::new(),
            foreground_names: std::collections::HashMap::new(),
            foreground_generation: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            branch_cache: branch::BranchCache::default(),
            pending_move_surface: None,
            explorer_clipboard: None,
            #[cfg(feature = "gui")]
            explorer_favorites: crate::explorer_ui::favorites::ExplorerFavorites::load(),
            #[cfg(feature = "gui")]
            port_favorites: crate::adapters::ui::popup::port_scanner_favorites::PortFavorites::load(
            ),
            terminals: crate::core::terminal_store::TerminalStore::new(),
            attach: crate::core::attach::OccupancyRegistry::new(),
            mesh_mirror: crate::core::mesh_mirror::MeshMirrorRegistry::default(),
            attach_mesh_frames: crate::core::attach_mesh_frames::AttachMeshFrameStore::default(),
            child_terminals: crate::core::child_terminal::ChildTerminalRegistry::load(),
            pty_registry: crate::core::pty_registry::PtyRegistry::with_counter(
                next_ids.pty_counter(),
            ),
            readonly_views: HashMap::new(),
            pending_gui_attach: Vec::new(),
            pending_screenshot_captures: Vec::new(),
            pending_image_uploads: Vec::new(),
            capture_uploads: crate::core::capture_upload::CaptureUploadRegistry::new(),
            bulk_transfers: crate::core::bulk_transfer::BulkTransferRegistry::new(),
            pending_structural_forward: Vec::new(),
            pending_resize_forward: std::collections::HashMap::new(),
            pending_list_dir_forward: Vec::new(),
            pending_git_query_forward: Vec::new(),
            pending_mesh_full_resend_forward: std::collections::HashSet::new(),
            pending_attention_clear_forward: std::collections::HashSet::new(),
            pending_mesh_context_forward: std::collections::HashMap::new(),
            pending_mesh_input_forward: std::collections::HashMap::new(),
            pending_gui_attach_user: Vec::new(),
            waker_factory: None,
            surface_registry: {
                let reg = SurfaceKindRegistry::new();
                crate::core::surface_registry::register_builtin_kinds(&reg);
                Arc::new(reg)
            },
            plugin_hook_events: Arc::new(
                crate::core::hook_event_registry::PluginHookEventRegistry::new(),
            ),
            file_format: {
                let reg = crate::file::format::FileFormatRegistry::new();
                reg.install_host_defaults(include_str!(
                    "../file/format/defaults/default-file-format.toml"
                ));
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            file_handler: {
                let reg = crate::file::handler::FileHandlerRegistry::new();
                reg.install_host_defaults(include_str!(
                    "../file/handler/defaults/default-file-handlers.toml"
                ));
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            file_handler_recent: crate::file::handler::recent::RecentPicks::load(
                &file_handler_recent_path(),
            ),
            #[cfg(feature = "gui")]
            identify_worker: None,
            layout_dirty: crate::core::layout_persistence::LayoutDirtyTracker::new(),
            restored_active_workspace: None,
            pending_scrollback_inject: HashMap::new(),
            pending_layout_restore: None,
            layout_slot,
            layout_slot_protected: false,
            layout_slot_unparsable: false,
            #[cfg(test)]
            layouts_dir_override: None,
            layout_slot_preserve_failed: false,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
            memory,
            agent_runner_registry: std::sync::OnceLock::new(),
        };

        // (Phase E) FileHandler 가 detector 메타 (광고 확장자 등) 를 조회할 수 있게
        // FileFormatRegistry 를 DetectorInfo 로 주입. host default 가 이미 로드된 시점.
        engine
            .file_handler
            .attach_detector_info(engine.file_format.clone());

        // Re-apply coalesce_ms from actual settings
        engine.notifications =
            NotificationStore::with_coalesce_ms(engine.settings.notification.coalesce_ms);

        // Try restoring saved layout. plugin이 제공하는 surface kind(예: explorer)는
        // PluginManager가 hello를 처리한 후에야 registry에 등록되므로, 여기서 즉시
        // 복원하면 그런 surface가 사라진다. 따라서 layout 복원은 첫 plugin pump 후로
        // 지연한다 (`App::apply_pending_layout_restore`).
        //
        // scrollback GC 는 여기서 하지 않는다. engine 하나가 읽는 것은 슬롯
        // **하나**뿐이라, 그 슬롯의 ref 집합으로 GC 하면 다른 슬롯이 참조하는
        // `.bin` 을 전부 orphan 으로 판정해 지운다. 전 슬롯 union GC 로 부팅 1 회
        // 옮겼다 (`layout_persistence::migrate_and_gc_on_boot`).
        if restore_layout && let Some(slot) = layout_slot {
            engine.accept_slot_load(crate::core::layout_persistence::load_slot(slot), slot);
        }

        // Fallback: 복원할 layout 이 없을 때만 기본 워크스페이스를 만든다.
        //
        // 복원 예정이면 여기서 아무것도 만들지 않는다. 예전에는 "첫 화면이 비지
        // 않도록" 일단 만들어 두고 복원이 교체하게 했는데, `SavedLayout::restore`
        // 는 `engine.workspaces` 만 통째 교체하고 여기서 spawn 한 PTY 는
        // `TerminalStore` 에 그대로 남는다. `TerminalStore` 에는 워크스페이스가
        // 참조하지 않는 터미널을 회수하는 경로가 없어서 engine 하나당 셸 프로세스
        // 하나가 영구히 누수됐다(창을 열 때마다 하나씩 더).
        //
        // "첫 화면이 빈다" 는 전제도 성립하지 않는다 — `AppState`(창의 view 상태)
        // 는 복원이 **끝난 뒤에** 조립된다(동기 경로 `create_app_state`, 부팅
        // 상태 머신 `finish_boot`). 그 사이 프레임은 부팅 로딩 화면이 그리므로
        // 워크스페이스 0개 상태가 렌더 경로에 노출되는 구간이 없다.
        //
        // 복원이 실패해 워크스페이스가 하나도 안 생기는 경우의 안전망은 복원 적용
        // 지점 양쪽에 있다 (`App::bootstrap_workspace_if_empty`).
        if engine.pending_layout_restore.is_none() {
            let ws_id = engine.next_ids.next_workspace();
            let pane_id = engine.next_ids.next_pane();
            let tab_id = engine.next_ids.next_tab();
            let surface_id = engine.next_ids.next_surface();
            let sh = ShellConfig::from_settings(&engine.settings);
            let terminal = crate::model::Pane::spawn_terminal(
                surface_id,
                crate::model::ShellSpawnOpts {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    extra_env: &sh.envs_ref(),
                    waker,
                    working_dir: None,
                },
            )?;
            engine.terminals.insert(surface_id, terminal);
            let ws = Workspace::new_with_terminal_marker(
                ws_id,
                "Workspace 1".to_string(),
                pane_id,
                tab_id,
                surface_id,
            );
            engine.workspaces = vec![ws];
            engine.send_fast_init(surface_id);
        }

        Ok(engine)
    }

    /// Send fast-mode init command to a terminal by surface ID and apply scrollback limit.
    /// Create a waker for a terminal. If targeted_pty_polling is enabled,
    /// the waker includes the surface_id so only that terminal is processed.
    /// Otherwise, returns the shared waker (all terminals polled).
    pub fn make_waker(&self, surface_id: u32) -> Waker {
        // targeted_pty_polling이 켜져 있고 factory가 주입되어 있으면 surface별 waker 생성.
        // 그 외에는 CoreState 생성 시 받은 base waker(`TerminalOutput(None)`)를 그대로 공유.
        if self.settings.performance.targeted_pty_polling
            && let Some(factory) = &self.waker_factory
        {
            return factory.make_targeted_waker(surface_id);
        }
        self.waker.clone()
    }

    /// Push a closed item, automatically injecting restore commands from surface metadata.
    /// Plugins write the `restore.command` meta key directly (host stays agent-agnostic).
    ///
    /// 반환값은 close 계측(`tasty::close` C2)의 세부 소요다. workspace close 경로만
    /// 이를 로그로 찍고, tab/pane close 는 같은 함수를 타지만 계측 대상이 아니라
    /// 값을 그대로 버린다 — 여기서 직접 로그를 찍으면 탭 하나 닫을 때마다 info 가
    /// 나가 close 계측의 신호 대 잡음비가 무너진다.
    pub fn push_closed_item(
        &mut self,
        mut item: crate::model::ClosedItem,
    ) -> crate::close_trace::PushClosedItemTimings {
        let mut timings = crate::close_trace::PushClosedItemTimings::default();
        let mem = self.memory.clone();
        let t_inject = std::time::Instant::now();
        crate::model::closed_item::inject_restore_commands(&mut item, &|sid| {
            let mut guard = crate::poison::recover_mutex(
                mem.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            crate::surface_meta::SurfaceMetaStore::get(&mut *guard, sid, "restore.command")
        });
        timings.restore_inject = t_inject.elapsed();
        // Persist the captured scrollback to disk so the retained closed item
        // holds only a reference (persist_id), not up to 10k lines per surface.
        // A fresh id is used (not the live surface's layout persist_id, which
        // `cleanup_surface` deletes on close), so the two never collide. Stale
        // closed-item files are reclaimed by `gc_orphans` on the next startup,
        // since closed items do not survive a restart.
        let t_persist = std::time::Instant::now();
        crate::model::closed_item::persist_closed_scrollback(&mut item, &mut |lines| {
            let id = crate::scrollback_store::new_persist_id();
            match crate::scrollback_store::write(&id, lines) {
                Ok(()) => Some(id),
                Err(e) => {
                    tracing::warn!("closed-item scrollback persist failed: {e}");
                    None
                }
            }
        });
        timings.scrollback_persist = t_persist.elapsed();
        // Evicting the oldest item must release its backing scrollback files,
        // otherwise `~/.tasty/scrollback/*.bin` orphans accumulate for the rest
        // of the session.
        let t_evict = std::time::Instant::now();
        if let Some(evicted) = self.closed_items.push(item) {
            let mut refs = Vec::new();
            crate::model::closed_item::collect_scrollback_refs(&evicted, &mut refs);
            for id in refs {
                crate::scrollback_store::delete(&id);
            }
        }
        timings.evict = t_evict.elapsed();
        timings
    }

    /// Record that the user typed on the given surface.
    pub fn record_typing(&mut self, surface_id: u32) {
        self.last_key_input
            .insert(surface_id, std::time::Instant::now());
    }

    /// Re-plumb the current global theme palette into every terminal so OSC
    /// 10/11/12/4 color queries report the new theme. Called on theme change.
    pub fn resync_terminal_palettes(&mut self) {
        self.terminals.resync_palettes();
    }

    /// Returns true if the surface received key input within the last 5 seconds.
    pub fn is_typing(&self, surface_id: u32) -> bool {
        if let Some(last) = self.last_key_input.get(&surface_id) {
            last.elapsed().as_secs_f64() < 5.0
        } else {
            false
        }
    }

    /// 사용자 picker 선택 기록 — 즉시 디스크에 atomic save. 실패 시 warn 로그.
    pub fn record_file_handler_pick(&mut self, id: &crate::file::handler::HandlerId) {
        self.file_handler_recent.record(id);
        let path = file_handler_recent_path();
        if let Err(e) = self.file_handler_recent.save_atomic(&path) {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "file_handler_recent: atomic save failed",
            );
        }
    }
}

impl CoreState {
    /// SurfaceKindRegistry를 통해 새 surface 인스턴스를 만든다.
    /// `"terminal"`은 호출자가 PTY spawn 경로로 분기 처리해야 하므로 여기서는 처리하지 않는다.
    ///
    /// AppState 가 아닌 CoreState 의 메서드 — surface_registry 는 engine 의 일.
    /// D.3.C.B.1 step 1 에서 AppState::create_surface_via_registry 를 옮김.
    ///
    /// `cwd` 는 *carry cwd* — 호출자(intent / preset / convert)가 source surface 의
    /// source_cwd 를 resolve 해 명시 전달한다. surface kind 가 사용 여부를 결정.
    /// Surface cwd invariant — `docs/architecture/invariants/surface-cwd.md` 참조.
    pub(crate) fn create_surface_via_registry(
        &self,
        kind: &str,
        surface_id: u32,
        cwd: Option<&std::path::Path>,
        params: &serde_json::Value,
    ) -> anyhow::Result<Box<dyn crate::model::Surface>> {
        let def = self
            .surface_registry
            .get(kind)
            .ok_or_else(|| anyhow::anyhow!("unknown surface kind: {}", kind))?;
        // kind별 default_params 정책 토큰을 주입한다(예: 새 explorer 는 "마지막으로
        // 고른 view mode"). params 에 없는 키만 채운다(명시 우선). restore 경로는 create
        // 를 거치지 않으므로 per-tab 저장값이 그대로 유지된다.
        //
        // home=None: `@home` 같은 파일시스템 컨텍스트 토큰은 여기서 해석하지 않는다.
        // 이 funnel 은 split/preset/workspace 등 cwd 를 상속·carry 하는 생성 경로가
        // 공유하므로, home 을 강제 주입하면 그 경로들이 회귀한다. `@home` 은 새 탭
        // (`handler/tab.rs::handle_tab_create`)만 fresh-context 로 적용한다.
        if def.default_params.is_empty() {
            return (def.create)(surface_id, cwd, params);
        }
        let mut owned = params.clone();
        if self.apply_kind_default_params(&def, &mut owned, None) {
            (def.create)(surface_id, cwd, &owned)
        } else {
            (def.create)(surface_id, cwd, params)
        }
    }

    /// `def.default_params` 의 기본값을 `params` 에 주입한다(이미 있는 키는 건너뜀 —
    /// 명시 우선). 정책 토큰 해석: `@settings.explorer_view_mode` → Settings 값,
    /// `@home` → `home`(주어질 때만), 그 외 `@`-prefix → unknown(warn+skip), 나머지는
    /// 리터럴. `home` 이 `None` 이면 `@home` 토큰은 건너뛴다. 하나라도 주입하면 `true`.
    pub(crate) fn apply_kind_default_params(
        &self,
        def: &crate::core::surface_registry::SurfaceKindDef,
        params: &mut serde_json::Value,
        home: Option<&std::path::Path>,
    ) -> bool {
        if def.default_params.is_empty() {
            return false;
        }
        let Some(obj) = params.as_object_mut() else {
            return false;
        };
        let mut injected = false;
        for (key, token) in &def.default_params {
            if obj.contains_key(key.as_str()) {
                continue;
            }
            let Some(val) = self.resolve_default_param_token(token, home) else {
                continue;
            };
            obj.insert(key.clone(), serde_json::Value::String(val));
            injected = true;
        }
        injected
    }

    /// default_params 정책 토큰 → 구체 값. 미해석 토큰은 `None`. `docs/dev-guide/
    /// plugin-development.md` 의 default_params 절 참조.
    fn resolve_default_param_token(
        &self,
        token: &str,
        home: Option<&std::path::Path>,
    ) -> Option<String> {
        match token {
            "@settings.explorer_view_mode" => {
                Some(self.settings.general.explorer_view_mode.clone())
            }
            "@home" => home.map(|p| p.to_string_lossy().to_string()),
            t if t.starts_with('@') => {
                tracing::warn!("unknown default_param policy token: {t}");
                None
            }
            literal => Some(literal.to_string()),
        }
    }
}

impl CoreState {
    /// Refresh the cached display name of the tab containing a given surface ID.
    pub fn refresh_tab_display_name(&mut self, surface_id: u32) {
        let workspaces = &mut self.workspaces;
        let terminals = &self.terminals;
        for workspace in workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if tab.contains_surface(surface_id) {
                            let cwd = terminals.get(tab.focused_surface).and_then(|t| t.get_cwd());
                            tab.refresh_display_name(cwd.as_deref());
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Re-project the OSC title of the tab containing `surface_id` from that
    /// tab's *focused* surface only. Mirror of `refresh_tab_display_name` for the
    /// OSC-title path — keeps both "focused-surface projection" policies aligned.
    ///
    /// `surface_id` need only be *some* surface of the tab (not necessarily the
    /// focused one) — the tab is located by membership, then its
    /// `focused_surface`'s current title is read. When the focused surface has no
    /// title (non-terminal, or a terminal that never emitted OSC 0/2), `osc_title`
    /// is cleared so `display_name()` falls back to the cwd-derived name → auto
    /// name. `explicit_name` tabs are left untouched.
    pub fn refresh_tab_osc_title(&mut self, surface_id: u32) {
        let workspaces = &mut self.workspaces;
        let terminals = &self.terminals;
        for workspace in workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if tab.contains_surface(surface_id) {
                            if tab.explicit_name.is_some() {
                                return;
                            }
                            tab.osc_title = terminals
                                .get(tab.focused_surface)
                                .and_then(|t| t.current_title());
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Update stored grid dimensions.
    pub fn update_grid_size(&mut self, cols: usize, rows: usize) {
        self.default_cols = cols;
        self.default_rows = rows;
    }
}

// ── Workspace category 관리 ──
impl CoreState {
    /// normal 불변식 보장 — `categories[0]` 이 항상 예약된 normal 이 되도록 정규화.
    ///
    /// 생성/복원 직후 호출한다. (1) normal 이 없으면 맨 앞에 삽입, (2) 있으나 0번이
    /// 아니면 0번으로 이동, (3) 또한 발급기 floor 를 기존 최대 카테고리 id + 1 이상으로
    /// 올려 재사용을 차단한다. 어떤 워크스페이스가 존재하지 않는 카테고리를 가리키면
    /// normal 로 귀속한다.
    pub fn ensure_normal_category(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        // (1)(2) normal 을 0번에 고정.
        match self.categories.iter().position(|c| c.is_normal()) {
            Some(0) => {}
            Some(idx) => {
                let normal = self.categories.remove(idx);
                self.categories.insert(0, normal);
            }
            None => self.categories.insert(0, WorkspaceCategory::normal()),
        }
        // (3) 발급기 floor 를 최대 사용자 카테고리 id + 1 위로.
        let max_id = self
            .categories
            .iter()
            .map(|c| c.id)
            .filter(|&id| id != NORMAL_CATEGORY_ID)
            .max();
        if let Some(max_id) = max_id {
            self.next_ids.bump_category_floor(max_id + 1);
        }
        // 존재하지 않는 카테고리를 가리키는 워크스페이스는 normal 로 귀속.
        let valid: std::collections::HashSet<u32> = self.categories.iter().map(|c| c.id).collect();
        for ws in &mut self.workspaces {
            if !valid.contains(&ws.category) {
                ws.set_category(NORMAL_CATEGORY_ID);
            }
        }
    }

    /// 카테고리 목록(읽기 전용).
    pub fn categories(&self) -> &[crate::model::WorkspaceCategory] {
        &self.categories
    }

    /// 카테고리 토글 **off** 마이그레이션 (§4-2 on→off). normal 외 모든 카테고리를
    /// 제거하고 그 안의 워크스페이스를 모두 normal 로 귀속한다. **워크스페이스의 물리
    /// 순서(전역 인덱스)는 그대로 두므로** active(전역 인덱스) 도 불변이다 — 사이드바가
    /// off 면 평면이라 순서만 보존되면 충분하다.
    pub fn collapse_categories_to_normal(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        for ws in &mut self.workspaces {
            ws.set_category(NORMAL_CATEGORY_ID);
        }
        self.categories = vec![WorkspaceCategory::normal()];
    }

    /// 새 카테고리 생성. 이름 검증(§3, 대소문자 무시 중복·예약어 거부) 후 새 id 를
    /// 발급해 Vec 끝(normal 뒤)에 push 한다. 성공 시 새 카테고리 id 반환.
    ///
    /// 카테고리 CRUD 는 **사용자 active/포커스에 닿지 않는** 순수 도메인 데이터 변경이라
    /// (원칙 1·3) cascade/active 보정이 필요 없다 — `set_attach_mapping` 직접 set +
    /// mark_layout_dirty 선례와 동형. 호출자가 mark_layout_dirty 를 책임진다.
    pub fn create_category(
        &mut self,
        raw_name: &str,
    ) -> Result<crate::model::WorkspaceCategoryId, crate::model::CategoryNameError> {
        let existing: Vec<&str> = self.categories.iter().map(|c| c.name.as_str()).collect();
        let name = crate::model::validate_new_category_name(raw_name, existing)?;
        let id = self.next_ids.next_category();
        self.categories
            .push(crate::model::WorkspaceCategory::new(id, name));
        Ok(id)
    }

    /// 카테고리 이름 변경. normal 은 거부(`IsNormal`), 이름 검증은 대상 자신을 제외한
    /// 나머지와 비교한다.
    pub fn rename_category(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
        raw_name: &str,
    ) -> Result<(), CategoryOpError> {
        use crate::model::NORMAL_CATEGORY_ID;
        if id == NORMAL_CATEGORY_ID {
            return Err(CategoryOpError::IsNormal);
        }
        if self.category_index(id).is_none() {
            return Err(CategoryOpError::NotFound);
        }
        let existing: Vec<&str> = self
            .categories
            .iter()
            .filter(|c| c.id != id)
            .map(|c| c.name.as_str())
            .collect();
        let name = crate::model::validate_rename_category_name(raw_name, existing)
            .map_err(CategoryOpError::Name)?;
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.name = name;
        }
        Ok(())
    }

    /// 카테고리 삭제. normal 은 거부. 삭제 대상 안의 워크스페이스는 **순서를 보존하며**
    /// normal 로 귀속한다(전역 인덱스 불변 → 사용자 active 영향 없음, 원칙 1·3).
    pub fn delete_category(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
    ) -> Result<(), CategoryOpError> {
        use crate::model::NORMAL_CATEGORY_ID;
        if id == NORMAL_CATEGORY_ID {
            return Err(CategoryOpError::IsNormal);
        }
        let idx = self.category_index(id).ok_or(CategoryOpError::NotFound)?;
        for ws in &mut self.workspaces {
            if ws.category == id {
                ws.set_category(NORMAL_CATEGORY_ID);
            }
        }
        self.categories.remove(idx);
        Ok(())
    }

    /// 카테고리 순서 이동(reorder). **from==0 또는 to==0 거부**(normal 0번 고정).
    /// 범위 밖이거나 from==to 면 no-op(false).
    pub fn reorder_category(
        &mut self,
        from_index: usize,
        to_index: usize,
    ) -> Result<(), CategoryOpError> {
        let len = self.categories.len();
        if from_index == 0 || to_index == 0 {
            return Err(CategoryOpError::NormalFixed);
        }
        if from_index >= len || to_index >= len {
            return Err(CategoryOpError::InvalidIndex);
        }
        if from_index == to_index {
            return Ok(());
        }
        let cat = self.categories.remove(from_index);
        self.categories.insert(to_index, cat);
        Ok(())
    }

    /// 워크스페이스의 카테고리 소속 변경. 대상 카테고리가 존재해야 한다. **사용자
    /// active(전역 인덱스) 불변** — 소속만 바꾼다(원칙 1·3).
    pub fn set_workspace_category(
        &mut self,
        ws_id: crate::model::WorkspaceId,
        cat_id: crate::model::WorkspaceCategoryId,
    ) -> Result<(), CategoryOpError> {
        if self.category_index(cat_id).is_none() {
            return Err(CategoryOpError::NotFound);
        }
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == ws_id)
            .ok_or(CategoryOpError::WorkspaceNotFound)?;
        ws.set_category(cat_id);
        Ok(())
    }

    /// 카테고리 접힘(collapsed) 상태 설정. id 로 카테고리를 찾아 `collapsed` 를
    /// 지정 값으로 둔다. normal 포함 모든 카테고리 접기 허용(디자인상 normal 도 접힘 가능).
    /// 접힘은 사용자 UI 상태지만 layout.json 영속 대상이므로 호출자가 다른 mutator
    /// 관례대로 `mark_layout_dirty` 를 책임진다. 대상이 없으면 no-op.
    pub fn set_category_collapsed(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
        collapsed: bool,
    ) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = collapsed;
        }
    }

    /// 카테고리 접힘 상태를 뒤집는다(호출부 단순화용 편의 메서드). 대상이 없으면 no-op.
    pub fn toggle_category_collapsed(&mut self, id: crate::model::WorkspaceCategoryId) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = !cat.collapsed;
        }
    }

    /// 모든 카테고리(normal 포함)의 접힘 상태를 일괄 토글한다. **하나라도 펼쳐져 있으면
    /// 전부 접고, 전부 접혀 있으면 전부 편다** — "전체 접기/펴기" 단축키용. `set_category_collapsed`
    /// 와 동일하게 접힘은 layout.json 영속 대상이므로 호출자가 `mark_layout_dirty` 를 책임진다.
    /// 카테고리가 normal 하나뿐이어도 그 하나를 토글한다.
    pub fn toggle_all_categories_collapsed(&mut self) {
        let target = self.categories.iter().any(|c| !c.collapsed);
        for cat in &mut self.categories {
            cat.collapsed = target;
        }
    }

    /// 이름(대소문자 무시) 또는 정확 id 로 카테고리를 해석한다. CLI/IPC 가
    /// `--category <name|id>` 를 받을 때 사용. 숫자 문자열은 id 로 우선 해석한다.
    pub fn resolve_category(&self, token: &str) -> Option<crate::model::WorkspaceCategoryId> {
        let t = token.trim();
        if let Ok(id) = t.parse::<u32>()
            && self.categories.iter().any(|c| c.id == id)
        {
            return Some(id);
        }
        self.categories
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(t))
            .map(|c| c.id)
    }

    /// 카테고리 id → 이름. 없으면 None.
    pub fn category_name(&self, id: crate::model::WorkspaceCategoryId) -> Option<&str> {
        self.categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
    }
}

/// 카테고리 변경 연산 에러. IPC 핸들러가 사용자 메시지로 매핑한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryOpError {
    /// 대상 카테고리를 찾을 수 없음.
    NotFound,
    /// normal 은 rename/delete 불가.
    IsNormal,
    /// normal(0번) 위치 고정 위반(reorder from/to == 0).
    NormalFixed,
    /// reorder 인덱스 범위 밖.
    InvalidIndex,
    /// 워크스페이스를 찾을 수 없음(set_workspace_category).
    WorkspaceNotFound,
    /// 이름 검증 실패.
    Name(crate::model::CategoryNameError),
}

impl std::fmt::Display for CategoryOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CategoryOpError::NotFound => f.write_str("category not found"),
            CategoryOpError::IsNormal => {
                f.write_str("the 'normal' category cannot be renamed or deleted")
            }
            CategoryOpError::NormalFixed => {
                f.write_str("the 'normal' category is fixed at position 0")
            }
            CategoryOpError::InvalidIndex => f.write_str("category index out of range"),
            CategoryOpError::WorkspaceNotFound => f.write_str("workspace not found"),
            CategoryOpError::Name(e) => write!(f, "{e}"),
        }
    }
}

/// `~/.tasty/file-handlers.toml` — 사용자 detector/handler 설정. 부팅 시 1회 로드.
fn file_handler_user_config_path() -> Option<std::path::PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("file-handlers.toml"))
}

/// `~/.tasty/file-handler-recent.json` — picker 선택 LRU. 부팅 시 로드, 매 선택마다 save.
/// 홈을 못 찾으면 (CI 등) 임시 경로로 fallback — save 가 안 되더라도 in-memory 동작.
fn file_handler_recent_path() -> std::path::PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handler-recent.json"))
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handler-recent.json"))
}

mod attention;
#[cfg(feature = "gui")]
mod branch;
mod busy;
pub mod child_liveness;
mod finders;
mod global_hooks;
mod idle_hooks;
mod message;
mod pty;
mod shell_integration_hint;
mod soft_occupancy;
mod terminal_finders;

pub(crate) use attention::AttentionKind;
// 유일한 소비자가 gui 전용 port_scanner popup 이라 headless 에서는 unused.
#[cfg(feature = "gui")]
pub use finders::SurfaceDisplayPath;

#[cfg(test)]
mod id_generator_tests {
    use super::IdGenerator;

    #[test]
    fn next_surface_starts_at_one() {
        let ids = IdGenerator::new();
        assert_eq!(ids.next_surface(), 1);
        assert_eq!(ids.next_surface(), 2);
    }

    #[test]
    fn bump_surface_floor_raises_counter() {
        let ids = IdGenerator::new();
        ids.bump_surface_floor(18);
        assert_eq!(
            ids.next_surface(),
            18,
            "floor 이후 첫 id 는 min_next 와 같아야 한다"
        );
        assert_eq!(ids.next_surface(), 19);
    }

    #[test]
    fn bump_surface_floor_is_noop_when_already_higher() {
        let ids = IdGenerator::new();
        // 카운터를 5 까지 소비 (1..=4 발급, 다음은 5).
        for _ in 0..4 {
            ids.next_surface();
        }
        ids.bump_surface_floor(3); // 현재 floor(5)보다 낮음 → 무시.
        assert_eq!(ids.next_surface(), 5);
    }
}

#[cfg(test)]
mod category_tests {
    use super::{CategoryOpError, CoreState};
    use crate::model::{CategoryNameError, NORMAL_CATEGORY_ID};

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn starts_with_normal_only() {
        let e = engine();
        assert_eq!(e.categories().len(), 1);
        assert_eq!(e.categories()[0].id, NORMAL_CATEGORY_ID);
        assert!(e.categories()[0].is_normal());
    }

    #[test]
    fn create_rejects_reserved_and_duplicate() {
        let mut e = engine();
        let id = e.create_category("Work").unwrap();
        assert_ne!(id, NORMAL_CATEGORY_ID);
        assert_eq!(e.categories().len(), 2);
        assert_eq!(
            e.create_category("normal"),
            Err(CategoryNameError::Reserved)
        );
        assert_eq!(e.create_category("WORK"), Err(CategoryNameError::Duplicate));
    }

    #[test]
    fn rename_rejects_normal() {
        let mut e = engine();
        assert_eq!(
            e.rename_category(NORMAL_CATEGORY_ID, "x"),
            Err(CategoryOpError::IsNormal)
        );
    }

    #[test]
    fn delete_moves_workspaces_to_normal() {
        let mut e = engine();
        let cat = e.create_category("Work").unwrap();
        // 기본 워크스페이스를 새 카테고리로 옮긴다.
        let ws_id = e.workspaces[0].id;
        e.set_workspace_category(ws_id, cat).unwrap();
        assert_eq!(e.workspaces[0].category, cat);
        // 삭제 → 워크스페이스는 normal 로 귀속, 카테고리 제거.
        e.delete_category(cat).unwrap();
        assert_eq!(e.workspaces[0].category, NORMAL_CATEGORY_ID);
        assert_eq!(e.categories().len(), 1);
        // normal 삭제는 거부.
        assert_eq!(
            e.delete_category(NORMAL_CATEGORY_ID),
            Err(CategoryOpError::IsNormal)
        );
    }

    #[test]
    fn reorder_protects_normal_position() {
        let mut e = engine();
        e.create_category("a").unwrap();
        e.create_category("b").unwrap();
        // from/to == 0 거부.
        assert_eq!(e.reorder_category(0, 1), Err(CategoryOpError::NormalFixed));
        assert_eq!(e.reorder_category(1, 0), Err(CategoryOpError::NormalFixed));
        // 1↔2 스왑은 허용.
        assert!(e.reorder_category(1, 2).is_ok());
        // normal 은 여전히 0번.
        assert_eq!(e.categories()[0].id, NORMAL_CATEGORY_ID);
    }

    #[test]
    fn resolve_category_by_name_or_id() {
        let mut e = engine();
        let id = e.create_category("Study").unwrap();
        assert_eq!(e.resolve_category("study"), Some(id));
        assert_eq!(e.resolve_category(&id.to_string()), Some(id));
        assert_eq!(e.resolve_category("nope"), None);
    }

    #[test]
    fn set_collapsed_updates_memory() {
        let mut e = engine();
        let id = e.create_category("Services").unwrap();
        // 기본은 펼침.
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.set_category_collapsed(id, true);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.set_category_collapsed(id, false);
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        // 없는 id 는 no-op(패닉 없이 무시).
        e.set_category_collapsed(9999, true);
    }

    #[test]
    fn toggle_collapsed_flips() {
        let mut e = engine();
        let id = e.create_category("Toggle").unwrap();
        e.toggle_category_collapsed(id);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.toggle_category_collapsed(id);
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
    }

    #[test]
    fn toggle_all_collapses_when_any_expanded_then_expands_when_all_collapsed() {
        let mut e = engine();
        let a = e.create_category("A").unwrap();
        let b = e.create_category("B").unwrap();
        // 하나만 접힌 초기 상태 → "하나라도 펼쳐짐" 이므로 전부 접혀야 한다.
        e.set_category_collapsed(a, true);
        e.toggle_all_categories_collapsed();
        assert!(
            e.categories().iter().all(|c| c.collapsed),
            "하나라도 펼쳐져 있으면 전부 접힌다(normal 포함)"
        );
        // 전부 접힌 상태 → 전부 펴져야 한다.
        e.toggle_all_categories_collapsed();
        assert!(
            e.categories().iter().all(|c| !c.collapsed),
            "전부 접혀 있으면 전부 펴진다"
        );
        // b 도 개별 확인(루프 전체 반영 여부).
        assert!(!e.categories().iter().find(|c| c.id == b).unwrap().collapsed);
    }

    #[test]
    fn set_collapsed_allows_normal() {
        // 디자인상 normal 도 접힘 가능.
        let mut e = engine();
        e.set_category_collapsed(NORMAL_CATEGORY_ID, true);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == NORMAL_CATEGORY_ID)
                .unwrap()
                .collapsed
        );
    }

    #[test]
    fn collapsed_survives_layout_round_trip() {
        use crate::core::layout_persistence::SavedLayout;
        // 접힘 상태를 설정한 뒤 layout capture→restore 왕복 후에도 유지되는지 회귀.
        let mut e = engine();
        let id = e.create_category("Services").unwrap();
        e.set_category_collapsed(id, true);
        let saved = SavedLayout::capture(&mut e, 0);

        let mut restored = engine();
        assert!(saved.restore(&mut restored));
        let cat = restored
            .categories()
            .iter()
            .find(|c| c.name == "Services")
            .expect("Services category should be restored");
        assert!(
            cat.collapsed,
            "collapsed 상태가 왕복 후에도 유지되어야 한다"
        );
    }

    #[test]
    fn mirror_workspace_not_persisted() {
        use crate::core::layout_persistence::SavedLayout;
        // N-RA02 회귀: 원격 attach 가 만드는 mirror workspace 는 layout.json 에 저장되면
        // 안 된다(재시작 시 원격 없는 죽은 일반 ws 로 복원되는 버그). capture 가 제외하고
        // active 인덱스도 필터 후 위치로 remap 하는지 확인.
        let mut e = engine();
        let base_count = e.workspaces.len();
        assert!(base_count >= 1, "엔진은 기본 workspace 를 하나 이상 가진다");
        // 원격 attach 가 만드는 mirror workspace 를 흉내 — 새 ws 를 만들고 mirror 플래그를 세운다.
        let idx = match crate::core::apply_create_workspace_inner(
            &mut e,
            crate::core::WorkspaceCreationParams::terminal(),
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => panic!("expected WorkspaceCreated"),
        };
        e.workspaces[idx].mirror = true;
        assert_eq!(e.workspaces.len(), base_count + 1);

        // active 를 mirror 로 둔 채 capture — mirror 는 저장 제외 + active 는 클램프.
        let saved = SavedLayout::capture(&mut e, idx);
        assert_eq!(
            saved.workspaces.len(),
            base_count,
            "mirror workspace 는 저장에서 제외돼야 한다"
        );
        assert!(
            saved.active_workspace < saved.workspaces.len(),
            "remap 된 active 인덱스가 저장 목록 범위 안이어야 한다"
        );

        // 왕복 복원 후에도 mirror 는 되살아나지 않는다.
        let mut restored = engine();
        assert!(saved.restore(&mut restored));
        assert!(
            restored.workspaces.iter().all(|w| !w.mirror),
            "복원본에 mirror workspace 가 없어야 한다"
        );
    }

    #[test]
    fn create_workspace_inner_assigns_category() {
        // 생성 시점 카테고리 소속(옵션 A). 유효 카테고리는 그 소속, dangling 은 normal.
        let mut e = engine();
        let cat = e.create_category("Services").unwrap();
        let idx = match crate::core::apply_create_workspace_inner(
            &mut e,
            crate::core::WorkspaceCreationParams {
                category: Some(cat),
                ..crate::core::WorkspaceCreationParams::terminal()
            },
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => panic!("expected WorkspaceCreated"),
        };
        assert_eq!(e.workspaces[idx].category, cat);

        // 존재하지 않는 카테고리 → normal 유지.
        let idx2 = match crate::core::apply_create_workspace_inner(
            &mut e,
            crate::core::WorkspaceCreationParams {
                category: Some(9999),
                ..crate::core::WorkspaceCreationParams::terminal()
            },
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => unreachable!(),
        };
        assert_eq!(e.workspaces[idx2].category, NORMAL_CATEGORY_ID);
    }
}

#[cfg(test)]
mod default_params_tests {
    use super::CoreState;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// explorer 는 default_params 로 view_mode(@settings)·path(@home)를 선언한다.
    /// home=None(상속 컨텍스트 funnel): view_mode 만 주입되고 @home 은 건너뛴다 →
    /// split/preset/workspace 회귀 방지.
    #[test]
    fn explorer_defaults_without_home_inject_view_mode_only() {
        let e = engine();
        let def = e.surface_registry.get("explorer").unwrap();
        let mut params = serde_json::json!({});
        let injected = e.apply_kind_default_params(&def, &mut params, None);
        assert!(injected);
        assert_eq!(params["view_mode"], e.settings.general.explorer_view_mode);
        assert!(
            params.get("path").is_none(),
            "@home must not resolve when home=None"
        );
    }

    /// home=Some(새 탭 fresh-context): view_mode + path(home) 모두 주입.
    #[test]
    fn explorer_defaults_with_home_inject_path() {
        let e = engine();
        let def = e.surface_registry.get("explorer").unwrap();
        let mut params = serde_json::json!({});
        let home = std::path::PathBuf::from("/home/tester");
        e.apply_kind_default_params(&def, &mut params, Some(&home));
        assert_eq!(params["view_mode"], e.settings.general.explorer_view_mode);
        assert_eq!(params["path"], "/home/tester");
    }

    /// 명시 지정된 키는 보존(주입 안 함).
    #[test]
    fn explicit_params_preserved() {
        let e = engine();
        let def = e.surface_registry.get("explorer").unwrap();
        let home = std::path::PathBuf::from("/home/tester");
        let mut params = serde_json::json!({"view_mode": "list", "path": "/explicit"});
        e.apply_kind_default_params(&def, &mut params, Some(&home));
        assert_eq!(params["view_mode"], "list");
        assert_eq!(params["path"], "/explicit");
    }

    /// default_params 없는 kind(terminal)는 no-op.
    #[test]
    fn kind_without_defaults_is_noop() {
        let e = engine();
        let def = e.surface_registry.get("terminal").unwrap();
        let mut params = serde_json::json!({});
        assert!(!e.apply_kind_default_params(&def, &mut params, None));
    }
}

/// 엔진 생성 실패가 **패닉이 아니라 `Err`** 로 표면화되는지 고정한다.
///
/// 창 생성 경로(`window_lifecycle::create_new_window`)는 이 `Err` 를 받아 창만 취소하고
/// 나머지 창의 세션을 살린다. 여기가 패닉하면 그 위의 graceful 처리가 전부 무의미해지고,
/// 사용자 `config.toml` 의 셸 경로 오타 하나가 실행 중인 모든 세션을 날린다
/// (`docs/adr/0117-window-and-modal-creation-failure-policy.md`).
#[cfg(test)]
mod engine_creation_failure_tests {
    use super::*;

    fn bogus_shell_settings() -> Settings {
        let mut s = Settings::default();
        s.general.shell = "/nonexistent/definitely/not/a/real/shell-xyzzy".to_string();
        // 레이아웃 복원이 켜져 있으면 셸 spawn 경로를 타지 않을 수 있다 — 첫 부팅과
        // 같은 "워크스페이스를 새로 만드는" 경로를 강제한다.
        s.general.restore_layout = false;
        s
    }

    fn in_memory() -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().expect("in-memory store"),
        ))
    }

    #[test]
    fn a_bogus_shell_path_makes_engine_creation_return_err_not_panic() {
        let waker: Waker = std::sync::Arc::new(|| {});
        let result = CoreState::new_with_ids_and_settings(
            80,
            24,
            waker,
            None,
            None,
            in_memory(),
            bogus_shell_settings(),
        );
        let err = result
            .err()
            .expect("a bogus shell must fail engine creation");
        let msg = format!("{err}");
        assert!(
            msg.contains("shell-xyzzy"),
            "the error must name the shell that could not be spawned, got: {msg}"
        );
    }

    #[test]
    fn a_valid_shell_still_produces_an_engine_with_one_workspace() {
        // 위 테스트가 "무조건 Err" 로 통과하지 않는다는 것을 함께 고정한다.
        let waker: Waker = std::sync::Arc::new(|| {});
        let mut ok = Settings::default();
        ok.general.restore_layout = false;
        let engine =
            CoreState::new_with_ids_and_settings(80, 24, waker, None, None, in_memory(), ok)
                .expect("default settings must produce an engine");
        assert_eq!(engine.workspaces.len(), 1);
    }
}
