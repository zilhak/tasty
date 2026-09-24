#![forbid(unsafe_code)]

//! Markdown 파일을 HTML로 렌더해 호스트 WebView에 표시한다.
//! 파일 감시는 SDK의 file_watch를 사용하고, 링크 열기는 호스트에 요청한다.
//! 대용량 파일 확인과 파일 열기 팝업은 egui-mesh로 그린다.
//! 상세: docs/plugins/markdown/index.md#내부-동작.

// 테스트의 let _ = 사용은 제품 코드의 오류 무시 목록에서 제외한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod popup;
mod render;

/// 팝업 아이콘의 빌드 시점 SVG 변환 결과. 0..24 좌표의 경로를
/// baked_icon::draw에서 크기에 맞춰 그린다.
mod baked_icons {
    include!(concat!(env!("OUT_DIR"), "/plugin_icons.rs"));
}

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::file_watch::{self, ContentDigest, WatchCmd};
use tasty_plugin_sdk::{
    BusHandle, EventDispatchCtx, EventScope, HostHandle, IpcMethodCtx, IpcMethodError, Plugin,
    PluginEnv, PopupClosedCtx, PopupOpenCtx, PopupOpenResult, PopupSetContextCtx, SurfaceCreateCtx,
    SurfaceRestoreCtx, SurfaceResult, Translator, WebviewNavigationAttemptCtx,
};
use tasty_type_appearance::theme::Theme;

use popup::{
    FILE_PICKER_RESULT_EVENT, FileOpenState, FilePickerResultWire, LargeFileConfirm, PickerStart,
    basename, format_size,
};
#[cfg(any(unix, windows))]
use tasty_plugin_sdk::EguiMeshPopup;

const PLUGIN_ID: &str = "com.tasty.markdown";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 로컬 파일을 열 때 확인 팝업을 띄우는 기준(1 MiB 초과).
/// 전체 읽기 크기를 제한하는 상수는 아니다.
const LARGE_FILE_LIMIT_BYTES: u64 = 1024 * 1024;

/// 대용량 감지 시 plugin 이 발행하는 이벤트 key. 매니페스트 `event_publish` 패턴 +
/// `[[contributes.popup]]` event trigger 가 이 key 로 매칭돼 확인 팝업을 연다.
const LARGE_FILE_EVENT_KEY: &str = "com.tasty.markdown.large_file_confirm";

/// host 가 발행하는 전역 테마 변경 이벤트 key(`event_subscribe`). webview-kind surface 는
/// `surface.set_context` 를 받지 않아 Theme 변경이 자동으로 밀리지 않으므로, 이 이벤트를
/// 받을 때마다 살아있는 모든 문서를 재생성한다.
const THEME_CHANGED_EVENT: &str = "theme.changed";

/// attach mirror 문서의 원문 조회를 host 에 거는 메서드. host 는 `request_id` 만 즉시
/// 돌려주고 원문은 [`MIRROR_CONTENT_RESULT_EVENT`] 로 나중에 온다
/// (`docs/dev-guide/attach-behavior.md#markdown-content-채널`).
const MIRROR_CONTENT_REQUEST_METHOD: &str = "markdown_mirror.content_request";

/// 원문 요청의 출처. 에이전트 요청에는 원문 잘림 토스트를 띄우지 않는다.
/// docs/design/systems/toast.md#origin이-적용되는-경로.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteRequester {
    /// 최초 열기 · 원격 변경 신호 재조회 · 새로고침 버튼 — 이 plugin 이 스스로 또는 사용자
    /// 클릭으로 건 요청.
    Plugin,
    /// `markdown.reload` IPC — 바깥 호출자(에이전트)가 건 요청.
    Agent,
}

/// 에이전트가 요청한 원문 조회에만 agent_origin: true를 넣는다.
fn mirror_content_request_params(surface_id: u32, requester: RemoteRequester) -> Value {
    match requester {
        RemoteRequester::Plugin => json!({ "surface_id": surface_id }),
        RemoteRequester::Agent => json!({ "surface_id": surface_id, "agent_origin": true }),
    }
}

/// host 가 이 plugin 에 unicast 하는 원문 조회 회신 이벤트.
const MIRROR_CONTENT_RESULT_EVENT: &str = "markdown_mirror.content_result";

