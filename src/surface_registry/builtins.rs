//! 본체 7종 surface의 SurfaceKindDef 등록.
//!
//! 03D-A에서는 create/restore/snapshot 함수만 채운다. render/on_close는 추후 단계에서
//! 추가될 예정 (egui_panels.rs dispatch 통합과 함께).

use std::sync::Arc;

use serde_json::{Value, json};

use tasty_core::model::{
    ClipboardViewerPanel, EmptySurface, HtmlPanel, ImagePanel, MarkdownPanel, Surface,
};

use super::{SurfaceKindDef, SurfaceKindRegistry};

/// 부팅 시 호출. EngineState 생성 직전에 빈 SurfaceKindRegistry에 호스트 내장 6종을 등록한다.
/// "explorer" kind는 `com.tasty.explorer` plugin이 hello를 보낼 때
/// `register_remote_kind`로 등록한다 — 호스트는 직접 등록하지 않는다.
pub fn register_builtin_kinds(registry: &SurfaceKindRegistry) {
    register_terminal(registry);
    register_markdown(registry);
    register_html(registry);
    register_image(registry);
    register_empty(registry);
    register_clipboard_viewer(registry);
}

// ── Terminal ────────────────────────────────────────────────────────────────
//
// Terminal은 PTY spawn이 호스트 책임이라 create/restore에서 자체적으로 surface를
// 생성하지 않는다. 호출자(IPC handler / split_pane_targeted / SavedSurface::Terminal
// 복원)가 별도 경로를 거치므로, 여기서는 안전한 sentinel만 반환한다.

fn register_terminal(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "terminal",
        display_name_i18n_key: "surface.kind.terminal",
        create: Arc::new(|_sid, _params| {
            anyhow::bail!("terminal surfaces require host-managed PTY spawn; use split_pane_targeted/add_terminal_tab")
        }),
        restore: Arc::new(|_sid, _data| {
            anyhow::bail!("terminal surfaces are restored via SavedSurface::Terminal, not Generic")
        }),
        snapshot: Arc::new(|_| None),
    });
}

// ── Markdown ────────────────────────────────────────────────────────────────

fn register_markdown(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "markdown",
        display_name_i18n_key: "surface.kind.markdown",
        create: Arc::new(|sid, params| {
            let file = params
                .get("file")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing 'file' for markdown surface"))?;
            Ok(Box::new(MarkdownPanel::new(sid, file.to_string())) as Box<dyn Surface>)
        }),
        restore: Arc::new(|sid, data| {
            let path = data
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing 'path' in markdown snapshot"))?;
            Ok(Box::new(MarkdownPanel::new(sid, path.to_string())) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|s: &dyn Surface| {
            let md = s.as_any().downcast_ref::<MarkdownPanel>()?;
            Some(json!({"path": md.file_path}))
        }),
    });
}

// ── Html ────────────────────────────────────────────────────────────────────

fn register_html(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "html",
        display_name_i18n_key: "surface.kind.html",
        create: Arc::new(|sid, params| {
            let url = params
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing 'url' for html surface"))?;
            Ok(Box::new(HtmlPanel::new(sid, url.to_string())) as Box<dyn Surface>)
        }),
        restore: Arc::new(|sid, data| {
            let url = data
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing 'url' in html snapshot"))?;
            Ok(Box::new(HtmlPanel::new(sid, url.to_string())) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|s| {
            let html = s.as_any().downcast_ref::<HtmlPanel>()?;
            Some(json!({"url": html.url}))
        }),
    });
}

// ── Image ───────────────────────────────────────────────────────────────────

