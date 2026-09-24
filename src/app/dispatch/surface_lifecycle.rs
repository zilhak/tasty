//! surface 닫기 이벤트를 플러그인에 전달한다.

use crate::app::App;

impl App {
    /// 모든 창·parked 상태의 닫기 큐를 처리한다. state가 plugin 타입에 의존하지 않도록
    /// 사용자 닫기 여부를 여기서 LifecycleReason으로 변환한다.
    pub(crate) fn dispatch_pending_surface_lifecycle(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::LifecycleReason;
        use tasty_plugin_protocol::events::payloads::SurfaceClosed;
        let mut drained: Vec<crate::state::PendingSurfaceClosed> = Vec::new();
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.extend(main.state.take_pending_lifecycle_events());
            }
        }
        for (s, _engine) in &mut self.parked_states {
            drained.extend(s.take_pending_lifecycle_events());
        }
        if drained.is_empty() {
            return;
        }
        let scripts = self.autofire_scripts();
        let lua = self.lua_engine.as_ref();
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for ev in drained {
            // 일반 닫기 알림과 별도로 소유 플러그인의 surface 자원도 정리한다.
            mgr.destroy_remote_surface(ev.surface_id, ev.kind);
            let bus_reason = if ev.is_user_close {
                LifecycleReason::User
            } else {
                LifecycleReason::Ipc
            };
            let payload = SurfaceClosed {
                surface_id: ev.surface_id,
                kind: ev.kind.map(|s| s.to_string()).unwrap_or_default(),
                reason: bus_reason,
            };
            mgr.emit_host_event("surface.closed", &payload, EventScope::Surface);
            crate::hooks::lua::fire(
                lua,
                crate::hooks::lua::AutofireCtx {
                    scripts: &scripts,
                    guard: &mut self.lua_autofire,
                },
                "surface.delete.post",
                &payload,
            );
        }
    }
}
