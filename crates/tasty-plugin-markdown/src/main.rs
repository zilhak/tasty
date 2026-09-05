#![forbid(unsafe_code)]

//! Tasty markdown plugin — **webview** markdown viewer surface (ADR-0028, Stage B).
//!
//! The plugin owns the markdown document (reads the `.md` file delivered via `surface.create`,
//! watches it for external changes) and renders it by generating a complete, sanitized HTML
//! document (`render::render_document`) that the host's native OS WebView displays — the
//! plugin no longer tessellates its own egui mesh for the document body (that was the former
//! `rendering = "egui-mesh"` design, ADR-0028 / B1). Address-bar navigation and content link
//! clicks are captured via the host's `webview.navigation_attempt` event (Stage A) and routed
//! back through `file_handler.dispatch` (files) or the OS (external URLs) — see `render.rs`'s
//! module doc for why link destinations are rewritten into an internal URL-fragment scheme
//! rather than left as plain `href`s.
//!
//! Two egui-mesh popups remain unchanged: the large-file confirmation and the file-open form
//! (`[[contributes.popup]]` in the manifest) are still self-rendered by the plugin — only the
//! main document surface moved to the webview channel.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod render;
mod watch;

/// 빌드타임 SVG 베이크 산출물 (방식 B). `build.rs` 가 `tasty-icons` 의 canonical
/// `<svg>` 를 usvg 로 파싱·평탄화해 `pub const <NAME>: &[&[[f32; 2]]]`(viewBox 0..24
/// 좌표)를 생성한다. 런타임은 이 점배열을 [`tasty_plugin_sdk::baked_icon::draw`] 로
/// 그릴 크기에 스케일해 벡터 stroke 로 그린다(텍스처 없음, DPI 독립). 확인 팝업
/// (large-file/file-open) 전용 — 본문은 더 이상 이 plugin 이 그리지 않는다.
mod baked_icons {
    include!(concat!(env!("OUT_DIR"), "/plugin_icons.rs"));
}

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::{
    BusHandle, EventDispatchCtx, EventScope, HostHandle, IpcMethodCtx, IpcMethodError, Plugin,
    PluginEnv, PopupClosedCtx, PopupOpenCtx, PopupOpenResult, PopupSetContextCtx, SurfaceCreateCtx,
    SurfaceRestoreCtx, SurfaceResult, Translator, WebviewNavigationAttemptCtx,
};
use tasty_type_appearance::theme::Theme;
use watch::WatchCmd;

#[cfg(any(unix, windows))]
use tasty_plugin_sdk::EguiMeshPopup;

const PLUGIN_ID: &str = "com.tasty.markdown";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often to check the file's mtime (in seconds).
const RELOAD_CHECK_INTERVAL_SECS: f64 = 1.0;

/// 대용량 파일 확인 게이트 임계값 (1MB). 이 크기를 *초과* 하는 파일은 읽기 전에 확인
/// 팝업을 띄운다. **크기 감지는 plugin in-process** — host 는 파일 크기를 stat 하지
/// 않는다(불가침 원칙: markdown 크기게이트는 plugin 소유). 이름도 plugin-local 이다.
const LARGE_FILE_LIMIT_BYTES: u64 = 1024 * 1024;

/// 대용량 감지 시 plugin 이 발행하는 이벤트 key. 매니페스트 `event_publish` 패턴 +
/// `[[contributes.popup]]` event trigger 가 이 key 로 매칭돼 확인 팝업을 연다.
const LARGE_FILE_EVENT_KEY: &str = "com.tasty.markdown.large_file_confirm";

/// host 가 발행하는 전역 테마 변경 이벤트 key(`event_subscribe`). webview-kind surface 는
/// `surface.set_context` 를 받지 않아 Theme 변경이 자동으로 밀리지 않으므로, 이 이벤트를
/// 받을 때마다 살아있는 모든 문서를 재생성한다.
const THEME_CHANGED_EVENT: &str = "theme.changed";

/// Per-surface markdown document state owned by the plugin (content, load outcome, base
/// dir for relative paths). mtime tracking to *decide when* to reload lives in `watch.rs`,
/// not here — `MdDoc` only knows how to (re-)read once asked.
struct MdDoc {
    file_path: Option<String>,
    base_dir: Option<PathBuf>,
    content: String,
    load_error: Option<String>,
    /// 대용량 확인 대기 중이면 true — 파일을 아직 읽지 않았다(빈 콘텐츠). 확인 팝업의
    /// [열기] 확정 시 [`MdDoc::resume_load`] 가 실제 read 를 재개한다.
    pending_large: bool,
}

impl MdDoc {
    fn new(file: Option<String>) -> Self {
        let base_dir = file
            .as_ref()
            .and_then(|f| PathBuf::from(f).parent().map(|p| p.to_path_buf()));
        let (content, load_error) = match &file {
            Some(f) => match std::fs::read_to_string(f) {
                Ok(text) => (text, None),
                Err(e) => (String::new(), Some(e.to_string())),
            },
            None => (String::new(), None),
        };
        Self {
            file_path: file,
            base_dir,
            content,
            load_error,
            pending_large: false,
        }
    }

    /// 대용량 파일을 **읽지 않고** 경로만 보관한 문서를 만든다(확인 팝업 대기). 확인
    /// 시 [`MdDoc::resume_load`] 가 read 를 재개한다.
    fn new_deferred(file: Option<String>) -> Self {
        let base_dir = file
            .as_ref()
            .and_then(|f| PathBuf::from(f).parent().map(|p| p.to_path_buf()));
        Self {
            file_path: file,
            base_dir,
            content: String::new(),
            load_error: None,
            pending_large: true,
        }
    }

    /// 대용량 확인 [열기] 후 실제 read 를 재개한다.
    fn resume_load(&mut self) {
        self.pending_large = false;
        if let Some(f) = self.file_path.clone() {
            self.read_now(&f);
        }
    }

