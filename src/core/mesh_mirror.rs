//! attach mesh mirror — 서버측 구독 상태(`.claude-workspace/todo/18-attach-server-mesh-context-forward.md`).
//!
//! `CoreState`가 소유한다(`PluginManager`는 `App` 소유라 여기 둘 수 없다 — 17번/20번
//! TODO의 아키텍처 결정). 이 레지스트리는 "누가 어떤 mesh surface 를 어떤
//! geometry/theme/focus 로 구독 중인가" 만 추적하고, 실제 plugin 구동(`surface.set_context`
//! 송신) 과 mesh 바이트 forward는 `PluginManager` 접근권이 있는 계층
//! (`src/boot/headless_plugins.rs`)이 이 상태를 읽어 수행한다.
//!
//! `apply_attached_mesh_context`/`apply_attached_mesh_full_resend`(둘 다 gui/headless
//! 공용, `attach_runtime.rs`)를 통해 구독 상태(upsert/dirty/need_full_textures)는
//! gui 빌드에서도 갱신된다 — 하지만 실제 forward 루프(`get`/`take_dirty`/
//! `should_forward_generation`/`mark_forwarded`/`active_surface_ids`/`remove` 소비)는
//! 현재 headless-as-attach-서버 시나리오(`src/boot/headless_plugins.rs`)에만 배선했다.
//! gui 가 attach 서버인 경우(로컬 창이 이미 그 mesh surface 를 렌더 중일 때의 geometry
//! 권위 조정까지 필요해 범위가 커짐 — TODO 18 결정 #4)는 의도적으로 범위 밖으로 남기고
//! `.claude-workspace/todo/22-attach-gui-server-mesh-forward.md`에 후속 작업으로 기록했다.
//! 그 결과 gui 빌드에서는 이 필드/메서드들이 아직 소비되지 않아 `-D dead-code`를
//! feature-gate 로 침묵한다(headless 빌드는 정상적으로 전부 소비 — lint 유효).
#![cfg_attr(feature = "gui", allow(dead_code))]

use std::collections::HashMap;

use crate::core::attach::AttachClientId;
use tasty_plugin_protocol::protocol::ThemeWire;

/// 한 mesh surface 의 최신 구독 상태.
#[derive(Debug, Clone)]
pub(crate) struct MeshMirrorContext {
    pub(crate) client_id: AttachClientId,
    pub(crate) width_px: u32,
    pub(crate) height_px: u32,
    pub(crate) pixels_per_point: f32,
    pub(crate) theme: Option<ThemeWire>,
    pub(crate) focused: bool,
    /// geometry/theme/focus 가 마지막 forward 이후 바뀌었거나(또는 신규 구독/
    /// full-resend 요청) — forward 루프가 이 surface 에 `surface.set_context` 를
    /// 다시 보내야 함을 뜻한다. 매 tick 무조건 재전송하지 않는 이유: 입력이 없는
    /// idle 구독에 반복 재-context 를 보내면 plugin CPU 를 불필요하게 태운다
    /// (TODO 18 "불필요한 plugin CPU 낭비 방지").
    pub(crate) dirty: bool,
    /// 다음 forward 시 `SurfaceSetContextParams.need_full_textures` 를 세워야 하는가
    /// (신규 구독 또는 명시적 [`crate::ipc::stream::StreamControl::MeshFullResendRequest`]).
    pub(crate) need_full_textures: bool,
    /// 이 surface 에 대해 마지막으로 forward 한 `EguiMeshFrame::generation` — 같은
    /// generation 을 중복 forward 하지 않기 위한 dedup 키. `None` = 아직 forward 없음.
    pub(crate) last_forwarded_generation: Option<u64>,
    /// chunk 재조립 키(`mesh_stream::MeshChunkMeta::frame_id`) 발급용 단조 카운터.
    /// `frame_seq`(plugin 렌더 코어의 시퀀스)와는 별개 — 이건 순수 attach 전송 계층의
    /// 재조립 키다.
    next_frame_id: u64,
}

impl MeshMirrorContext {
    fn geometry_changed(&self, width_px: u32, height_px: u32, ppp: f32) -> bool {
        self.width_px != width_px || self.height_px != height_px || self.pixels_per_point != ppp
    }
}

