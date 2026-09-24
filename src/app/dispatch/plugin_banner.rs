//! 플러그인 매니저의 배너 인스턴스와 소유 창의 BannerManager를 함께 갱신한다.

use crate::app::App;
use tasty_type_geometry::length::LogicalPx;

/// 매니페스트에 높이가 없을 때 사용할 배너 높이.
const DEFAULT_BANNER_MESH_HEIGHT: LogicalPx = LogicalPx(64.0);

impl App {
    /// caller가 있으면 surface 소유자를 검사한다. debug 요청의 None은 이 검사를 생략한다.
    /// 같은 배너가 열려 있으면 기존 instance_id를 반환한다.
    pub(crate) fn open_plugin_banner(
        &mut self,
        caller: Option<&str>,
        banner_id: &str,
        surface_id: u32,
    ) -> Result<u64, String> {
        let mut owner: Option<(_, String)> = None;
        for (wid, w) in self.view.views.iter() {
            if let Some(main) = w.as_main() {
                if let Some(surface) = main.core_state.find_surface_by_id(surface_id)
                    && let Some(ms) = surface
                        .as_any()
                        .downcast_ref::<crate::core::egui_mesh_surface::EguiMeshSurface>(
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
        if let Some(caller) = caller
            && caller != owner_plugin
        {
            return Err(format!(
                "banner.open: plugin '{caller}' does not own surface {surface_id} (owner '{owner_plugin}')"
            ));
        }

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
        let (ttl_seconds, height) = mgr
            .banner_instances()
            .find(|(iid, _)| *iid == instance_id)
            .map(|(_, inst)| {
                (
                    inst.contribute.ttl_seconds,
                    inst.contribute
                        .size_hint
                        .map(|s| s.height as f32)
                        .unwrap_or(DEFAULT_BANNER_MESH_HEIGHT.value()),
                )
            })
            .unwrap_or((None, DEFAULT_BANNER_MESH_HEIGHT.value()));

        // 이미 열린 인스턴스는 같은 ID로 BannerManager의 중복 처리를 따른다.
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

    /// 호스트 UI에서 제거하고 플러그인 매니저에도 닫기를 알린다.
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