    /// Force a re-read (`markdown.reload` IPC, idle watch — `watch.rs` owns its own mtime
    /// tracking to decide *when* to call this; `MdDoc` itself no longer tracks mtime since
    /// `paint_surface` never fires for a webview-kind surface, so there's no per-frame
    /// throttled poll to gate anymore — see the module doc on the removed `poll_reload`).
    fn force_reload(&mut self) {
        let Some(f) = self.file_path.clone() else {
            return;
        };
        self.read_now(&f);
    }

    fn read_now(&mut self, path: &str) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                self.content = text;
                self.load_error = None;
            }
            Err(e) => self.load_error = Some(e.to_string()),
        }
    }
}

/// 대용량 확인 팝업 인스턴스의 대상 정보(open_popup context 로 받아 보관). [열기] 시
/// 이 surface 의 문서 read 를 재개한다.
#[derive(Clone)]
struct LargeFileConfirm {
    surface_id: u32,
    /// 표시용 파일명(basename). 경로 전체 대신 파일명만 팝업에 노출.
    file_name: String,
    /// 크기 칩 라벨 (예: "3.2 MB").
    size_label: String,
}

/// 파일열기 팝업 인스턴스 상태 — 경로 입력 버퍼 + (선택) convert 대상 surface.
/// TextEdit 이 `path_input` 을 mutate 하고 browse(native 다이얼로그) 결과가 채운다.
/// `convert_surface_id` 가 `Some` 이면 확정 시 그 surface 를 제자리 markdown 변환
/// (`markdown.navigate`), `None` 이면 새 탭으로 연다(`file_handler.dispatch`). host 가
/// open context 의 `surface_id` 유무로 이 값을 정한다.
#[derive(Default)]
struct FileOpenState {
    path_input: String,
    convert_surface_id: Option<u32>,
}

struct MarkdownPlugin {
    /// surface_id → markdown document state.
    docs: HashMap<u32, MdDoc>,
    /// large-file 이벤트 발행용 Event Bus 핸들(`on_start` 에서 저장).
    bus: Option<BusHandle>,
    /// host IPC 호출용 핸들(`on_start` 에서 저장) — `create_surface`/`on_event`/
    /// `on_webview_navigation_attempt` 컨텍스트에는 host 필드가 없다(SDK 계약). idle 감시
    /// worker 로 옮기는 클론과는 별개(그쪽은 watch.rs 전용).
    host: Option<HostHandle>,
    /// popup instance_id → 대용량 확인 대상.
    confirm: HashMap<u64, LargeFileConfirm>,
    /// popup instance_id → 파일열기 팝업 상태(경로 입력 버퍼).
    file_open: HashMap<u64, FileOpenState>,
    /// `file_picker.trigger`(ADR-0058) 로 보낸 요청의 `request_id` → 그
    /// 요청을 낸 파일열기 팝업 instance_id. `"file_picker.result"` 이벤트 수신 시
    /// 이 맵으로 상관관계를 맞춰 `path_input` 을 채운다.
    pending_file_picker: HashMap<u64, u64>,
    /// popup instance_id → egui-mesh popup 렌더 상태(폰트 atlas·shared buffer 소유).
    /// 대용량/파일열기 확인 팝업 전용 — 본문은 더 이상 이 채널을 쓰지 않는다.
    #[cfg(any(unix, windows))]
    popups: HashMap<u64, EguiMeshPopup>,
    /// CJK fallback 폰트를 이미 설치한 popup instance_id — set_fonts 재업로드 방지.
    #[cfg(any(unix, windows))]
    popup_fonts_installed: std::collections::HashSet<u64>,
    /// plugin lang 카탈로그 (state.failed / state.empty / addr.* 등 UI 문자열).
    tr: Translator,
    /// idle auto-reload 감시 worker(`watch::run`) 로 등록/해제 명령을 보내는 채널
    /// (단계 06). `on_start` 에서 worker 를 spawn 하며 채워진다 — 그 전에는 감시가
    /// 비활성(사실상 도달하지 않음, worker_loop 이 on_start 를 먼저 호출).
    watch_tx: Option<mpsc::Sender<WatchCmd>>,
}

impl MarkdownPlugin {
    fn new(tr: Translator) -> Self {
        Self {
            docs: HashMap::new(),
            bus: None,
            host: None,
            confirm: HashMap::new(),
            file_open: HashMap::new(),
            pending_file_picker: HashMap::new(),
            #[cfg(any(unix, windows))]
            popups: HashMap::new(),
            #[cfg(any(unix, windows))]
            popup_fonts_installed: std::collections::HashSet::new(),
            tr,
            watch_tx: None,
        }
    }
}

impl Plugin for MarkdownPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn on_start(&mut self, host: HostHandle, bus: BusHandle) {
        // large-file 확인 이벤트 발행 + `theme.changed` 구독에 Event Bus 핸들이 필요하다.
        if let Err(e) = bus.subscribe(THEME_CHANGED_EVENT) {
            tracing::warn!("markdown: theme.changed subscribe failed: {e}");
        }
        self.bus = Some(bus);
        // `create_surface`/`on_event`/`on_webview_navigation_attempt` 는 host 를 받지
        // 않으므로(SDK 계약) 별도로 보관한다 — idle 감시 worker 로 옮기는 클론과는 독립.
        self.host = Some(host.clone());

