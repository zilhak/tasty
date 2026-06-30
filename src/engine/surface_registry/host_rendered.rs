//! Host-rendered surface kind 화이트리스트 + 등록.
//!
//! plugin 매니페스트의 `[[surface_kinds]]`에 `rendering = "host"`를 선언하면
//! plugin은 UiNode tree를 보내지 않고, 호스트 본문이 egui로 직접 surface를 그린다.
//! plugin은 manifest 등록(이름/i18n/icon)과 IPC namespace만 담당한다.
//!
//! 안전을 위해 임의 plugin이 호스트 내부 kind를 가로채지 못하도록 화이트리스트로
//! 매칭되는 `(kind, plugin_id)` 쌍만 허용한다. 매칭 실패 시 등록을 거부하고 warn
//! 로그를 남긴다.

use crate::engine::surface_registry::{SurfaceKindRegistry, builtins};

/// `(kind, plugin_id)` 쌍이 host-rendered로 허용된 조합인지 확인.
///
/// markdown 은 B1(ADR-0028)에서 egui-mesh 채널로 전환돼 더 이상 host-rendered 가
/// 아니다(`surface_registry/egui_mesh.rs` 화이트리스트). image 는 비트맵 하이브리드
/// 전환(B2) 전까지 host-rendered 로 남는다.
fn is_host_rendered_allowed(kind: &str, plugin_id: &str) -> bool {
    matches!((kind, plugin_id), ("image", "com.tasty.image"))
}

/// plugin manager가 hello 직후 매니페스트에 `rendering = "host"` 선언이 있을 때 호출.
///
/// 화이트리스트에 매칭되는 kind이고 아직 registry에 등록되지 않았으면 호스트가
/// 정의한 `SurfaceKindDef`를 등록한다. 이미 등록돼 있으면 idempotent하게 no-op
/// (호스트 부팅 시 `register_builtin_kinds`가 미리 등록한 경우).
///
/// 반환값: 화이트리스트 매칭 성공이면 `true`. 매칭 실패면 `false` (warn 로그 + 등록 거부).
pub fn register_host_rendered_kind(
    registry: &SurfaceKindRegistry,
    plugin_id: &str,
    kind: &str,
) -> bool {
    if !is_host_rendered_allowed(kind, plugin_id) {
        tracing::warn!(
            "plugin '{}' declared host-rendered kind '{}' which is not allowed by the \
             host whitelist; ignoring",
            plugin_id,
            kind
        );
        return false;
    }
    if registry.contains(kind) {
        // 호스트가 부팅 시 이미 등록한 경우: 그대로 사용.
        tracing::debug!(
            "host-rendered kind '{}' from plugin '{}' already registered by host",
            kind,
            plugin_id
        );
        return true;
    }
    match kind {
        "image" => builtins::register_image(registry),
        _ => unreachable!("whitelist guard ensures only known kinds reach here"),
    }
    tracing::info!(
        "registered host-rendered surface kind '{}' for plugin '{}'",
        kind,
        plugin_id
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_kind_allowed_for_image_plugin() {
        assert!(is_host_rendered_allowed("image", "com.tasty.image"));
    }

    #[test]
    fn image_kind_rejected_for_other_plugin() {
        assert!(!is_host_rendered_allowed("image", "com.example.evil"));
    }

    #[test]
    fn unknown_kind_rejected() {
        assert!(!is_host_rendered_allowed("explorer", "com.tasty.image"));
        assert!(!is_host_rendered_allowed("foo", "com.tasty.foo"));
    }

    #[test]
    fn register_returns_false_for_unauthorized() {
        let reg = SurfaceKindRegistry::new();
        assert!(!register_host_rendered_kind(
            &reg,
            "com.example.evil",
            "image"
        ));
        assert!(!reg.contains("image"));
    }

    #[test]
    fn register_succeeds_for_image_plugin() {
        let reg = SurfaceKindRegistry::new();
        assert!(register_host_rendered_kind(
            &reg,
            "com.tasty.image",
            "image"
        ));
        assert!(reg.contains("image"));
    }

    #[test]
    fn markdown_kind_no_longer_host_rendered() {
        // markdown 은 B1(ADR-0028)에서 egui-mesh 로 전환 — host-rendered 화이트리스트에서 빠졌다.
        assert!(!is_host_rendered_allowed("markdown", "com.tasty.markdown"));
    }

    #[test]
    fn register_is_idempotent() {
        let reg = SurfaceKindRegistry::new();
        assert!(register_host_rendered_kind(
            &reg,
            "com.tasty.image",
            "image"
        ));
        // 두 번째 호출: registry에 이미 있으므로 no-op + 성공 반환.
        assert!(register_host_rendered_kind(
            &reg,
            "com.tasty.image",
            "image"
        ));
        assert!(reg.contains("image"));
    }
}
