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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use render::{LinkClick, MdStyle};
use serde_json::{Value, json};
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, SurfaceCreateCtx, SurfaceEventCtx,
    SurfaceResult, SurfaceSetContextCtx, Translator,
};
use tasty_type_appearance::theme::Theme;

#[cfg(unix)]
use tasty_plugin_sdk::EguiMeshSurface;
#[cfg(unix)]
use tasty_plugin_sdk::HostHandle;

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

struct MarkdownPlugin {
    /// surface_id → plugin egui render state (font atlas + shared buffer; unix-only paint).
    #[cfg(unix)]
    meshes: HashMap<u32, EguiMeshSurface>,
    /// surface_id 들 중 폰트(CJK fallback)를 이미 설치한 것 — set_fonts 재업로드 방지.
    #[cfg(unix)]
    fonts_installed: std::collections::HashSet<u32>,
    /// surface_id → markdown document state.
    docs: HashMap<u32, MdDoc>,
    /// plugin lang 카탈로그 (state.failed / state.empty 등 UI 문자열).
    tr: Translator,
}

impl MarkdownPlugin {
    fn new(tr: Translator) -> Self {
        Self {
            #[cfg(unix)]
            meshes: HashMap::new(),
            #[cfg(unix)]
            fonts_installed: std::collections::HashSet::new(),
            docs: HashMap::new(),
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

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn destroy_surface(&mut self, surface_id: u32) {
        #[cfg(unix)]
        {
            self.meshes.remove(&surface_id);
            self.fonts_installed.remove(&surface_id);
        }
        self.docs.remove(&surface_id);
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "markdown.reload" => self.markdown_reload(&ctx.params),
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
    #[cfg(unix)]
    fn paint(&mut self, ctx: SurfaceSetContextCtx) {
        let sid = ctx.params.surface_id;

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

        let link_slot = egui::Id::new(("md_link_click", sid));
        // 본문 폰트 크기: host 는 surface EffectiveFont.font_size 를 썼다(폰트 parity defer).
        // body 토큰으로 대체한다 — 디자인 본문 크기와 동일.
        let body_px = theme.font_size_body.value();
        let style = MdStyle::new(&theme, body_px, base_dir, link_slot);

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

        let result = mesh.paint(&ctx.host, &ctx.params, |egui_ctx| {
            draw(
                egui_ctx,
                &theme,
                &style,
                &content,
                load_error.as_deref(),
                tr,
            );
        });
        if let Err(e) = result {
            tracing::warn!("markdown surface {sid} paint failed: {e}");
        }

        // egui run 중 stash 된 링크 클릭을 꺼내 dispatch (file → host, url → OS).
        let click = mesh.context().data_mut(|d| {
            let c = d.get_temp::<LinkClick>(link_slot);
            if c.is_some() {
                d.remove::<LinkClick>(link_slot);
            }
            c
        });
        if let Some(click) = click {
            dispatch_link(&ctx.host, sid, click);
        }
    }

    /// egui-mesh shared-buffer 송신은 현재 unix 전용(host buffer.rs 가 windows 미구현).
    /// 다른 OS 에선 채널이 비활성이라 no-op — 크로스플랫폼 컴파일만 보장한다.
    #[cfg(not(unix))]
    fn paint(&mut self, _ctx: SurfaceSetContextCtx) {}
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
#[cfg(unix)]
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

/// egui closure: scroll area + content render, mirroring the former host `draw_markdown`.
fn draw(
    ctx: &egui::Context,
    theme: &Theme,
    style: &MdStyle,
    content: &str,
    load_error: Option<&str>,
    tr: &Translator,
) {
    let frame = egui::Frame::new()
        .fill(theme.base.to_egui())
        .inner_margin(egui::Margin::same(8));
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
                render::render(ui, style, content);
                // Trailing space so the last line doesn't collide with the bottom margin.
                ui.add_space(8.0);
            });
    });
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
#[cfg(unix)]
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
#[cfg(unix)]
fn load_system_cjk_font_data() -> Option<Vec<u8>> {
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
