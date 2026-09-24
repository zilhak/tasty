//! 서버의 mesh 구독 상태. 실제 plugin context 전송과 frame 중계는 PluginManager를 가진 계층이 맡는다.
//! GUI·헤드리스가 같은 상태를 갱신하며 각 전송 루프가 변경 표시와 입력을 소비한다.

use std::collections::HashMap;

use crate::core::attach::AttachClientId;
use tasty_plugin_protocol::protocol::{ModifiersWire, RawInputEventWire, RawInputWire, ThemeWire};

#[derive(Debug, Clone)]
pub(crate) struct MeshMirrorContext {
    pub(crate) client_id: AttachClientId,
    pub(crate) width_px: u32,
    pub(crate) height_px: u32,
    pub(crate) pixels_per_point: f32,
    pub(crate) theme: Option<ThemeWire>,
    pub(crate) focused: bool,
    /// 바뀐 context나 입력만 보내도록 전송 루프가 소비하는 표시.
    pub(crate) dirty: bool,
    /// 신규 구독이나 재전송 요청에서 전체 texture를 요구한다.
    pub(crate) need_full_textures: bool,
    /// 마지막 전송 generation과 같은 값의 중복 전송을 피한다. 대소 비교는 하지 않는다.
    pub(crate) last_forwarded_generation: Option<u64>,
    /// chunk 재조립용 번호. plugin의 frame_seq와 별개다.
    next_frame_id: u64,
    pending_events: Vec<RawInputEventWire>,
    /// MeshContext에는 modifier가 없어 마지막 MeshInput의 값을 다음 context 전송에 쓴다.
    pub(crate) last_modifiers: ModifiersWire,
}

impl MeshMirrorContext {
    fn geometry_changed(&self, width_px: u32, height_px: u32, ppp: f32) -> bool {
        self.width_px != width_px || self.height_px != height_px || self.pixels_per_point != ppp
    }
}

#[derive(Debug, Default)]
pub(crate) struct MeshMirrorRegistry {
    contexts: HashMap<u32, MeshMirrorContext>,
}

impl MeshMirrorRegistry {
    /// surface별 구독을 등록·갱신한다. 바뀌었으면 dirty를 세우며 반환값은 없다.
    /// 기존 client가 바뀌어도 texture·generation·대기 입력을 여기서 초기화하지는 않는다.
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
                        // 새 구독은 이전 texture delta를 못 받았으므로 전체 texture가 필요하다.
                        need_full_textures: true,
                        last_forwarded_generation: None,
                        next_frame_id: 0,
                        pending_events: Vec::new(),
                        last_modifiers: ModifiersWire::default(),
                    },
                );
            }
        }
    }

    pub(crate) fn get(&self, surface_id: u32) -> Option<&MeshMirrorContext> {
        self.contexts.get(&surface_id)
    }

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

    /// dirty만 소비한다. 전체 texture 요청과 대기 입력은 별도 함수로 가져간다.
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

    /// 마지막 전송과 다른 generation인지 확인한다. 그보다 오래된 값도 다르면 true다.
    pub(crate) fn should_forward_generation(&self, surface_id: u32, generation: u64) -> bool {
        self.contexts
            .get(&surface_id)
            .is_some_and(|c| c.last_forwarded_generation != Some(generation))
    }

    /// 전송했다고 전달받은 generation을 기록하고 frame ID를 발급한다. 실제 송신 성공을 검사하지 않는다.
    pub(crate) fn mark_forwarded(&mut self, surface_id: u32, generation: u64) -> Option<u64> {
        let ctx = self.contexts.get_mut(&surface_id)?;
        ctx.last_forwarded_generation = Some(generation);
        let id = ctx.next_frame_id;
        ctx.next_frame_id += 1;
        Some(id)
    }

    /// 구독이 있으면 입력을 누적하고 modifier·dirty를 갱신한다. 없으면 false다.
    pub(crate) fn push_input(&mut self, surface_id: u32, input: RawInputWire) -> bool {
        match self.contexts.get_mut(&surface_id) {
            Some(ctx) => {
                ctx.last_modifiers = input.modifiers;
                ctx.pending_events.extend(input.events);
                ctx.dirty = true;
                true
            }
            None => false,
        }
    }

    pub(crate) fn take_pending_events(&mut self, surface_id: u32) -> Vec<RawInputEventWire> {
        self.contexts
            .get_mut(&surface_id)
            .map(|c| std::mem::take(&mut c.pending_events))
            .unwrap_or_default()
    }

    /// 순회 중 상태를 바꿀 수 있도록 ID 목록을 복사해 반환한다.
    pub(crate) fn active_surface_ids(&self) -> Vec<u32> {
        self.contexts.keys().copied().collect()
    }

    pub(crate) fn remove(&mut self, surface_id: u32) {
        self.contexts.remove(&surface_id);
    }

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
        assert!(!reg.take_dirty(1));
        assert!(!reg.take_need_full_textures(1));
    }

    #[test]
    fn unchanged_upsert_does_not_redirty() {
        let mut reg = MeshMirrorRegistry::default();
        reg.upsert(1, 100, 800, 600, 2.0, None, true);
        reg.take_dirty(1);
        reg.take_need_full_textures(1);
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
        assert!(!reg.take_need_full_textures(1));
    }

    #[test]
    fn full_resend_request_requires_existing_subscription() {
        let mut reg = MeshMirrorRegistry::default();
        assert!(!reg.request_full_resend(9));
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
    fn push_input_requires_existing_subscription() {
        let mut reg = MeshMirrorRegistry::default();
        let input = RawInputWire {
            time: None,
            focused: true,
            modifiers: ModifiersWire {
                ctrl: true,
                ..Default::default()
            },
            events: vec![RawInputEventWire::PointerMoved { x: 1.0, y: 2.0 }],
        };
        assert!(!reg.push_input(9, input.clone()));
        reg.upsert(9, 1, 10, 10, 1.0, None, false);
        reg.take_dirty(9);
        assert!(reg.push_input(9, input));
        assert!(reg.take_dirty(9));
        let events = reg.take_pending_events(9);
        assert_eq!(events.len(), 1);
        assert!(reg.get(9).unwrap().last_modifiers.ctrl);
        assert!(reg.take_pending_events(9).is_empty());
    }
}
