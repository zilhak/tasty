//! Manifest schema 타입 정의 — 모든 `struct` / `enum` + `impl Permission`.

use std::collections::HashMap;

use serde::Deserialize;

use super::validators::{is_reserved_ipc_prefix, is_valid_ipc_prefix, is_valid_plugin_id};

/// 호스트가 지원하는 plugin protocol 메이저 버전.
/// plugin 매니페스트의 `api_version`과 일치해야 한다.
pub const HOST_API_VERSION: &str = "1";

/// 매니페스트 스키마 버전 (이 파일 형식 자체의 버전).
pub const MANIFEST_VERSION: u32 = 1;

/// hook 항목의 `timeout_ms` 상한. 1초를 넘는 hook은 거부.
pub const HOOK_TIMEOUT_MS_MAX: u32 = 1000;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    pub api_version: String,
    pub entry: Entry,
    #[serde(default)]
    pub surface_kinds: Vec<SurfaceKindDecl>,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Event Bus 구독 허용 패턴 일람. 정확한 키 또는 `<namespace>.*` 와일드카드.
    /// 비어 있으면 plugin은 이벤트를 구독할 수 없다.
    #[serde(default)]
    pub event_subscribe: Vec<String>,
    /// Event Bus 발화 허용 패턴 일람. plugin이 자기 namespace로 publish할 때 필요.
    /// 예약 네임스페이스(`surface.*`, `system.*` 등)는 호스트만 publish 가능 — plugin이
    /// 적어도 매니페스트 검증 단계에서 거부된다.
    #[serde(default)]
    pub event_publish: Vec<String>,
    /// plugin이 publish하는 이벤트 카탈로그. 검토 항목 #6 — 1.0에 포함.
    /// `event_publish` 권한 패턴이 *허용 범위*라면, `events_emitted`는 그 안에서
    /// 실제로 어떤 정확 키를 발화하는지 plugin이 사전 선언하는 *카탈로그*다.
    /// 외부 tool(`tasty plugin show`)이 어떤 이벤트가 나오는지 확인할 수 있게 하고,
    /// extension plugin이 hook 대상으로 참조 가능하게 한다.
    #[serde(default, rename = "events_emitted")]
    pub events_emitted: Vec<EventEmittedDecl>,
    #[serde(default)]
    pub contributes: Contributes,
    /// Extension 선언. 존재 시 이 plugin은 다른 plugin의 IPC/이벤트 흐름을 가로채는
    /// 확장 plugin이 된다. 일반 contribute(`commands`, `surface_kinds`, …)와 공존 가능.
    /// 한 plugin은 정확히 하나의 target만 지정 가능 (1.0 제약).
    #[serde(default)]
    pub extends: Option<ExtendsDecl>,
    /// plugin이 동봉한 lang 파일 디렉터리 (매니페스트 디렉터리 기준 상대).
    /// 기본 `"lang"`. 호스트는 `<plugin_dir>/<lang_dir>/<locale>.toml`을 i18n
    /// registry에 머지한다.
    #[serde(default = "default_lang_dir")]
    pub lang_dir: String,
}