/// 원격 변경 통지. 정상 문서는 stale 표시를 켜고, 끊김·실패 상태는 다시 조회한다.
const MIRROR_CHANGED_EVENT: &str = "markdown_mirror.changed";

/// host 가 대기 중인 요청을 버리라고 알릴 때 쓰는 `request_id`. host 는 이 값을 발급하지
/// 않는다(attach 연결이 끊겨 회신이 영영 안 올 때 보낸다).
const MIRROR_ABANDON_REQUEST_ID: u64 = 0;

/// Surface별 문서 내용, 읽기 결과와 상대경로 기준 디렉터리.
/// 다시 읽을 시점은 SDK의 파일 감시가 결정한다.
struct MdDoc {
    file_path: Option<String>,
    base_dir: Option<PathBuf>,
    content: String,
    load_error: Option<String>,
    /// 대용량 확인 대기 중이면 true — 파일을 아직 읽지 않았다(빈 콘텐츠). 확인 팝업의
    /// [열기] 확정 시 [`MdDoc::resume_load`] 가 실제 read 를 재개한다.
    pending_large: bool,
    /// 원격 문서 상태. file_path는 표시용 원격 경로이며 로컬에서 읽지 않는다.
    remote: Option<RemoteDoc>,
}

/// attach mirror 문서의 원격 조회 상태.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RemoteDoc {
    /// 회신을 기다리는 요청. 새 요청이 나가면 덮어써져 늦게 온 옛 회신은 버려진다.
    pending: Option<u64>,
    /// 원문을 한 번이라도 받았는가 — 받기 전에는 본문 자리에 로딩 상태를 그린다.
    loaded: bool,
    /// 받은 뒤 원격 파일이 바뀌었다는 신호가 왔는가.
    stale: bool,
    /// 연결 종료 통지 후 아직 원문을 다시 받지 못했는가.
    /// 기존 원문은 보관하되 화면에는 연결이 끊겼음을 표시한다.
    disconnected: bool,
}

/// [`MdDoc::on_remote_changed`] 의 결정 — 변경 신호 하나에 plugin 이 할 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteChange {
    /// 할 일이 없다(로컬 문서 · 이미 stale · 이미 받는 중).
    Ignore,
    /// stale 표시를 켰다 — 다시 그린다.
    Redraw,
    /// 원문을 보여 주지 못하는 문서다 — 다시 요청한다.
    Refetch,
}

