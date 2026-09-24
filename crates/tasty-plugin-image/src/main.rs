#![forbid(unsafe_code)]

//! 이미지 픽셀과 편집 상태를 플러그인에서 관리하고 egui-mesh로 그린다.
//! 저장·붙여넣기·파일 탐색은 문서 상태를 바꾸며, 화면 변환·목록 조회는 호스트에 맡긴다.

// 시험의 let _는 제품 코드에서 반환값을 버리는 목록에 포함하지 않는다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod doc;
#[cfg(any(unix, windows))]
mod render;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;

use doc::ImageDoc;
use serde_json::{Value, json};
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::file_watch::{self, StatGatedDigest, WatchCmd};
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
    /// 화면별 egui 렌더 상태와 공유 버퍼.
    #[cfg(any(unix, windows))]
    meshes: HashMap<u32, EguiMeshSurface>,
    /// surface_id 들 중 폰트(CJK fallback)를 이미 설치한 것 — set_fonts 재업로드 방지.
    #[cfg(any(unix, windows))]
    fonts_installed: std::collections::HashSet<u32>,
    /// surface_id → image document state.
    docs: HashMap<u32, ImageDoc>,
    /// plugin lang 카탈로그 (UI 문자열).
    tr: Translator,
    /// 외부 변경 감시 스레드에 파일 등록·해제를 보내는 채널.
    watch_tx: Option<mpsc::Sender<WatchCmd>>,
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
            watch_tx: None,
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

    fn on_start(&mut self, host: HostHandle, _bus: tasty_plugin_sdk::BusHandle) {
        // 입력이 없을 때도 외부 저장을 반영하도록 별도 스레드에서 감시한다.
        // 메타데이터가 바뀔 때만 내용을 비교해 불필요한 읽기·디코딩을 줄인다.
        let (tx, rx) = mpsc::channel();
        self.watch_tx = Some(tx);
        if let Err(e) = std::thread::Builder::new()
            .name("image-watch".to_string())
            .spawn(move || file_watch::run::<StatGatedDigest>(host, rx, "image.reload"))
        {
            tracing::warn!("image watch worker spawn failed — idle auto-reload disabled: {e}");
        }
    }

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        // SDK가 전달한 전체 요청에서 중첩된 파일 파라미터를 읽는다.
        let file = surface_param_file(&ctx.params);
        self.watch_register(ctx.surface_id, file.clone());
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
        self.watch_unregister(surface_id);
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            // 화면 변환과 전체 목록 조회는 호스트에 맡긴다.
            "image.open" | "image.list" => trampoline(&ctx.host, &ctx.method, ctx.params),
            // 사용자 입력 없이 문서가 바뀌므로 마지막 컨텍스트로 다시 그린다.
            "image.save" | "image.export_png" => {
                let out = self.image_save(&ctx.params)?;
                self.repaint_after_edit(&ctx.host, &ctx.params);
                Ok(out)
            }
            // 파일 읽기와 문서 변경은 플러그인 처리 스레드에서 수행한다.
            "image.reload" => {
                let out = self.image_reload(&ctx.params)?;
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
                // 직접 저장한 내용을 외부 변경으로 다시 읽지 않도록 감시 기준을 갱신한다.
                let watched = doc.file_path.clone();
                self.watch_register(sid, watched);
                Ok(json!({ "ok": true, "path": final_path }))
            }
            Err(e) => Err(IpcMethodError::new(format!("save failed: {e}"))),
        }
    }

    /// 파일을 다시 읽어 문서에 반영한다. 편집 중이면 미뤄 둔다(`apply_external_change`).
    fn image_reload(&mut self, params: &Value) -> Result<Value, IpcMethodError> {
        let sid = require_surface(params)?;
        let doc = self.docs.get_mut(&sid).ok_or_else(|| {
            IpcMethodError::invalid_params(&format!("Surface {sid} is not an image"))
        })?;
        let applied = doc.apply_external_change();
        Ok(json!({ "ok": true, "surface_id": sid, "applied": applied }))
    }

    /// 감시 파일과 비교 기준을 등록·갱신한다.
    fn watch_register(&self, surface_id: u32, path: Option<String>) {
        let Some(tx) = &self.watch_tx else { return };
        if tx.send(WatchCmd::Register { surface_id, path }).is_err() {
            tracing::warn!(
                "image watch: register send failed for surface {surface_id} (worker gone)"
            );
        }
    }

    fn watch_unregister(&self, surface_id: u32) {
        let Some(tx) = &self.watch_tx else { return };
        if tx.send(WatchCmd::Unregister { surface_id }).is_err() {
            tracing::warn!(
                "image watch: unregister send failed for surface {surface_id} (worker gone)"
            );
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
                // 선택한 파일로 감시 대상도 바꾼다.
                self.watch_register(sid, Some(path.clone()));
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
            // 인스턴스마다 CJK 대체 폰트를 한 번 설치한다.
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

    /// IPC로 바뀐 문서를 마지막 컨텍스트와 빈 입력으로 다시 그린다. 테마가 없으면 생략한다.
    #[cfg(any(unix, windows))]
    fn repaint_after_edit(&mut self, host: &HostHandle, params: &Value) {
        let Ok(sid) = require_surface(params) else {
            return;
        };
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

    /// Unix·Windows 외에는 공유 버퍼 그리기를 생략한다.
    #[cfg(not(any(unix, windows)))]
    fn paint(&mut self, _ctx: SurfaceSetContextCtx) {}

    /// Unix·Windows 외에는 다시 그리기도 생략한다.
    #[cfg(not(any(unix, windows)))]
    fn repaint_after_edit(&mut self, _host: &HostHandle, _params: &Value) {}
}

/// 같은 이름의 호스트 메서드를 호출한다. 호스트가 이 자기 호출을 내부 처리기로 보낸다.
fn trampoline(host: &HostHandle, method: &str, params: Value) -> Result<Value, IpcMethodError> {
    Ok(host.call(method, params)?)
}

/// IPC의 surface 인자를 읽는다.
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

/// 전달받은 색·밝기·확대 비율로 테마를 만든다.
fn theme_from_wire(w: &ThemeWire) -> Theme {
    Theme::with_colors_and_zoom(w.colors.clone(), w.is_light, w.ui_zoom)
}

/// 구할 수 있는 시스템 CJK 폰트를 대체 폰트로 추가한다.
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
    // 호스트와 같은 검사 함수로 언어팩 폰트를 대체 폰트 목록 끝에 추가한다.
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
        // 맑은 고딕이 없으면 추가하지 않는다.
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
