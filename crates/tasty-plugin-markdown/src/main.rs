#![forbid(unsafe_code)]

//! Tasty markdown plugin — **egui-mesh** markdown viewer surface (ADR-0028 / B1).
//!
//! The plugin owns the markdown content and renders it in its own process: it reads the
//! `.md` file (delivered via `surface.create`), parses + lays it out with egui from the
//! host `Theme` tokens (delivered each frame via `set_context`), tessellates the mesh, and
//! the host composites it over the surface region. Link clicks (forwarded real user input)
//! are routed back to the host (`file_handler.dispatch` for files) or the OS (URLs).
//!
//! Codec/SDK are not reimplemented — only [`EguiMeshSurface`] is called. The former
//! host-side `MarkdownView` render path stays compiled until C1 removes it.

mod render;
mod watch;

/// 빌드타임 SVG 베이크 산출물 (방식 B). `build.rs` 가 `tasty-icons` 의 canonical
/// `<svg>` 를 usvg 로 파싱·평탄화해 `pub const <NAME>: &[&[[f32; 2]]]`(viewBox 0..24
/// 좌표)를 생성한다. 런타임은 이 점배열을 [`tasty_plugin_sdk::baked_icon::draw`] 로
/// 그릴 크기에 스케일해 벡터 stroke 로 그린다(텍스처 없음, DPI 독립).
mod baked_icons {
    include!(concat!(env!("OUT_DIR"), "/plugin_icons.rs"));
}

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Instant, SystemTime};

use render::{LinkClick, MdCache};
use serde_json::{Value, json};
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::{
    BusHandle, EventScope, IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, PopupClosedCtx,
    PopupOpenCtx, PopupOpenResult, PopupSetContextCtx, SurfaceCreateCtx, SurfaceResult,
    SurfaceSetContextCtx, Translator,
};
use tasty_type_appearance::theme::Theme;
use watch::WatchCmd;

#[cfg(any(unix, windows))]
use tasty_plugin_sdk::EguiMeshPopup;
#[cfg(any(unix, windows))]
use tasty_plugin_sdk::EguiMeshSurface;
use tasty_plugin_sdk::HostHandle;
use tasty_ui_widgets::{
    Button, ButtonVariant, PathField, PathFieldOutcome, TagVariant, margin_all, tag, vspace,
};

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

/// Per-surface markdown document state owned by the plugin (content, load outcome, base
/// dir for relative paths, mtime reload tracking). Mirrors the former host `MarkdownView`
/// + `MarkdownPanel::poll_reload`, now living in the plugin process.
struct MdDoc {
    file_path: Option<String>,
    base_dir: Option<PathBuf>,
    content: String,
    load_error: Option<String>,
    last_mtime: Option<SystemTime>,
    last_check: Instant,
    /// 대용량 확인 대기 중이면 true — 파일을 아직 읽지 않았다(빈 콘텐츠). 확인 팝업의
    /// [열기] 확정 시 [`MdDoc::resume_load`] 가 실제 read 를 재개한다. 이 동안 poll_reload
    /// 도 읽지 않는다.
    pending_large: bool,
}

impl MdDoc {
    fn new(file: Option<String>) -> Self {
        let base_dir = file
            .as_ref()
            .and_then(|f| PathBuf::from(f).parent().map(|p| p.to_path_buf()));
        let (content, load_error, last_mtime) = match &file {
            Some(f) => match std::fs::read_to_string(f) {
                Ok(text) => (
                    text,
                    None,
                    std::fs::metadata(f).and_then(|m| m.modified()).ok(),
                ),
                Err(e) => (String::new(), Some(e.to_string()), None),
            },
            None => (String::new(), None, None),
        };
        Self {
            file_path: file,
            base_dir,
            content,
            load_error,
            last_mtime,
            last_check: Instant::now(),
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
            last_mtime: None,
            last_check: Instant::now(),
            pending_large: true,
        }
    }

    /// 대용량 확인 [열기] 후 실제 read 를 재개한다.
    fn resume_load(&mut self) {
        self.pending_large = false;
        if let Some(f) = self.file_path.clone() {
            self.last_mtime = std::fs::metadata(&f).and_then(|m| m.modified()).ok();
            self.read_now(&f);
        }
    }

    /// Throttled mtime poll; refresh content on external change. Runs on each paint, so a
    /// changed file is picked up the next time the surface paints — on user input, or on
    /// the idle `SurfaceInvalidated` re-forward path (`watch.rs`, 단계 06) which the host
    /// turns into an input-less re-paint within `RELOAD_CHECK_INTERVAL_SECS`.
    fn poll_reload(&mut self) {
        // 대용량 확인 대기 중이면 아직 읽지 않는다([열기] 확정 전).
        if self.pending_large {
            return;
        }
        let Some(f) = self.file_path.clone() else {
            return;
        };
        if self.last_check.elapsed().as_secs_f64() < RELOAD_CHECK_INTERVAL_SECS {
            return;
        }
        self.last_check = Instant::now();
        // 삭제(metadata 실패=None)를 "변경 없음"과 구별하려면 Option 채로 비교한다.
        // 다른 리로드 경로(force_reload/resume_load)와 동일하게 last_mtime=metadata().ok()
        // → read_now 규약으로 수렴 → 삭제 시 read_now 가 load_error 를 세팅해 error 상태로
        // 통일된다. 삭제 지속(None==None)은 무동작이라 반복 read 가 없다.
        let current = std::fs::metadata(&f).and_then(|m| m.modified()).ok();
        if self.last_mtime == current {
            return;
        }
        self.last_mtime = current;
        self.read_now(&f);
    }