fn default_lang_dir() -> String {
    "lang".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Entry {
    #[serde(rename = "process")]
    Process {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    // WASM entry는 0.7 이후 재검토. 보류 이유는 docs/dev-guide/plugin-ecosystem.md
    // §1 참조 (강제 가능한 sandbox 가치 vs. 1.0 전 보안/도구체인 비용).
}

/// Plugin이 매니페스트에 선언할 수 있는 권한 카테고리.
///
/// 평면 enum — `fs.write`는 `fs.read`를 자동 포함하지 않는다.
/// 매니페스트에 두 권한이 모두 필요하면 명시적으로 선언해야 한다.
///
/// `IpcInvoke(prefix)`는 동적 토큰을 보유하므로 `Copy`를 derive할 수 없다.
/// 정적 enum variant도 함께 `Clone`만 derive하여 일관성을 유지한다.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum Permission {
    /// surface/tab/workspace 트리 조회
    SurfaceRead,
    /// surface 생성/닫기/이동
    SurfaceWrite,
    /// 알림 생성/관리
    Notification,
    /// 클립보드 읽기
    ClipboardRead,
    /// 클립보드 쓰기
    ClipboardWrite,
    /// 호스트 노출 fs 읽기
    FsRead,
    /// 파일 쓰기
    FsWrite,
    /// 외부 프로세스 실행
    ProcessSpawn,
    /// 새 터미널 surface 생성
    TerminalSpawn,
    /// 터미널 입력 송신
    TerminalWrite,
    /// 터미널 출력/scrollback 읽기
    TerminalRead,
    /// 호스트를 통한 네트워크 (예약)
    Network,
    /// 에이전트 메모리(`memory.*`) 읽기
    MemoryRead,
    /// 에이전트 메모리(`memory.*`) 쓰기
    MemoryWrite,
    /// Secret memory(`memory.secret.*`) 접근. plugin 별 사전 분할이라 R/W를 분리하지 않는다.
    MemorySecret,
    /// Approval namespace 접근 (`approval.*`). 휴먼 결정 게이트를 요청·응답·조회.
    /// 토큰 `approval`.
    Approval,
    /// Telemetry namespace 접근 (`telemetry.*`). 메트릭 기록·조회, cap/anomaly 관리.
    /// 토큰 `telemetry`.
    Telemetry,
    /// Agent namespace 접근 (`agent.*`). Task/Barrier/Semaphore/Lease/Reducer/RateLimit
    /// 협업 primitive 사용. 토큰 `agent`.
    AgentManage,
    /// 다른 plugin이 점유한 IPC namespace prefix의 메서드 호출.
    /// 토큰 형식: `ipc.invoke:<prefix>` (예: `ipc.invoke:codex`).
    IpcInvoke(String),
    /// 다른 plugin(target)의 IPC/이벤트 흐름을 가로채는 extension 권한.
    /// 토큰 형식: `ext:<target_plugin_id>` (예: `ext:com.tasty.clipboard`).
    ///
    /// `[extends]` 블록을 선언한 plugin은 매니페스트의 `permissions`에 반드시 이 토큰을
    /// 포함해야 한다. 사용자가 grant하기 전까지 extension은 `Pending(PermissionNotGranted)`
    /// 상태로 유지된다. 세부 mode(transform/filter/observe)와 hook 대상(event/method)은
    /// 매니페스트의 `[[extends.*]]` 항목으로 표현되며 별도 권한 grant는 받지 않는다 —
    /// 사용자 인지 부하를 낮추기 위해 target plugin 단위 단일 토큰만 노출.
    Extension(String),
    /// 사이드바 도구 메뉴에 항목을 contribute할 수 있는 권한. 토큰 `ui.tool_item`.
    /// `[[contributes.tool]]` 항목을 선언한 plugin은 매니페스트의 `permissions`에 반드시
    /// 이 토큰을 포함해야 한다.
    UiToolItem,
    /// Popup contribute 권한. 토큰 `ui.popup`.
    /// `[[contributes.popup]]` 항목을 선언한 plugin은 매니페스트의 `permissions`에
    /// 반드시 이 토큰을 포함해야 한다.
    UiPopup,
    /// Settings page contribute 권한. 토큰 `ui.settings_page`.
    /// `[[contributes.settings_pages]]` 항목을 선언한 plugin 은 매니페스트의
    /// `permissions` 에 반드시 이 토큰을 포함해야 한다.
    UiSettingsPage,
    /// `[[contributes.window]]` 권한. 토큰 `window.spawn`.
    /// Plugin 이 OS-level 별도 윈도우를 contribute 하려면 이 권한이 필요하다.
    /// 1.0 에서는 schema + stub 만 — 실제 spawn handler 는 별도 영역에서 도입.
    WindowSpawn,
    /// 새 detector 정의 권한. 토큰 `file_handler.define`.
    /// `[[contributes.detector]]` 로 **신규** detector id 를 선언할 때 필요.
    /// 기존 id 재선언(rule 추가)은 `FileHandlerExtend` 가 담당.
    FileHandlerDefine,
    /// 기존 detector 재선언(rule 추가) 권한. 토큰 형식: `file_handler.extend:<id>`.
    /// `$unknown` 은 존재하지 않는 detector 라서 token parse 단계에서 reject.
    FileHandlerExtend(String),
    /// 특정 detector 에 handler attach 권한. 토큰 형식: `file_handler.handle:<id>`.
    /// `$unknown` 은 reject. `$directory` 등 실 등록된 reserved id 는 허용.
    FileHandlerHandle(String),
}

impl Permission {
    pub fn from_token(s: &str) -> Option<Self> {
        Some(match s {
            "surface.read" => Self::SurfaceRead,
            "surface.write" => Self::SurfaceWrite,
            "notification" => Self::Notification,
            "clipboard.read" => Self::ClipboardRead,
            "clipboard.write" => Self::ClipboardWrite,
            "fs.read" => Self::FsRead,
            "fs.write" => Self::FsWrite,
            "process.spawn" => Self::ProcessSpawn,
            "terminal.spawn" => Self::TerminalSpawn,
            "terminal.write" => Self::TerminalWrite,
            "terminal.read" => Self::TerminalRead,
            "network" => Self::Network,
            "memory.read" => Self::MemoryRead,
            "memory.write" => Self::MemoryWrite,
            "memory.secret" => Self::MemorySecret,
            "approval" => Self::Approval,
            "telemetry" => Self::Telemetry,
            "agent" => Self::AgentManage,
            "ui.tool_item" => Self::UiToolItem,
            "ui.popup" => Self::UiPopup,
            "ui.settings_page" => Self::UiSettingsPage,
            "window.spawn" => Self::WindowSpawn,
            "file_handler.define" => Self::FileHandlerDefine,
            other => {
                if let Some(prefix) = other.strip_prefix("ipc.invoke:") {
                    if !is_valid_ipc_prefix(prefix) || is_reserved_ipc_prefix(prefix) {
                        return None;
                    }
                    return Some(Self::IpcInvoke(prefix.to_string()));
                }
                if let Some(target) = other.strip_prefix("ext:") {
                    if !is_valid_plugin_id(target) {
                        return None;
                    }
                    return Some(Self::Extension(target.to_string()));
                }
                if let Some(id) = other.strip_prefix("file_handler.extend:") {
                    // F.B.6-2: is_valid_detector_id 의 schema 부분만 inline 검증.
                    // host file 도메인의 추가 검증은 install 시점에 한다.
                    if !is_valid_detector_id_local(id) || id == "$unknown" {
                        return None;
                    }
                    return Some(Self::FileHandlerExtend(id.to_string()));
                }
                if let Some(id) = other.strip_prefix("file_handler.handle:") {
                    if !is_valid_detector_id_local(id) || id == "$unknown" {
                        return None;
                    }
                    return Some(Self::FileHandlerHandle(id.to_string()));
                }
                return None;
            }
        })
    }

    // F.B.6-2 — host 의 `file::format::is_valid_detector_id` 와 동일 규칙을 local
    // 복제. 본 crate 가 호스트 file 도메인 결합 없이 token 파싱을 마치기 위함이며,
    // 두 함수가 어긋나면 install 단계의 file 도메인 검증에서 reject 된다.
    /// 권한의 토큰 문자열 형태. `IpcInvoke`는 prefix를 포함하므로 owned `String`을
    /// 반환한다. 비교/저장에는 `&token`을 그대로 사용하면 된다.
    pub fn as_token(&self) -> String {
        match self {
            Self::SurfaceRead => "surface.read".into(),
            Self::SurfaceWrite => "surface.write".into(),
            Self::Notification => "notification".into(),
            Self::ClipboardRead => "clipboard.read".into(),
            Self::ClipboardWrite => "clipboard.write".into(),
            Self::FsRead => "fs.read".into(),
            Self::FsWrite => "fs.write".into(),
            Self::ProcessSpawn => "process.spawn".into(),
            Self::TerminalSpawn => "terminal.spawn".into(),
            Self::TerminalWrite => "terminal.write".into(),
            Self::TerminalRead => "terminal.read".into(),
            Self::Network => "network".into(),
            Self::MemoryRead => "memory.read".into(),
            Self::MemoryWrite => "memory.write".into(),
            Self::MemorySecret => "memory.secret".into(),
            Self::Approval => "approval".into(),
            Self::Telemetry => "telemetry".into(),
            Self::AgentManage => "agent".into(),
            Self::IpcInvoke(prefix) => format!("ipc.invoke:{prefix}"),
            Self::Extension(target) => format!("ext:{target}"),
            Self::UiToolItem => "ui.tool_item".into(),
            Self::UiPopup => "ui.popup".into(),
            Self::UiSettingsPage => "ui.settings_page".into(),
            Self::WindowSpawn => "window.spawn".into(),
            Self::FileHandlerDefine => "file_handler.define".into(),
            Self::FileHandlerExtend(id) => format!("file_handler.extend:{id}"),
            Self::FileHandlerHandle(id) => format!("file_handler.handle:{id}"),
        }
    }
}

/// plugin이 publish할 이벤트 카탈로그 항목. `events_emitted = [...]`로 선언.
///
/// - `key`: 정확한 이벤트 키 (와일드카드 불가, 예약 네임스페이스 불가).
///   `event_publish` 권한 패턴 안에 포함되어야 한다 (그렇지 않으면 publish 시점에 호스트가 거부).
/// - `description`: 사람용 짧은 설명.
/// - `stability`: 이벤트 안정성 등급. 기본 `stable`. 새 이벤트를 도입할 때 plugin 작성자가
///   `experimental`로 표기해 호환성 약속을 약화시킬 수 있다.
/// - `payload_schema`: 옵션. 페이로드 JSON Schema 파일의 매니페스트 디렉터리 기준 상대 경로.
///   1.0에서는 호스트가 검증에 사용하지 않고 카탈로그용으로만 보유한다.
#[derive(Debug, Clone, Deserialize)]
pub struct EventEmittedDecl {
    pub key: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub stability: EventStability,
    #[serde(default)]
    pub payload_schema: Option<String>,
}

/// `events_emitted` 항목의 안정성 등급. `event-catalog.md`의 정책을 따른다.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventStability {
    #[default]
    Stable,
    Experimental,
}

