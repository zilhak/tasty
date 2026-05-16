//! IPC 메서드별 메타데이터 — plugin이 호출 가능한지, 어떤 권한이 필요한지.
//!
//! 이 테이블이 **단일 진실 원천**이다. 새 IPC 메서드를 추가할 때 반드시
//! 여기에도 등록한다. 매핑되지 않은 메서드는 [`method_meta`]가 `None`을 반환하며,
//! `CallerContext::ensure_allowed`가 plugin 호출을 거부한다.
//!
//! Local caller(CLI/사용자)는 권한 검사를 거치지 않는다. 이 테이블은 **plugin이
//! 호출했을 때**의 권한 요구사항이다.

use crate::plugin::manifest::Permission;

/// 한 IPC 메서드에 대한 권한 메타.
#[derive(Debug, Clone, Copy)]
pub struct MethodMeta {
    /// plugin이 이 메서드를 호출할 수 있는지. false면 plugin은 어떤 경우에도 호출 불가.
    pub plugin_callable: bool,
    /// plugin이 호출하려면 매니페스트에 이 권한들이 모두 선언돼 있어야 함.
    pub required: &'static [Permission],
}

const fn plugin(required: &'static [Permission]) -> MethodMeta {
    MethodMeta {
        plugin_callable: true,
        required,
    }
}

const fn local_only() -> MethodMeta {
    MethodMeta {
        plugin_callable: false,
        required: &[],
    }
}