fn register_image(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "image",
        display_name_i18n_key: "surface.kind.image",
        create: Arc::new(|sid, params| {
            let panel = match params.get("file").and_then(|v| v.as_str()) {
                Some(path) => ImagePanel::new(sid, path.to_string()),
                None => ImagePanel::new_blank(sid),
            };
            Ok(Box::new(panel) as Box<dyn Surface>)
        }),
        restore: Arc::new(|sid, data| {
            let panel = match data.get("path").and_then(|v| v.as_str()) {
                Some(path) => ImagePanel::new(sid, path.to_string()),
                None => ImagePanel::new_blank(sid),
            };
            Ok(Box::new(panel) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|s| {
            let img = s.as_any().downcast_ref::<ImagePanel>()?;
            // 빈 캔버스(미저장)은 path null로 직렬화 — 복원 시 다시 빈 캔버스.
            Some(json!({"path": img.file_path}))
        }),
    });
}

// ── Empty ───────────────────────────────────────────────────────────────────

fn register_empty(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "empty",
        display_name_i18n_key: "surface.kind.empty",
        create: Arc::new(|sid, _params| {
            Ok(Box::new(EmptySurface::new(sid)) as Box<dyn Surface>)
        }),
        restore: Arc::new(|sid, _data| {
            Ok(Box::new(EmptySurface::new(sid)) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|_| Some(Value::Object(Default::default()))),
    });
}

// ── ClipboardViewer ─────────────────────────────────────────────────────────

fn register_clipboard_viewer(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "clipboard_viewer",
        display_name_i18n_key: "surface.kind.clipboard_viewer",
        create: Arc::new(|sid, _params| {
            Ok(Box::new(ClipboardViewerPanel::new(sid)) as Box<dyn Surface>)
        }),
        // ClipboardViewer는 휘발성이라 영속화하지 않는다. 호출되면 빈 인스턴스 반환.
        restore: Arc::new(|sid, _data| {
            Ok(Box::new(ClipboardViewerPanel::new(sid)) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|_| None),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with_builtins() -> SurfaceKindRegistry {
        let mut r = SurfaceKindRegistry::new();
        register_builtin_kinds(&mut r);
        r
    }

    #[test]
    fn markdown_create_requires_file() {
        let reg = registry_with_builtins();
        let def = reg.get("markdown").unwrap();
        assert!((def.create)(1, &json!({})).is_err());
        let s = (def.create)(1, &json!({"file": "/tmp/x.md"})).unwrap();
        assert_eq!(s.kind(), "markdown");
        assert_eq!(s.surface_id(), Some(1));
    }

    #[test]
    fn markdown_snapshot_round_trips() {
        let reg = registry_with_builtins();
        let def = reg.get("markdown").unwrap();
        let s = (def.create)(7, &json!({"file": "/tmp/y.md"})).unwrap();
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert_eq!(snap["path"], "/tmp/y.md");
        let restored = (def.restore)(7, &snap).unwrap();
        assert_eq!(restored.kind(), "markdown");
    }

    #[test]
    fn html_create_requires_url() {
        let reg = registry_with_builtins();
        let def = reg.get("html").unwrap();
        assert!((def.create)(1, &json!({})).is_err());
        let s = (def.create)(1, &json!({"url": "https://example.com"})).unwrap();
        assert_eq!(s.kind(), "html");
    }

    #[test]
    fn image_create_blank_when_no_file() {
        let reg = registry_with_builtins();
        let def = reg.get("image").unwrap();
        let s = (def.create)(3, &json!({})).unwrap();
        let img = s.as_any().downcast_ref::<ImagePanel>().unwrap();
        assert!(img.is_blank());
    }

    #[test]
    fn empty_snapshot_returns_object() {
        let reg = registry_with_builtins();
        let def = reg.get("empty").unwrap();
        let s = (def.create)(1, &json!({})).unwrap();
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert!(snap.is_object());
    }

    #[test]
    fn clipboard_viewer_snapshot_is_none() {
        let reg = registry_with_builtins();
        let def = reg.get("clipboard_viewer").unwrap();
        let s = (def.create)(1, &json!({})).unwrap();
        assert!((def.snapshot)(s.as_ref()).is_none());
    }

    #[test]
    fn terminal_create_errors() {
        let reg = registry_with_builtins();
        let def = reg.get("terminal").unwrap();
        assert!((def.create)(1, &json!({})).is_err());
    }
}