/// `markdown_mirror.content_result` 페이로드. host 가 surface_id 를 로컬 id 로 바꿔 보낸다.
#[derive(Debug, serde::Deserialize)]
struct MirrorContentResultWire {
    surface_id: u32,
    request_id: u64,
    ok: bool,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    reason: Option<String>,
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
            remote: None,
        }
    }

    /// attach mirror 문서 — 파일을 읽지 않고 `base_dir` 도 두지 않는다. 상대경로 이미지·
    /// 링크가 이 머신의 파일로 풀리면 원격 문서가 로컬 파일을 끌어다 쓰게 된다.
    fn new_remote(file: String) -> Self {
        Self {
            file_path: (!file.is_empty()).then_some(file),
            base_dir: None,
            content: String::new(),
            load_error: None,
            pending_large: false,
            remote: Some(RemoteDoc::default()),
        }
    }

    /// 원문 조회 회신을 반영한다. 다시 그려야 하면 true.
    ///
    /// 대기 중인 요청과 id 가 다르면 버린다(새로고침을 연달아 눌러 옛 회신이 늦게 온 경우).
    /// abandon id 는 연결이 끊겼다는 통지다 — 기다리던 요청을 끝내고, 원문을 이미 받은
    /// 문서도 끊김 상태로 둔다. 받은 원문은 지우지 않는다(다시 받으면 그대로 갈아끼운다).
    fn apply_remote_result(&mut self, reply: MirrorContentResultWire) -> bool {
        let Some(remote) = self.remote.as_mut() else {
            return false;
        };
        if reply.request_id == MIRROR_ABANDON_REQUEST_ID {
            remote.pending = None;
            let changed = !remote.disconnected;
            remote.disconnected = true;
            return changed;
        }
        let Some(pending) = remote.pending else {
            return false;
        };
        if reply.request_id != pending {
            return false;
        }
        remote.pending = None;
        if reply.ok {
            self.content = reply.source.unwrap_or_default();
            self.load_error = None;
            remote.loaded = true;
            remote.stale = false;
            remote.disconnected = false;
        } else {
            self.load_error = Some(reply.reason.unwrap_or_default());
        }
        true
    }

    /// 원격 파일 변경에 반응한다. 읽던 위치를 보호하기 위해 정상 문서는 stale 표시만 켠다.
    /// 이미 stale이면 다시 그리지 않는다. 끊김·실패 상태는 원문을 다시 요청하고,
    /// 요청 중이면 기존 응답을 기다린다.
    /// docs/dev-guide/attach-behavior.md#markdown-content-채널.
    fn on_remote_changed(&mut self) -> RemoteChange {
        let showing_error = self.load_error.is_some();
        let Some(remote) = self.remote.as_mut() else {
            return RemoteChange::Ignore;
        };
        if remote.loaded && !remote.disconnected && !showing_error {
            if remote.stale {
                return RemoteChange::Ignore;
            }
            remote.stale = true;
            return RemoteChange::Redraw;
        }
        if remote.pending.is_some() {
            return RemoteChange::Ignore;
        }
        RemoteChange::Refetch
    }

    fn remote_view(&self) -> Option<render::RemoteView> {
        self.remote.as_ref().map(|r| render::RemoteView {
            loading: !r.loaded,
            stale: r.stale,
            disconnected: r.disconnected,
        })
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
            remote: None,
        }
    }

    /// 대용량 확인 [열기] 후 실제 read 를 재개한다.
    fn resume_load(&mut self) {
        self.pending_large = false;
        if let Some(f) = self.file_path.clone() {
            self.read_now(&f);
        }
    }

    /// 로컬 파일을 다시 읽는다. IPC와 SDK 파일 감시에서 호출한다.
    fn force_reload(&mut self) {
        // mirror 문서의 경로는 원격 호스트의 것이다 — 로컬에서 읽으면 엉뚱한 파일이 뜬다.
        if self.remote.is_some() {
            return;
        }
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

struct MarkdownPlugin {
    /// surface_id → markdown document state.
    docs: HashMap<u32, MdDoc>,
    /// large-file 이벤트 발행용 Event Bus 핸들(`on_start` 에서 저장).
    bus: Option<BusHandle>,
    /// HostHandle을 받지 않는 surface·이벤트 콜백에서 사용할 핸들.
    host: Option<HostHandle>,
    /// popup instance_id → 대용량 확인 대상.
    confirm: HashMap<u64, LargeFileConfirm>,
    /// popup instance_id → 파일열기 팝업 상태(경로 입력 버퍼).
    file_open: HashMap<u64, FileOpenState>,
    /// `file_picker.trigger`(docs/dev-guide/popup-implementation.md#플러그인이-호스트-팝업-결과를-기다릴-때) 로 보낸 요청의 `request_id` → 그
    /// 요청을 낸 파일열기 팝업 instance_id. `"file_picker.result"` 이벤트 수신 시
    /// 이 맵으로 상관관계를 맞춰 `path_input` 을 채운다.
    pending_file_picker: HashMap<u64, u64>,
    /// Popup 인스턴스별 egui-mesh 렌더 상태(폰트 atlas와 공유 버퍼).
    #[cfg(any(unix, windows))]
    popups: HashMap<u64, EguiMeshPopup>,
    /// CJK fallback 폰트를 이미 설치한 popup instance_id — set_fonts 재업로드 방지.
    #[cfg(any(unix, windows))]
    popup_fonts_installed: std::collections::HashSet<u64>,
    /// plugin lang 카탈로그 (state.failed / state.empty / addr.* 등 UI 문자열).
    tr: Translator,
    /// 파일 감시 워커에 등록·해제 명령을 보내는 채널. on_start에서 생성한다.
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
        // HostHandle이 없는 콜백에서도 사용할 수 있도록 보관한다.
        self.host = Some(host.clone());

        // 파일 감시 워커는 ContentDigest로 파일을 읽어 변경 여부를 확인한다.
        // 변경되면 markdown.reload를 호출해 문서를 다시 읽고 렌더한다.
        // WebView는 paint 콜백을 받지 않으므로 별도 워커에서 감시한다.
        let (tx, rx) = mpsc::channel();
        self.watch_tx = Some(tx);
        if let Err(e) = std::thread::Builder::new()
            .name("markdown-watch".to_string())
            .spawn(move || file_watch::run::<ContentDigest>(host, rx, "markdown.reload"))
        {
            tracing::warn!("markdown watch worker spawn failed — idle auto-reload disabled: {e}");
        }
    }

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        // SDK가 전달하는 envelope의 params 안에 실제 생성 인자가 있다.
        if let Some(remote_file) = surface_param_remote_file(&ctx.params) {
            return self.open_remote_surface(ctx.surface_id, remote_file);
        }
        let file = surface_param_file(&ctx.params);
        self.open_file_surface(ctx.surface_id, file)
    }

    // 저장된 snapshot의 file을 사용해 같은 문서를 복원한다.
    fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
        // 미러 문서 복원 데이터는 {display_name, remote: {file}} 형식이다.
        if let Some(remote_file) = remote_file_of(&ctx.data) {
            let mut result = self.open_remote_surface(ctx.surface_id, remote_file);
            result.display_name = ctx
                .data
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            return result;
        }
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
            // 이 네임스페이스의 외부 요청은 플러그인에 먼저 오므로 호스트 구현에 다시 전달한다.
            "markdown.navigate" => Ok(ctx.host.call(&ctx.method, ctx.params)?),
            // 최근 파일은 호스트가 관리하므로 kind를 지정해 조회한다.
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
                        picker_start: PickerStart::from_context(&ctx.context),
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

    /// 파일 피커 결과, 테마 변경과 미러 문서 이벤트를 처리한다.
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
            // WebView에는 surface.set_context가 오지 않으므로 테마 변경 시 문서를 다시 만든다.
            THEME_CHANGED_EVENT => self.reload_all_webviews(),
            MIRROR_CONTENT_RESULT_EVENT => self.on_mirror_content_result(ctx.envelope.payload),
            MIRROR_CHANGED_EVENT => self.on_mirror_changed(&ctx.envelope.payload),
            _ => {}
        }
    }

    /// WebView 이동 요청에서 render.rs가 사용하는 #tasty-nav 마커를 해석한다.
    /// 마커가 없는 요청은 무시한다. 사용자 동작인지는 호스트가 별도로 판단한다.
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
        let is_remote = self
            .docs
            .get(&ctx.surface_id)
            .is_some_and(|d| d.remote.is_some());
        match intent {
            render::NavIntent::Refresh if is_remote => {
                self.request_remote_content(ctx.surface_id, RemoteRequester::Plugin)
            }
            // 버튼은 mirror 문서에만 그려진다 — 로컬 문서에 온 것은 이 plugin 이 낸 것이 아니다.
            render::NavIntent::Refresh => {}
            // mirror 문서의 파일 링크와 주소창 경로는 원격 호스트의 것이라 이 머신에서 열 수
            // 없다. 외부 URL 만 연다.
            render::NavIntent::Link(dest) if is_remote => {
                if let Some(render::LinkClick::External(url)) = render::classify_link(&dest, None) {
                    dispatch_external_link(&host, ctx.surface_id, &url);
                }
            }
            render::NavIntent::Addr(_) if is_remote => {}
            render::NavIntent::Link(dest) => {
                let base_dir = self
                    .docs
                    .get(&ctx.surface_id)
                    .and_then(|d| d.base_dir.clone());
                if let Some(click) = render::classify_link(&dest, base_dir.as_deref()) {
                    dispatch_link(&host, ctx.surface_id, &ctx.url, click);
                }
            }
            render::NavIntent::Addr(path) => navigate(&host, ctx.surface_id, &path),
        }
    }
}

