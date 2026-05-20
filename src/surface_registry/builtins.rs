//! 본체 7종 surface의 SurfaceKindDef 등록.
//!
//! 03D-A에서는 create/restore/snapshot 함수만 채운다. render/on_close는 추후 단계에서
//! 추가될 예정 (egui_panels.rs dispatch 통합과 함께).

use std::sync::Arc;

use serde_json::{Value, json};

use tasty_core::model::{DiffPanel, EmptySurface, HtmlPanel, ImagePanel, MarkdownPanel, Surface};

use super::{SurfaceKindDef, SurfaceKindRegistry};

/// 부팅 시 호출. EngineState 생성 직전에 빈 SurfaceKindRegistry에 호스트 내장 kind를 등록한다.
///
/// 등록되지 않는 kind:
/// - `"explorer"`: `com.tasty.explorer` plugin이 hello 시 remote kind로 등록.
/// - `"image"`: `com.tasty.image` plugin이 hello 시 `rendering = "host"` 매니페스트로
///   호스트 화이트리스트 매칭 후 [`register_image`]를 호출하여 등록.
pub fn register_builtin_kinds(registry: &SurfaceKindRegistry) {
    register_terminal(registry);
    register_markdown(registry);
    register_html(registry);
    register_empty(registry);
    register_diff(registry);
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
//
// Image kind는 `com.tasty.image` plugin이 `rendering = "host"`로 선언한
// host-rendered kind다. plugin manager가 매니페스트를 화이트리스트에 매칭한 뒤
// 본 함수를 호출해 실제 `SurfaceKindDef`를 호스트가 제공한다.

pub fn register_image(registry: &SurfaceKindRegistry) {
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

// ── Diff ────────────────────────────────────────────────────────────────────
//
// Diff surface 는 표시 전용. `before`/`after` 텍스트 또는 `before_file`/`after_file`
// 경로 둘 중 한 쌍을 받는다. `apply_action` 은 사용자 Apply 클릭 시 실행될 명령으로
// metadata 보관 — 도메인 layer 는 실행에 관여하지 않는다.

fn read_diff_input(params: &Value, text_key: &str, file_key: &str) -> anyhow::Result<String> {
    if let Some(s) = params.get(text_key).and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(path) = params.get(file_key).and_then(|v| v.as_str()) {
        return std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read '{path}' for diff surface failed: {e}"));
    }
    anyhow::bail!("diff surface requires '{text_key}' or '{file_key}'")
}

fn register_diff(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "diff",
        display_name_i18n_key: "surface.kind.diff",
        create: Arc::new(|sid, params| {
            let before = read_diff_input(params, "before", "before_file")?;
            let after = read_diff_input(params, "after", "after_file")?;
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let apply_action = params
                .get("apply_action")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            Ok(
                Box::new(DiffPanel::new(sid, title, before, after).with_apply_action(apply_action))
                    as Box<dyn Surface>,
            )
        }),
        restore: Arc::new(|sid, data| {
            let title = data
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let before = data
                .get("before")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let after = data
                .get("after")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let apply_action = data
                .get("apply_action")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            Ok(
                Box::new(DiffPanel::new(sid, title, before, after).with_apply_action(apply_action))
                    as Box<dyn Surface>,
            )
        }),
        snapshot: Arc::new(|s| {
            let d = s.as_any().downcast_ref::<DiffPanel>()?;
            Some(json!({
                "title": d.title,
                "before": d.before,
                "after": d.after,
                "apply_action": d.apply_action,
            }))
        }),
    });
}

// ── Empty ───────────────────────────────────────────────────────────────────

fn register_empty(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "empty",
        display_name_i18n_key: "surface.kind.empty",
        create: Arc::new(|sid, _params| Ok(Box::new(EmptySurface::new(sid)) as Box<dyn Surface>)),
        restore: Arc::new(|sid, _data| Ok(Box::new(EmptySurface::new(sid)) as Box<dyn Surface>)),
        snapshot: Arc::new(|_| Some(Value::Object(Default::default()))),
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
        // image kind는 register_builtin_kinds가 아닌 com.tasty.image plugin 활성화
        // 경로에서 등록된다. 본 테스트는 host가 제공하는 SurfaceKindDef 자체의
        // 동작을 검증하므로 직접 register_image를 호출한다.
        let reg = SurfaceKindRegistry::new();
        register_image(&reg);
        let def = reg.get("image").unwrap();
        let s = (def.create)(3, &json!({})).unwrap();
        let img = s.as_any().downcast_ref::<ImagePanel>().unwrap();
        assert!(img.is_blank());
    }

    #[test]
    fn builtin_kinds_no_longer_include_image() {
        let reg = registry_with_builtins();
        assert!(
            reg.get("image").is_none(),
            "image kind must be registered via com.tasty.image plugin, not register_builtin_kinds"
        );
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
    fn terminal_create_errors() {
        let reg = registry_with_builtins();
        let def = reg.get("terminal").unwrap();
        assert!((def.create)(1, &json!({})).is_err());
    }
}