        // idle auto-reload(단계 06, Stage B 갱신): paint 에 종속되지 않는 별도 스레드가
        // mtime 을 폴링하다가 변경을 감지하면 host 를 왕복해 이 plugin 자신의
        // `markdown.reload` IPC 를 직접 호출한다(worker 는 read 하지 않음 — 상세는
        // watch::run 모듈 문서). webview kind 는 `paint`/`set_context` 를 전혀 받지
        // 않으므로 `SurfaceInvalidated` 기반 idle-invalidate 경로는 쓰지 않는다.
        let (tx, rx) = mpsc::channel();
        self.watch_tx = Some(tx);
        if let Err(e) = std::thread::Builder::new()
            .name("markdown-watch".to_string())
            .spawn(move || watch::run(host, rx))
        {
            tracing::warn!("markdown watch worker spawn failed — idle auto-reload disabled: {e}");
        }
    }

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        // SDK 는 surface.create 의 **전체 envelope** 을 `ctx.params` 로 넘긴다 — 실제 생성
        // params(file 등)는 `params.params` 아래에 중첩돼 있다.
        //
        // 대용량 파일(> LARGE_FILE_LIMIT_BYTES)은 **plugin in-process** 로 크기를 감지해
        // read 를 보류하고(확인 대기), large-file 이벤트를 발행한다. host 는 파일 크기를
        // stat 하지 않는다(크기게이트는 plugin 소유). 이벤트 → host `fire_popup_triggers`
        // → 이 plugin 의 `[[contributes.popup]]`(event trigger) 확인 팝업이 열린다.
        let file = surface_param_file(&ctx.params);
        self.open_file_surface(ctx.surface_id, file)
    }

    // layout 재시작 복원 경로. preset apply 는 `surface.create` 를 타지만 layout
    // 재시작은 `surface.restore` 를 탄다 — SDK 기본 구현은 빈 `SurfaceResult` 라,
    // 구현하지 않으면 재시작 시 markdown 이 file 을 잃고 빈 채로 살아난다. create 가
    // 실어 둔 snapshot(`{"file": ...}`)을 그대로 받아 같은 문서를 연다.
    fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
        let file = ctx
            .data
            .get("file")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.open_file_surface(ctx.surface_id, file)
    }

    fn destroy_surface(&mut self, surface_id: u32) {
        self.docs.remove(&surface_id);
        self.watch_unregister(surface_id);
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "markdown.reload" => self.markdown_reload(&ctx.params),
            // host 가 구현한 이름이다(surface 를 열고 있는 창을 host 가 안다). 이
            // namespace 를 plugin 이 점유하는 순간 외부 호출은 전부 여기로 forward 되므로,
            // arm 이 없으면 host 구현이 **외부에서만** 안 닿는다 — plugin 이 설치돼 있으면
            // 막히고 빠지면 열리는, 설치 상태에 따라 흔들리는 표면이 된다.
            // 실측(2026-09-05): arm 이 없을 때 외부 `markdown.navigate` 는 plugin 의
            // not_found 로 끝났고, plugin 을 빼면 같은 호출이 host arm 에 닿았다.
            // image.open/list 와 같은 self-call trampoline 로 host 에 돌려준다.
            "markdown.navigate" => Ok(ctx.host.call(&ctx.method, ctx.params)?),
            // 최근목록 조회는 host 소유(AppState.recent_files) — plugin 은 저장소를 못 본다.
            // host 는 generic `recent.query {kind}` 만 알고 "markdown" 을 모른다. CLI/주소창
            // caller 가 이 plugin namespace 로 보낸 호출을 host 의 generic 메서드로 kind 를
            // 채워 trampoline 한다(host 무지 유지 — image.open/list 와 동형 host-adapter).
            "markdown.recent" => Ok(ctx
                .host
                .call("recent.query", json!({ "kind": "markdown" }))?),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn open_popup(&mut self, ctx: PopupOpenCtx) -> PopupOpenResult {
        // egui-mesh popup 이라 tree 를 반환하지 않는다(mesh 채널 paint_popup 로 그린다).
        // popup_id 로 두 팝업을 구분한다.
        match ctx.popup_id.as_str() {
            // large-file 확인 — event payload({surface_id, path, size})가 context 로 온다.
            "large-file-confirm" => {
                if let (Some(surface_id), Some(path), Some(size)) = (
                    ctx.context.get("surface_id").and_then(|v| v.as_u64()),
                    ctx.context.get("path").and_then(|v| v.as_str()),
                    ctx.context.get("size").and_then(|v| v.as_u64()),
                ) {
                    self.confirm.insert(
                        ctx.instance_id,
                        LargeFileConfirm {
                            surface_id: surface_id as u32,
                            file_name: basename(path),
                            size_label: format_size(size),
                        },
                    );
                }
            }
            // 파일열기 폼 — 경로 입력 버퍼는 빈 상태로 시작. context 에 `surface_id` 가
            // 있으면 그 surface 를 제자리 변환할 대상으로 기억한다(없으면 새 탭 열기).
            "file-open" => {
                let convert_surface_id = ctx
                    .context
                    .get("surface_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                self.file_open.insert(
                    ctx.instance_id,
                    FileOpenState {
                        path_input: String::new(),
                        convert_surface_id,
                    },
                );
            }
            other => {
                tracing::warn!("markdown open_popup: unknown popup_id '{other}'");
            }
        }
        PopupOpenResult::default()
    }

    fn paint_popup(&mut self, ctx: PopupSetContextCtx) {
        // instance 가 어느 팝업 맵에 있는지로 분기한다(paint 시점엔 popup_id 가 없다).
        let iid = ctx.params.instance_id;
        if self.file_open.contains_key(&iid) {
            self.paint_file_open(ctx);
        } else {
            self.paint_confirm(ctx);
        }
    }

    fn on_popup_closed(&mut self, ctx: PopupClosedCtx) {
        let iid = ctx.instance_id;
        #[cfg(any(unix, windows))]
        {
            self.popups.remove(&iid);
            self.popup_fonts_installed.remove(&iid);
        }
        // 확인 없이 닫힘(취소/outside-click/Esc)이면 surface 는 대기(빈) 상태로 유지한다.
        self.confirm.remove(&iid);
        self.file_open.remove(&iid);
        // 이 팝업이 낸 file_picker.trigger 요청이 아직 응답 전이면 상관관계 항목을
        // 같이 정리한다 — 늦게 도착한 결과는 (그때 iid 를 못 찾으므로) 조용히 무시된다.
        self.pending_file_picker.retain(|_, v| *v != iid);
    }

    /// host 가 push 하는 이벤트: `"file_picker.result"`(ADR-0058)와 `"theme.changed"`.
    fn on_event(&mut self, ctx: EventDispatchCtx) {
        match ctx.envelope.key.as_str() {
            FILE_PICKER_RESULT_EVENT => {
                let Ok(reply) =
                    serde_json::from_value::<FilePickerResultWire>(ctx.envelope.payload)
                else {
                    tracing::warn!("markdown: malformed file_picker.result event");
                    return;
                };
                let Some(iid) = self.pending_file_picker.remove(&reply.request_id) else {
                    return;
                };
                if reply.cancelled {
                    return;
                }
                if let (Some(path), Some(st)) =
                    (reply.paths.into_iter().next(), self.file_open.get_mut(&iid))
                {
                    st.path_input = path;
                }
            }
            // 전역 테마가 바뀌면(라이트/다크 토글 등) 살아있는 모든 문서를 최신 CSS 로
            // 재생성한다 — webview-kind surface 는 `surface.set_context` 를 받지 않아
            // Theme 이 자동으로 밀리지 않는다(`host_api/webview.rs::handle_theme_query` 문서).
            THEME_CHANGED_EVENT => self.reload_all_webviews(),
            _ => {}
        }
    }

    /// `webview.navigation_attempt`(Stage A) — 소유 webview surface 가 navigation 을
    /// 시도함(주소창 Go/Enter, 콘텐츠 링크 클릭). `render.rs` 가 생성한 문서는 모든
    /// 실제 목적지를 `#tasty-nav:{link,addr}:<enc>` fragment 로 감싸므로, 그 마커가
    /// 없는 navigation 시도는 이 plugin 이 낸 것이 아니라 조용히 무시한다.
    fn on_webview_navigation_attempt(&mut self, ctx: WebviewNavigationAttemptCtx) {
        let Some(intent) = render::parse_nav_fragment(&ctx.url) else {
            return;
        };
        let Some(host) = self.host.clone() else {
            tracing::warn!(
                "markdown surface {}: navigation attempt before host handle ready — dropping",
                ctx.surface_id
            );
            return;
        };
        match intent {
            render::NavIntent::Link(dest) => {
                let base_dir = self
                    .docs
                    .get(&ctx.surface_id)
                    .and_then(|d| d.base_dir.clone());
                if let Some(click) = render::classify_link(&dest, base_dir.as_deref()) {
                    dispatch_link(&host, ctx.surface_id, click);
                }
            }
            render::NavIntent::Addr(path) => navigate(&host, ctx.surface_id, &path),
        }
    }
}

