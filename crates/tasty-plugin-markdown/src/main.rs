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
use std::time::{Instant, SystemTime};

use render::{LinkClick, MdCache};
use serde_json::{Value, json};
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, SurfaceCreateCtx, SurfaceResult,
    SurfaceSetContextCtx, Translator,
};
use tasty_type_appearance::theme::Theme;

#[cfg(any(unix, windows))]
use tasty_plugin_sdk::EguiMeshSurface;
use tasty_plugin_sdk::HostHandle;
use tasty_ui_widgets::{margin_all, vspace};

const PLUGIN_ID: &str = "com.tasty.markdown";
const PLUGIN_VERSION: &str = "0.1.0";

/// How often to check the file's mtime (in seconds).
const RELOAD_CHECK_INTERVAL_SECS: f64 = 1.0;

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
        }
    }

    /// Throttled mtime poll; refresh content on external change. Runs on each paint, so a
    /// changed file is picked up the next time the surface paints (i.e. on user input —
    /// idle auto-reload without input awaits the `SurfaceInvalidated` re-forward path).
    fn poll_reload(&mut self) {
        let Some(f) = self.file_path.clone() else {
            return;
        };
        if self.last_check.elapsed().as_secs_f64() < RELOAD_CHECK_INTERVAL_SECS {
            return;
        }
        self.last_check = Instant::now();
        let Some(current) = std::fs::metadata(&f).and_then(|m| m.modified()).ok() else {
            return;
        };
        if self.last_mtime == Some(current) {
            return;
        }
        self.last_mtime = Some(current);
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
    /// TextEdit 이 포커스를 가진 편집 모드 여부.
    editing: bool,
    /// paint 클로저가 채우는 확정된 이동 경로 — paint 후 host `markdown.navigate` 로 소비.
    pending_navigate: Option<String>,
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
    /// plugin lang 카탈로그 (state.failed / state.empty 등 UI 문자열).
    tr: Translator,
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
            tr,
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

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        // egui-mesh surface 는 tree 를 안 그린다 — file 만 적재하고 빈 결과를 돌린다.
        // SDK 는 surface.create 의 **전체 envelope** 을 `ctx.params` 로 넘긴다 — 실제 생성
        // params(file 등)는 `params.params` 아래에 중첩돼 있다.
        let file = surface_param_file(&ctx.params);
        self.docs.insert(ctx.surface_id, MdDoc::new(file));
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
            // 네트워크/CLI caller 가 이 namespace 로 보낸 호출을 host 본문으로 trampoline
            // 한다(image.open/list 와 동형 host-adapter). owner==self 인 self-call 이라
            // plugin_ipc 가 forward 하지 않고 host dispatch 로 통과시켜 handle_recent 에 닿는다.
            "markdown.recent" => Ok(ctx.host.call(&ctx.method, ctx.params)?),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn paint_surface(&mut self, ctx: SurfaceSetContextCtx) {
        self.paint(ctx);
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

        // 주소창 확정 이동 요청을 host 로 보낸다 (제자리 이동, 04). forward 된 실제
        // 사용자 입력(Enter/Go)에서만 채워진다 — identity 경계 준수.
        if let Some(path) = addr.pending_navigate.take() {
            navigate(&ctx.host, sid, &path);
        }

        // show() 후 라이브러리 링크 훅에서 클릭된 destination 을 꺼내 dispatch
        // (file → host, url → OS). 소비 시 훅을 리셋해 재발화하지 않는다.
        if let Some(click) = render::take_clicked_link(cache, base_dir.as_deref()) {
            dispatch_link(&ctx.host, sid, click);
        }
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

    /// egui-mesh shared-buffer 송신은 현재 unix 전용(host buffer.rs 가 windows 미구현).
    /// 다른 OS 에선 채널이 비활성이라 no-op — 크로스플랫폼 컴파일만 보장한다.
    #[cfg(not(any(unix, windows)))]
    fn paint(&mut self, _ctx: SurfaceSetContextCtx) {}

    /// unix 외에는 egui-mesh 채널이 비활성이라 재-paint 도 no-op.
    #[cfg(not(any(unix, windows)))]
    fn repaint_after_reload(&mut self, _host: &HostHandle, _params: &Value) {}
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
        LinkClick::File(path) => {
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
        LinkClick::External(url) => {
            if let Err(e) = webbrowser::open(&url) {
                tracing::warn!("markdown external link open failed ({url}): {e}");
            }
        }
    }
}

/// 주소창 바 높이 / 경로 필드 높이 (4px 그리드; 디자인 40 / 28).
const ADDR_BAR_HEIGHT: f32 = 40.0;
const ADDR_FIELD_HEIGHT: f32 = 28.0;

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
        ui.set_min_width(ui.available_width());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
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

/// 주소창 바: 경로 필드(표시/편집) + Go 버튼. 확정 시 `addr.pending_navigate` 를 채운다.
fn draw_addr_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    file_path: &str,
    addr: &mut AddrState,
) {
    let gap = theme.spacing_xs.value();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let go_w = ADDR_FIELD_HEIGHT;
        let field_w = (ui.available_width() - go_w - gap).max(40.0);
        draw_path_field(ui, theme, tr, field_w, file_path, addr);
        if go_button(ui, theme, tr).clicked() {
            addr.pending_navigate = Some(addr.buffer.clone());
            addr.editing = false;
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

/// 경로 표시/편집 필드 — surface_raised 배경, idle/편집 border + focus ring, 선두 글리프,
/// mono 경로 텍스트. TextEdit 이 forward 된 사용자 입력으로 편집된다.
fn draw_path_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    width: f32,
    file_path: &str,
    addr: &mut AddrState,
) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, ADDR_FIELD_HEIGHT), egui::Sense::hover());
    let focused = addr.editing;
    let border = if focused {
        theme.border_focus()
    } else {
        theme.border_default()
    };
    ui.painter().rect(
        rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), border.to_egui()),
        egui::StrokeKind::Inside,
    );
    if focused {
        // 2px focus ring, border_focus 35% 알파.
        let ring = theme.border_focus().to_egui().gamma_multiply(0.35);
        ui.painter().rect_stroke(
            rect.expand(1.0),
            theme.corner_radius.value(),
            egui::Stroke::new(2.0, ring),
            egui::StrokeKind::Outside,
        );
    }

    let pad = theme.spacing_sm.value();
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + pad, rect.min.y),
        egui::pos2(rect.max.x - pad, rect.max.y),
    );
    let mut cui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut cui;
    ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
    // 선두 문서 글리프 — 베이크된 FILE 벡터 아이콘. 이전 이모지 라벨과 동일 위치·크기
    // (left_to_right flow, caption 크기, text_muted 색)를 유지한다.
    let icon_sz = theme.font_size_caption.value();
    let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(icon_sz, icon_sz), egui::Sense::hover());
    tasty_plugin_sdk::baked_icon::draw(
        ui.painter(),
        baked_icons::FILE,
        icon_rect.center(),
        icon_sz,
        theme.text_muted().to_egui(),
    );
    ui.visuals_mut().text_cursor.stroke = egui::Stroke::new(1.0, theme.accent_primary().to_egui());
    let text_color = if focused {
        theme.text_primary()
    } else {
        theme.text_secondary()
    };
    let resp = ui.add(
        egui::TextEdit::singleline(&mut addr.buffer)
            .frame(false)
            .desired_width(ui.available_width())
            .hint_text(tr.t("markdown.addr.placeholder"))
            .font(egui::FontId::new(
                theme.font_size_caption.value(),
                egui::FontFamily::Monospace,
            ))
            .text_color(text_color.to_egui()),
    );
    if resp.gained_focus() {
        addr.editing = true;
    }
    let (enter, esc) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    match addr_key_decision(resp.has_focus(), resp.lost_focus(), enter, esc) {
        AddrKey::Navigate => {
            addr.pending_navigate = Some(addr.buffer.clone());
            addr.editing = false;
        }
        AddrKey::Revert => {
            // Esc / 확정 없는 포커스 이탈 → 원래 경로 원복, 아무것도 안 열림.
            addr.buffer = file_path.to_string();
            addr.editing = false;
            if esc {
                resp.surrender_focus();
            }
        }
        AddrKey::None => {}
    }
}

