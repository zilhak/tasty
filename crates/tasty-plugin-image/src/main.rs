#![forbid(unsafe_code)]

//! Tasty Image plugin — **egui-mesh + bitmap-texture** image surface (ADR-0028 / B2).
//!
//! The plugin owns the image content and renders it in its own process (mirroring B1
//! markdown): it loads the bitmap (delivered via `surface.create`), uploads it to its own
//! egui `Context` as a texture, draws the viewer / paint chrome from the host `Theme`
//! tokens (delivered each frame via `set_context`), tessellates the mesh, and the host
//! composites it over the surface region. The bitmap texture flows through the same mesh
//! `textures_delta` channel as the font atlas — uploaded once and cached in the host's
//! per-surface `egui_wgpu::Renderer` — so there is no separate host Canvas layer.
//!
//! Edit state (brush strokes, undo/redo, paste→floating selection→commit/Esc) lives in the
//! plugin ([`doc::ImageDoc`]). `image.save`/`export`/`paste`/`next`/`prev` operate on that
//! state directly; `image.open` (surface conversion) and `image.list` (host surface
//! enumeration) trampoline to the host. The former host `ImageView` render path stays
//! compiled until C1 removes it.

mod doc;
#[cfg(any(unix, windows))]
mod render;

use std::collections::HashMap;
use std::sync::Arc;

use doc::ImageDoc;
use serde_json::{Value, json};
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, SurfaceCreateCtx, SurfaceResult,
    SurfaceSetContextCtx, Translator, host::HostHandle,
};
use tasty_type_appearance::theme::Theme;

#[cfg(any(unix, windows))]
use tasty_plugin_sdk::EguiMeshSurface;

const PLUGIN_ID: &str = "com.tasty.image";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

struct ImagePlugin {
    /// surface_id → plugin egui render state (font atlas + shared buffer; unix-only paint).
    #[cfg(any(unix, windows))]
    meshes: HashMap<u32, EguiMeshSurface>,
    /// surface_id 들 중 폰트(CJK fallback)를 이미 설치한 것 — set_fonts 재업로드 방지.
    #[cfg(any(unix, windows))]
    fonts_installed: std::collections::HashSet<u32>,
    /// surface_id → image document state.
    docs: HashMap<u32, ImageDoc>,
    /// plugin lang 카탈로그 (UI 문자열).
    tr: Translator,
}

impl ImagePlugin {
    fn new(tr: Translator) -> Self {
        Self {
            #[cfg(any(unix, windows))]
            meshes: HashMap::new(),
            #[cfg(any(unix, windows))]
            fonts_installed: std::collections::HashSet::new(),
            docs: HashMap::new(),
            tr,
        }
    }
}

impl Plugin for ImagePlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        // egui-mesh surface: no tree — load the file and return an empty result. The SDK
        // hands the full `surface.create` envelope as `ctx.params`; the real params
        // (`file`) are nested under `params.params`.
        let file = surface_param_file(&ctx.params);
        self.docs.insert(ctx.surface_id, ImageDoc::new(file));
        SurfaceResult::default()
    }

    fn destroy_surface(&mut self, surface_id: u32) {
        #[cfg(any(unix, windows))]
        {
            self.meshes.remove(&surface_id);
            self.fonts_installed.remove(&surface_id);
        }
        self.docs.remove(&surface_id);
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            // Surface conversion + host enumeration stay host-owned (self-call trampoline).
            "image.open" | "image.list" => trampoline(&ctx.host, &ctx.method, ctx.params),
            // Edit / navigation operate on plugin-owned document state. These change the
            // document out-of-band (no user input), so after a successful mutation we
            // self-repaint the last context (empty input) — otherwise the new image/paste
            // wouldn't show until the next user input (egui-mesh re-forward gap, option A).
            "image.save" | "image.export_png" => {
                let out = self.image_save(&ctx.params)?;
                self.repaint_after_edit(&ctx.host, &ctx.params);
                Ok(out)
            }
            "image.paste" => {
                let out = self.image_paste(&ctx.params)?;
                self.repaint_after_edit(&ctx.host, &ctx.params);
                Ok(out)
            }
            "image.next" => {
                let out = self.image_step(&ctx.params, true)?;
                self.repaint_after_edit(&ctx.host, &ctx.params);
                Ok(out)
            }
            "image.prev" => {
                let out = self.image_step(&ctx.params, false)?;
                self.repaint_after_edit(&ctx.host, &ctx.params);
                Ok(out)
            }
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn paint_surface(&mut self, ctx: SurfaceSetContextCtx) {
        self.paint(ctx);
    }
}