/// `[[contributes.hook_events]]` 한 항목 — plugin 이 `surface.fire_hook` 로 발사하는
/// surface hook 이벤트 키 카탈로그. `events_emitted`(pub/sub 이벤트 버스)와는 **별개
/// 시스템**이다 — 본 선언은 host 의 `hook.set` / `surface.fire_hook` 검증에만 쓰인다.
///
/// 코어 `HookEvent` 는 claude 등 에이전트 고유 이벤트명을 모른 채 미인식 문자열을
/// `Custom(String)` 으로 수용한다. 그 결과 오타·미존재 이벤트도 조용히 등록될 수
/// 있으므로, plugin 이 자기가 발사하는 키를 선언하고 host 가 (내장 ∪ 활성 plugin 선언)
/// 집합으로 검증해 죽은 hook 등록을 막는다.
///
/// - `key`: 정확한 hook 이벤트 키 (소문자 ascii + 숫자 + `-`, 알파벳으로 시작,
///   와일드카드 불가). 내장 이벤트(`process-exit`/`bell`/`notification`/
///   `output-match:`/`idle-timeout:`)와 충돌 불가.
/// - `description`: 사람용 짧은 설명.
/// - `stability`: 이벤트 안정성 등급. 기본 `stable`.
#[derive(Debug, Clone, Deserialize)]
pub struct HookEventDecl {
    pub key: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub stability: EventStability,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SurfaceKindDecl {
    pub kind: String,
    pub display_name_i18n_key: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// surface 렌더링을 호스트 GUI가 직접 담당하는지(`Host`), plugin이 UiNode tree를
    /// 보내는 일반 remote 방식인지(`Remote`). 호스트가 화이트리스트로 등록한 kind에만
    /// `Host`를 허용한다. 기본 `Remote`.
    #[serde(default)]
    pub rendering: SurfaceKindRendering,
    /// plugin 이 권장하는 surface 기본 색. 사용자 theme TOML 의
    /// `[surfaces.<kind>]` 정의가 있으면 *그쪽이 우선*.
    /// fallback chain: 사용자 TOML > plugin default > FALLBACK_SURFACE.
    #[serde(default)]
    pub default_colors: Option<tasty_type_appearance::theme::PartialSurfaceTheme>,
    /// surface 열기 요청 시 반드시 포함되어야 하는 IPC params 키 목록. 호스트가
    /// surface 생성 요청을 generic 하게 검증할 때 사용한다 — plugin 별로 host
    /// 본체에 박혀 있던 `if kind == "markdown"` 류 직결 분기를 대체.
    /// 예: markdown 의 경우 `["file"]`.
    #[serde(default)]
    pub required_params: Vec<String>,
    /// surface 열기 요청 params 의 key alias 매핑. caller 가 옛 키로 넘기면
    /// host 가 canonical 키로 정규화한다. host 본체의 `kind == "markdown"` 일 때
    /// `file_path` → `file` 정규화 같은 결합을 generic 화 하기 위한 메타.
    /// 예: `{"file_path": "file"}`.
    #[serde(default)]
    pub param_aliases: HashMap<String, String>,
    /// 이 kind 의 surface 가 활성일 때 host 의 egui 입력 라우팅이 plugin/host
    /// 본체 측으로 입력을 흘려야 하는지. host 본체의 `kind == "markdown" || kind == "image"`
    /// 같은 하드코딩 분기를 generic 화 하기 위한 capability 메타. 기본 false.
    #[serde(default)]
    pub consumes_egui_input: bool,
}

/// surface kind의 렌더링 방식. plugin 매니페스트 `rendering = "host" | "remote" | "webview" | "egui-mesh"`.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceKindRendering {
    /// plugin이 UiNode tree로 화면을 그린다 (일반 plugin surface). 기본값.
    #[default]
    Remote,
    /// 호스트 본문이 직접 egui로 그린다. 매니페스트는 등록(메타데이터)만 담당하고
    /// 픽셀 처리는 호스트가 한다. 호스트 화이트리스트 매칭이 필요하다.
    Host,
    /// 호스트가 OS-level native WebView overlay 로 surface 영역을 자동 관리한다.
    /// plugin 은 `webview.set_url(surface_id, url)` 등 IPC 로 URL/navigation 만 제어.
    /// host 는 어떤 컨텐츠 (html/svg/...) 인지 모름 — webview 토대만 제공.
    Webview,
    /// plugin 이 자기 프로세스에서 egui 를 tessellate 한 mesh 를 host 가 합성한다
    /// (ADR-0028). bundled 전용 화이트리스트 + api_version 게이트로 제한된다.
    ///
    /// 와이어 키는 하이픈 포함 `"egui-mesh"` — `rename_all = "lowercase"` 가 만드는
    /// `"eguimesh"` 를 variant 단위 rename 으로 덮어쓴다. 이 rename 을 빠뜨리면
    /// 매니페스트 `rendering = "egui-mesh"` 가 파싱되지 않는다.
    #[serde(rename = "egui-mesh")]
    EguiMesh,
}

