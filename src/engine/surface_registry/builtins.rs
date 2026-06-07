//! 본체 7종 surface의 SurfaceKindDef 등록.
//!
//! 03D-A에서는 create/restore/snapshot 함수만 채운다. render/on_close는 추후 단계에서
//! 추가될 예정 (egui_panels.rs dispatch 통합과 함께).

use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::{EmptySurface, ImagePanel, MarkdownPanel, Surface};

use super::{SurfaceKindDef, SurfaceKindRegistry};

/// 부팅 시 호출. CoreState 생성 직전에 빈 SurfaceKindRegistry에 호스트 내장 kind를 등록한다.
///
/// 등록되지 않는 kind:
/// - `"explorer"`: `com.tasty.explorer` plugin이 hello 시 remote kind로 등록.
/// - `"image"`: `com.tasty.image` plugin이 hello 시 `rendering = "host"` 매니페스트로
///   호스트 화이트리스트 매칭 후 [`register_image`]를 호출하여 등록.
/// - `"markdown"`: `com.tasty.markdown` plugin이 hello 시 host_rendered whitelist
///   매칭 후 [`register_markdown`]을 호출하여 등록.
pub fn register_builtin_kinds(registry: &SurfaceKindRegistry) {
    register_terminal(registry);
    register_empty(registry);
    register_attached(registry);
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
        create: Arc::new(|_sid, _cwd, _params| {
            anyhow::bail!("terminal surfaces require host-managed PTY spawn; use split_pane_targeted/add_terminal_tab")
        }),
        restore: Arc::new(|_sid, _data| {
            anyhow::bail!("terminal surfaces are restored via SavedSurface::Terminal, not Generic")
        }),
        snapshot: Arc::new(|_| None),
    });
}

// ── Markdown ────────────────────────────────────────────────────────────────

pub(crate) fn register_markdown(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "markdown",
        display_name_i18n_key: "surface.kind.markdown",
        create: Arc::new(|sid, _cwd, params| {
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

// ── Image ───────────────────────────────────────────────────────────────────
//
// Image kind는 `com.tasty.image` plugin이 `rendering = "host"`로 선언한
// host-rendered kind다. plugin manager가 매니페스트를 화이트리스트에 매칭한 뒤
// 본 함수를 호출해 실제 `SurfaceKindDef`를 호스트가 제공한다.

pub fn register_image(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "image",
        display_name_i18n_key: "surface.kind.image",
        create: Arc::new(|sid, _cwd, params| {
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

// ── Attached ──────────────────────────────────────────────────────────────
//
// attached surface 는 attach 핸들러(단계 4)가 직접 생성하는 런타임 marker
// (배타 점유 lock 의 양쪽 표현 — placeholder/mirror). create/restore 경로 없음
// (sentinel bail). decision 2(휘발성): snapshot=None 으로 layout.json 에서 제외 —
// 재시작 시 내부 Terminal 이 일반 `SavedSurface::Terminal` 로 free 환원된다.

fn register_attached(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "attached",
        display_name_i18n_key: "surface.kind.attached",
        create: Arc::new(|_sid, _cwd, _params| {
            anyhow::bail!(
                "attached surfaces are created by the attach handler, not via registry create"
            )
        }),
        restore: Arc::new(|_sid, _data| {
            anyhow::bail!("attached surfaces are volatile (decision 2); not restored")
        }),
        snapshot: Arc::new(|_| None),
    });
}

// ── Empty ───────────────────────────────────────────────────────────────────

fn register_empty(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "empty",
        display_name_i18n_key: "surface.kind.empty",
        create: Arc::new(|sid, cwd, _params| {
            Ok(
                Box::new(EmptySurface::new(sid).with_cwd(cwd.map(std::path::PathBuf::from)))
                    as Box<dyn Surface>,
            )
        }),
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
        // markdown kind는 register_builtin_kinds가 아닌 com.tasty.markdown plugin
        // 활성화 경로에서 등록된다. host가 제공하는 SurfaceKindDef 자체의 동작을
        // 검증하므로 직접 register_markdown을 호출한다.
        let reg = SurfaceKindRegistry::new();
        register_markdown(&reg);
        let def = reg.get("markdown").unwrap();
        assert!((def.create)(1, None, &json!({})).is_err());
        let s = (def.create)(1, None, &json!({"file": "/tmp/x.md"})).unwrap();
        assert_eq!(s.kind(), "markdown");
        assert_eq!(s.surface_id(), Some(1));
    }

    #[test]
    fn markdown_snapshot_round_trips() {
        let reg = SurfaceKindRegistry::new();
        register_markdown(&reg);
        let def = reg.get("markdown").unwrap();
        let s = (def.create)(7, None, &json!({"file": "/tmp/y.md"})).unwrap();
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert_eq!(snap["path"], "/tmp/y.md");
        let restored = (def.restore)(7, &snap).unwrap();
        assert_eq!(restored.kind(), "markdown");
    }

    #[test]
    fn image_create_blank_when_no_file() {
        // image kind는 register_builtin_kinds가 아닌 com.tasty.image plugin 활성화
        // 경로에서 등록된다. 본 테스트는 host가 제공하는 SurfaceKindDef 자체의
        // 동작을 검증하므로 직접 register_image를 호출한다.
        let reg = SurfaceKindRegistry::new();
        register_image(&reg);
        let def = reg.get("image").unwrap();
        let s = (def.create)(3, None, &json!({})).unwrap();
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
    fn builtin_kinds_no_longer_include_markdown() {
        let reg = registry_with_builtins();
        assert!(
            reg.get("markdown").is_none(),
            "markdown kind must be registered via com.tasty.markdown plugin, not register_builtin_kinds"
        );
    }

    #[test]
    fn empty_snapshot_returns_object() {
        let reg = registry_with_builtins();
        let def = reg.get("empty").unwrap();
        let s = (def.create)(1, None, &json!({})).unwrap();
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert!(snap.is_object());
    }

    #[test]
    fn empty_carries_cwd_from_create() {
        // Surface cwd invariant: SurfaceKindDef.create 가 받은 cwd 가 EmptySurface
        // 본체에 carry 되어 source_cwd() 로 그대로 노출되어야 한다.
        let reg = registry_with_builtins();
        let def = reg.get("empty").unwrap();
        let cwd = std::path::PathBuf::from("/tmp/carry-test");
        let s = (def.create)(1, Some(cwd.as_path()), &json!({})).unwrap();
        assert_eq!(s.source_cwd().as_deref(), Some(cwd.as_path()));
    }

    #[test]
    fn terminal_create_errors() {
        let reg = registry_with_builtins();
        let def = reg.get("terminal").unwrap();
        assert!((def.create)(1, None, &json!({})).is_err());
    }

    #[test]
    fn attached_is_volatile_sentinel() {
        // attached kind 는 attach 핸들러가 직접 생성하는 런타임 marker —
        // create/restore 는 sentinel bail, snapshot 은 휘발성(None, decision 2).
        let reg = registry_with_builtins();
        let def = reg.get("attached").unwrap();
        assert!((def.create)(1, None, &json!({})).is_err());
        assert!((def.restore)(1, &json!({})).is_err());
        assert!((def.snapshot)(&crate::model::EmptySurface::new(1)).is_none());
    }
}