impl ImagePlugin {
    fn image_save(&mut self, params: &Value) -> Result<Value, IpcMethodError> {
        let sid = require_surface(params)?;
        let explicit = params
            .get("path")
            .and_then(|v| v.as_str())
            .map(String::from);
        let doc = self.docs.get_mut(&sid).ok_or_else(|| {
            IpcMethodError::invalid_params(&format!("Surface {sid} is not an image"))
        })?;
        let final_path = match explicit.or_else(|| doc.save_path()) {
            Some(p) => p,
            None => {
                return Err(IpcMethodError::invalid_params(
                    "No save path: provide 'path' or open a file first",
                ));
            }
        };
        match doc.save_png(&final_path) {
            Ok(()) => {
                if doc.is_blank() {
                    doc.file_path = Some(final_path.clone());
                }
                Ok(json!({ "ok": true, "path": final_path }))
            }
            Err(e) => Err(IpcMethodError::new(format!("save failed: {e}"))),
        }
    }

    fn image_paste(&mut self, params: &Value) -> Result<Value, IpcMethodError> {
        let sid = require_surface(params)?;
        let color_image = read_clipboard_image()?;
        let doc = self.docs.get_mut(&sid).ok_or_else(|| {
            IpcMethodError::invalid_params(&format!("Surface {sid} is not an image"))
        })?;
        doc.ensure_loaded();
        doc.paste_image(color_image);
        Ok(json!({ "ok": true, "surface_id": sid }))
    }

    fn image_step(&mut self, params: &Value, forward: bool) -> Result<Value, IpcMethodError> {
        let sid = require_surface(params)?;
        let doc = self.docs.get_mut(&sid).ok_or_else(|| {
            IpcMethodError::invalid_params(&format!("Surface {sid} is not an image"))
        })?;
        doc.ensure_loaded();
        let new_path = if forward {
            doc.step_next()
        } else {
            doc.step_prev()
        };
        match new_path {
            Some(path) => {
                doc.load_after_navigation();
                Ok(json!({ "ok": true, "path": path }))
            }
            None => Err(IpcMethodError::invalid_params(
                "No sibling images available",
            )),
        }
    }

    /// `set_context` 한 frame 을 그려 host 에 mesh 를 회신한다.
    #[cfg(any(unix, windows))]
    fn paint(&mut self, ctx: SurfaceSetContextCtx) {
        let sid = ctx.params.surface_id;

        // host 가 Theme 을 아직 안 보냈으면 토큰을 풀 수 없으므로 이 frame 은 건너뛴다.
        let Some(theme) = ctx.params.theme.as_ref().map(theme_from_wire) else {
            tracing::debug!("image surface {sid}: set_context without theme — skipping paint");
            return;
        };

        let tr = &self.tr;

        let doc = self.docs.entry(sid).or_insert_with(|| ImageDoc::new(None));
        doc.ensure_loaded();
        doc.ensure_brush_themed(theme.accent_danger().to_egui());

        let is_new = !self.meshes.contains_key(&sid);
        let mesh = self
            .meshes
            .entry(sid)
            .or_insert_with(|| EguiMeshSurface::new(sid));
        if is_new {
            // 한글/일문 파일명·라벨이 tofu(□) 되지 않도록 CJK fallback 을 설치한다.
            install_fonts(mesh.context());
            self.fonts_installed.insert(sid);
        }

        let result = mesh.paint(&ctx.host, &ctx.params, |egui_ctx| {
            render::draw(egui_ctx, &theme, tr, doc);
        });
        if let Err(e) = result {
            tracing::warn!("image surface {sid} paint failed: {e}");
        }
    }