/// Plugin extension 선언. 대상(plugin_id + version_req) 하나에 대해
/// pre/post IPC/event hook을 걸 수 있다.
///
/// - `plugin_id`: 확장 대상 plugin id (정확 일치)
/// - `version_req`: 대상 버전 범위 (semver). 벗어나면 extension은 pending.
/// - `api_version`: extension 자체가 따르는 호스트 protocol 버전. `HOST_API_VERSION`과 같아야.
/// - `pre_event` / `post_event`: 대상이 publisher인 envelope의 fan-out 전/후 hook
/// - `pre_ipc` / `post_ipc`: 대상이 caller 또는 callee인 IPC 호출의 invoke 전/응답 후 hook
#[derive(Debug, Clone, Deserialize)]
pub struct ExtendsDecl {
    pub plugin_id: String,
    pub version_req: String,
    pub api_version: String,
    #[serde(default)]
    pub pre_event: Vec<EventHookDecl>,
    #[serde(default)]
    pub post_event: Vec<EventHookDecl>,
    #[serde(default)]
    pub pre_ipc: Vec<IpcHookDecl>,
    #[serde(default)]
    pub post_ipc: Vec<IpcHookDecl>,
}

/// Event hook 한 항목. `[[extends.pre_event]]` 또는 `[[extends.post_event]]`.
#[derive(Debug, Clone, Deserialize)]
pub struct EventHookDecl {
    /// 정확한 이벤트 키. 와일드카드 불가. 대상 plugin의 `events_emitted`에 선언된 키여야
    /// 한다 (실 매칭은 ExtensionRegistry 활성화 시점에 검증).
    pub event: String,
    /// transform 모드에서 변경하려는 payload 경로 일람. observe/filter는 빈 배열 가능.
    #[serde(default)]
    pub modifies: Vec<String>,
    pub mode: HookMode,
    pub timeout_ms: u32,
}

/// IPC hook 한 항목. `[[extends.pre_ipc]]` 또는 `[[extends.post_ipc]]`.
#[derive(Debug, Clone, Deserialize)]
pub struct IpcHookDecl {
    /// 정확한 IPC 메서드 이름 (예: `clipboard.add`). 대상 plugin의 IPC namespace prefix에
    /// 속해야 한다 (실 매칭은 ExtensionRegistry 활성화 시점에 검증).
    pub method: String,
    #[serde(default)]
    pub modifies: Vec<String>,
    pub mode: HookMode,
    pub timeout_ms: u32,
}