impl MarkdownPlugin {
    fn markdown_reload(&mut self, params: &Value) -> Result<Value, IpcMethodError> {
        let surface_id = params
            .get("surface")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| IpcMethodError::invalid_params("missing 'surface'"))?
            as u32;
        if let Some(doc) = self.docs.get_mut(&surface_id) {
            doc.force_reload();
        }
        self.reload_webview(surface_id);
        Ok(json!({ "ok": true, "surface_id": surface_id }))
    }

    /// 문서를 만든다. 파일이 임계값을 *초과* 하면 read 를 보류(`new_deferred`)하고
    /// large-file 이벤트를 발행해 확인 팝업을 띄운다(크기 감지는 plugin in-process).
    /// bus 가 없으면(초기화 전) 게이트를 건너뛰고 즉시 로드한다(fail-open).
    /// create/restore 공용 — file 로 문서를 열고, host 에 snapshot(`{"file": ...}`)을
    /// 올려 layout/preset round-trip 에 file 을 보존한다. host 는 이 snapshot 을
    /// `RemoteSurface.snapshot_cache` 로 캐시했다가 `SavedSurface::Generic.data` 로
    /// 저장하고, 다음 실행의 `surface.restore` 에 `data` 로 되돌려준다. file 이 없으면
    /// 저장할 것이 없어 `None`(호스트는 기존 캐시 유지).
    fn open_file_surface(&mut self, surface_id: u32, file: Option<String>) -> SurfaceResult {
        let doc = self.make_doc(file.clone(), surface_id);
        self.docs.insert(surface_id, doc);
        // idle 감시 등록(단계 06). `markdown.navigate` 제자리 이동도 같은 surface_id 로
        // create_surface 를 다시 호출하므로 여기서 자연스럽게 갱신된다.
        self.watch_register(surface_id, file.clone());
        // 문서를 HTML 로 렌더해 host WebView 에 싣는다 — 이 kind 는 mesh 를 그리지 않는다.
        self.reload_webview(surface_id);
        SurfaceResult {
            display_name: None,
            snapshot: file.as_ref().map(|f| json!({ "file": f })),
        }
    }

    fn make_doc(&self, file: Option<String>, surface_id: u32) -> MdDoc {
        if let Some(path) = file.as_deref()
            && let Some(size) = file_exceeds_limit(path)
        {
            if let Some(bus) = self.bus.as_ref() {
                let payload = json!({
                    "surface_id": surface_id,
                    "path": path,
                    "size": size,
                });
                if let Err(e) =
                    bus.publish_fresh(LARGE_FILE_EVENT_KEY, payload, EventScope::Surface)
                {
                    tracing::warn!("markdown large-file event publish failed: {e}");
                }
                return MdDoc::new_deferred(file);
            }
            tracing::warn!("markdown large-file gate: event bus unavailable — loading anyway");
        }
        MdDoc::new(file)
    }

    /// idle 감시 worker(단계 06)에 surface 의 감시 대상 경로를 등록/갱신한다. worker 가
    /// 없으면(spawn 실패) 조용히 무시 — idle auto-reload 만 비활성화된다(webview kind 는
    /// paint 되지 않으므로 대체할 다른 자동 갱신 경로가 없다 — `markdown.reload` 를 명시
    /// 호출하거나 파일을 다시 열어야 갱신된다).
    fn watch_register(&self, surface_id: u32, path: Option<String>) {
        let Some(tx) = &self.watch_tx else { return };
        if tx.send(WatchCmd::Register { surface_id, path }).is_err() {
            tracing::warn!(
                "markdown watch: register send failed for surface {surface_id} (worker gone)"
            );
        }
    }

    /// idle 감시 worker(단계 06)에서 surface 를 해제한다.
    fn watch_unregister(&self, surface_id: u32) {
        let Some(tx) = &self.watch_tx else { return };
        if tx.send(WatchCmd::Unregister { surface_id }).is_err() {
            tracing::warn!(
                "markdown watch: unregister send failed for surface {surface_id} (worker gone)"
            );
        }
    }

    /// 문서를 (재)렌더해 host WebView 에 싣는다: 현재 Theme + recent 목록을 조회하고
    /// [`render::render_document`] 로 sanitize 된 HTML 문서를 만들어 `webview.set_url` 로
    /// 전달한다. host 의 `sync_webviews` 가 scheme 없는 문자열을 raw HTML 로 인식해
    /// `PlatformWebView::load_html` 을 호출한다(`src/view/main/redraw.rs`).
    fn reload_webview(&self, surface_id: u32) {
        let Some(host) = &self.host else {
            tracing::warn!(
                "markdown surface {surface_id}: no host handle yet (on_start not called?) — cannot load webview"
            );
            return;
        };
        let Some(doc) = self.docs.get(&surface_id) else {
            return;
        };
        let Some(theme) = fetch_theme(host) else {
            return;
        };
        let recent = fetch_recent(host);
        let file_path = doc.file_path.as_deref().unwrap_or_default();
        let html = render::render_document(render::DocumentInput {
            theme: &theme,
            tr: &self.tr,
            file_path,
            source: &doc.content,
            load_error: doc.load_error.as_deref(),
            base_dir: doc.base_dir.as_deref(),
            recent: &recent,
        });
        if let Err(e) = host.call(
            "webview.set_url",
            json!({ "surface_id": surface_id, "url": html }),
        ) {
            tracing::warn!("markdown surface {surface_id}: webview.set_url failed: {e}");
        }
    }

    /// 살아있는 모든 markdown 문서를 재렌더한다(`theme.changed` 수신 시).
    fn reload_all_webviews(&self) {
        for &surface_id in self.docs.keys() {
            self.reload_webview(surface_id);
        }
    }

    /// large-file 확인 팝업 한 frame 을 egui-mesh 로 그린다. [열기] 시 대상 surface 의
    /// 문서 read 를 재개하고 그 surface 의 webview 를 재생성한 뒤 팝업을 닫는다. [취소]
    /// 는 팝업만 닫는다(surface 는 대기 상태 유지). chrome(scrim/border/Esc/outside-click)
    /// 은 host 소유.
    #[cfg(any(unix, windows))]
    fn paint_confirm(&mut self, ctx: PopupSetContextCtx) {
        let iid = ctx.params.instance_id;
        let Some(theme) = ctx.params.theme.as_ref().map(theme_from_wire) else {
            tracing::debug!("markdown confirm popup {iid}: set_context without theme — skipping");
            return;
        };
        let Some(confirm) = self.confirm.get(&iid).cloned() else {
            // 우리 확인 팝업이 아닌 instance — 무시(있을 수 없음).
            return;
        };

        let mut chosen: Option<ConfirmChoice> = None;
        {
            // popups/popup_fonts_installed/tr 만 빌린다(docs·confirm 과 서로소) — 아래 처리부가
            // self 전체를 다시 빌릴 수 있도록 이 블록에서 borrow 를 끝낸다.
            let popups = &mut self.popups;
            let installed = &mut self.popup_fonts_installed;
            let tr = &self.tr;
            let popup = popups.entry(iid).or_insert_with(|| EguiMeshPopup::new(iid));
            if installed.insert(iid) {
                install_fonts(popup.context());
            }
            let result = popup.paint(&ctx.host, &ctx.params, |egui_ctx| {
                chosen = draw_confirm(egui_ctx, &theme, &confirm, tr);
            });
            if let Err(e) = result {
                tracing::warn!("markdown confirm popup {iid} paint failed: {e}");
            }
        }

        match chosen {
            Some(ConfirmChoice::Open) => {
                if let Some(doc) = self.docs.get_mut(&confirm.surface_id) {
                    doc.resume_load();
                }
                self.reload_webview(confirm.surface_id);
                close_popup(&ctx.host, iid);
            }
            Some(ConfirmChoice::Cancel) => close_popup(&ctx.host, iid),
            None => {}
        }
    }

    /// 파일열기 팝업 한 frame 을 egui-mesh 로 그린다. [browse] 는 host `fs.pick_file`
    /// (native 다이얼로그)로 경로를 채우고, [열기] 는 입력 경로를 host `file_handler.dispatch`
    /// 로 열고 팝업을 닫는다. [취소]/Esc 는 팝업만 닫는다. chrome(scrim/border)은 host 소유.
    #[cfg(any(unix, windows))]
    fn paint_file_open(&mut self, ctx: PopupSetContextCtx) {
        let iid = ctx.params.instance_id;
        let Some(theme) = ctx.params.theme.as_ref().map(theme_from_wire) else {
            tracing::debug!("markdown file-open popup {iid}: set_context without theme — skipping");
            return;
        };

        let mut action = FileOpenAction::None;
        {
            // popups/popup_fonts_installed/tr/file_open 서로소 필드만 빌린다 — 아래 처리부가
            // self 를 다시 빌릴 수 있도록 이 블록에서 borrow 를 끝낸다.
            let popups = &mut self.popups;
            let installed = &mut self.popup_fonts_installed;
            let tr = &self.tr;
            let Some(st) = self.file_open.get_mut(&iid) else {
                return;
            };
            let popup = popups.entry(iid).or_insert_with(|| EguiMeshPopup::new(iid));
            if installed.insert(iid) {
                install_fonts(popup.context());
            }
            let result = popup.paint(&ctx.host, &ctx.params, |egui_ctx| {
                action = draw_file_open(egui_ctx, &theme, st, tr);
            });
            if let Err(e) = result {
                tracing::warn!("markdown file-open popup {iid} paint failed: {e}");
            }
        }

        match action {
            FileOpenAction::Browse => {
                // host 소유 file_picker popup 을 트리거(ADR-0058) — attach
                // (원격) workspace 에서도 동작한다(native rfd 다이얼로그와 달리 원격
                // 개념이 있다). 즉시 request_id 만 돌아오고, 실제 선택 결과는 나중에
                // `on_event` 의 `"file_picker.result"` 로 비동기 도착한다.
                if let Some(request_id) = trigger_file_picker(&ctx.host, iid) {
                    self.pending_file_picker.insert(request_id, iid);
                }
            }
            FileOpenAction::Open => {
                let (path, convert_sid) = self
                    .file_open
                    .get(&iid)
                    .map(|s| (s.path_input.trim().to_string(), s.convert_surface_id))
                    .unwrap_or_default();
                if !path.is_empty() {
                    match convert_sid {
                        // convert 대상 surface → 제자리 markdown 변환.
                        Some(sid) => navigate(&ctx.host, sid, &path),
                        // 대상 없음 → 새 탭으로 연다.
                        None => open_markdown_file(&ctx.host, &path),
                    }
                    close_popup(&ctx.host, iid);
                }
            }
            FileOpenAction::Cancel => close_popup(&ctx.host, iid),
            FileOpenAction::None => {}
        }
    }

    /// unix/windows 외에는 egui-mesh 채널이 비활성이라 확인 팝업 렌더는 no-op(참고:
    /// 이 두 cfg 는 사실상 모든 실제 지원 플랫폼을 덮는다 — 예외적 타겟용 안전망).
    #[cfg(not(any(unix, windows)))]
    fn paint_confirm(&mut self, _ctx: PopupSetContextCtx) {}

    /// unix/windows 외에는 egui-mesh 채널이 비활성이라 파일열기 팝업 렌더는 no-op.
    #[cfg(not(any(unix, windows)))]
    fn paint_file_open(&mut self, _ctx: PopupSetContextCtx) {}
}