/// 주소창 키 입력 결정 (순수 — 테스트 용이). Esc(포커스 중)=원복, Enter(포커스 이탈)=이동,
/// 그 외 포커스 이탈=원복.
#[derive(Debug, PartialEq, Eq)]
enum AddrKey {
    None,
    Navigate,
    Revert,
}

fn addr_key_decision(has_focus: bool, lost_focus: bool, enter: bool, esc: bool) -> AddrKey {
    if has_focus && esc {
        AddrKey::Revert
    } else if lost_focus && enter {
        AddrKey::Navigate
    } else if lost_focus {
        AddrKey::Revert
    } else {
        AddrKey::None
    }
}

/// Go 버튼 — arrow-right 글리프. 클릭 시 caller 가 이동을 확정한다.
fn go_button(ui: &mut egui::Ui, theme: &Theme, tr: &Translator) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ADDR_FIELD_HEIGHT, ADDR_FIELD_HEIGHT),
        egui::Sense::click(),
    );
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius.value(),
            theme.hover_overlay.to_egui_premultiplied(),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if hovered {
        theme.text_primary()
    } else {
        theme.text_secondary()
    };
    // go 화살표 — 베이크된 ARROW_RIGHT 벡터 아이콘. 이전 → 글리프와 동일 위치(rect
    // center)·크기(body)를 유지한다.
    tasty_plugin_sdk::baked_icon::draw(
        ui.painter(),
        baked_icons::ARROW_RIGHT,
        rect.center(),
        theme.font_size_body.value(),
        color.to_egui(),
    );
    resp.on_hover_text(tr.t("markdown.addr.go"))
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

    #[test]
    fn addr_key_enter_navigates_on_blur() {
        // Enter 는 포커스 이탈(lost_focus)과 함께 도착 → 이동.
        assert_eq!(
            addr_key_decision(false, true, true, false),
            AddrKey::Navigate
        );
    }

    #[test]
    fn addr_key_esc_reverts_while_focused() {
        assert_eq!(addr_key_decision(true, false, false, true), AddrKey::Revert);
    }

    #[test]
    fn addr_key_blur_without_enter_reverts() {
        assert_eq!(
            addr_key_decision(false, true, false, false),
            AddrKey::Revert
        );
    }

    #[test]
    fn addr_key_typing_is_none() {
        assert_eq!(addr_key_decision(true, false, false, false), AddrKey::None);
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