/// Hook의 동작 모드.
///
/// - `Transform`: payload를 변경할 수 있다 (반환값으로 덮어쓰기). 가장 강력.
/// - `Filter`: `pass: bool`만 반환. 차단 가능하지만 payload 변경 불가.
/// - `Observe`: 결과는 호스트가 무시. 단순 관찰/로깅. timeout/실패도 체인에 영향 없음.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HookMode {
    Transform,
    Filter,
    Observe,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub menu_items: Vec<MenuItemDecl>,
    #[serde(default)]
    pub ipc_namespace: Vec<IpcNamespaceDecl>,
    #[serde(default)]
    pub cli: Vec<CliCommandDecl>,
    /// 사이드바 "도구" 팝업에 표시될 항목. 클릭 시 plugin이 정의한 동작(event publish,
    /// surface 열기, popup 열기) 발생. 항목당 `[[contributes.tool]]` 한 블록.
    #[serde(default)]
    pub tool: Vec<ToolContribute>,
    /// Plugin 이 contribute 하는 OS-level 윈도우 정의. 본 schema 는 1.0 에서
    /// *등록 stub 만 동작* — 실제 spawn handler 는 별도 영역. validator + 권한
    /// 검사 + hello 시 `CoreEvent::PluginWindowDeclared` 발화까지 수행한다.
    #[serde(default)]
    pub window: Vec<WindowContribute>,
    /// Plugin이 띄울 수 있는 popup 정의. trigger 종류에 따라 자동으로 열리거나
    /// 명시적인 IPC 호출로 열린다.
    #[serde(default)]
    pub popup: Vec<PopupContribute>,
    /// 새 detector 정의 또는 기존 detector 재선언(rule 추가 + 메타 patch).
    /// 같은 id 가 host/다른 plugin/user 에 이미 있으면 rule union, 메타는
    /// last-writer-wins (install 순서 host → plugin → user).
    ///
    /// **Opaque payload** (F.B.2): manifest crate 가 본 바이너리 `file::format::config`
    /// 결합 없이 분리 가능하도록 raw JSON Value 로 보관. concrete `DetectorDecl`
    /// 변환/검증은 본 바이너리 `plugin_bridge::manifest_validate` 또는 host
    /// FileFormatRegistryPort impl 에서 수행.
    #[serde(default)]
    pub detector: Vec<serde_json::Value>,
    /// 파일 핸들러 contribute. plugin 은 `OpenSurface` / `Ipc` 만 사용 가능.
    ///
    /// **Opaque payload** (F.B.2) — detector 와 동일 사유.
    #[serde(default)]
    pub handler: Vec<serde_json::Value>,
    /// Plugin 이 설정 모달에 노출할 sub-page 정의. 항목당 `[[contributes.settings_pages]]`
    /// 한 블록. host 가 plugin registry 를 순회해 동적으로 sub-tab 을 그린다.
    /// 1 차 schema 는 `FontOverride` 항목만 지원 (Color/Bool/Enum 등은 후속 확장).
    #[serde(default)]
    pub settings_pages: Vec<SettingsPageContribute>,
    /// Plugin 이 `surface.fire_hook` 로 발사하는 surface hook 이벤트 키 카탈로그.
    /// host 가 `hook.set` / `surface.fire_hook` 검증을 (내장 ∪ 활성 plugin 선언)
    /// 집합으로 수행하는 데 쓴다. 항목당 `[[contributes.hook_events]]` 한 블록.
    #[serde(default)]
    pub hook_events: Vec<HookEventDecl>,
}

/// Plugin 이 contribute 하는 설정 모달 sub-page 정의 (`[[contributes.settings_pages]]`).
///
/// host 가 설정 UI 의 카테고리별 sub-tab 영역에 plugin registry 를 순회해
/// 동적으로 추가한다. plugin 비활성 시 자동으로 사라지므로 dead-setting 이
/// 노출되지 않는다.
///
/// - `id`: plugin 내 고유 (소문자/숫자 + `_` + `-`). 호스트는 `<plugin_id>/<id>`
///   로 전역 식별.
/// - `title_key`: sub-tab 라벨 i18n 키.
/// - `category`: 호스트가 받아들이는 설정 카테고리 (예: `appearance`).
/// - `items`: 이 page 에서 host 가 generic 렌더할 항목 목록.
#[derive(Debug, Clone, Deserialize)]
pub struct SettingsPageContribute {
    pub id: String,
    pub title_key: String,
    pub category: SettingsCategory,
    #[serde(default)]
    pub items: Vec<SettingsItemDecl>,
}

/// 설정 모달의 상위 카테고리. host 가 받아들이는 값만 enumerate.
/// 알 수 없는 카테고리는 `Other(name)` 로 보존 (host 측에서 무시 또는 fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsCategory {
    Appearance,
    General,
    Keybindings,
    Plugin,
    Other(String),
}

impl<'de> Deserialize<'de> for SettingsCategory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "appearance" => SettingsCategory::Appearance,
            "general" => SettingsCategory::General,
            "keybindings" => SettingsCategory::Keybindings,
            "plugin" => SettingsCategory::Plugin,
            _ => SettingsCategory::Other(s),
        })
    }
}

/// `[[contributes.settings_pages.items]]` 의 한 항목.
///
/// `#[serde(tag = "kind", rename_all = "snake_case")]` 로 직렬화한다(toml `kind = "..."`).
/// 1 차는 `FontOverride` 만 지원했고, 16-B 에서 generic 컨트롤(`Toggle`/`Select`/`Number`)
/// 이 추가됐다. 모든 variant 는 공통으로 `id`(page 내 식별자) · `label_key`(라벨 i18n 키) ·
/// `storage_key`(host 측 저장 슬롯 키) 를 갖는다.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingsItemDecl {
    /// 폰트 override 항목. host 는 `plugin_font_overrides.<storage_key>` 슬롯에
    /// FontOverride 값을 read/write 하는 generic UI 를 그린다.
    ///
    /// - `id`: page 내 항목 식별자 (소문자/숫자 + `_` + `-`).
    /// - `label_key`: 항목 라벨 i18n 키.
    /// - `storage_key`: host 측 settings 의 `plugin_font_overrides.<storage_key>` slot.
    FontOverride {
        id: String,
        label_key: String,
        storage_key: String,
    },

    /// on/off 토글. host 는 디자인 Switch 로 그리고 `plugin_settings.<plugin_id>.<storage_key>`
    /// 에 bool 로 저장한다. `default` 는 키 부재 시 적용할 초기값.
    Toggle {
        id: String,
        label_key: String,
        storage_key: String,
        #[serde(default)]
        default: bool,
    },

    /// 선택지(드롭다운). host 는 디자인 Select 로 그리고 선택된 `value` 를 문자열로 저장한다.
    /// `default` 는 `options` 의 `value` 중 하나여야 한다(검증). 옵션 라벨은 `label_key` i18n.
    Select {
        id: String,
        label_key: String,
        storage_key: String,
        options: Vec<SelectOptionDecl>,
        default: String,
    },

    /// 수치 입력. host 는 Input(+선택적 suffix) 으로 그리고 f64 로 저장한다(정수 표기 가능).
    /// `min`/`max` 가 주어지면 clamp 범위이며 `default` 는 그 범위 안이어야 한다(검증).
    Number {
        id: String,
        label_key: String,
        storage_key: String,
        #[serde(default)]
        default: f64,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        /// 값 뒤에 붙일 단위 라벨 i18n 키 (예 `"%"`). 없으면 suffix 미표시.
        #[serde(default)]
        suffix_key: Option<String>,
    },
}