/// surface.create envelope 에서 `file` 을 꺼낸다. SDK 가 `ctx.params` 로 넘기는 것은
/// `{surface_id, kind, cwd, params:{file, ...}}` 전체이므로 `params.file` 을 본다(중첩).
/// 혹시 flat 으로 온 경우(`file` top-level)도 fallback 으로 받는다.
fn surface_param_file(envelope: &Value) -> Option<String> {
    envelope
        .get("params")
        .and_then(|p| p.get("file"))
        .or_else(|| envelope.get("file"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 파일이 대용량 임계값([`LARGE_FILE_LIMIT_BYTES`])을 *초과* 하면 그 크기(bytes)를 반환.
/// stat 실패/이하/경계값은 `None`(게이트 통과 — 즉시 로드). 경계값(정확히 limit)은 통과.
fn file_exceeds_limit(path: &str) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .map(|m| m.len())
        .filter(|&len| len > LARGE_FILE_LIMIT_BYTES)
}

/// 경로에서 표시용 파일명(basename)을 파생한다. 파생 실패 시 경로 그대로.
fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// bytes → "3.2 MB" 형태. 10MB 이상은 소수점 없이(host size_confirm 미러).
fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

/// host 에 팝업 인스턴스 닫기를 요청한다(셸 생명주기는 host 소유).
#[cfg(any(unix, windows))]
fn close_popup(host: &HostHandle, instance_id: u64) {
    if let Err(e) = host.call("popup.close", json!({ "instance_id": instance_id })) {
        tracing::warn!("markdown confirm popup close failed: {e}");
    }
}

/// 확인 팝업에서 사용자가 고른 결정.
#[cfg(any(unix, windows))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfirmChoice {
    /// 대용량이어도 읽어 렌더한다.
    Open,
    /// 열지 않는다(surface 대기 유지).
    Cancel,
}

/// large-file 확인 팝업 콘텐츠. 파일명 + 크기 태그 + 안내문 + [취소]/[열기]. 색·폰트·
/// 간격은 host 가 보낸 `Theme` 토큰에서만 가져온다. 셸(scrim/border/Esc)은 host 소유.
#[cfg(any(unix, windows))]
fn draw_confirm(
    ctx: &egui::Context,
    theme: &Theme,
    confirm: &LargeFileConfirm,
    tr: &Translator,
) -> Option<ConfirmChoice> {
    use tasty_ui_widgets::{Button, ButtonVariant, TagVariant, margin_all, tag, vspace};

    let frame = egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .inner_margin(margin_all(theme.spacing_md));
    let mut choice = None;
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

        // 제목.
        ui.label(
            egui::RichText::new(tr.t("markdown.large_file.title"))
                .size(theme.font_size_body.value())
                .strong()
                .color(theme.text_primary().to_egui()),
        );

        // 파일명 (mono, muted).
        ui.add(
            egui::Label::new(
                egui::RichText::new(&confirm.file_name)
                    .size(theme.font_size_caption.value())
                    .family(egui::FontFamily::Monospace)
                    .color(theme.text_muted().to_egui()),
            )
            .truncate(),
        );

        // 경고 태그(크기) + 안내문.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            tag(ui, theme, &confirm.size_label, TagVariant::Warning, false);
            ui.label(
                egui::RichText::new(tr.t("markdown.large_file.body"))
                    .size(theme.font_size_caption.value())
                    .color(theme.text_secondary().to_egui()),
            );
        });

        vspace(ui, theme.spacing_xs);

        // 푸터: 취소(ghost) / 열기(primary), 우측 정렬.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if Button::new(tr.t("markdown.large_file.open"))
                    .variant(ButtonVariant::Primary)
                    .show(ui, theme)
                    .clicked()
                {
                    choice = Some(ConfirmChoice::Open);
                }
                if Button::new(tr.t("markdown.large_file.cancel"))
                    .variant(ButtonVariant::Ghost)
                    .show(ui, theme)
                    .clicked()
                {
                    choice = Some(ConfirmChoice::Cancel);
                }
            });
        });
    });
    choice
}

