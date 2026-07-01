//! Banner 인스턴스 생명주기(A3): open/close, host 발급 instance_id 관리.
//!
//! popup([`super::popup`]) 과 평행하되 banner 는 non-modal 공지라 초기 tree/event 채널이
//! 없고 egui-mesh 로만 콘텐츠를 그린다 — `banner.open` 응답([`BannerOpenResult`])은 빈
//! 결과라 `PendingRequestKind::Other` 로 무시한다. 셸/스택/위치/dismiss 타이밍은 host
//! 소유이며, 여기서는 인스턴스 발급/dedup/정리만 담당한다.

use serde_json::json;

use crate::protocol;

use super::{BannerInstance, PendingRequestKind, PluginManager};

impl PluginManager {
    /// Plugin 이 contribute 한 banner 의 새 인스턴스를 연다. plugin process 에
    /// `banner.open` IPC 를 보내고 host 발급 `instance_id` 를 돌려준다.
    ///
    /// `surface_id` 는 banner 가 도킹될 스코프 surface(D1). **소유권 검증(그 surface 가
    /// 호출 plugin 소유인지)은 caller(App) 가 이미 마친 상태**로 전달된다 — host 본문이
    /// surface→plugin 매핑을 알고 여기(host-plugin 크레이트)는 모르기 때문. debug 트리거는
    /// 소유권 검증을 우회한다(격리 빌드 한정).
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
        // 단일 인스턴스 dedup: 같은 (plugin_id, banner_id) 가 열려 있으면 기존 id 반환.
        if let Some((existing, _)) = self
            .banner_instances
            .iter()
            .find(|(_, inst)| inst.plugin_id == plugin_id && inst.banner_id == banner_id)
        {
            tracing::debug!(
                "banner.open: '{plugin_id}/{banner_id}' already open (instance {existing}); dedup"
            );
            return Some(*existing);
        }
        let pkg = self.packages.iter().find(|p| p.manifest.id == plugin_id)?;
        let contribute = pkg
            .manifest
            .contributes
            .banner
            .iter()
            .find(|b| b.id == banner_id)
            .cloned()?;
        // egui-mesh banner 는 epaint 와이어가 host·plugin 동일 컴파일을 강제하므로
        // api_version 일치를 게이트한다(surface/popup egui-mesh 등록 정책 미러, ADR-0028).
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
        // egui-mesh banner 면 합성기가 참조하던 frame 메타를 함께 정리 (stale buffer 방지).
        self.banner_mesh_frames.remove(&instance_id);
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