    /// 편집/탐색 IPC 로 doc 이 out-of-band 로 바뀐 뒤, **입력 없이** 화면을 갱신한다(옵션 A).
    /// 마지막 set_context 의 캐시된 컨텍스트(geom/ppp/theme)로 빈 입력 재-paint → 출력이
    /// 바뀌면 host 로 PaintFrame 을 송신한다. theme 미수신(첫 set_context 전)이면 no-op.
    #[cfg(any(unix, windows))]
    fn repaint_after_edit(&mut self, host: &HostHandle, params: &Value) {
        let Ok(sid) = require_surface(params) else {
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
        let Some(doc) = self.docs.get_mut(&sid) else {
            return;
        };
        doc.ensure_loaded();
        doc.ensure_brush_themed(theme.accent_danger().to_egui());
        let Some(mesh) = self.meshes.get_mut(&sid) else {
            return;
        };
        let result = mesh.repaint_last(host, |egui_ctx| {
            render::draw(egui_ctx, &theme, tr, doc);
        });
        if let Err(e) = result {
            tracing::warn!("image surface {sid} repaint failed: {e}");
        }
    }

    /// egui-mesh shared-buffer 송신은 현재 unix 전용(host buffer.rs 가 windows 미구현).
    /// 다른 OS 에선 채널이 비활성이라 no-op — 크로스플랫폼 컴파일만 보장한다.
    #[cfg(not(any(unix, windows)))]
    fn paint(&mut self, _ctx: SurfaceSetContextCtx) {}

    /// unix 외에는 egui-mesh 채널이 비활성이라 재-paint 도 no-op.
    #[cfg(not(any(unix, windows)))]
    fn repaint_after_edit(&mut self, _host: &HostHandle, _params: &Value) {}
}

/// `image.*` 메서드를 호스트의 동명 IPC로 위임한다. plugin manager가 self-call을
/// 호스트 dispatcher로 우회시키므로 무한 forward 루프는 발생하지 않는다.
fn trampoline(host: &HostHandle, method: &str, params: Value) -> Result<Value, IpcMethodError> {
    Ok(host.call(method, params)?)
}

/// Read `surface` from IPC params (all image.* methods take it explicitly — focus独立).
fn require_surface(params: &Value) -> Result<u32, IpcMethodError> {
    params
        .get("surface")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'surface'"))
}

/// Read the system clipboard image into a `ColorImage` (paste → floating selection).
fn read_clipboard_image() -> Result<egui::ColorImage, IpcMethodError> {
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| IpcMethodError::new(format!("clipboard open failed: {e}")))?;
    let image = cb
        .get_image()
        .map_err(|e| IpcMethodError::invalid_params(&format!("no image on clipboard: {e}")))?;
    // 외부 입력 (클립보드 이미지 바이트) → ColorImage 픽셀.
    #[allow(clippy::disallowed_methods)]
    let pixels: Vec<egui::Color32> = image
        .bytes
        .chunks_exact(4)
        .map(|c| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        .collect();
    Ok(egui::ColorImage {
        size: [image.width, image.height],
        pixels,
    })
}

/// surface.create envelope 에서 `file` 을 꺼낸다 (nested `params.file`, flat fallback).
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

/// plugin Context 에 CJK fallback 을 설치한다 (host `font_registry` 미러, B1 markdown 동일).
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
    // 언어팩 `[font]` 폰트를 CJK 뒤, 체인 맨 뒤 폴백으로 붙인다. host 두 경로와 같은
    // 판정기(`tasty_egui_theme::install_locale_font_fallback`)를 쓴다 — 검증이 곧 "어떤
    // 폰트를 거부하는가" 라는 판정이라 사본을 두면 host 는 받고 plugin 은 거부하는 갈림이
    // 생긴다. 경로는 host 가 resolve 해 `TASTY_LOCALE_FONT` 로 물려준 것(SDK
    // `PluginEnv.locale_font` 와 같은 출처).
    if let Some(path) = std::env::var_os("TASTY_LOCALE_FONT").filter(|v| !v.is_empty()) {
        let path = std::path::PathBuf::from(path);
        if let Err(e) = tasty_egui_theme::install_locale_font_fallback(&mut fonts, &path) {
            tracing::warn!(
                "locale font at {} could not be installed: {e}",
                path.display()
            );
        }
    }
    ctx.set_fonts(fonts);
}

/// 시스템 CJK 폰트 바이트 로드.
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
    tasty_plugin_sdk::run(ImagePlugin::new(tr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_param_file_reads_nested_and_flat() {
        assert_eq!(
            surface_param_file(&json!({ "params": { "file": "/a/b.png" } })).as_deref(),
            Some("/a/b.png")
        );
        assert_eq!(
            surface_param_file(&json!({ "file": "/c/d.png" })).as_deref(),
            Some("/c/d.png")
        );
        assert_eq!(surface_param_file(&json!({ "params": {} })), None);
    }

    #[test]
    fn create_surface_inserts_doc() {
        let mut p = ImagePlugin::new(Translator::default());
        p.create_surface(SurfaceCreateCtx {
            surface_id: 1,
            kind: "image".into(),
            cwd: None,
            params: json!({ "surface_id": 1, "kind": "image", "params": { "file": "/x/y.png" } }),
        });
        assert_eq!(
            p.docs.get(&1).unwrap().file_path.as_deref(),
            Some("/x/y.png")
        );
    }

    #[test]
    fn save_without_surface_is_invalid_params() {
        let mut p = ImagePlugin::new(Translator::default());
        let err = p.image_save(&json!({})).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn step_on_missing_surface_is_invalid_params() {
        let mut p = ImagePlugin::new(Translator::default());
        let err = p.image_step(&json!({ "surface": 7 }), true).unwrap_err();
        assert_eq!(err.code, -32602);
    }
}
