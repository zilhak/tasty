//! surface별 크기·입력·테마·포커스를 플러그인에 보내고 비동기 paint를 받는다.
//! 초기 생성·전체 재전송·무효화 요청도 여기서 처리한다.
//! raw_input은 사용자 입력이며 강제 주입은 debug 전용이다.
//! 물리 화면 좌표를 ppp로 나누어 surface 기준 논리 좌표로 보낸다.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Instant;

use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey};

use tasty_plugin_protocol::{
    ImeWire, ModifiersWire, PointerButtonWire, RawInputEventWire, RawInputWire,
    SurfaceSetContextParams, ThemeWire,
};

use crate::core::egui_mesh_surface::EguiMeshSurface;
use crate::model::{PhysicalPx, PhysicalRect};
use crate::plugin::PluginManager;
use crate::plugin_bridge::MeshForwardCommon;
use tasty_ipc::stream_hub::StreamHub;

use super::MainView;

/// 두 egui 프레임 사이의 실제 경과 시간을 계산하도록 공통 Instant 기준의 초를 보낸다.
fn mesh_time_now() -> f64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// 숨겨진 surface도 전체 재전송에 마지막 컨텍스트가 필요해 레이아웃에 존재하는 동안 상태를 유지한다.
#[derive(Default)]
pub(crate) struct MeshForwardState {
    /// surface·팝업·배너의 공용 전송 상태.
    common: MeshForwardCommon,
    /// 다음 set_context 에 실어 보낼 누적 입력 이벤트(순서 보존).
    events: Vec<RawInputEventWire>,
    /// 이 surface 의 owning plugin id (첫 forward 시 기록). 비가시 상태에서 full
    /// 재전송 요청을 보낼 때 대상 plugin 을 알기 위해 보관한다.
    plugin_id: Option<String>,
    /// 직전 forward 의 focused 상태. 포커스만 바뀌어도(입력·크기·테마 무변) set_context
    /// 재전송을 트리거하기 위해 추적한다 — markdown 등 focused/unfocused 배경 즉시 전환.
    last_focused: Option<bool>,
    /// SurfaceInvalidated를 받으면 입력 변화 없이도 다음 컨텍스트를 한 번 보낸다.
    invalidated: bool,
}

impl MeshForwardState {
    /// 다음 전송에 전체 텍스처 요청을 포함한다.
    pub(crate) fn set_pending_full(&mut self) {
        self.common.pending_full = true;
    }

    /// 플러그인의 변경 알림을 다음 전송에 반영한다.
    pub(crate) fn set_invalidated(&mut self) {
        self.invalidated = true;
    }
}

/// forward 대상 egui-mesh surface 1개의 메타 — set_context 송신 + bootstrap create 용.
struct MeshTarget {
    sid: u32,
    plugin_id: String,
    rect: PhysicalRect,
    kind: &'static str,
    /// 생성 params 의 `file`(예: markdown 경로). bootstrap surface.create 로 plugin 에 전달.
    file: Option<String>,
    display_name: String,
}

impl MainView {
    /// 합성과 같은 surface 영역으로 포인터 대상을 찾는다.
    pub(super) fn egui_mesh_target_at(
        &self,
        x: f32,
        y: f32,
    ) -> Option<(u32, String, PhysicalRect)> {
        let terminal_rect = self.compute_terminal_rect();
        for (_pane_id, _pane_rect, regions) in self.state.surface_regions(
            &self.core_state,
            terminal_rect,
            self.base.gpu.scale_factor(),
        ) {
            for r in regions {
                if r.rect.contains(PhysicalPx(x), PhysicalPx(y))
                    && let Some(ms) = r.surface.as_any().downcast_ref::<EguiMeshSurface>()
                {
                    return Some((r.id, ms.plugin_id.clone(), r.rect));
                }
            }
        }
        None
    }

    /// wire에 전달할 색·is_light·UI 배율. 현재 reduced_motion은 포함하지 않는다.
    pub(super) fn mesh_theme_snapshot(&self) -> ThemeWire {
        let theme = crate::theme::theme();
        ThemeWire {
            colors: theme.to_colors(),
            is_light: theme.is_light,
            ui_zoom: self.core_state.settings.appearance.ui_scale_factor(),
        }
    }