/// `Select` 항목의 한 선택지 — 저장될 `value` 와 표시용 `label_key`(i18n).
#[derive(Debug, Clone, Deserialize)]
pub struct SelectOptionDecl {
    pub value: String,
    pub label_key: String,
}

impl SettingsItemDecl {
    /// 모든 variant 가 공통으로 갖는 `(id, label_key, storage_key)`. 검증이 variant 무관하게
    /// 공통 형식 검사를 한 곳에서 수행하도록 노출한다.
    pub(crate) fn common(&self) -> (&str, &str, &str) {
        match self {
            SettingsItemDecl::FontOverride {
                id,
                label_key,
                storage_key,
            }
            | SettingsItemDecl::Toggle {
                id,
                label_key,
                storage_key,
                ..
            }
            | SettingsItemDecl::Select {
                id,
                label_key,
                storage_key,
                ..
            }
            | SettingsItemDecl::Number {
                id,
                label_key,
                storage_key,
                ..
            } => (id, label_key, storage_key),
        }
    }
}

/// Plugin 이 contribute 하는 OS-level 윈도우 정의 (`[[contributes.window]]`).
///
/// 1.0 에서는 *schema + 등록 stub* 까지만 동작한다. 실 spawn handler /
/// multi-window 라우팅은 별도 영역에서 추가될 때까지 호스트는 hello 시
/// `tracing::info!` 와 `CoreEvent::PluginWindowDeclared` 만 발화.
///
/// - `id`: plugin 내 고유 식별자 (`is_valid_kind` 규칙 — 소문자/숫자/`_`).
/// - `display_name_i18n_key`: 메뉴 등에 노출될 라벨 키.
/// - `icon`: 옵션. 아이콘 이름.
/// - `default_size`: 옵션. LogicalPx 단위 권장 크기.
/// - `multi_instance`: 동시 여러 인스턴스를 허용하는지. 기본 false.
#[derive(Debug, Clone, Deserialize)]
pub struct WindowContribute {
    pub id: String,
    pub display_name_i18n_key: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub default_size: Option<WindowSizeHint>,
    #[serde(default)]
    pub multi_instance: bool,
}

/// `WindowContribute.default_size` 의 LogicalPx 권장 크기.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct WindowSizeHint {
    pub width: u32,
    pub height: u32,
}

/// Plugin이 contribute하는 popup의 정의.
///
/// - `id`: plugin 내 고유 (소문자+숫자+`-`, 글자로 시작, 길이 ≤ 64).
///   호스트는 `<plugin_id>/<popup_id>`로 전역 식별.
/// - `trigger`: 어떤 조건으로 popup을 여는지. `event` 또는 `ipc`.
/// - `size_hint`: 옵션. 호스트가 LogicalPx 단위로 popup 크기에 적용.
/// - `anchor`: 옵션. 위치 정책. 기본 `screen-center`.
/// - `dismiss_on_outside_click`: 옵션. 기본 true.
#[derive(Debug, Clone, Deserialize)]
pub struct PopupContribute {
    pub id: String,
    pub trigger: PopupTrigger,
    #[serde(default)]
    pub size_hint: Option<PopupSizeHint>,
    #[serde(default = "default_popup_anchor")]
    pub anchor: PopupAnchor,
    #[serde(default = "default_dismiss_on_outside_click")]
    pub dismiss_on_outside_click: bool,
}

fn default_dismiss_on_outside_click() -> bool {
    true
}

fn default_popup_anchor() -> PopupAnchor {
    PopupAnchor::ScreenCenter
}

/// popup이 열리는 조건.
///
/// - `event`: 매니페스트에 적힌 이벤트가 발화하면 자동으로 열림. plugin은 같은 키를
///   `event_subscribe` 또는 `event_publish`로 매니페스트에 노출해야 한다 (자기 plugin이
///   발화한 이벤트로 자기 popup을 여는 흐름이 일반적).
/// - `ipc`: plugin이 host IPC `popup.open`을 호출해 명시적으로 연다.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PopupTrigger {
    Event { event_key: String },
    Ipc,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PopupSizeHint {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PopupAnchor {
    ScreenCenter,
    ActiveSurfaceCenter,
    Cursor,
}

/// 사이드바 도구 메뉴 항목. plugin이 자기 동작을 사용자 진입점으로 노출하는 방식.
///
/// - `id`: plugin 내 고유. 호스트는 `<plugin_id>/<tool_id>`로 전역 식별.
/// - `label_i18n_key`: 라벨. `t()` 카탈로그에 키가 없으면 원본 문자열 fallback.
/// - `icon`: 옵션. 아이콘 이름 (호스트 catalog 또는 plugin 패키지 내 SVG path).
/// - `action`: 클릭 시 수행할 동작.
/// - `order_hint`: 작을수록 위. 호스트 내장은 0..=99, plugin은 100 이상 권장. 기본 100.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolContribute {
    pub id: String,
    pub label_i18n_key: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub action: ToolAction,
    #[serde(default = "default_tool_order_hint")]
    pub order_hint: i32,
}