impl MarkdownPlugin {
    /// `markdown_mirror.content_result` — 대기 중인 요청의 회신이면 문서에 반영하고 다시 그린다.
    fn on_mirror_content_result(&mut self, payload: Value) {
        let Ok(reply) = serde_json::from_value::<MirrorContentResultWire>(payload) else {
            tracing::warn!("markdown: malformed {MIRROR_CONTENT_RESULT_EVENT} event");
            return;
        };
        let surface_id = reply.surface_id;
        if self
            .docs
            .get_mut(&surface_id)
            .is_some_and(|doc| doc.apply_remote_result(reply))
        {
            self.reload_webview(surface_id);
        }
    }

    /// `markdown_mirror.changed` — 원문을 보여 주는 문서는 stale 표시만 켜고, 끊김·실패를
    /// 보여 주는 문서는 다시 요청한다([`MdDoc::on_remote_changed`]).
    fn on_mirror_changed(&mut self, payload: &Value) {
        let Some(surface_id) = payload
            .get("surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
        else {
            tracing::warn!("markdown: malformed {MIRROR_CHANGED_EVENT} event");
            return;
        };
        let change = self
            .docs
            .get_mut(&surface_id)
            .map_or(RemoteChange::Ignore, MdDoc::on_remote_changed);
        match change {
            RemoteChange::Ignore => {}
            RemoteChange::Redraw => self.reload_webview(surface_id),
            RemoteChange::Refetch => {
                self.request_remote_content(surface_id, RemoteRequester::Plugin)
            }
        }
    }

    fn markdown_reload(&mut self, params: &Value) -> Result<Value, IpcMethodError> {
        let surface_id = params
            .get("surface")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| IpcMethodError::invalid_params("missing 'surface'"))?
            as u32;
        // 원격 문서는 파일 감시에 등록하지 않는다. 이 IPC에서 시작한 원문 조회에는
        // 에이전트 출처를 표시해 사용자 토스트를 띄우지 않도록 한다.
        if self
            .docs
            .get(&surface_id)
            .is_some_and(|d| d.remote.is_some())
        {
            self.request_remote_content(surface_id, RemoteRequester::Agent);
            return Ok(json!({ "ok": true, "surface_id": surface_id }));
        }
        if let Some(doc) = self.docs.get_mut(&surface_id) {
            doc.force_reload();
        }
        self.reload_webview(surface_id);
        Ok(json!({ "ok": true, "surface_id": surface_id }))
    }