/// 등록된 IPC 메서드 — 단일 진실 원천. lint/검증 테스트가 이 테이블 위에서
/// 동작한다. 새 메서드는 여기에 추가한다.
///
/// prefix-기반 fallback(`surface.ime_*` 등)은 [`PREFIX_RULES`] 참조.
pub const METHOD_TABLE: &[(&str, MethodMeta)] = {
    use Permission::*;
    &[
        // ── 호스트 system ─────────────────────────────────────────────
        ("system.info", plugin(&[])),
        // ── workspace (read/write) ────────────────────────────────────
        ("workspace.list", plugin(&[SurfaceRead])),
        ("workspace.create", plugin(&[SurfaceWrite])),
        ("workspace.update", plugin(&[SurfaceWrite])),
        ("workspace.move", plugin(&[SurfaceWrite])),
        // ── pane / split ──────────────────────────────────────────────
        ("pane.list", plugin(&[SurfaceRead])),
        ("pane.close", plugin(&[SurfaceWrite])),
        ("split", plugin(&[SurfaceWrite])),
        // ── tab ───────────────────────────────────────────────────────
        ("tab.list", plugin(&[SurfaceRead])),
        ("tab.create", plugin(&[SurfaceWrite])),
        ("tab.close", plugin(&[SurfaceWrite])),
        ("tab.move", plugin(&[SurfaceWrite])),
        // ── surface (구조 조작) ───────────────────────────────────────
        ("surface.list", plugin(&[SurfaceRead])),
        ("surface.close", plugin(&[SurfaceWrite])),
        ("surface.close_self", plugin(&[SurfaceWrite])),
        // tree/meta는 read 권한
        ("tree", plugin(&[SurfaceRead])),
        ("surface.meta.get", plugin(&[SurfaceRead])),
        ("surface.meta.list", plugin(&[SurfaceRead])),
        ("surface.meta.set", plugin(&[SurfaceWrite])),
        ("surface.meta.unset", plugin(&[SurfaceWrite])),
        // ── terminal I/O ──────────────────────────────────────────────
        ("surface.send", plugin(&[TerminalWrite])),
        ("surface.send_key", plugin(&[TerminalWrite])),
        ("surface.send_combo", plugin(&[TerminalWrite])),
        ("surface.send_to", plugin(&[TerminalWrite])),
        ("surface.send_wait_idle", plugin(&[TerminalWrite])),
        ("surface.wake", plugin(&[TerminalSpawn])),
        ("surface.set_mark", plugin(&[TerminalRead])),
        ("surface.read_since_mark", plugin(&[TerminalRead])),
        ("surface.screen_text", plugin(&[TerminalRead])),
        ("surface.cursor_position", plugin(&[TerminalRead])),
        ("surface.foreground_process", plugin(&[TerminalRead])),
        ("surface.locate", plugin(&[SurfaceRead])),
        ("surface.respawn_terminal", plugin(&[TerminalSpawn])),
        ("surface.is_typing", plugin(&[TerminalRead])),
        ("surface.fire_hook", plugin(&[SurfaceWrite])),
        // ── hooks ─────────────────────────────────────────────────────
        ("hook.set", plugin(&[SurfaceWrite])),
        ("hook.list", plugin(&[SurfaceRead])),
        ("hook.unset", plugin(&[SurfaceWrite])),
        ("global_hook.set", plugin(&[SurfaceWrite])),
        ("global_hook.list", plugin(&[SurfaceRead])),
        ("global_hook.unset", plugin(&[SurfaceWrite])),
        // ── message (surface 간 메시지 큐) ─────────────────────────────
        ("message.send", plugin(&[SurfaceWrite])),
        ("message.read", plugin(&[SurfaceRead])),
        ("message.count", plugin(&[SurfaceRead])),
        ("message.clear", plugin(&[SurfaceWrite])),
        // ── tool.clipboard ────────────────────────────────────────────
        ("tool.clipboard.list", plugin(&[ClipboardRead])),
        ("tool.clipboard.get", plugin(&[ClipboardRead])),
        ("tool.clipboard.paste", plugin(&[ClipboardWrite])),
        ("tool.clipboard.remove", plugin(&[ClipboardWrite])),
        ("tool.clipboard.clear", plugin(&[ClipboardWrite])),
        // ── image surface ─────────────────────────────────────────────
        // com.tasty.image plugin이 namespace를 점유하지만, 호스트 어댑터는
        // plugin 비활성 상태에서도 동작한다. plugin은 ipc.invoke:image 권한으로
        // 위 메서드들을 호출한다.
        ("image.open", plugin(&[SurfaceWrite, FsRead])),
        ("image.save", plugin(&[FsWrite])),
        ("image.export_png", plugin(&[FsWrite])),
        ("image.next", plugin(&[SurfaceWrite])),
        ("image.prev", plugin(&[SurfaceWrite])),
        ("image.paste", plugin(&[SurfaceWrite, ClipboardRead])),
        ("image.list", plugin(&[SurfaceRead])),
        // ── memory: regular (공유 네임스페이스, owner enforcement) ────
        ("memory.put", plugin(&[MemoryWrite])),
        ("memory.get", plugin(&[MemoryRead])),
        ("memory.delete", plugin(&[MemoryWrite])),
        ("memory.list", plugin(&[MemoryRead])),
        ("memory.exists", plugin(&[MemoryRead])),
        ("memory.count", plugin(&[MemoryRead])),
        ("memory.scopes", plugin(&[MemoryRead])),
        ("memory.stats", plugin(&[MemoryRead])),
        // ── memory: secret (plugin 별 사전 분할) ──────────────────────
        ("memory.secret.put", plugin(&[MemorySecret])),
        ("memory.secret.get", plugin(&[MemorySecret])),
        ("memory.secret.delete", plugin(&[MemorySecret])),
        ("memory.secret.list", plugin(&[MemorySecret])),
        ("memory.secret.exists", plugin(&[MemorySecret])),
        ("memory.secret.count", plugin(&[MemorySecret])),
        ("memory.secret.scopes", plugin(&[MemorySecret])),
        ("memory.secret.stats", plugin(&[MemorySecret])),
        // ── memory: 유지 보수 (host 전용) ─────────────────────────────
        ("memory.gc", local_only()),
        // ── notification ──────────────────────────────────────────────
        ("notification.list", plugin(&[Notification])),
        ("notification.create", plugin(&[Notification])),
        // ── popup (plugin → host) ─────────────────────────────────────
        // 자기 contribute popup 인스턴스를 명시적으로 닫는다. METHOD_POPUP_CLOSED
        // (host → plugin)와는 다른 방향. plugin은 자기 instance_id만 닫을 수 있다 —
        // 다른 plugin의 인스턴스 close 요청은 만들어진 응답에서 거부.
        ("popup.close", plugin(&[UiPopup])),
        // ── input source (macOS) ──────────────────────────────────────
        ("surface.switch_input_source", plugin(&[TerminalWrite])),
        ("surface.raw_key", plugin(&[TerminalWrite])),
        // ── 호스트 자체 메서드 (plugin/window 관리) — local-only ──────
        ("plugin.list", local_only()),
        ("plugin.show", local_only()),
        ("plugin.extension.list", local_only()),
        ("plugin.install", local_only()),
        ("plugin.remove", local_only()),
        ("plugin.enable", local_only()),
        ("plugin.disable", local_only()),
        ("plugin.permissions", local_only()),
        ("plugin.grant", local_only()),
        ("plugin.revoke", local_only()),
        ("window.create", local_only()),
        ("window.close", local_only()),
        ("window.focus", local_only()),
        ("window.list", local_only()),
    ]
};