    /// 로컬·원격 mesh가 공유하는 modifier 변환.
    pub(super) fn mesh_modifiers(&self) -> ModifiersWire {
        let m = &self.base.modifiers;
        let cmd = if cfg!(target_os = "macos") {
            m.super_key()
        } else {
            m.control_key()
        };
        ModifiersWire {
            alt: m.alt_key(),
            ctrl: m.control_key(),
            shift: m.shift_key(),
            mac_cmd: m.super_key(),
            command: cmd,
        }
    }

    /// 화면 물리 좌표를 surface 기준 논리 좌표로 바꾼다.
    pub(super) fn mesh_local_point(&self, rect: PhysicalRect, x: f32, y: f32) -> (f32, f32) {
        let ppp = self.base.gpu.scale_factor().max(f32::EPSILON);
        let local_x = PhysicalPx(x) - rect.x;
        let local_y = PhysicalPx(y) - rect.y;
        (
            local_x.to_logical(ppp).value(),
            local_y.to_logical(ppp).value(),
        )
    }

    /// 포인터 버튼 누름/뗌을 egui-mesh surface 에 누적.
    pub(super) fn egui_mesh_push_pointer_button(
        &mut self,
        surface_id: u32,
        rect: PhysicalRect,
        x: f32,
        y: f32,
        button: MouseButton,
        pressed: bool,
    ) {
        let Some(button) = map_button(button) else {
            return;
        };
        let (lx, ly) = self.mesh_local_point(rect, x, y);
        let modifiers = self.mesh_modifiers();
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::PointerButton {
            x: lx,
            y: ly,
            button,
            pressed,
            modifiers,
        });
    }

    /// 포인터 이동을 egui-mesh surface 에 누적(hover/interact_pos 추적용).
    pub(super) fn egui_mesh_push_pointer_moved(
        &mut self,
        surface_id: u32,
        rect: PhysicalRect,
        x: f32,
        y: f32,
    ) {
        let (lx, ly) = self.mesh_local_point(rect, x, y);
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events
            .push(RawInputEventWire::PointerMoved { x: lx, y: ly });
    }

    /// 이전 hover 대상에 PointerGone을 누적한다.
    pub(super) fn egui_mesh_push_pointer_gone(&mut self, surface_id: u32) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::PointerGone);
    }

    /// 스크롤 델타(논리 포인트)를 egui-mesh surface 에 누적.
    pub(super) fn egui_mesh_push_scroll(&mut self, surface_id: u32, dx: f32, dy: f32) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Scroll { x: dx, y: dy });
    }

    /// 플러그인 자체 선택을 복사하도록 Copy 이벤트를 누적한다.
    pub(crate) fn egui_mesh_push_copy(&mut self, surface_id: u32) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Copy);
    }

    /// 포커스된 EguiMeshSurface의 ID. 다른 종류에는 입력을 전달하지 않는다.
    pub(crate) fn focused_egui_mesh_surface_id(&self) -> Option<u32> {
        let sid = self.state.focused_surface_id(&self.core_state)?;
        let surface = self.core_state.find_surface_by_id(sid)?;
        surface
            .as_any()
            .downcast_ref::<EguiMeshSurface>()
            .map(|_| sid)
    }

    /// 매핑 가능한 키 누름을 누적한다. 뗌은 키보드 진입부에서 제외한다.
    pub(super) fn egui_mesh_push_key(&mut self, surface_id: u32, event: &winit::event::KeyEvent) {
        let modifiers = self.mesh_modifiers();
        let Some(ev) = key_wire_event(
            &event.logical_key,
            event.physical_key,
            matches!(event.state, ElementState::Pressed),
            event.repeat,
            modifiers,
        ) else {
            return;
        };
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(ev);
    }

    /// 텍스트 입력을 egui-mesh surface 에 누적(Text wire 이벤트). 빈 문자열은 무시.
    pub(super) fn egui_mesh_push_text(&mut self, surface_id: u32, text: &str) {
        if text.is_empty() {
            return;
        }
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Text {
            text: text.to_string(),
        });
    }

    /// 조합 중 문자열과 확정 문자열을 플러그인에 전달한다.
    pub(super) fn egui_mesh_push_ime(&mut self, surface_id: u32, event: ImeWire) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Ime { event });
    }

    /// 이 창의 surface가 무효화됐으면 다음 컨텍스트 전송을 요청한다. 다른 창의 ID는 무시한다.
    pub(crate) fn mark_surface_invalidated(&mut self, surface_id: u32) -> bool {
        let exists = self
            .state
            .egui_mesh_surfaces_existing(&self.core_state)
            .iter()
            .any(|(sid, _)| *sid == surface_id);
        if !exists {
            return false;
        }
        self.egui_mesh
            .entry(surface_id)
            .or_default()
            .set_invalidated();
        true
    }

    /// 활성 workspace 의 egui-mesh surface 들에 렌더 컨텍스트를 forward.
    /// [`MainView::handle_redraw`] 가 합성(`gpu.render`) 직전에 부른다.
    pub(super) fn forward_egui_mesh_context(&mut self, mgr: &PluginManager) {
        let terminal_rect = self.compute_terminal_rect();
        let ppp = self.base.gpu.scale_factor();
        let focused = self.state.focused_surface_id(&self.core_state);
        let modifiers = self.mesh_modifiers();
        let current_theme = self.mesh_theme_snapshot();

        let mut targets: Vec<MeshTarget> = Vec::new();
        for (_pane_id, _pane_rect, regions) in self.state.surface_regions(
            &self.core_state,
            terminal_rect,
            self.base.gpu.scale_factor(),
        ) {
            for r in regions {
                if let Some(ms) = r.surface.as_any().downcast_ref::<EguiMeshSurface>() {
                    targets.push(MeshTarget {
                        sid: r.id,
                        plugin_id: ms.plugin_id.clone(),
                        rect: r.rect,
                        kind: ms.kind_static,
                        file: ms.file.clone(),
                        display_name: ms.display_name.clone(),
                    });
                }
            }
        }

        // 숨겨진 surface 상태도 보존하고 레이아웃에서 사라진 것만 정리한다.
        let existing = self.state.egui_mesh_surfaces_existing(&self.core_state);
        let live: HashSet<u32> = existing.iter().map(|e| e.0).collect();
        self.egui_mesh.retain(|sid, _| live.contains(sid));

        let visible: HashSet<u32> = targets.iter().map(|t| t.sid).collect();

        // 포커스가 바뀌면 이전 IME 위치가 남지 않게 매 프레임 새로 채운다.
        self.egui_mesh_ime_cursor_area = None;

        for MeshTarget {
            sid,
            plugin_id,
            rect,
            kind,
            file,
            display_name,
        } in targets
        {
            let w = rect.width.value().round().max(1.0) as u32;
            let h = rect.height.value().round().max(1.0) as u32;
            let geom = (w, h, ppp.to_bits());
            let has_frame = mgr.egui_mesh_frame(sid).is_some();

            let st = self.egui_mesh.entry(sid).or_default();
            st.plugin_id = Some(plugin_id.clone());
            st.common.watch_blank(
                has_frame,
                format_args!("surface {sid} (kind '{kind}', plugin '{plugin_id}')"),
                &plugin_id,
            );
            let is_focused = focused == Some(sid);
            // 전송을 생략할 프레임도 IME 위치는 필요하므로 조기 반환 전에 갱신한다.
            if is_focused
                && let Some(ime) = mgr.egui_mesh_frame(sid).and_then(|f| f.ime_cursor.as_ref())
            {
                self.egui_mesh_ime_cursor_area =
                    Some(crate::plugin_bridge::mesh_ime_cursor_area(rect, ime, ppp));
            }
            let geom_changed = st.common.geom_changed(geom);
            let has_input = !st.events.is_empty();
            let need_bootstrap = st.common.need_bootstrap(has_frame);
            let theme_changed = st.common.theme_changed(&current_theme);
            let need_full = st.common.pending_full;
            let focus_changed = st.last_focused != Some(is_focused);
            let invalidated = st.invalidated;

            if !(geom_changed
                || has_input
                || need_bootstrap
                || theme_changed
                || need_full
                || focus_changed
                || invalidated)
            {
                continue;
            }

            let events = std::mem::take(&mut st.events);
            st.common.record_sent(geom, &current_theme, has_frame);
            st.common.pending_full = false;
            st.last_focused = Some(is_focused);
            st.invalidated = false;

            // 생성 params가 먼저 도착하도록 set_context보다 surface.create를 먼저 보낸다.
            if need_bootstrap {
                mgr.send_egui_mesh_surface_create(
                    &plugin_id,
                    sid,
                    kind,
                    file.as_deref(),
                    &display_name,
                );
            }

            let params = SurfaceSetContextParams {
                surface_id: sid,
                width_px: w,
                height_px: h,
                pixels_per_point: ppp,
                raw_input: RawInputWire {
                    time: Some(mesh_time_now()),
                    focused: is_focused,
                    modifiers,
                    events,
                },
                theme: Some(current_theme.clone()),
                need_full_textures: need_full,
            };
            mgr.send_surface_set_context(&plugin_id, &params);
        }

        // 숨겨진 surface의 전체 텍스처 요청은 마지막 영역·테마와 빈 입력으로 보낸다.
        for (sid, st) in self.egui_mesh.iter_mut() {
            if !st.common.pending_full || visible.contains(sid) {
                continue;
            }
            let (Some((w, h, ppp_bits)), Some(plugin_id)) =
                (st.common.last_geom, st.plugin_id.as_ref())
            else {
                st.common.pending_full = false;
                continue;
            };
            st.common.pending_full = false;
            let params = SurfaceSetContextParams {
                surface_id: *sid,
                width_px: w,
                height_px: h,
                pixels_per_point: f32::from_bits(ppp_bits),
                raw_input: RawInputWire {
                    time: Some(mesh_time_now()),
                    focused: false,
                    modifiers: ModifiersWire::default(),
                    events: Vec::new(),
                },
                theme: st.common.last_theme.clone(),
                need_full_textures: true,
            };
            mgr.send_surface_set_context(plugin_id, &params);
        }
    }

    /// GUI가 만든 mesh를 attach 구독자에게 중계한다. 평소에는 새 컨텍스트를 만들지 않는다.
    /// 아직 한 번도 렌더하지 않은 surface만 초기 생성·컨텍스트를 보낸다.
    /// 기존 surface의 새 구독·복구 요청은 pending_full에 넣고 해당 tick의 중계를 생략한다.
    pub(super) fn forward_mesh_to_attach_subscribers(
        &mut self,
        mgr: &PluginManager,
        stream_hub: &StreamHub,
    ) {
        for sid in self.core_state.mesh_mirror.active_surface_ids() {
            let Some(ctx) = self.core_state.mesh_mirror.get(sid) else {
                continue;
            };
            let client_id = ctx.client_id;
            let width_px = ctx.width_px;
            let height_px = ctx.height_px;
            let pixels_per_point = ctx.pixels_per_point;
            let theme = ctx.theme.clone();
            let focused = ctx.focused;

            let need_full = self.core_state.mesh_mirror.take_need_full_textures(sid);
            // 컨텍스트 전송은 기존 로컬 경로가 맡으며 여기서는 변경 표시만 비운다.
            let _ = self.core_state.mesh_mirror.take_dirty(sid);

            if mgr.egui_mesh_frame(sid).is_none() {
                let Some(ms) = self.core_state.find_egui_mesh_surface(sid) else {
                    self.core_state.mesh_mirror.remove(sid);
                    continue;
                };
                let plugin_id = ms.plugin_id.clone();
                mgr.send_egui_mesh_surface_create(
                    &plugin_id,
                    sid,
                    ms.kind_static,
                    ms.file.as_deref(),
                    &ms.display_name,
                );
                mgr.send_surface_set_context(
                    &plugin_id,
                    &SurfaceSetContextParams {
                        surface_id: sid,
                        width_px,
                        height_px,
                        pixels_per_point,
                        raw_input: RawInputWire {
                            time: None,
                            focused,
                            modifiers: ModifiersWire::default(),
                            events: Vec::new(),
                        },
                        theme: theme.clone(),
                        need_full_textures: true,
                    },
                );

                // 이후 로컬 표시·전체 재전송에 사용할 최소 상태를 기록한다.
                let st = self.egui_mesh.entry(sid).or_default();
                st.plugin_id = Some(plugin_id);
                st.common.last_geom = Some((width_px, height_px, pixels_per_point.to_bits()));
                st.common.last_theme = theme;
                st.last_focused = Some(focused);
                st.common.bootstrap_sent = true;
            } else if need_full {
                self.egui_mesh.entry(sid).or_default().set_pending_full();
                self.base.dirty = true;
                continue;
            }

            crate::plugin_bridge::mesh_forward::relay_mesh_frame_if_new(
                &mut self.core_state,
                mgr,
                stream_hub,
                sid,
                client_id,
            );
        }
    }
}