/// 파일열기 팝업에서 사용자가 취한 동작.
#[cfg(any(unix, windows))]
enum FileOpenAction {
    /// 아무것도 안 함.
    None,
    /// native 파일 다이얼로그를 연다(host fs.pick_file).
    Browse,
    /// 입력 경로로 파일을 연다.
    Open,
    /// 열지 않고 닫는다.
    Cancel,
}

/// host → 이 plugin unicast 이벤트 key(ADR-0058). host 측 대응값은
/// `src/app/dispatch/file_picker.rs::FILE_PICKER_RESULT_EVENT` — 공유 crate 가
/// 없어 리터럴을 양쪽에 중복 정의한다(git-viewer 의 `GIT_VIEWER_QUERY_RESULT_EVENT`
/// 와 동일 근거).
const FILE_PICKER_RESULT_EVENT: &str = "file_picker.result";

/// `"file_picker.result"` 이벤트 payload wire — ADR-0058 Decision 4 의 최소 필드
/// (`request_id`/`paths`/`cancelled`) 그대로.
#[derive(serde::Deserialize)]
struct FilePickerResultWire {
    request_id: u64,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    cancelled: bool,
}

/// browse — host 소유 `file_picker` popup 을 트리거한다(`file_picker.trigger`,
/// ADR-0058). plugin 프로세스는 native OS 다이얼로그도, host 의 in-app
/// popup 도 직접 못 열기 때문에 host 에 위임한다. markdown 확장자로 필터. 반환값은
/// **선택 경로가 아니라** 이 요청의 `request_id` — 실제 경로는 나중에 `on_event`
/// 의 `"file_picker.result"` 로 비동기 도착한다(ADR-0058 의 즉시 ack + 이벤트 push,
/// 옛 `fs.pick_file`/rfd 동기 모달과 달리 이 호출 자체는 popup 확정을 기다리지 않고
/// 곧장 반환된다).
///
/// `owner_popup_instance` 로 자기 popup instance 를 함께 신고한다 — host 가 두 팝업을
/// 부모-자식 스택으로 다루는 근거다(ADR-0084).
#[cfg(any(unix, windows))]
fn trigger_file_picker(host: &HostHandle, owner_popup_instance: u64) -> Option<u64> {
    match host.call(
        "file_picker.trigger",
        json!({
            "filters": ["md", "markdown"],
            // 부모-자식 스택을 host 가 세울 수 있게 자기 popup instance 를 신고한다
            // (ADR-0084). 이게 없으면 이 팝업이 피커보다 먼저 닫혀 고아가 생기고,
            // 고른 파일이 조용히 버려진다.
            "owner_popup_instance": owner_popup_instance,
        }),
    ) {
        Ok(v) => v.get("request_id").and_then(Value::as_u64),
        Err(e) => {
            tracing::warn!("markdown file-open browse (file_picker.trigger) failed: {e}");
            None
        }
    }
}

