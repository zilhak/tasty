//! banner 인스턴스를 발급하고 중복 열기와 닫기를 처리한다.
//! 콘텐츠는 egui-mesh로 받으며 셸·위치·닫기 시점은 호스트가 관리한다.

use serde_json::json;

use crate::protocol;

use super::{BannerInstance, PendingRequestKind, PluginManager};

impl PluginManager {
    /// Plugin 이 contribute 한 banner 의 새 인스턴스를 연다. plugin process 에
    /// `banner.open` IPC 를 보내고 host 발급 `instance_id` 를 돌려준다.
    ///
    /// surface_id는 호스트가 소유권을 확인한 대상이다. 이 크레이트는 surface 소유자를 조회하지 않는다.
    /// debug 전용 트리거는 해당 검사를 우회할 수 있다.
    ///
    /// 같은 (plugin_id, banner_id) 인스턴스가 이미 열려 있으면 새로 열지 않고 기존
    /// instance_id 를 돌려준다(단일 인스턴스 dedup — host 소유 정책, identity 원칙 1).
    /// plugin 이 실행 중이 아니거나 banner contribute 가 없으면 `None`.
    pub fn open_banner_instance(
        &mut self,
        plugin_id: &str,
        banner_id: &str,
        surface_id: u32,
        context: serde_json::Value,
    ) -> Option<u64> {
        if !self.processes.contains_key(plugin_id) {
            tracing::warn!(
                "banner.open: plugin '{plugin_id}' is not running, dropping banner '{banner_id}'"
            );
            return None;
        }
        if let Some(existing) = self.find_open_banner_instance(plugin_id, banner_id) {
            return Some(existing);
        }
        let contribute = self.resolve_open_banner_contribute(plugin_id, banner_id)?;
        let instance_id = self.next_banner_instance_id;
        self.next_banner_instance_id = self.next_banner_instance_id.wrapping_add(1);
        self.banner_instances.insert(
            instance_id,
            BannerInstance {
                plugin_id: plugin_id.to_string(),
                banner_id: banner_id.to_string(),
                contribute,
                surface_id,
            },
        );
        self.send_surface_request(
            plugin_id,
            protocol::METHOD_BANNER_OPEN,
            json!({
                "banner_id": banner_id,
                "instance_id": instance_id,
                "context": context,
            }),
            // banner.open 응답(BannerOpenResult)은 빈 결과 — 무시한다.
            PendingRequestKind::Other,
        );
        Some(instance_id)
    }

    /// 단일 인스턴스 dedup: 같은 (plugin_id, banner_id) 가 이미 열려 있으면 그 id.
    fn find_open_banner_instance(&self, plugin_id: &str, banner_id: &str) -> Option<u64> {
        let (existing, _) = self
            .banner_instances
            .iter()
            .find(|(_, inst)| inst.plugin_id == plugin_id && inst.banner_id == banner_id)?;
        tracing::debug!(
            "banner.open: '{plugin_id}/{banner_id}' already open (instance {existing}); dedup"
        );
        Some(*existing)
    }

    /// 매니페스트에 banner가 있고 mesh API 버전이 호환되면 정의를 반환한다.
    fn resolve_open_banner_contribute(
        &self,
        plugin_id: &str,
        banner_id: &str,
    ) -> Option<tasty_plugin_manifest::BannerContribute> {
        let pkg = self.packages.iter().find(|p| p.manifest.id == plugin_id)?;
        let contribute = pkg
            .manifest
            .contributes
            .banner
            .iter()
            .find(|b| b.id == banner_id)
            .cloned()?;
        if contribute.rendering == tasty_plugin_manifest::BannerRendering::EguiMesh
            && pkg.manifest.api_version != tasty_plugin_manifest::HOST_API_VERSION
        {
            tracing::warn!(
                "banner.open: egui-mesh banner '{plugin_id}/{banner_id}' has api_version '{}' \
                 incompatible with host '{}'; ignoring",
                pkg.manifest.api_version,
                tasty_plugin_manifest::HOST_API_VERSION
            );
            return None;
        }
        Some(contribute)
    }

    /// banner 인스턴스를 닫는다. plugin 에 `banner.closed` fire-and-forget IPC 를 보내고
    /// 호스트 측 인스턴스와 mesh frame 메타를 함께 제거. 닫는 이유는
    /// [`BannerCloseReason`](tasty_plugin_protocol::BannerCloseReason) 그대로 전달.
    pub fn close_banner_instance(
        &mut self,
        instance_id: u64,
        reason: tasty_plugin_protocol::BannerCloseReason,
    ) {
        let Some(inst) = self.banner_instances.remove(&instance_id) else {
            return;
        };
        // 닫은 banner의 프레임과 호스트 공유 매핑도 해제해 plugin 종료까지 쌓이지 않게 한다.
        if let Some(f) = self.banner_mesh_frames.remove(&instance_id) {
            self.release_plugin_buffer(&f.plugin_id, f.buffer_id);
        }
        // plugin process 가 살아있을 때만 알린다. 종료 중이면 다음 spawn 에서 새 인스턴스로 시작.
        if self.processes.contains_key(&inst.plugin_id) {
            self.send_surface_request(
                &inst.plugin_id,
                protocol::METHOD_BANNER_CLOSED,
                json!({
                    "instance_id": instance_id,
                    "reason": reason,
                }),
                PendingRequestKind::Other,
            );
        }
    }

    /// 현재 활성 banner 인스턴스 목록. host 합성기 / debug IPC 가 사용.
    pub fn banner_instances(&self) -> impl Iterator<Item = (u64, &BannerInstance)> {
        self.banner_instances.iter().map(|(k, v)| (*k, v))
    }
}