    /// 로컬 파일을 열고 경로를 snapshot에 저장한다. 생성과 복원에서 함께 사용한다.
    /// 크기가 기준을 초과하면 읽기를 보류하고 확인 이벤트를 보낸다.
    /// 이벤트 버스가 없으면 확인 없이 읽는다. 파일 경로가 없으면 snapshot도 반환하지 않는다.
    fn open_file_surface(&mut self, surface_id: u32, file: Option<String>) -> SurfaceResult {
        let doc = self.make_doc(file.clone(), surface_id);
        self.docs.insert(surface_id, doc);
        // 같은 surface의 경로가 바뀌면 감시 등록도 갱신한다.
        self.watch_register(surface_id, file.clone());
        // 문서를 HTML 로 렌더해 host WebView 에 싣는다 — 이 kind 는 mesh 를 그리지 않는다.
        self.reload_webview(surface_id);
        SurfaceResult {
            display_name: None,
            snapshot: file.as_ref().map(|f| json!({ "file": f })),
        }
    }

    /// attach mirror 문서를 연다. 파일 감시도 snapshot 도 없다 — 원문의 주인은 원격이고,
    /// mirror 워크스페이스는 저장되지 않는다. 원문 요청을 먼저 걸고 로딩 상태를 그린다.
    fn open_remote_surface(&mut self, surface_id: u32, file: String) -> SurfaceResult {
        self.docs.insert(surface_id, MdDoc::new_remote(file));
        self.request_remote_content(surface_id, RemoteRequester::Plugin);
        SurfaceResult {
            display_name: None,
            snapshot: None,
        }
    }