/// 입력/선택한 markdown 파일을 host `file_handler.dispatch` 로 연다(origin 없이 → focused
/// pane 의 새 탭). markdown 감지·surface 생성은 host detector 가 담당한다.
#[cfg(any(unix, windows))]
fn open_markdown_file(host: &HostHandle, path: &str) {
    if let Err(e) = host.call(
        "file_handler.dispatch",
        json!({ "path": path, "depth": "deep" }),
    ) {
        tracing::warn!("markdown file-open dispatch failed: {e}");
    }
}

/// 파일열기 팝업 콘텐츠. 경로 입력 필드 + [browse] + [취소]/[열기]. 색·폰트·간격은 host 가
/// 보낸 `Theme` 토큰에서만 가져온다. 키보드/텍스트 입력은 host 가 popup raw_input 으로
/// forward 한다. 셸(scrim/border/Esc/outside-click)은 host 소유.
#[cfg(any(unix, windows))]
fn draw_file_open(
    ctx: &egui::Context,
    theme: &Theme,
    st: &mut FileOpenState,
    tr: &Translator,
) -> FileOpenAction {
    use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, margin_all, vspace};

    let frame = egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .inner_margin(margin_all(theme.spacing_md));
    let mut action = FileOpenAction::None;
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

        // Esc → 닫기(host 셸도 처리하나 forward 된 입력에서 즉시 반응).
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = FileOpenAction::Cancel;
        }

        // 제목.
        ui.label(
            egui::RichText::new(tr.t("markdown.file_open.title"))
                .size(theme.font_size_body.value())
                .strong()
                .color(theme.text_primary().to_egui()),
        );

        // 경로 라벨.
        ui.label(
            egui::RichText::new(tr.t("markdown.file_open.path_label"))
                .size(theme.font_size_caption.value())
                .color(theme.text_secondary().to_egui()),
        );

        // 경로 입력 + browse. right_to_left 로 Browse 를 먼저(우측) 배치해 실제 렌더
        // 폭(아이콘+라벨+패딩)만큼 자연히 소비시키고, 입력 필드는 남은
        // `ui.available_width()` 를 그대로 쓴다 — misc.rs/remote_transfer.rs 의 Browse
        // 선례와 동형이라 고정 spacing 상수를 빼는 수동 계산(클리핑 원인)이 필요 없다.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let browse_clicked = Button::new(tr.t("markdown.file_open.browse"))
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Sm)
                    .leading_icon(&|ui, rect, c| {
                        tasty_plugin_sdk::baked_icon::draw(
                            ui.painter(),
                            baked_icons::FOLDER,
                            rect.center(),
                            rect.width(),
                            c,
                        );
                    })
                    .show(ui, theme)
                    .clicked();

                let field_w = ui.available_width();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut st.path_input)
                        .desired_width(field_w)
                        .hint_text(tr.t("markdown.file_open.path_label"))
                        .font(egui::FontId::new(
                            theme.font_size_body.value(),
                            egui::FontFamily::Monospace,
                        )),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action = FileOpenAction::Open;
                }
                if browse_clicked {
                    action = FileOpenAction::Browse;
                }
            });
        });

        vspace(ui, theme.spacing_xs);

        // 푸터: 취소(ghost) / 열기(primary), 우측 정렬.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if Button::new(tr.t("markdown.file_open.open"))
                    .variant(ButtonVariant::Primary)
                    .show(ui, theme)
                    .clicked()
                {
                    action = FileOpenAction::Open;
                }
                if Button::new(tr.t("markdown.file_open.cancel"))
                    .variant(ButtonVariant::Ghost)
                    .show(ui, theme)
                    .clicked()
                {
                    action = FileOpenAction::Cancel;
                }
            });
        });
    });
    action
}