    /// Force a re-read regardless of throttle/mtime (`markdown.reload` IPC).
    fn force_reload(&mut self) {
        let Some(f) = self.file_path.clone() else {
            return;
        };
        self.last_check = Instant::now();
        self.last_mtime = std::fs::metadata(&f).and_then(|m| m.modified()).ok();
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

/// 상단 주소창의 per-surface 편집 상태 (03). egui-mesh 는 입력 있을 때만 재-forward
/// 되므로 편집은 사용자 입력(클릭/타이핑)에서만 진행된다 — identity 경계 준수.
#[derive(Default)]
struct AddrState {
    /// 경로 편집 버퍼. 비편집 중엔 표시 경로와 동기화된다.
    buffer: String,
    /// PathField 가 포커스를 가진 편집 모드 여부(= 히스토리 드롭다운 열림).
    editing: bool,
    /// paint 클로저가 채우는 확정된 이동 경로 — paint 후 host `markdown.navigate` 로 소비.
    pending_navigate: Option<String>,
    /// 편집 진입 시 `recent.query {kind:"markdown"}` 로 캐시한 최근 경로(최신순 최대 10).
    /// 드롭다운 후보.
    recent: Vec<String>,
    /// 히스토리 드롭다운의 keyboard-active 행 index(↑/↓ 커서). 첫 오픈 시 `None`.
    active: Option<usize>,
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
    /// surface_id → plugin egui render state (font atlas + shared buffer; unix-only paint).
    #[cfg(any(unix, windows))]
    meshes: HashMap<u32, EguiMeshSurface>,
    /// surface_id 들 중 폰트(CJK fallback)를 이미 설치한 것 — set_fonts 재업로드 방지.
    #[cfg(any(unix, windows))]
    fonts_installed: std::collections::HashSet<u32>,
    /// surface_id → 직전 paint 의 focused 상태. reload 재-paint(입력 없는 재구성)가
    /// focused 를 잃지 않도록 보존한다 (C).
    #[cfg(any(unix, windows))]
    last_focused: HashMap<u32, bool>,
    /// surface_id → markdown document state.
    docs: HashMap<u32, MdDoc>,
    /// surface_id → egui_commonmark 라이브러리 캐시 (이미지 로더 설치 플래그·링크 훅 테이블).
    /// frame 마다 재생성하지 않도록 per-surface 로 보존한다.
    #[cfg(any(unix, windows))]
    caches: HashMap<u32, MdCache>,
    /// surface_id → 주소창 편집 상태 (03).
    addr: HashMap<u32, AddrState>,
    /// large-file 이벤트 발행용 Event Bus 핸들(`on_start` 에서 저장).
    bus: Option<BusHandle>,
    /// popup instance_id → 대용량 확인 대상.
    confirm: HashMap<u64, LargeFileConfirm>,
    /// popup instance_id → 파일열기 팝업 상태(경로 입력 버퍼).
    file_open: HashMap<u64, FileOpenState>,
    /// popup instance_id → egui-mesh popup 렌더 상태(폰트 atlas·shared buffer 소유).
    #[cfg(any(unix, windows))]
    popups: HashMap<u64, EguiMeshPopup>,
    /// CJK fallback 폰트를 이미 설치한 popup instance_id — set_fonts 재업로드 방지.
    #[cfg(any(unix, windows))]
    popup_fonts_installed: std::collections::HashSet<u64>,
    /// plugin lang 카탈로그 (state.failed / state.empty 등 UI 문자열).
    tr: Translator,
    /// idle auto-reload 감시 worker(`watch::run`) 로 등록/해제 명령을 보내는 채널
    /// (단계 06). `on_start` 에서 worker 를 spawn 하며 채워진다 — 그 전에는 감시가
    /// 비활성(사실상 도달하지 않음, worker_loop 이 on_start 를 먼저 호출).
    watch_tx: Option<mpsc::Sender<WatchCmd>>,
}

impl MarkdownPlugin {
    fn new(tr: Translator) -> Self {
        Self {
            #[cfg(any(unix, windows))]
            meshes: HashMap::new(),
            #[cfg(any(unix, windows))]
            fonts_installed: std::collections::HashSet::new(),
            #[cfg(any(unix, windows))]
            last_focused: HashMap::new(),
            #[cfg(any(unix, windows))]
            caches: HashMap::new(),
            docs: HashMap::new(),
            addr: HashMap::new(),
            bus: None,
            confirm: HashMap::new(),
            file_open: HashMap::new(),
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
        // large-file 확인 이벤트를 발행하려면 Event Bus 핸들이 필요하다(create_surface 에는
        // host/bus 가 없으므로 여기서 저장).
        self.bus = Some(bus);

        // idle auto-reload(단계 06): paint 에 종속되지 않는 별도 스레드가 mtime 을
        // 폴링하다가 변경을 감지하면 `SurfaceInvalidated` 를 emit 한다. worker는 emit만
        // 하고 실제 read 는 다음 재-forward 의 기존 `MdDoc::poll_reload` 가 담당한다.
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
        // egui-mesh surface 는 tree 를 안 그린다 — file 만 적재하고 빈 결과를 돌린다.
        // SDK 는 surface.create 의 **전체 envelope** 을 `ctx.params` 로 넘긴다 — 실제 생성
        // params(file 등)는 `params.params` 아래에 중첩돼 있다.
        //
        // 대용량 파일(> LARGE_FILE_LIMIT_BYTES)은 **plugin in-process** 로 크기를 감지해
        // read 를 보류하고(확인 대기), large-file 이벤트를 발행한다. host 는 파일 크기를
        // stat 하지 않는다(크기게이트는 plugin 소유). 이벤트 → host `fire_popup_triggers`
        // → 이 plugin 의 `[[contributes.popup]]`(event trigger) 확인 팝업이 열린다.
        let file = surface_param_file(&ctx.params);
        let doc = self.make_doc(file.clone(), ctx.surface_id);
        self.docs.insert(ctx.surface_id, doc);
        // idle 감시 등록(단계 06). `markdown.navigate` 제자리 이동도 같은 surface_id 로
        // create_surface 를 다시 호출하므로 여기서 자연스럽게 갱신된다.
        self.watch_register(ctx.surface_id, file);
        SurfaceResult::default()
    }

    fn destroy_surface(&mut self, surface_id: u32) {
        #[cfg(any(unix, windows))]
        {
            self.meshes.remove(&surface_id);
            self.fonts_installed.remove(&surface_id);
            self.last_focused.remove(&surface_id);
            self.caches.remove(&surface_id);
        }
        self.docs.remove(&surface_id);
        self.addr.remove(&surface_id);
        self.watch_unregister(surface_id);
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            // reload 는 파일 내용을 out-of-band 로 다시 읽으므로(사용자 입력 무관), 성공 후
            // 마지막 컨텍스트를 빈 입력 재-paint 한다 — 안 그러면 갱신된 내용이 다음 사용자
            // 입력 전까지 화면에 안 뜬다(egui-mesh 재-forward 공백, 옵션 A).
            "markdown.reload" => {
                let out = self.markdown_reload(&ctx.params)?;
                self.repaint_after_reload(&ctx.host, &ctx.params);
                Ok(out)
            }
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

    fn paint_surface(&mut self, ctx: SurfaceSetContextCtx) {
        self.paint(ctx);
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
        Ok(json!({ "ok": true, "surface_id": surface_id }))
    }

    /// 문서를 만든다. 파일이 임계값을 *초과* 하면 read 를 보류(`new_deferred`)하고
    /// large-file 이벤트를 발행해 확인 팝업을 띄운다(크기 감지는 plugin in-process).
    /// bus 가 없으면(초기화 전) 게이트를 건너뛰고 즉시 로드한다(fail-open).
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

    /// idle 감시 worker(단계 06)에 surface 의 감시 대상 경로를 등록/갱신한다.
    /// worker 가 없으면(spawn 실패) 조용히 무시 — idle auto-reload 만 비활성화되고
    /// 기존 paint-종속 폴링(`MdDoc::poll_reload`)은 그대로 동작한다.
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

    /// `set_context` 한 frame 을 그려 host 에 mesh 를 회신한다.
    #[cfg(any(unix, windows))]
    fn paint(&mut self, ctx: SurfaceSetContextCtx) {
        let sid = ctx.params.surface_id;
        let focused = ctx.params.raw_input.focused;
        // reload 재-paint(입력 없는 재구성)가 focused 를 잃지 않도록 직전 값 보존 (C).
        self.last_focused.insert(sid, focused);

        // host 가 Theme 을 아직 안 보냈으면(theme 미동봉 set_context) 토큰을 풀 수 없으므로
        // 이 frame 은 건너뛴다. host 는 테마 변경/입력 시 theme 을 동봉해 재forward 한다.
        let Some(theme) = ctx.params.theme.as_ref().map(theme_from_wire) else {
            tracing::debug!("markdown surface {sid}: set_context without theme — skipping paint");
            return;
        };

        // tr 는 self.tr (meshes/docs 와 서로소 필드) — 클로저가 self 전체를 잡지 않게
        // 미리 지역 참조로 뽑는다.
        let tr = &self.tr;

        let doc = self.docs.entry(sid).or_insert_with(|| MdDoc::new(None));
        doc.poll_reload();
        let content = doc.content.clone();
        let load_error = doc.load_error.clone();
        let base_dir = doc.base_dir.clone();
        let file_path = doc.file_path.clone().unwrap_or_default();

        // 주소창 편집 상태 — 비편집 중이면 버퍼를 현재 경로와 동기화(표시용).
        let addr = self.addr.entry(sid).or_default();
        if !addr.editing {
            addr.buffer = file_path.clone();
        }
        // 편집 진입(비편집→편집) 전환 감지용 — draw 가 편집모드를 갱신하기 전 값.
        let prev_editing = addr.editing;

        // 본문 폰트 크기: host 는 surface EffectiveFont.font_size 를 썼다(폰트 parity defer).
        // body 토큰으로 대체한다 — 디자인 본문 크기와 동일.
        let body_px = theme.font_size_body.value();

        let is_new = !self.meshes.contains_key(&sid);
        let mesh = self
            .meshes
            .entry(sid)
            .or_insert_with(|| EguiMeshSurface::new(sid));
        if is_new {
            // 한글/일문이 tofu(□) 되지 않도록 CJK fallback 을 plugin Context 에 설치한다
            // (커스텀 markdown 폰트-패밀리 parity 는 후속 — B1 scope 밖).
            install_fonts(mesh.context());
            self.fonts_installed.insert(sid);
        }
        // egui_commonmark 캐시(mesh 와 서로소 필드)를 미리 뽑아 클로저가 self 를 잡지 않게 한다.
        let cache = self.caches.entry(sid).or_default();

        let result = mesh.paint(&ctx.host, &ctx.params, |egui_ctx| {
            draw(
                egui_ctx,
                &theme,
                body_px,
                cache,
                &content,
                load_error.as_deref(),
                tr,
                &file_path,
                addr,
                focused,
            );
        });
        if let Err(e) = result {
            tracing::warn!("markdown surface {sid} paint failed: {e}");
        }

        after_paint_side_effects(
            &ctx.host,
            sid,
            &theme,
            body_px,
            cache,
            &content,
            load_error.as_deref(),
            tr,
            &file_path,
            base_dir.as_deref(),
            addr,
            prev_editing,
            focused,
            mesh,
        );
    }

    /// `markdown.reload` 로 내용이 out-of-band 로 갱신된 뒤, **입력 없이** 화면을 갱신한다
    /// (옵션 A). 마지막 set_context 의 캐시된 컨텍스트(geom/ppp/theme)로 빈 입력 재-paint →
    /// 출력이 바뀌면 host 로 PaintFrame 송신. theme 미수신(첫 set_context 전)이면 no-op.
    /// 재-paint 는 빈 입력이므로 링크 클릭은 발생하지 않는다(dispatch 불필요).
    #[cfg(any(unix, windows))]
    fn repaint_after_reload(&mut self, host: &HostHandle, params: &Value) {
        let Some(sid) = params
            .get("surface")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
        else {
            return;
        };
        // 캐시된 theme 으로 draw 를 재구성한다. 첫 set_context 전이면 theme 이 없어 no-op.
        let Some(theme) = self
            .meshes
            .get(&sid)
            .and_then(|m| m.last_theme())
            .map(theme_from_wire)
        else {
            return;
        };
        let tr = &self.tr;
        let Some(doc) = self.docs.get(&sid) else {
            return;
        };
        let content = doc.content.clone();
        let load_error = doc.load_error.clone();
        let file_path = doc.file_path.clone().unwrap_or_default();

        let addr = self.addr.entry(sid).or_default();
        if !addr.editing {
            addr.buffer = file_path.clone();
        }

        let body_px = theme.font_size_body.value();

        // 입력이 없는 재-paint 라 raw_input.focused 를 못 얻는다 — 직전 paint 의 focused 를
        // 재사용해 focused 배경이 unfocused 로 튀지 않게 한다 (C).
        let focused = self.last_focused.get(&sid).copied().unwrap_or(false);
        let cache = self.caches.entry(sid).or_default();
        let Some(mesh) = self.meshes.get_mut(&sid) else {
            return;
        };
        // 재-paint 는 빈 입력이므로 링크 클릭은 발생하지 않는다(dispatch 불필요).
        let result = mesh.repaint_last(host, |egui_ctx| {
            draw(
                egui_ctx,
                &theme,
                body_px,
                cache,
                &content,
                load_error.as_deref(),
                tr,
                &file_path,
                addr,
                focused,
            );
        });
        if let Err(e) = result {
            tracing::warn!("markdown surface {sid} repaint failed: {e}");
        }
    }

    /// large-file 확인 팝업 한 frame 을 egui-mesh 로 그린다. [열기] 시 대상 surface 의
    /// 문서 read 를 재개하고 그 surface 를 재-paint 한 뒤 팝업을 닫는다. [취소] 는 팝업만
    /// 닫는다(surface 는 대기 상태 유지). chrome(scrim/border/Esc/outside-click)은 host 소유.
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
                // 대상 surface 를 재-paint 해 로드된 내용을 즉시 반영(입력 없는 갱신).
                self.repaint_after_reload(&ctx.host, &json!({ "surface": confirm.surface_id }));
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
                // native 파일 다이얼로그는 plugin 프로세스에서 못 연다 → host 에 위임(fs.pick_file).
                // host UI 스레드가 rfd 모달을 여는 동안 이 호출은 블로킹되나 데드락은 없다.
                if let Some(path) = pick_markdown_file(&ctx.host)
                    && let Some(st) = self.file_open.get_mut(&iid)
                {
                    st.path_input = path;
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

    /// egui-mesh shared-buffer 송신은 현재 unix 전용(host buffer.rs 가 windows 미구현).
    /// 다른 OS 에선 채널이 비활성이라 no-op — 크로스플랫폼 컴파일만 보장한다.
    #[cfg(not(any(unix, windows)))]
    fn paint(&mut self, _ctx: SurfaceSetContextCtx) {}

    /// unix 외에는 egui-mesh 채널이 비활성이라 재-paint 도 no-op.
    #[cfg(not(any(unix, windows)))]
    fn repaint_after_reload(&mut self, _host: &HostHandle, _params: &Value) {}

    /// unix 외에는 egui-mesh 채널이 비활성이라 확인 팝업 렌더도 no-op.
    #[cfg(not(any(unix, windows)))]
    fn paint_confirm(&mut self, _ctx: PopupSetContextCtx) {}

    /// unix 외에는 egui-mesh 채널이 비활성이라 파일열기 팝업 렌더도 no-op.
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

/// browse — host 에 native 파일 다이얼로그(rfd)를 위임한다(fs.pick_file). plugin 프로세스는
/// native OS 다이얼로그를 못 열기 때문. markdown 확장자로 필터. 선택 경로(취소면 None)를
/// 반환한다. host UI 스레드가 모달을 여는 동안 이 호출은 블로킹되나 데드락은 없다(ADR-0042).
#[cfg(any(unix, windows))]
fn pick_markdown_file(host: &HostHandle) -> Option<String> {
    match host.call(
        "fs.pick_file",
        json!({ "filters": [{ "name": "Markdown", "exts": ["md", "markdown"] }] }),
    ) {
        Ok(v) => v
            .get("path")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string()),
        Err(e) => {
            tracing::warn!("markdown file-open browse (fs.pick_file) failed: {e}");
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

        // 경로 입력 + browse.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            let browse =
                Button::new(tr.t("markdown.file_open.browse")).variant(ButtonVariant::Ghost);
            // browse 버튼 폭을 확보하고 남은 폭을 입력 필드에 준다.
            let field_w =
                (ui.available_width() - theme.spacing_xl.value()).max(theme.spacing_xl.value());
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
            if browse.show(ui, theme).clicked() {
                action = FileOpenAction::Browse;
            }
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

/// 링크 클릭 부수효과. **forward 된 실제 사용자 클릭에서만** 도달한다(identity 경계).
/// 파일은 host `file_handler.dispatch`(같은 Pane 새 탭, origin_surface_id)로, 외부 URL 은
/// OS 핸들러로 연다. 파일 열기 외의 사용자 상태(focus/선택/스크롤)는 건드리지 않는다.
#[cfg(any(unix, windows))]
fn dispatch_link(host: &HostHandle, sid: u32, click: LinkClick) {
    match click {
        LinkClick::File(path) => dispatch_file_link(host, sid, &path),
        LinkClick::External(url) => dispatch_external_link(&url),
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

/// 주소창 바 높이 (4px 그리드; 디자인 40). 필드/Go 높이는 공용 `PathField`(ControlSize::Sm)가 소유.
const ADDR_BAR_HEIGHT: f32 = 40.0;

/// 본문 배경색: focused 면 markdown surface 의 focused_bg, 아니면 unfocused_bg (A).
/// 하드코딩(`theme.base`) 대신 `theme.surface("markdown")` 토큰을 경유한다.
fn md_body_bg(theme: &Theme, focused: bool) -> tasty_type_appearance::color::HexColor {
    let s = theme.surface("markdown");
    if focused {
        s.focused_bg
    } else {
        s.unfocused_bg
    }
}

/// egui closure: 상단 주소창 chrome(03) + scroll area 본문 render.
#[allow(clippy::too_many_arguments)]
fn draw(
    ctx: &egui::Context,
    theme: &Theme,
    body_px: f32,
    cache: &mut MdCache,
    content: &str,
    load_error: Option<&str>,
    tr: &Translator,
    file_path: &str,
    addr: &mut AddrState,
    focused: bool,
) {
    // ── 상단 주소창 (03) ──
    let bar_frame = egui::Frame::new()
        .fill(theme.bg_sidebar().to_egui())
        .inner_margin(egui::Margin::symmetric(theme.spacing_sm.value() as i8, 0));
    egui::TopBottomPanel::top("md_addr_bar")
        .exact_height(ADDR_BAR_HEIGHT)
        .frame(bar_frame)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            draw_addr_bar(ui, theme, tr, file_path, addr);
        });

    // ── 본문 ── 배경은 focused 여부에 따라 markdown surface 토큰에서 가져온다 (A).
    // (heading/link 다색 렌더라 본문 fg 일괄 전환은 하지 않고 배경만 전환.)
    let frame = egui::Frame::new()
        .fill(md_body_bg(theme, focused).to_egui())
        .inner_margin(margin_all(theme.spacing_sm));
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        let viewport_w = ui.available_width();
        ui.set_min_width(viewport_w);
        // 세로/가로 모두 스크롤. 뷰포트보다 넓은 테이블(라이브러리가 wrap 없이 Grid 로
        // 그려 뷰포트를 넘김)이 가로 스크롤로 도달 가능해진다. 세로 전용이면 오른쪽이 클립됨.
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                // prose 는 뷰포트 폭에 맞춰 wrap 시키고(available_width 를 뷰포트로 고정 →
                // 라이브러리가 max_width 를 뷰포트로 읽음), 뷰포트보다 넓은 테이블만 넘쳐
                // 가로 스크롤로 도달하게 한다. max 를 안 잡으면 both() 가 무한 폭을 줘 문단도
                // wrap 되지 않는다.
                ui.set_max_width(viewport_w);
                ui.set_min_width(viewport_w);
                ui.style_mut().interaction.selectable_labels = true;

                if let Some(err) = load_error {
                    state_failed(ui, theme, tr, err);
                    return;
                }
                if content.trim().is_empty() {
                    state_empty(ui, theme, tr);
                    return;
                }
                render::render(ui, theme, body_px, cache, content);
                // Trailing space so the last line doesn't collide with the bottom margin.
                vspace(ui, theme.spacing_sm);
            });
    });
}

/// 주소창 바: 공용 [`PathField`] (경로 표시/편집 트리거 + 히스토리 드롭다운 + Go). 확정 시
/// `addr.pending_navigate` 를 채운다(paint 후 host `markdown.navigate` 로 소비). 비편집=경로
/// 표시(text-secondary), 클릭=편집모드 진입 → recent 캐시가 드롭다운으로 펼쳐지고 타이핑이
/// substring 필터. 키보드: ↑/↓ active 행 · Enter/행클릭/Go=navigate · Esc=닫고 원복.
fn draw_addr_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    file_path: &str,
    addr: &mut AddrState,
) {
    // 선두/후보 파일 아이콘 · Go 화살표 — 베이크된 벡터 아이콘을 painter 로 주입한다
    // (위젯 내부 아이콘 상수 금지). 위젯이 넘긴 정사각 rect 중심에 그 폭 크기로 그린다
    // (색은 위젯이 상태별 토큰으로 호출).
    let file_icon = |ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32| {
        tasty_plugin_sdk::baked_icon::draw(
            ui.painter(),
            baked_icons::FILE,
            rect.center(),
            rect.width(),
            color,
        );
    };
    let go_icon = |ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32| {
        tasty_plugin_sdk::baked_icon::draw(
            ui.painter(),
            baked_icons::ARROW_RIGHT,
            rect.center(),
            rect.width(),
            color,
        );
    };

    // addr 필드를 분해해 buffer(&mut)/recent(&)/editing·active(&mut) 를 동시 대여한다.
    let AddrState {
        buffer,
        editing,
        pending_navigate,
        recent,
        active,
    } = addr;
    // 후보 = 최근 경로(최신순). PathField 는 `&[&str]` 을 받는다.
    let entries: Vec<&str> = recent.iter().map(String::as_str).collect();

    ui.horizontal_centered(|ui| {
        // Navigate 확정 경로는 PathField 가 준 문자열(필터된 가시 후보/버퍼)이다 — 원본
        // recent[i] 를 인덱스 역참조하지 않는다(회귀 방지). Esc/blur 원복은 위젯이 buffer 를
        // file_path 로 되돌린다.
        let outcome = PathField::new("md_addr")
            .placeholder(tr.t("markdown.addr.placeholder"))
            .empty_label(tr.t("markdown.addr.no_recent"))
            .width(ui.available_width())
            .leading_icon(&file_icon)
            .row_icon(&file_icon)
            .go_icon(&go_icon)
            .go_tooltip(tr.t("markdown.addr.go"))
            .show(ui, theme, buffer, editing, active, &entries, file_path);
        if let PathFieldOutcome::Navigate(path) = outcome {
            *pending_navigate = Some(path);
        }
    });
    // 본문과의 경계 1px separator.
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        r.bottom(),
        egui::Stroke::new(1.0, theme.separator.to_egui()),
    );
}

/// 주소창 확정 이동을 host `markdown.navigate`(04) 로 보낸다 — 같은 surface 제자리 이동.
#[cfg(any(unix, windows))]
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

/// 편집 진입 시 host 의 generic `recent.query {kind}` 로 최근목록을 조회한다 — 최신순
/// 최대 10개 경로. host 는 "markdown" 을 모르므로 plugin 이 kind 를 채운다. 조회 전용
/// (사용자 상태 불변). 실패하면 빈 목록으로 폴백한다.
#[cfg(any(unix, windows))]
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

/// `paint` 의 mesh.paint(show()) 이후 부수효과 3가지: 주소창 확정 이동 dispatch,
/// 라이브러리 링크 클릭 dispatch(file → host, url → OS), 편집 진입 프레임의 recent
/// 캐시 fetch + 즉시 재-paint(드롭다운이 채워진 채로 뜨게 함).
#[cfg(any(unix, windows))]
#[allow(clippy::too_many_arguments)]
fn after_paint_side_effects(
    host: &HostHandle,
    sid: u32,
    theme: &Theme,
    body_px: f32,
    cache: &mut MdCache,
    content: &str,
    load_error: Option<&str>,
    tr: &Translator,
    file_path: &str,
    base_dir: Option<&std::path::Path>,
    addr: &mut AddrState,
    prev_editing: bool,
    focused: bool,
    mesh: &mut EguiMeshSurface,
) {
    // 주소창 확정 이동 요청을 host 로 보낸다 (제자리 이동, 04). forward 된 실제
    // 사용자 입력(Enter/Go)에서만 채워진다 — identity 경계 준수.
    if let Some(path) = addr.pending_navigate.take() {
        navigate(host, sid, &path);
    }

    // show() 후 라이브러리 링크 훅에서 클릭된 destination 을 꺼내 dispatch
    // (file → host, url → OS). 소비 시 훅을 리셋해 재발화하지 않는다.
    if let Some(click) = render::take_clicked_link(cache, base_dir) {
        dispatch_link(host, sid, click);
    }

    // 편집 진입 프레임이면 `recent.query` 로 최근목록을 캐시하고, 이미 채워진 addr 로
    // 마지막 컨텍스트를 재-paint 한다 — 진입 클릭 한 프레임 안에서 드롭다운이 채워진
    // 채로 뜨게 한다(캐시가 비어 첫 프레임이 empty 로 뜨는 것을 막는다). 캐시 조회는
    // 편집 진입 시 1회뿐 — 타이핑 프레임마다 host 를 호출하지 않는다.
    if !(addr.editing && !prev_editing) {
        return;
    }
    let fetched = fetch_recent(host);
    addr.recent = fetched;
    // 편집 진입 프레임에 recent 를 fetch 해 곧바로 재-paint 한다 — egui-mesh 는 입력
    // 있을 때만 재-forward 되므로, 여기서 즉시 재그리지 않으면 드롭다운 후보가 다음
    // 입력까지 안 뜬다. 버퍼는 비우지 않고 현재 경로(L480 동기화값)를 유지한다 —
    // explorer 와 동일하게 편집 진입 시 경로가 남아 그대로 선택·편집 가능하다. 진입
    // 드롭다운은 그 경로 substring 으로 필터되며, 이후 타이핑이 typeahead 필터가 된다.
    // 기존 mesh/cache 바인딩 재사용(origin egui_commonmark draw 시그니처: body_px + cache).
    let result = mesh.repaint_last(host, |egui_ctx| {
        draw(
            egui_ctx, theme, body_px, cache, content, load_error, tr, file_path, addr, focused,
        );
    });
    if let Err(e) = result {
        tracing::warn!("markdown surface {sid} recent-repaint failed: {e}");
    }
}

/// Load failure — "Failed to load" title (accent-danger) over the error in a muted caption.
fn state_failed(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, detail: &str) {
    centered(ui, |ui| {
        ui.label(
            egui::RichText::new(tr.t("markdown.state.failed"))
                .size(theme.font_size_max.value())
                .color(theme.accent_danger().to_egui()),
        );
        ui.add_space(theme.spacing_xs.value());
        ui.label(
            egui::RichText::new(detail)
                .monospace()
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
}

/// Empty file — a centered, muted "This file is empty".
fn state_empty(ui: &mut egui::Ui, theme: &Theme, tr: &Translator) {
    centered(ui, |ui| {
        ui.label(
            egui::RichText::new(tr.t("markdown.state.empty"))
                .size(theme.font_size_body.value())
                .color(theme.text_muted().to_egui()),
        );
    });
}

/// Center `content` within the scroll viewport.
fn centered(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let h = ui.available_height().max(1.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.vertical_centered(|ui| content(ui));
        },
    );
}

/// plugin Context 에 CJK fallback 을 설치한다. egui 기본 폰트(Proportional/Monospace)
/// 뒤에 시스템 CJK 폰트를 fallback 으로 붙여 한글/일문/한자가 tofu 되지 않게 한다.
#[cfg(any(unix, windows))]
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = load_system_cjk_font_data() {
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
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
mod tests {
    use super::*;

    #[test]
    fn reload_with_surface_id_returns_ok() {
        let mut p = MarkdownPlugin::new(Translator::default());
        let resp = p
            .markdown_reload(&json!({ "surface": 42 }))
            .expect("reload should succeed");
        assert_eq!(resp["ok"], json!(true));
        assert_eq!(resp["surface_id"], json!(42));
    }

    #[test]
    fn reload_without_surface_id_is_invalid_params() {
        let mut p = MarkdownPlugin::new(Translator::default());
        let err = p.markdown_reload(&json!({})).unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("surface"));
    }

    #[test]
    fn create_surface_loads_missing_file_as_error() {
        let mut p = MarkdownPlugin::new(Translator::default());
        // SDK 가 넘기는 envelope 형태(file 은 nested `params.file`)로 구성한다.
        p.create_surface(SurfaceCreateCtx {
            surface_id: 1,
            kind: "markdown".into(),
            cwd: None,
            params: json!({
                "surface_id": 1,
                "kind": "markdown",
                "params": { "file": "\0nonexistent-md-for-test" }
            }),
        });
        let doc = p.docs.get(&1).expect("doc inserted");
        assert!(doc.load_error.is_some());
        assert!(doc.content.is_empty());
    }

    /// 크기게이트 임계값 판정 — 초과만 게이트(경계값·이하·부재는 통과). host
    /// `file/dispatch.rs` 의 `size_gate_boundary_and_over` 를 plugin in-process 로 이관.
    #[test]
    fn file_exceeds_limit_gates_over_only() {
        let dir = std::env::temp_dir();
        let big = dir.join(format!("tasty-md-big-{}.md", std::process::id()));
        let exact = dir.join(format!("tasty-md-exact-{}.md", std::process::id()));
        let small = dir.join(format!("tasty-md-small-{}.md", std::process::id()));
        std::fs::write(&big, vec![b'x'; LARGE_FILE_LIMIT_BYTES as usize + 1]).unwrap();
        std::fs::write(&exact, vec![b'x'; LARGE_FILE_LIMIT_BYTES as usize]).unwrap();
        std::fs::write(&small, vec![b'x'; 500 * 1024]).unwrap();

        assert_eq!(
            file_exceeds_limit(big.to_str().unwrap()),
            Some(LARGE_FILE_LIMIT_BYTES + 1)
        );
        // 정확히 임계값 → None (초과만 게이트).
        assert_eq!(file_exceeds_limit(exact.to_str().unwrap()), None);
        assert_eq!(file_exceeds_limit(small.to_str().unwrap()), None);
        // 없는 파일 → None (게이트 통과, 로드 시 error 표시).
        assert_eq!(file_exceeds_limit("\0nonexistent-md-for-test"), None);

        let _ = std::fs::remove_file(&big); // best-effort 정리 — 실패 무시(테스트 결과 무관).
        let _ = std::fs::remove_file(&exact); // best-effort 정리 — 실패 무시.
        let _ = std::fs::remove_file(&small); // best-effort 정리 — 실패 무시.
    }

    /// deferred 문서는 [열기] 확정(`resume_load`) 전까지 read 를 보류한다(poll 도 안 읽음).
    #[test]
    fn deferred_doc_holds_read_until_resume() {
        let path =
            std::env::temp_dir().join(format!("tasty-md-deferred-{}.md", std::process::id()));
        std::fs::write(&path, b"# hello deferred").unwrap();
        let mut doc = MdDoc::new_deferred(Some(path.to_string_lossy().into_owned()));
        assert!(doc.pending_large);
        assert!(doc.content.is_empty());
        // 대기 중 poll 은 읽지 않는다.
        doc.poll_reload();
        assert!(doc.content.is_empty());
        // 확정 후 실제 로드.
        doc.resume_load();
        assert!(!doc.pending_large);
        assert!(doc.content.contains("hello deferred"));
        // best-effort 정리 — 실패해도 테스트 결과에 영향 없음.
        let _ = std::fs::remove_file(&path);
    }

    /// 자동 mtime 폴링 경로가 외부 삭제를 error 상태로 감지한다 — 다른 리로드 경로
    /// (force_reload/resume_load/초기 로드)와 동일한 삭제 처리 규약으로 통일됨을 확인한다.
    #[test]
    fn poll_reload_detects_external_deletion_as_error() {
        use std::time::Duration;

        let path = std::env::temp_dir().join(format!("tasty-md-delpoll-{}.md", std::process::id()));
        std::fs::write(&path, b"# hello poll").unwrap();
        let mut doc = MdDoc::new(Some(path.to_string_lossy().into_owned()));
        // 정상 로드 baseline.
        assert!(!doc.content.is_empty());
        assert!(doc.load_error.is_none());
        assert!(doc.last_mtime.is_some());

        // throttle 우회 — last_check 를 충분히 과거로.
        doc.last_check = Instant::now()
            .checked_sub(Duration::from_secs(RELOAD_CHECK_INTERVAL_SECS as u64 + 5))
            .expect("instant underflow");
        // 외부 삭제 → poll 이 metadata 실패(None)를 변경으로 감지해 read_now → load_error.
        std::fs::remove_file(&path).unwrap();
        doc.poll_reload();
        assert!(
            doc.load_error.is_some(),
            "삭제가 error 상태로 감지되어야 한다"
        );
        assert!(doc.last_mtime.is_none(), "삭제 후 last_mtime 은 None");

        // 삭제 지속(None==None) — 반복 read 없이 무동작, error 유지.
        doc.last_check = Instant::now()
            .checked_sub(Duration::from_secs(RELOAD_CHECK_INTERVAL_SECS as u64 + 5))
            .expect("instant underflow");
        doc.poll_reload();
        assert!(doc.load_error.is_some(), "삭제 지속 시 error 유지");

        // best-effort 정리 — 이미 삭제되었을 수 있음.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn format_size_and_basename_examples() {
        assert_eq!(format_size(2 * 1024 * 1024 + 200 * 1024), "2.2 MB");
        assert_eq!(format_size(12 * 1024 * 1024), "12 MB");
        #[cfg(not(windows))]
        assert_eq!(basename("/docs/big-notes.md"), "big-notes.md");
    }

    // 주소창 확정 결정(Enter/Pick/Esc/blur/Go)의 단위테스트는 공용 위젯 `PathField` 의
    // `decide`(crates/tasty-ui-widgets/src/path_field.rs) 로 이관됐다 — 여기선 markdown
    // 고유 로직(recent 파싱 등)만 검증한다.

    #[test]
    fn parse_recent_extracts_paths_in_order() {
        let v = json!({ "recent": [
            { "path": "/a/first.md", "file_name": "first.md" },
            { "path": "/b/second.md", "file_name": "second.md" },
        ]});
        assert_eq!(parse_recent(&v), vec!["/a/first.md", "/b/second.md"]);
    }

    #[test]
    fn parse_recent_tolerates_missing_or_malformed() {
        assert!(parse_recent(&json!({})).is_empty());
        assert!(parse_recent(&json!({ "recent": "nope" })).is_empty());
        // path 없는 항목은 건너뛴다.
        assert_eq!(
            parse_recent(&json!({ "recent": [{ "file_name": "x" }, { "path": "/ok.md" }] })),
            vec!["/ok.md"]
        );
    }

    #[test]
    fn surface_param_file_reads_nested_and_flat() {
        assert_eq!(
            surface_param_file(&json!({ "params": { "file": "/a/b.md" } })).as_deref(),
            Some("/a/b.md")
        );
        assert_eq!(
            surface_param_file(&json!({ "file": "/c/d.md" })).as_deref(),
            Some("/c/d.md")
        );
        assert_eq!(surface_param_file(&json!({ "params": {} })), None);
    }
}