    /// mirror 문서의 원문을 host 에 요청하고 다시 그린다. 호출이 실패하면 그 사유를 문서의
    /// 실패 상태로 둔다 — 회신이 올 길이 없으므로 로딩 상태로 남기지 않는다.
    fn request_remote_content(&mut self, surface_id: u32, requester: RemoteRequester) {
        let Some(host) = self.host.clone() else {
            tracing::warn!(
                "markdown surface {surface_id}: no host handle yet — cannot request remote content"
            );
            return;
        };
        let outcome = host
            .call(
                MIRROR_CONTENT_REQUEST_METHOD,
                mirror_content_request_params(surface_id, requester),
            )
            .map_err(|e| e.to_string())
            .and_then(|v| {
                v.get("request_id")
                    .and_then(|id| id.as_u64())
                    .ok_or_else(|| format!("{MIRROR_CONTENT_REQUEST_METHOD}: no request_id"))
            });
        let Some(doc) = self.docs.get_mut(&surface_id) else {
            return;
        };
        let Some(remote) = doc.remote.as_mut() else {
            return;
        };
        match outcome {
            Ok(request_id) => remote.pending = Some(request_id),
            Err(e) => {
                tracing::warn!("markdown surface {surface_id}: remote content request failed: {e}");
                remote.pending = None;
                doc.load_error = Some(e);
            }
        }
        self.reload_webview(surface_id);
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

    /// 파일 감시 대상을 등록하거나 갱신한다. 워커 생성에 실패했다면 자동 갱신은
    /// 동작하지 않으며 markdown.reload를 호출하거나 파일을 다시 열어야 한다.
    fn watch_register(&self, surface_id: u32, path: Option<String>) {
        let Some(tx) = &self.watch_tx else { return };
        if tx.send(WatchCmd::Register { surface_id, path }).is_err() {
            tracing::warn!(
                "markdown watch: register send failed for surface {surface_id} (worker gone)"
            );
        }
    }

    /// 파일 감시 대상에서 surface를 해제한다.
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
            tracing::warn!(
                "markdown surface {surface_id}: no document registered — nothing to load"
            );
            return;
        };
        // 테마 조회 실패는 fetch_theme에서 기록한다. 새 HTML을 보내지 않아 이전 표시가 남는다.
        let Some(theme) = fetch_theme(host, surface_id) else {
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
            remote: doc.remote_view(),
        });
        push_html(host, surface_id, file_path, html);
    }

    /// 살아있는 모든 markdown 문서를 재렌더한다(`theme.changed` 수신 시).
    fn reload_all_webviews(&self) {
        for &surface_id in self.docs.keys() {
            self.reload_webview(surface_id);
        }
    }
}

/// surface.create envelope 에서 attach mirror 문서의 원격 경로를 꺼낸다(`params.remote.file`).
/// `remote` 객체가 있으면 mirror 문서다 — 경로가 비어 있어도(원격이 파일 없이 연 문서) 그렇다.
/// 최상위 `file` 과 자리를 나눈 것은 그 키가 로컬 읽기·감시·cwd 도출을 모두 켜기 때문이다.
fn surface_param_remote_file(envelope: &Value) -> Option<String> {
    remote_file_of(envelope.get("params")?)
}