fn default_tool_order_hint() -> i32 {
    100
}

/// `[[contributes.tool]]` 클릭 시 수행되는 동작.
///
/// - `event`: 호스트가 envelope를 `publish_from_host`로 발화. plugin이 자기 namespace의
///   key를 subscribe하고 있으면 받음. payload는 `{ "tool_id": "<plugin_id>/<tool_id>" }`.
/// - `open_surface`: 활성 tab에 해당 surface kind를 추가. plugin이 `[[surface_kinds]]`로
///   선언한 kind만 허용.
/// - `open_popup`: plugin이 contribute한 popup을 연다. (phase2-popup이 완성되기 전까지는
///   호스트가 warn 로그를 남기고 무시.)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolAction {
    Event { event_key: String },
    OpenSurface { surface_kind: String },
    OpenPopup { popup_id: String },
}

/// Plugin이 점유할 IPC 메서드 namespace prefix.
///
/// 호스트는 `<prefix>.*` 패턴의 모든 IPC 메서드를 등록된 plugin에 forward한다.
/// 예: prefix="codex" → "codex.spawn", "codex.wait" 등을 모두 그 plugin이 처리.
#[derive(Debug, Clone, Deserialize)]
pub struct IpcNamespaceDecl {
    pub prefix: String,
    #[serde(default)]
    pub description_i18n_key: Option<String>,
}