/// wire 스냅샷을 host 와 동일한 `Theme` 인스턴스로 재구성 (sizing 은 zoom 으로 재도출).
fn theme_from_wire(w: &ThemeWire) -> Theme {
    Theme::with_colors_and_zoom(w.colors.clone(), w.is_light, w.ui_zoom)
}

/// host `theme.query` IPC(webview.rs 참조)로 현재 Theme 을 동기 조회한다. webview-kind
/// surface 는 `surface.set_context` 를 받지 않아(egui-mesh 와 달리 host 가 mesh 프레임을
/// 합성하지 않으므로) 이 조회가 유일한 Theme 획득 경로다. 실패하면 `None` — 호출자는
/// 문서 재생성을 건너뛴다(다음 성공한 조회가 갱신할 때까지 이전 내용 유지).
fn fetch_theme(host: &HostHandle) -> Option<Theme> {
    match host.call("theme.query", json!({})) {
        Ok(v) => match serde_json::from_value::<ThemeWire>(v) {
            Ok(wire) => Some(theme_from_wire(&wire)),
            Err(e) => {
                tracing::warn!("markdown: malformed theme.query response: {e}");
                None
            }
        },
        Err(e) => {
            tracing::warn!("markdown: theme.query failed: {e}");
            None
        }
    }
}

/// 편집 진입 시 host 의 generic `recent.query {kind}` 로 최근목록을 조회한다 — 최신순
/// 최대 10개 경로. host 는 "markdown" 을 모르므로 plugin 이 kind 를 채운다. 문서 생성
/// 시점에 baked-in 되어 주소창 `<datalist>` 후보가 된다(render.rs 모듈 문서 — 웹뷰
/// surface 엔 JS↔plugin 메시지 브리지가 없어 반응형 fetch 를 할 수 없다). 실패하면
/// 빈 목록으로 폴백한다.
fn fetch_recent(host: &HostHandle) -> Vec<String> {
    match host.call("recent.query", json!({ "kind": "markdown" })) {
        Ok(v) => parse_recent(&v),
        Err(e) => {
            tracing::warn!("recent.query fetch failed: {e}");
            Vec::new()
        }
    }
}

/// `recent.query` 응답(`{ "recent": [{ path, file_name }] }`)에서 경로 목록을 추출한다
/// (순수 — 단위테스트로 격리). 응답 형태가 어긋나면 빈 목록.
fn parse_recent(v: &Value) -> Vec<String> {
    v.get("recent")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("path").and_then(|p| p.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 링크 클릭 부수효과. `webview.navigation_attempt` 로 도착한, 이 plugin 이 생성한
/// `#tasty-nav:link:` fragment 에서만 도달한다(module doc). 파일은 host
/// `file_handler.dispatch`(같은 Pane 새 탭, origin_surface_id)로, 외부 URL 은 OS
/// 핸들러로 연다.
fn dispatch_link(host: &HostHandle, sid: u32, click: render::LinkClick) {
    match click {
        render::LinkClick::File(path) => dispatch_file_link(host, sid, &path),
        render::LinkClick::External(url) => dispatch_external_link(&url),
    }
}

fn dispatch_file_link(host: &HostHandle, sid: u32, path: &std::path::Path) {
    if !path.exists() {
        tracing::debug!("markdown link target does not exist: {}", path.display());
        return;
    }
    let params = json!({
        "path": path.to_string_lossy(),
        "depth": "deep",
        "origin_surface_id": sid,
    });
    if let Err(e) = host.call("file_handler.dispatch", params) {
        tracing::warn!("markdown link file dispatch failed: {e}");
    }
}

fn dispatch_external_link(url: &str) {
    if let Err(e) = webbrowser::open(url) {
        tracing::warn!("markdown external link open failed ({url}): {e}");
    }
}

/// 주소창 확정 이동을 host `markdown.navigate`(04) 로 보낸다 — 같은 surface 제자리 이동.
fn navigate(host: &HostHandle, sid: u32, path: &str) {
    let path = path.trim();
    if path.is_empty() {
        return;
    }
    let params = json!({ "surface_id": sid, "path": path });
    if let Err(e) = host.call("markdown.navigate", params) {
        tracing::warn!("markdown navigate failed: {e}");
    }
}

/// plugin Context 에 CJK fallback 을 설치한다(확인 팝업 전용 — 본문은 native WebView 가
/// 자체 폰트 스택으로 그린다). egui 기본 폰트(Proportional/Monospace) 뒤에 시스템 CJK
/// 폰트를 fallback 으로 붙여 한글/일문/한자가 tofu 되지 않게 한다.
#[cfg(any(unix, windows))]
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = load_system_cjk_font_data() {
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(fam)
                .or_default()
                .push("system_cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

/// 시스템 CJK 폰트 바이트 로드 (host `font_registry::load_system_cjk_font_data` 미러).
#[cfg(any(unix, windows))]
fn load_system_cjk_font_data() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        // host font_registry 미러 — 맑은 고딕(한글 tofu 방지). 없으면 None.
        if let Ok(data) = std::fs::read("C:/Windows/Fonts/malgun.ttf") {
            return Some(data);
        }
    }
    #[cfg(target_os = "macos")]
    {
        for path in &[
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for path in &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }
    None
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let env = PluginEnv::load()?;
    let tr = Translator::from_plugin_env(&env);
    tasty_plugin_sdk::run(MarkdownPlugin::new(tr))
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests;