/// winit 마우스 버튼 → wire 포인터 버튼. 매핑 불가한 버튼(Back/Forward/Other)은 무시.
/// `pub(super)` — `attach_mesh_input.rs` 재사용.
pub(super) fn map_button(button: MouseButton) -> Option<PointerButtonWire> {
    match button {
        MouseButton::Left => Some(PointerButtonWire::Primary),
        MouseButton::Right => Some(PointerButtonWire::Secondary),
        MouseButton::Middle => Some(PointerButtonWire::Middle),
        _ => None,
    }
}

/// `ElementState` → pressed bool 헬퍼 (호출부 가독성).
pub(super) fn is_pressed(state: ElementState) -> bool {
    matches!(state, ElementState::Pressed)
}

/// 매핑 가능한 키를 wire 이벤트로 만든다. egui 키 이름으로 보내 SDK가 복원한다.
pub(super) fn key_wire_event(
    logical: &WinitKey,
    physical: PhysicalKey,
    pressed: bool,
    repeat: bool,
    modifiers: ModifiersWire,
) -> Option<RawInputEventWire> {
    let key = winit_key_to_egui(logical, physical)?;
    Some(RawInputEventWire::Key {
        key: key.name().to_string(),
        pressed,
        repeat,
        modifiers,
    })
}