/// Plugin이 contributes하는 최상위 CLI 명령. `tasty <name> <sub>` 형태로 노출된다.
#[derive(Debug, Clone, Deserialize)]
pub struct CliCommandDecl {
    pub name: String,
    /// CLI help용 한 줄 설명. plain text는 plugin manager 없이 동작하는 CLI 클라이언트
    /// 진입 경로(`tasty <plugin> --help`)에서도 곧장 사용할 수 있다.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub description_i18n_key: Option<String>,
    #[serde(default)]
    pub subcommands: Vec<CliSubcommandDecl>,
    /// arg group 이름 → 정의. subcommand가 `args = "<key>"`로 참조한다.
    #[serde(default)]
    pub arg_groups: HashMap<String, CliArgGroup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CliSubcommandDecl {
    pub name: String,
    /// 이 서브커맨드가 호출할 IPC 메서드 (예: "codex.spawn").
    /// plugin 자기 namespace prefix로 시작해야 한다.
    pub ipc_method: String,
    /// `arg_groups`의 키. 비어있는 그룹이라도 명시적으로 가리켜야 한다.
    pub args: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub description_i18n_key: Option<String>,
    /// true 면 stdin 이 TTY 가 아닐 때 stdin 의 JSON 한 덩이를 읽어, CLI 인자로
    /// 명시되지 않은 params 필드를 채운다. Claude Code 처럼 hook payload 를
    /// stdin JSON 으로 전달하는 외부 시스템과 연동할 때 사용. 매칭 키는 각
    /// `CliArg.stdin_field` 또는 (없으면) `CliArg.name`.
    #[serde(default)]
    pub stdin_json: bool,
    /// 있으면 CLI 가 *1 회 응답 + 즉시 종료* 가 아니라, `terminal_states` 중
    /// 하나에 도달하거나 `--timeout` 초가 지날 때까지 *반복 IPC 호출* 한다.
    /// `tasty claude wait` 처럼 *진짜 blocking subprocess* 가 필요한 경우 활성화.
    /// timeout 도달 시 마지막 응답을 그대로 출력한다.
    #[serde(default)]
    pub polling: Option<PollingDecl>,
    /// 있으면 CLI 가 1 차 IPC 응답을 출력한 *뒤* `auto_wait.method` 를 chain
    /// 호출하여 terminal_states 도달까지 block 한다 (claude/codex spawn·tell 의
    /// 자동 wait). `polling` 과 동시 선언 금지 (validator 가 reject).
    #[serde(default)]
    pub auto_wait: Option<AutoWaitDecl>,
}

/// `spawn` / `tell` 같이 1 차 응답 후 *chained wait* 를 자동으로 거는 CLI 명령
/// 의 설정. CLI 는 1 차 IPC 응답을 line-delimited JSON 으로 출력한 뒤,
/// `--no-wait` 가 아닐 경우 `method` 를 호출하여 `polling.terminal_states`
/// 도달까지 block 하고 wait 응답을 두 번째 JSON line 으로 출력한다.
#[derive(Debug, Clone, Deserialize)]
pub struct AutoWaitDecl {
    /// chain 호출할 IPC method (예: `"claude.wait"`, `"claude.wait_by_surface"`).
    pub method: String,
    /// 1 차 응답 JSON 의 어떤 키를 wait params 의 어떤 키로 매핑할지.
    /// 키: 1 차 응답 키, 값: wait params 키.
    /// 예: `{ "child_index": "child_index", "parent_surface_id": "surface" }`.
    #[serde(default)]
    pub map_from_response: std::collections::HashMap<String, String>,
    /// 1 차 응답에 없는 wait params 를 *원 요청* 의 어떤 params 키에서 가져올지.
    /// 키: 원 요청 params 키, 값: wait params 키.
    /// `tell` 처럼 응답에 child_index 가 없는 경우 fallback 매핑에 사용.
    #[serde(default)]
    pub map_from_request: std::collections::HashMap<String, String>,
    /// wait 폴링 사양 (`PollingDecl` 과 동일 모양 — state_field / terminal_states /
    /// interval_ms). `timeout_field` 는 본 `AutoWaitDecl` 의 `timeout_field` 가
    /// 우선이므로 무시된다.
    pub polling: PollingDecl,
    /// `--no-wait` flag 의 CLI arg name. CLI 가 이 키를 true 로 받으면 chain skip.
    /// 기본 `"no_wait"`.
    #[serde(default = "default_no_wait_field")]
    pub no_wait_field: String,
    /// `--timeout` flag 의 CLI arg name. wait 폴링의 timeout 으로 사용.
    /// 비어있으면 무한 대기. 기본 `"timeout"`.
    #[serde(default = "default_timeout_field")]
    pub timeout_field: String,
}

fn default_no_wait_field() -> String {
    "no_wait".into()
}

fn default_timeout_field() -> String {
    "timeout".into()
}

/// CLI 명령이 *blocking polling* 모드일 때의 설정.
#[derive(Debug, Clone, Deserialize)]
pub struct PollingDecl {
    /// IPC 응답 JSON 의 어떤 필드를 보고 terminal 판정할지. 보통 `"state"`.
    pub state_field: String,
    /// 이 값들 중 하나에 도달하면 polling 종료. 예: `["idle", "needs_input", "exited"]`.
    pub terminal_states: Vec<String>,
    /// polling 간격 (밀리초). 기본 500ms.
    #[serde(default = "default_polling_interval_ms")]
    pub interval_ms: u64,
    /// `--timeout` 플래그 이름. CLI args 의 이 필드 (u32) 가 초 단위 timeout.
    /// 비어 있으면 무한 대기 (CLI process 가 죽을 때까지).
    #[serde(default)]
    pub timeout_field: Option<String>,
}

fn default_polling_interval_ms() -> u64 {
    500
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CliArgGroup {
    #[serde(default)]
    pub positional: Vec<CliArg>,
    #[serde(default)]
    pub flags: Vec<CliArg>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CliArg {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: CliArgType,
    /// `flags`에 들어가는 인자에만 존재. `positional`에서는 None.
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<toml::Value>,
    /// 한 줄 도움말. clap의 `Arg::help`에 그대로 전달된다.
    #[serde(default)]
    pub help: Option<String>,
    /// subcommand 의 `stdin_json = true` 일 때, stdin JSON 의 어느 키에서
    /// 이 인자의 fallback 값을 가져올지. 없으면 `name` 을 그대로 키로 쓴다.
    /// 예: Claude Code hook payload 의 `session_id` 를 `--session` 인자에
    /// 매핑하려면 `stdin_field = "session_id"`.
    #[serde(default)]
    pub stdin_field: Option<String>,
    /// path 인자의 의미론적 종류. 현재 `Some("directory")` 만 사용한다 — CLI 가
    /// 이 인자를 발견하면 호출자 cwd 기준 absolute path 로 정규화 + 디렉토리
    /// 존재 검증 후 IPC 로 전달. (호스트는 absolute + valid 만 받는다는 contract.)
    #[serde(default)]
    pub path_kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CliArgType {
    U32,
    I64,
    String,
    Bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title_i18n_key: String,
    #[serde(default)]
    pub default_keybinding: Option<String>,
    /// 단축키를 호스트의 의미론적 액션과 어떻게 묶을지 plugin 작성자가 선언한다.
    /// `"independent"` (기본) 또는 `"inherit:<host_action>"`.
    #[serde(default)]
    pub binding_mode: BindingMode,
    /// command 발화 범위. `global`은 어디서나 동작(조합키 권장), `surface`는
    /// owner plugin이 만든 surface에 포커스가 있을 때만 동작(단일 키 허용).
    /// 기본 `global`.
    #[serde(default)]
    pub scope: CommandScope,
}

/// command가 어떤 포커스 컨텍스트에서 발화하는지.
///
/// - `Global`: 어디서나 동작. 단축키는 조합키만 권장.
/// - `Surface`: owner plugin이 만든 surface에 포커스가 있을 때만 동작.
///   단일 키도 허용.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommandScope {
    #[default]
    Global,
    Surface,
}

/// command가 호스트 액션 키와 어떤 관계를 갖는지.
///
/// - `Independent`: plugin 자체 키. 사용자가 설정에서 자유롭게 변경 가능.
/// - `InheritHost(action)`: 호스트의 의미론적 액션(예: `"clipboard.copy"`)
///   키 설정을 그대로 따라감. 사용자가 설정 UI에서 떼어내 독립 키로 만들 수 있다.
///
/// TOML 표기: `binding_mode = "independent"` 또는
/// `binding_mode = "inherit:clipboard.copy"`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BindingMode {
    #[default]
    Independent,
    InheritHost(String),
}

impl<'de> Deserialize<'de> for BindingMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "independent" {
            return Ok(BindingMode::Independent);
        }
        if let Some(action) = s.strip_prefix("inherit:") {
            let action = action.trim();
            if action.is_empty() {
                return Err(serde::de::Error::custom(
                    "binding_mode 'inherit:' must be followed by a host action id",
                ));
            }
            return Ok(BindingMode::InheritHost(action.to_string()));
        }
        Err(serde::de::Error::custom(format!(
            "invalid binding_mode '{}': expected 'independent' or 'inherit:<host_action>'",
            s
        )))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MenuItemDecl {
    pub menu: String,
    pub command: String,
    #[serde(default)]
    pub when: Option<String>,
}

/// `file::format::is_valid_detector_id` 의 로컬 복제. 길이 1..=64, lowercase ascii
/// + 숫자 + `-`, optional `$`-prefix. 호스트 측 식별기와 동일 규칙이며 schema 차원
///   검증 only.
fn is_valid_detector_id_local(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    if let Some(rest) = s.strip_prefix('$') {
        if rest.is_empty() {
            return false;
        }
        return rest
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