/// surface_id → 구독 상태. attach 세션이 붙어있는 동안만 채워진다(빈 레지스트리는
/// mesh mirror 를 전혀 안 쓰는 일반적인 headless/GUI 상태와 동일한 zero-cost 경로).
#[derive(Debug, Default)]
pub(crate) struct MeshMirrorRegistry {
    contexts: HashMap<u32, MeshMirrorContext>,
}

impl MeshMirrorRegistry {
    /// 구독 요청을 반영한다(신규 구독 또는 기존 갱신). 반환: 이 tick 에 forward 루프가
    /// `surface.set_context` 를 다시 보내야 하는지(geometry/theme/focus 변경, 또는
    /// 신규 구독이라 무조건 최초 1 회 필요).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upsert(
        &mut self,
        surface_id: u32,
        client_id: AttachClientId,
        width_px: u32,
        height_px: u32,
        pixels_per_point: f32,
        theme: Option<ThemeWire>,
        focused: bool,
    ) {
        match self.contexts.get_mut(&surface_id) {
            Some(ctx) => {
                let changed = ctx.client_id != client_id
                    || ctx.geometry_changed(width_px, height_px, pixels_per_point)
                    || ctx.theme != theme
                    || ctx.focused != focused;
                ctx.client_id = client_id;
                ctx.width_px = width_px;
                ctx.height_px = height_px;
                ctx.pixels_per_point = pixels_per_point;
                ctx.theme = theme;
                ctx.focused = focused;
                if changed {
                    ctx.dirty = true;
                }
            }
            None => {
                self.contexts.insert(
                    surface_id,
                    MeshMirrorContext {
                        client_id,
                        width_px,
                        height_px,
                        pixels_per_point,
                        theme,
                        focused,
                        dirty: true,
                        // 신규 구독 — SharedBuffer의 "최신 generation만 보기" 특성상
                        // 중간 texture delta 를 절대 못 보므로 첫 프레임은 항상 full.
                        need_full_textures: true,
                        last_forwarded_generation: None,
                        next_frame_id: 0,
                    },
                );
            }
        }
    }

    pub(crate) fn get(&self, surface_id: u32) -> Option<&MeshMirrorContext> {
        self.contexts.get(&surface_id)
    }

    /// 이 surface 가 지금 이 client 에 의해 구독 중인지 확인(holder 검증에 이미
    /// `attach` 레지스트리를 썼다는 전제 하에, 여기선 "실제로 등록돼 있는가"만 본다).
    pub(crate) fn is_subscribed_by(&self, surface_id: u32, client_id: AttachClientId) -> bool {
        self.contexts
            .get(&surface_id)
            .is_some_and(|c| c.client_id == client_id)
    }

    /// full-resend 요청을 반영. 구독돼 있지 않으면 `false`(호출자는 MeshError 회신).
    pub(crate) fn request_full_resend(&mut self, surface_id: u32) -> bool {
        match self.contexts.get_mut(&surface_id) {
            Some(ctx) => {
                ctx.need_full_textures = true;
                ctx.dirty = true;
                true
            }
            None => false,
        }
    }

    /// forward 루프 전용 — dirty 플래그를 읽고 초기화. `need_full_textures` 는
    /// 별도로 [`Self::take_need_full_textures`] 가 소비한다(이 함수가 같이 지우면
    /// dirty=false 인데 need_full 만 남는 상태를 만들 수 없어 순서 독립적으로 분리).
    pub(crate) fn take_dirty(&mut self, surface_id: u32) -> bool {
        self.contexts
            .get_mut(&surface_id)
            .map(|c| std::mem::take(&mut c.dirty))
            .unwrap_or(false)
    }

    pub(crate) fn take_need_full_textures(&mut self, surface_id: u32) -> bool {
        self.contexts
            .get_mut(&surface_id)
            .map(|c| std::mem::take(&mut c.need_full_textures))
            .unwrap_or(false)
    }

    /// 이 generation 의 frame 을 아직 forward 하지 않았는지 확인.
    pub(crate) fn should_forward_generation(&self, surface_id: u32, generation: u64) -> bool {
        self.contexts
            .get(&surface_id)
            .is_some_and(|c| c.last_forwarded_generation != Some(generation))
    }

    /// forward 완료 기록 + chunk 재조립용 frame_id 발급(호출 시 1 증가).
    pub(crate) fn mark_forwarded(&mut self, surface_id: u32, generation: u64) -> Option<u64> {
        let ctx = self.contexts.get_mut(&surface_id)?;
        ctx.last_forwarded_generation = Some(generation);
        let id = ctx.next_frame_id;
        ctx.next_frame_id += 1;
        Some(id)
    }

    /// 구독 중인 모든 surface_id 스냅샷(순회 중 mutate 를 피하기 위해 collect 해 반환).
    pub(crate) fn active_surface_ids(&self) -> Vec<u32> {
        self.contexts.keys().copied().collect()
    }

    /// surface 가 사라졌거나(닫힘) 화이트리스트 재검증에 실패했을 때 정리.
    pub(crate) fn remove(&mut self, surface_id: u32) {
        self.contexts.remove(&surface_id);
    }

    /// attach client 연결 종료 시 그 client 가 구독하던 전부를 정리 — 불필요한
    /// plugin CPU 낭비 방지(TODO 18 완료 확인 절차 "detach 시 context 전달 중단").
    pub(crate) fn remove_for_client(&mut self, client_id: AttachClientId) {
        self.contexts.retain(|_, c| c.client_id != client_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_subscribe_is_dirty_and_needs_full_textures() {
        let mut reg = MeshMirrorRegistry::default();
        reg.upsert(1, 100, 800, 600, 2.0, None, true);
        assert!(reg.take_dirty(1));
        assert!(reg.take_need_full_textures(1));
        // 두 번째 조회는 소비됐으므로 false.
        assert!(!reg.take_dirty(1));
        assert!(!reg.take_need_full_textures(1));
    }

    #[test]
    fn unchanged_upsert_does_not_redirty() {
        let mut reg = MeshMirrorRegistry::default();
        reg.upsert(1, 100, 800, 600, 2.0, None, true);
        reg.take_dirty(1);
        reg.take_need_full_textures(1);
        // 동일 geometry/theme/focus 재전송 — dirty 재설정 안 됨.
        reg.upsert(1, 100, 800, 600, 2.0, None, true);
        assert!(!reg.take_dirty(1));
    }

    #[test]
    fn geometry_change_redirties() {
        let mut reg = MeshMirrorRegistry::default();
        reg.upsert(1, 100, 800, 600, 2.0, None, true);
        reg.take_dirty(1);
        reg.take_need_full_textures(1);
        reg.upsert(1, 100, 801, 600, 2.0, None, true);
        assert!(reg.take_dirty(1));
        // geometry 변경만으로는 need_full_textures 를 다시 세우지 않는다(그건 별도
        // 명시 요청 전용) — 이미 소비됐으므로 여기선 false 여야 한다.
        assert!(!reg.take_need_full_textures(1));
    }

    #[test]
    fn full_resend_request_requires_existing_subscription() {
        let mut reg = MeshMirrorRegistry::default();
        assert!(!reg.request_full_resend(9)); // 구독 없음
        reg.upsert(9, 1, 10, 10, 1.0, None, false);
        reg.take_dirty(9);
        reg.take_need_full_textures(9);
        assert!(reg.request_full_resend(9));
        assert!(reg.take_need_full_textures(9));
        assert!(reg.take_dirty(9));
    }

    #[test]
    fn should_forward_generation_dedupes() {
        let mut reg = MeshMirrorRegistry::default();
        reg.upsert(1, 1, 10, 10, 1.0, None, false);
        assert!(reg.should_forward_generation(1, 5));
        let frame_id = reg.mark_forwarded(1, 5).unwrap();
        assert_eq!(frame_id, 0);
        assert!(!reg.should_forward_generation(1, 5));
        assert!(reg.should_forward_generation(1, 6));
        assert_eq!(reg.mark_forwarded(1, 6).unwrap(), 1);
    }

    #[test]
    fn remove_for_client_only_drops_that_clients_subscriptions() {
        let mut reg = MeshMirrorRegistry::default();
        reg.upsert(1, 100, 1, 1, 1.0, None, false);
        reg.upsert(2, 200, 1, 1, 1.0, None, false);
        reg.remove_for_client(100);
        assert!(reg.get(1).is_none());
        assert!(reg.get(2).is_some());
    }

    #[test]
    fn is_subscribed_by_checks_current_holder() {
        let mut reg = MeshMirrorRegistry::default();
        reg.upsert(1, 100, 1, 1, 1.0, None, false);
        assert!(reg.is_subscribed_by(1, 100));
        assert!(!reg.is_subscribed_by(1, 200));
        assert!(!reg.is_subscribed_by(2, 100));
    }
}