/// debug 빌드에서만 등록되는 메서드. release에서는 [`method_meta`]가 `None`을
/// 반환해 IPC 표면에서 완전히 사라진다. 핸들러 함수 본체와 라우터 분기는
/// 이미 `#[cfg(debug_assertions)]`로 보호되어 있으므로, 표 등록만 게이트하면
/// 일관된 release 표면이 된다.
///
/// 카테고리:
/// - `system.shutdown` — 호스트 종료 (사용자가 직접 종료해야 하는 동작)
/// - `ui.state` / `ui.screenshot` — UI 상태 dump (디버깅용)
/// - `debug.*` — 사용자 입력 재현 / 디버그 dump
#[cfg(debug_assertions)]
pub const DEBUG_METHODS: &[(&str, MethodMeta)] = &[
    ("system.shutdown", local_only()),
    ("ui.state", local_only()),
    ("ui.screenshot", local_only()),
    ("debug.info", local_only()),
    ("debug.cell_info", local_only()),
    ("debug.screen_attrs", local_only()),
    ("debug.glyph_color", local_only()),
    ("debug.feed_bytes", local_only()),
    ("debug.inject_mouse", local_only()),
    ("debug.inject_key", local_only()),
    ("debug.tool.list", local_only()),
    ("debug.tool.invoke", local_only()),
    ("debug.popup.list", local_only()),
    ("debug.popup.open", local_only()),
    ("debug.popup.close", local_only()),
    ("debug.event_bus.list_subscribers", local_only()),
    ("debug.event_bus.publish", local_only()),
    ("debug.event_bus.trace", local_only()),
    ("debug.extension.invoke_hook", local_only()),
];
#[cfg(not(debug_assertions))]
pub const DEBUG_METHODS: &[(&str, MethodMeta)] = &[];

/// prefix 기반 fallback. METHOD_TABLE에 없는 메서드를 prefix로 매칭한다.
/// 현재는 IME 메서드만 (window 의존, 사용자 입력 영역).
pub const PREFIX_RULES: &[(&str, MethodMeta)] = &[("surface.ime_", local_only())];

/// 알려진 메서드의 메타. 미등록 메서드는 `None`.
pub fn method_meta(method: &str) -> Option<MethodMeta> {
    for (name, meta) in METHOD_TABLE {
        if *name == method {
            return Some(*meta);
        }
    }
    for (name, meta) in DEBUG_METHODS {
        if *name == method {
            return Some(*meta);
        }
    }
    for (prefix, meta) in PREFIX_RULES {
        if method.starts_with(prefix) {
            return Some(*meta);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_method_returns_none() {
        assert!(method_meta("not.a.real.method").is_none());
    }

    #[test]
    fn no_duplicate_method_names() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in METHOD_TABLE {
            assert!(seen.insert(*name), "duplicate method name: {name}");
        }
    }

    /// 모든 등록 메서드는 명명 규칙을 따라야 한다 (docs/dev-guide/cli-naming.md):
    ///
    /// 1. `<namespace>.<verb>` 또는 `<namespace>.<sub>.<verb>` 3단까지
    /// 2. 또는 [`ROOT_EXCEPTIONS`]에 등록된 root 메서드
    /// 3. 각 부분은 소문자 알파벳/숫자/`_` 만 허용
    #[test]
    fn all_registered_methods_match_naming_policy() {
        const ROOT_EXCEPTIONS: &[&str] = &["split", "tree"];

        for (name, _) in METHOD_TABLE {
            if ROOT_EXCEPTIONS.contains(name) {
                continue;
            }
            let parts: Vec<&str> = name.split('.').collect();
            assert!(
                parts.len() >= 2 && parts.len() <= 3,
                "method '{name}' must be <namespace>.<verb> or <namespace>.<sub>.<verb> \
                 (or registered in ROOT_EXCEPTIONS)"
            );
            for part in &parts {
                assert!(
                    !part.is_empty(),
                    "method '{name}' has empty segment"
                );
                assert!(
                    part.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "method '{name}': segment '{part}' has invalid characters \
                     (only lowercase a-z, 0-9, _)"
                );
            }
        }
    }

    #[test]
    fn prefix_rules_target_valid_namespaces() {
        for (prefix, _) in PREFIX_RULES {
            assert!(
                prefix.contains('.'),
                "prefix '{prefix}' must include a namespace separator"
            );
            assert!(
                prefix.ends_with('_') || prefix.ends_with('.'),
                "prefix '{prefix}' should end with `_` or `.` to avoid mid-token matches"
            );
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    fn debug_methods_are_local_only() {
        let m = method_meta("debug.inject_key").expect("registered (debug build)");
        assert!(!m.plugin_callable);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn debug_methods_absent_in_release() {
        assert!(method_meta("debug.inject_key").is_none());
        assert!(method_meta("system.shutdown").is_none());
        assert!(method_meta("ui.screenshot").is_none());
    }

    #[test]
    fn surface_list_requires_surface_read() {
        let m = method_meta("surface.list").expect("registered");
        assert!(m.plugin_callable);
        assert!(m.required.contains(&Permission::SurfaceRead));
    }

    #[test]
    fn tab_create_requires_surface_write() {
        let m = method_meta("tab.create").expect("registered");
        assert!(m.plugin_callable);
        assert!(m.required.contains(&Permission::SurfaceWrite));
    }

    #[test]
    fn ime_methods_are_local_only_via_prefix() {
        let m = method_meta("surface.ime_commit").expect("registered");
        assert!(!m.plugin_callable);
    }

    #[test]
    fn plugin_management_is_local_only() {
        let m = method_meta("plugin.enable").expect("registered");
        assert!(!m.plugin_callable);
    }
}