/// winit 논리/물리 키를 egui `Key` 로 변환한다(egui-winit `key_from_winit_key` +
/// `key_from_key_code` 미러). 논리 키를 우선하고, 비-라틴 레이아웃(예: 한글)에서
/// `Ctrl+A`(select-all)·`Ctrl+화살표` 같은 편집 단축키가 **물리 키 위치**로 매칭되도록
/// 물리 키로 폴백한다(<https://github.com/emilk/egui/issues/3653> 와 동일 근거).
fn winit_key_to_egui(logical: &WinitKey, physical: PhysicalKey) -> Option<egui::Key> {
    let logical = match logical {
        WinitKey::Named(named) => named_key_to_egui(*named),
        WinitKey::Character(s) => egui::Key::from_name(s.as_str()),
        WinitKey::Unidentified(_) | WinitKey::Dead(_) => None,
    };
    let physical = match physical {
        PhysicalKey::Code(code) => keycode_to_egui(code),
        PhysicalKey::Unidentified(_) => None,
    };
    logical.or(physical)
}

/// winit `NamedKey` → egui `Key` (egui-winit `key_from_named_key` 미러).
fn named_key_to_egui(named: NamedKey) -> Option<egui::Key> {
    use egui::Key;
    Some(match named {
        NamedKey::Enter => Key::Enter,
        NamedKey::Tab => Key::Tab,
        NamedKey::ArrowDown => Key::ArrowDown,
        NamedKey::ArrowLeft => Key::ArrowLeft,
        NamedKey::ArrowRight => Key::ArrowRight,
        NamedKey::ArrowUp => Key::ArrowUp,
        NamedKey::End => Key::End,
        NamedKey::Home => Key::Home,
        NamedKey::PageDown => Key::PageDown,
        NamedKey::PageUp => Key::PageUp,
        NamedKey::Backspace => Key::Backspace,
        NamedKey::Delete => Key::Delete,
        NamedKey::Insert => Key::Insert,
        NamedKey::Escape => Key::Escape,
        NamedKey::Cut => Key::Cut,
        NamedKey::Copy => Key::Copy,
        NamedKey::Paste => Key::Paste,
        NamedKey::Space => Key::Space,
        NamedKey::F1 => Key::F1,
        NamedKey::F2 => Key::F2,
        NamedKey::F3 => Key::F3,
        NamedKey::F4 => Key::F4,
        NamedKey::F5 => Key::F5,
        NamedKey::F6 => Key::F6,
        NamedKey::F7 => Key::F7,
        NamedKey::F8 => Key::F8,
        NamedKey::F9 => Key::F9,
        NamedKey::F10 => Key::F10,
        NamedKey::F11 => Key::F11,
        NamedKey::F12 => Key::F12,
        NamedKey::F13 => Key::F13,
        NamedKey::F14 => Key::F14,
        NamedKey::F15 => Key::F15,
        NamedKey::F16 => Key::F16,
        NamedKey::F17 => Key::F17,
        NamedKey::F18 => Key::F18,
        NamedKey::F19 => Key::F19,
        NamedKey::F20 => Key::F20,
        NamedKey::F21 => Key::F21,
        NamedKey::F22 => Key::F22,
        NamedKey::F23 => Key::F23,
        NamedKey::F24 => Key::F24,
        NamedKey::F25 => Key::F25,
        NamedKey::F26 => Key::F26,
        NamedKey::F27 => Key::F27,
        NamedKey::F28 => Key::F28,
        NamedKey::F29 => Key::F29,
        NamedKey::F30 => Key::F30,
        NamedKey::F31 => Key::F31,
        NamedKey::F32 => Key::F32,
        NamedKey::F33 => Key::F33,
        NamedKey::F34 => Key::F34,
        NamedKey::F35 => Key::F35,
        _ => return None,
    })
}