/// 생성 params(또는 restore data) 객체에서 mirror 문서의 원격 경로를 꺼낸다(`remote.file`).
fn remote_file_of(params: &Value) -> Option<String> {
    let remote = params.get("remote")?.as_object()?;
    Some(
        remote
            .get("file")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    )
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

/// wire 스냅샷을 host 와 동일한 `Theme` 인스턴스로 재구성 (sizing 은 zoom 으로 재도출).
fn theme_from_wire(w: &ThemeWire) -> Theme {
    Theme::with_colors_and_zoom(w.colors.clone(), w.is_light, w.ui_zoom)
}

/// 렌더한 HTML을 호스트 WebView에 보내고 결과를 기록한다.
fn push_html(host: &HostHandle, surface_id: u32, file_path: &str, html: String) {
    let html_len = html.len();
    if let Err(e) = host.call(
        "webview.set_url",
        json!({ "surface_id": surface_id, "url": html }),
    ) {
        tracing::warn!("markdown surface {surface_id}: webview.set_url failed: {e}");
    } else {
        tracing::info!(
            "markdown surface {surface_id}: loaded {html_len} bytes of HTML (file={file_path})"
        );
    }
}

/// theme.query로 현재 테마를 조회한다. WebView surface는 set_context로 테마를 받지 않는다.
/// 실패하면 None을 반환하고 해당 surface를 로그에 남긴다. 호출자는 갱신을 건너뛴다.
fn fetch_theme(host: &HostHandle, surface_id: u32) -> Option<Theme> {
    match host.call("theme.query", json!({})) {
        Ok(v) => match serde_json::from_value::<ThemeWire>(v) {
            Ok(wire) => Some(theme_from_wire(&wire)),
            Err(e) => {
                tracing::warn!(
                    "markdown surface {surface_id}: malformed theme.query response: {e} — \
                     skipping webview load"
                );
                None
            }
        },
        Err(e) => {
            tracing::warn!(
                "markdown surface {surface_id}: theme.query failed: {e} — skipping webview load"
            );
            None
        }
    }
}

/// 호스트의 recent.query에서 최신 경로를 최대 10개 조회한다.
/// 문서를 생성할 때 주소창 후보에 넣으며, 조회에 실패하면 빈 목록을 쓴다.
fn fetch_recent(host: &HostHandle) -> Vec<String> {
    match host.call("recent.query", json!({ "kind": "markdown" })) {
        Ok(v) => parse_recent(&v),
        Err(e) => {
            tracing::warn!("recent.query fetch failed: {e}");
            Vec::new()
        }
    }
}

/// recent.query 응답의 recent 배열에서 경로를 꺼낸다. 배열이 없으면 빈 목록을 반환한다.
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

/// 문서 링크를 호스트에 전달한다. 파일은 같은 Pane의 새 탭으로 열도록 요청하고,
/// 외부 URL은 OS의 기본 앱으로 열도록 요청한다.
/// nav_url은 사용자 동작 판정을 위해 호스트가 보낸 값을 그대로 돌려준다.
fn dispatch_link(host: &HostHandle, sid: u32, nav_url: &str, click: render::LinkClick) {
    match click {
        render::LinkClick::File(path) => dispatch_file_link(host, sid, nav_url, &path),
        render::LinkClick::External(url) => dispatch_external_link(host, sid, &url),
    }
}

fn dispatch_file_link(host: &HostHandle, sid: u32, nav_url: &str, path: &std::path::Path) {
    if !path.exists() {
        tracing::debug!("markdown link target does not exist: {}", path.display());
        return;
    }
    if let Err(e) = host.call(
        "file_handler.dispatch",
        file_link_params(sid, nav_url, path),
    ) {
        tracing::warn!("markdown link file dispatch failed: {e}");
    }
}

/// 파일 링크 열기 요청. user_navigation_url을 호스트에 돌려주면 호스트가
/// 페이지 소유자와 엔진의 사용자 제스처 정보를 확인한다. macOS처럼 해당 정보를
/// 제공하지 않거나 확인에 실패하면 에이전트 요청으로 처리해 기존 선택을 유지한다.
/// docs/features/file-handler/index.md#origin-소유권과-비동기-완료.
fn file_link_params(sid: u32, nav_url: &str, path: &std::path::Path) -> Value {
    json!({
        "path": path.to_string_lossy(),
        "depth": "deep",
        "origin_surface_id": sid,
        "user_navigation_url": nav_url,
    })
}

/// 외부 URL 열기를 호스트에 요청한다. 호스트는 surface 소유자를 확인한다.
/// OS 열기 기록: docs/dev-guide/self-verification.md#os-열기와-지연-주입.
fn dispatch_external_link(host: &HostHandle, sid: u32, url: &str) {
    match host.call("webview.open_external", external_link_params(sid, url)) {
        Ok(v) if v.get("opened").and_then(Value::as_bool) == Some(false) => {
            tracing::warn!("markdown external link was not opened by the OS ({url})");
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("markdown external link open failed ({url}): {e}"),
    }
}

/// `webview.open_external` 파라미터. 호출(`HostHandle`)과 떼어 두어 단위 테스트가 wire 모양을 본다.
fn external_link_params(sid: u32, url: &str) -> Value {
    json!({ "surface_id": sid, "url": url })
}

/// 주소창 확정 이동을 host `markdown.navigate` 로 보낸다 — 같은 surface 제자리 이동.
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
// 테스트의 let _ = 사용은 제품 코드의 오류 무시 목록에서 제외한다.
#[allow(clippy::let_underscore_must_use)]
mod tests;
