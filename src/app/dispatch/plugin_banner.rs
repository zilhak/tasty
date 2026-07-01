//! Plugin egui-mesh banner(A3) open/close 오케스트레이션.
//!
//! banner 인스턴스는 두 곳에 살아야 한다 — host-plugin manager(인스턴스 메타 + mesh
//! frame)와 해당 view 의 [`BannerManager`](crate::adapters::ui::BannerManager)(큐/TTL/
//! z-order/위치). 이 모듈이 둘을 함께 열고 닫는 App-level glue 다. surface→plugin 매핑은
//! host 본문(각 view 의 `core_state`)만 알기 때문에 여기서 소유권(D1)을 검증한다.

use crate::app::App;

/// plugin egui-mesh banner 미지정 시 셸 높이(LogicalPx). manifest `size_hint.height`
/// 가 없을 때 host 가 도킹 높이로 쓰는 기본값.
const DEFAULT_BANNER_MESH_HEIGHT: f32 = 64.0;

impl App {
    /// plugin banner 를 연다. `surface_id` 가 가리키는 egui-mesh surface 의 소유 plugin 을
    /// 찾아, `caller` 가 지정되면(production `banner.open`) 그 소유자와 일치하는지 검증한다
    /// (D1: 남의 surface 에 배너 금지). `caller` 가 `None` 이면(debug 트리거) 소유권 검증을
    /// 우회하되 여전히 실제 소유 plugin 으로 연다.
    ///
    /// 성공 시 host 발급 instance_id 를 돌려준다. 이미 열려 있으면 기존 id(dedup).
    pub(crate) fn open_plugin_banner(
        &mut self,
        caller: Option<&str>,
        banner_id: &str,
        surface_id: u32,
    ) -> Result<u64, String> {
        // 1) surface_id 를 소유한 egui-mesh surface 의 plugin 과 그 view id 를 찾는다.
        //    view 맵 키 타입(WindowId)은 추론에 맡긴다 (`_`).
        let mut owner: Option<(_, String)> = None;
        for (wid, w) in self.view.views.iter() {
            if let Some(main) = w.as_main() {
                if let Some(surface) = main.core_state.find_surface_by_id(surface_id)
                    && let Some(ms) = surface
                        .as_any()
                        .downcast_ref::<crate::plugin_bridge::egui_mesh_surface::EguiMeshSurface>(
                    )
                {
                    owner = Some((*wid, ms.plugin_id.clone()));
                    break;
                }
            }
        }
        let Some((wid, owner_plugin)) = owner else {
            return Err(format!(
                "banner.open: surface {surface_id} is not a live egui-mesh surface"
            ));
        };
        // D1 소유권 검증 — 호출 plugin 은 자기 surface 에만 배너를 띄울 수 있다.
        if let Some(caller) = caller
            && caller != owner_plugin
        {
            return Err(format!(
                "banner.open: plugin '{caller}' does not own surface {surface_id} (owner '{owner_plugin}')"
            ));
        }

        // 2) host-plugin manager 에 인스턴스 발급 + banner.open 송신.
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return Err("banner.open: plugin manager unavailable".to_string());
        };
        let Some(instance_id) = mgr.open_banner_instance(
            &owner_plugin,
            banner_id,
            surface_id,
            serde_json::Value::Null,
        ) else {
            return Err(format!(
                "banner.open: banner '{owner_plugin}/{banner_id}' not found or plugin not running"
            ));
        };
        // contribute 에서 TTL / 높이 힌트를 읽는다 (방금 만든 인스턴스에 보관됨).
        let (ttl_seconds, height) = mgr
            .banner_instances()
            .find(|(iid, _)| *iid == instance_id)
            .map(|(_, inst)| {
                (
                    inst.contribute.ttl_seconds,
                    inst.contribute
                        .size_hint
                        .map(|s| s.height as f32)
                        .unwrap_or(DEFAULT_BANNER_MESH_HEIGHT),
                )
            })
            .unwrap_or((None, DEFAULT_BANNER_MESH_HEIGHT));

        // 3) 소유 view 의 BannerManager 에 push (Surface scope). 이미 열려 있으면 dedup 로
        //    같은 instance_id 가 오므로, key(Plugin(iid)) 중복이면 push 가 무시/리셋한다.
        if let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) {
            main.state
                .banners
                .push(crate::adapters::ui::BannerState::plugin_mesh(
                    crate::adapters::ui::BannerScope::Surface(surface_id),
                    owner_plugin,
                    instance_id,
                    ttl_seconds,
                    height,
                ));
        }
        Ok(instance_id)
    }

    /// plugin banner 를 instance_id 로 닫는다. host UI(모든 view 의 BannerManager)에서
    /// 제거하고 host-plugin manager 에 close(→ `banner.closed`)를 전파한다. plugin 요청
    /// (`banner.close`) / debug 강제 close 공용.
    pub(crate) fn close_plugin_banner(
        &mut self,
        instance_id: u64,
        reason: tasty_plugin_protocol::BannerCloseReason,
    ) -> bool {
        let mut removed = false;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.state.banners.close_by_instance(instance_id)
            {
                removed = true;
            }
        }
        if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.close_banner_instance(instance_id, reason);
        }
        removed
    }

    /// 특정 plugin 이 소유한 banner 인스턴스인지 확인 (production `banner.close` 소유권 검증).
    pub(crate) fn plugin_owns_banner(&self, plugin_id: &str, instance_id: u64) -> bool {
        self.plugin_manager
            .as_ref()
            .and_then(|m| {
                m.banner_instances()
                    .find(|(iid, _)| *iid == instance_id)
                    .map(|(_, inst)| inst.plugin_id == plugin_id)
            })
            .unwrap_or(false)
    }
}