/// winit `KeyCode`(물리 위치) → egui `Key` (egui-winit `key_from_key_code` 미러).
fn keycode_to_egui(code: KeyCode) -> Option<egui::Key> {
    use egui::Key;
    Some(match code {
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Insert => Key::Insert,
        KeyCode::Delete => Key::Delete,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Space => Key::Space,
        KeyCode::Comma => Key::Comma,
        KeyCode::Period => Key::Period,
        KeyCode::Semicolon => Key::Semicolon,
        KeyCode::Backslash => Key::Backslash,
        KeyCode::Slash | KeyCode::NumpadDivide => Key::Slash,
        KeyCode::BracketLeft => Key::OpenBracket,
        KeyCode::BracketRight => Key::CloseBracket,
        KeyCode::Backquote => Key::Backtick,
        KeyCode::Quote => Key::Quote,
        KeyCode::Cut => Key::Cut,
        KeyCode::Copy => Key::Copy,
        KeyCode::Paste => Key::Paste,
        KeyCode::Minus | KeyCode::NumpadSubtract => Key::Minus,
        KeyCode::NumpadAdd => Key::Plus,
        KeyCode::Equal => Key::Equals,
        KeyCode::Digit0 | KeyCode::Numpad0 => Key::Num0,
        KeyCode::Digit1 | KeyCode::Numpad1 => Key::Num1,
        KeyCode::Digit2 | KeyCode::Numpad2 => Key::Num2,
        KeyCode::Digit3 | KeyCode::Numpad3 => Key::Num3,
        KeyCode::Digit4 | KeyCode::Numpad4 => Key::Num4,
        KeyCode::Digit5 | KeyCode::Numpad5 => Key::Num5,
        KeyCode::Digit6 | KeyCode::Numpad6 => Key::Num6,
        KeyCode::Digit7 | KeyCode::Numpad7 => Key::Num7,
        KeyCode::Digit8 | KeyCode::Numpad8 => Key::Num8,
        KeyCode::Digit9 | KeyCode::Numpad9 => Key::Num9,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        KeyCode::F4 => Key::F4,
        KeyCode::F5 => Key::F5,
        KeyCode::F6 => Key::F6,
        KeyCode::F7 => Key::F7,
        KeyCode::F8 => Key::F8,
        KeyCode::F9 => Key::F9,
        KeyCode::F10 => Key::F10,
        KeyCode::F11 => Key::F11,
        KeyCode::F12 => Key::F12,
        KeyCode::F13 => Key::F13,
        KeyCode::F14 => Key::F14,
        KeyCode::F15 => Key::F15,
        KeyCode::F16 => Key::F16,
        KeyCode::F17 => Key::F17,
        KeyCode::F18 => Key::F18,
        KeyCode::F19 => Key::F19,
        KeyCode::F20 => Key::F20,
        KeyCode::F21 => Key::F21,
        KeyCode::F22 => Key::F22,
        KeyCode::F23 => Key::F23,
        KeyCode::F24 => Key::F24,
        KeyCode::F25 => Key::F25,
        KeyCode::F26 => Key::F26,
        KeyCode::F27 => Key::F27,
        KeyCode::F28 => Key::F28,
        KeyCode::F29 => Key::F29,
        KeyCode::F30 => Key::F30,
        KeyCode::F31 => Key::F31,
        KeyCode::F32 => Key::F32,
        KeyCode::F33 => Key::F33,
        KeyCode::F34 => Key::F34,
        KeyCode::F35 => Key::F35,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn phys(code: KeyCode) -> PhysicalKey {
        PhysicalKey::Code(code)
    }

    #[test]
    fn named_keys_map_from_logical() {
        assert_eq!(
            winit_key_to_egui(&WinitKey::Named(NamedKey::Space), phys(KeyCode::Space)),
            Some(egui::Key::Space)
        );
        assert_eq!(
            winit_key_to_egui(
                &WinitKey::Named(NamedKey::Enter),
                PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified)
            ),
            Some(egui::Key::Enter)
        );
    }

    #[test]
    fn latin_char_maps_from_logical() {
        let a: SmolStr = "a".into();
        assert_eq!(
            winit_key_to_egui(&WinitKey::Character(a), phys(KeyCode::KeyA)),
            Some(egui::Key::A)
        );
    }

    #[test]
    fn non_latin_char_falls_back_to_physical() {
        let hangul: SmolStr = "ㅁ".into();
        assert_eq!(
            winit_key_to_egui(&WinitKey::Character(hangul), phys(KeyCode::KeyA)),
            Some(egui::Key::A)
        );
    }

    #[test]
    fn unmappable_key_is_none() {
        let dead: SmolStr = "\u{1}".into();
        assert_eq!(
            winit_key_to_egui(
                &WinitKey::Character(dead),
                PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified)
            ),
            None
        );
    }

    #[test]
    fn key_wire_event_carries_egui_key_name() {
        let mods = ModifiersWire {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        let ev = key_wire_event(
            &WinitKey::Character("a".into()),
            phys(KeyCode::KeyA),
            true,
            false,
            mods,
        )
        .expect("mapped");
        match ev {
            RawInputEventWire::Key {
                key,
                pressed,
                repeat,
                modifiers,
            } => {
                assert_eq!(egui::Key::from_name(&key), Some(egui::Key::A));
                assert!(pressed);
                assert!(!repeat);
                assert!(modifiers.ctrl && modifiers.command);
            }
            other => panic!("expected Key wire event, got {other:?}"),
        }
    }

    #[test]
    fn key_wire_event_skips_unmappable() {
        let ev = key_wire_event(
            &WinitKey::Character("\u{1}".into()),
            PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
            true,
            false,
            ModifiersWire::default(),
        );
        assert!(ev.is_none());
    }
}
