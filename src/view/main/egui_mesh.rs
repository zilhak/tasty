//! egui-mesh surface 의 host→plugin 렌더 컨텍스트 forward (A1-S7).
//!
//! host 는 egui-mesh surface 마다 `surface.set_context { width_px, height_px,
//! pixels_per_point, raw_input }` 를 owning plugin 에 보낸다. plugin 은 그 컨텍스트로
//! 자기 프로세스에서 egui 를 tessellate 한 mesh 를 [`PluginEvent::PaintFrame`] 으로
//! 비동기 회신하고, host 합성기(`gpu/egui_mesh_prepare.rs`)가 surface 영역에 그린다.
//!
//! # 언제 보내는가 (research-a1 §9-5)
//!
//! 정적 화면을 매 frame 무조건 보내지 않는다. surface 마다 마지막으로 보낸
//! (크기, ppp) 를 추적해 **다음 중 하나**일 때만 보낸다:
//! - 크기/ppp 변경 (리사이즈·DPI 전환)
//! - 누적된 사용자 입력 (클릭/스크롤/포인터 이동)
//! - 아직 한 번도 paint 받지 못함 (첫 bootstrap, 또는 plugin crash 후 재bootstrap)
//!
//! plugin 이 paint 를 보내면(=`egui_mesh_frame` 존재) bootstrap 플래그를 풀어, 이후
//! crash 로 frame 이 사라지면 자동으로 다시 bootstrap 한다(§9-7 crash 격리와 맞물림).
//!
//! # identity 경계 (불가침 원칙 1·3)
//!
//! set_context 송신 자체는 *에이전트 행동이 아니라 host 렌더 파이프라인의 일부*다 —
//! 사용자 상태(focus/스크롤/선택)에 부수효과를 주지 않는다. `raw_input` 에는 host 가
//! 받은 **실제 사용자 입력만** 담는다. 에이전트 IPC/CLI 가 raw_input 을 합성·주입하는
//! 진입로는 만들지 않는다(release 에 없음). 입력 주입이 필요하면 debug 격리만
//! (`docs/dev-guide/debug-ipc.md`).
//!
//! # 좌표 (typed-length)
//!
//! host 좌표는 [`PhysicalPx`]. egui 경계로 넘길 때 ppp 로 나눠 surface-local 논리
//! 포인트(좌상단 0,0)로 변환한다. wire 의 좌표는 egui interop ABI 미러라 raw f32.

use std::collections::HashSet;

use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey};

use tasty_plugin_protocol::{
    ImeWire, ModifiersWire, PointerButtonWire, RawInputEventWire, RawInputWire,
    SurfaceSetContextParams, ThemeWire,
};

use crate::model::{PhysicalPx, PhysicalRect};
use crate::plugin::PluginManager;
use crate::plugin_bridge::egui_mesh_surface::EguiMeshSurface;

use super::MainView;

/// 한 egui-mesh surface 의 host 측 forward 추적 상태.
///
/// layout 에 존재하는 동안 유지된다(가시성 무관) — 비가시 surface 의 full 재전송
/// 요청([`MeshForwardState::pending_full`])을 마지막 geom/plugin_id 로 보낼 수 있어야
/// 하기 때문. surface 가 닫히면 정리된다.
#[derive(Default)]
pub(crate) struct MeshForwardState {
    /// 마지막으로 보낸 (width_px, height_px, ppp.to_bits()). 변경 감지에 사용.
    last_geom: Option<(u32, u32, u32)>,
    /// 다음 set_context 에 실어 보낼 누적 입력 이벤트(순서 보존).
    events: Vec<RawInputEventWire>,
    /// plugin paint 를 아직 못 받은 동안 bootstrap set_context 를 1회만 보내기 위한 플래그.
    /// `egui_mesh_frame` 이 보이면 풀려, crash 후 frame 소실 시 재bootstrap 된다.
    bootstrap_sent: bool,
    /// 마지막으로 보낸 Theme 스냅샷. 테마 변경 시(크기/입력 무변이어도) 재forward 트리거.
    last_theme: Option<ThemeWire>,
    /// 렌더 prepare 가 textures_delta 체인 단절을 감지했다 — 다음 set_context 에
    /// `need_full_textures` 를 실어 보낸다(송신 시 해제).
    pending_full: bool,
    /// 이 surface 의 owning plugin id (첫 forward 시 기록). 비가시 상태에서 full
    /// 재전송 요청을 보낼 때 대상 plugin 을 알기 위해 보관한다.
    plugin_id: Option<String>,
    /// 직전 forward 의 focused 상태. 포커스만 바뀌어도(입력·크기·테마 무변) set_context
    /// 재전송을 트리거하기 위해 추적한다 — markdown 등 focused/unfocused 배경 즉시 전환.
    last_focused: Option<bool>,
    /// plugin 이 `SurfaceInvalidated` 로 알렸다(단계 06) — 다음 forward 게이트에서
    /// 무입력 재-forward 를 1회 트리거한다(송신 시 소거). idle(입력 무) 상태에서도
    /// 파일 변경이 반영되게 하는 유일한 진입점 — `App::event_handler` 가
    /// `mark_surface_invalidated` 로 세팅한다.
    invalidated: bool,
}

impl MeshForwardState {
    /// 렌더 prepare 의 full 재전송 요청을 기록한다 — 다음 forward 에서 소비된다.
    /// (redraw 가 gpu 요청 대기열을 drain 하며 호출.)
    pub(crate) fn set_pending_full(&mut self) {
        self.pending_full = true;
    }

    /// idle 상태에서 plugin 이 알린 파일 변경을 다음 forward 게이트에 무장한다(단계 06).
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
    /// (x, y) 물리 좌표가 egui-mesh surface 위에 있으면 (surface_id, plugin_id, rect) 반환.
    ///
    /// 합성기(`collect_egui_mesh_targets`)와 동일하게 `surface_regions` + `EguiMeshSurface`
    /// 다운캐스트로 판정해, 입력 좌표 변환이 합성 좌표와 같은 출처를 공유한다.
    pub(super) fn egui_mesh_target_at(
        &self,
        x: f32,
        y: f32,
    ) -> Option<(u32, String, PhysicalRect)> {
        let terminal_rect = self.compute_terminal_rect();
        for (_pane_id, _pane_rect, regions) in
            self.state.surface_regions(&self.core_state, terminal_rect)
        {
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

    /// 현재 resolved 전역 Theme 을 wire 스냅샷으로. plugin 이 host 와 동일 Theme 을
    /// 재구성하도록 색 집합 + is_light + UI zoom 을 담는다(sizing 은 plugin 이 zoom 으로
    /// 재도출). 매 forward 마다 1회 만들어, 테마 변경을 set_context 재송신 트리거로 쓴다.
    pub(super) fn mesh_theme_snapshot(&self) -> ThemeWire {
        let theme = crate::theme::theme();
        ThemeWire {
            colors: theme.to_colors(),
            is_light: theme.is_light,
            ui_zoom: self.core_state.settings.appearance.ui_scale_factor(),
        }
    }

    /// 현재 modifier 상태를 wire 형태로. `pub(super)` — attach mesh mirror(TODO 20,
    /// `attach_mesh_input.rs`)의 로컬 입력 캡처가 동일 좌표계/modifier 계산을 재사용한다.
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

    /// 물리 window 좌표를 surface-local 논리 포인트로 변환. `pub(super)` —
    /// `attach_mesh_input.rs` 재사용.
    pub(super) fn mesh_local_point(&self, rect: PhysicalRect, x: f32, y: f32) -> (f32, f32) {
        let ppp = self.base.gpu.scale_factor().max(f32::EPSILON);
        ((x - rect.x.value()) / ppp, (y - rect.y.value()) / ppp)
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

    /// 포인터가 이 egui-mesh surface 밖으로 나갔음을 1 회 forward(TODO 26) — 좌표
    /// 없이(`PointerGone` 은 위치 필드가 없다) hover 상태 해제만 알린다. `mouse.rs`
    /// 의 `update_mesh_hover` 가 hover 대상 전환 시점에 호출한다.
    pub(super) fn egui_mesh_push_pointer_gone(&mut self, surface_id: u32) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::PointerGone);
    }

    /// 스크롤 델타(논리 포인트)를 egui-mesh surface 에 누적.
    pub(super) fn egui_mesh_push_scroll(&mut self, surface_id: u32, dx: f32, dy: f32) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Scroll { x: dx, y: dy });
    }

    /// 포커스된 surface 가 egui-mesh(plugin 렌더 markdown/image 등)면 그 surface_id 반환.
    /// terminal·host-egui surface 는 `None` — 키/Text/IME forward 대상 판정에 쓴다.
    /// `downcast` 로 실제 [`EguiMeshSurface`] 인지 확인하므로, 임의 plugin 의 `Kind`
    /// surface(RemoteSurface 등)로 잘못 forward 되지 않는다.
    pub(super) fn focused_egui_mesh_surface_id(&self) -> Option<u32> {
        let sid = self.state.focused_surface_id(&self.core_state)?;
        let surface = self.core_state.find_surface_by_id(sid)?;
        surface
            .as_any()
            .downcast_ref::<EguiMeshSurface>()
            .map(|_| sid)
    }

    /// 키 누름을 egui-mesh surface 에 누적(Key wire 이벤트). 매핑 불가한 키는 무시.
    /// press-only — release 는 [`MainView::handle_keyboard_input`] 이 이미 걸러낸다
    /// (egui `TextEdit` 은 press + `RawInput.modifiers`(매 set_context 최신)로 편집·
    /// 네비게이션을 처리하므로 release 없이도 동작).
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

    /// IME 조합 이벤트를 egui-mesh surface 에 누적(라이브 preedit + commit). plugin 의
    /// `TextEdit` 이 조합 중간 상태를 인라인 렌더한다(commit-only 가 아닌 라이브 표시).
    pub(super) fn egui_mesh_push_ime(&mut self, surface_id: u32, event: ImeWire) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Ime { event });
    }

    /// plugin 이 `SurfaceInvalidated` 로 알린 surface 를 dirty 표시한다(단계 06). 다음
    /// forward 게이트에서 무입력 재-forward 를 트리거해, idle(입력 무) 상태에서도 파일
    /// 변경이 `RELOAD_CHECK_INTERVAL_SECS` 내 반영되게 한다. 이 View 의 layout 에 없는
    /// surface_id 는 무시(`App` 이 모든 window 의 View 를 순회하며 호출하므로 다른
    /// window 소관일 수 있다). 반환값은 `mark_dirty()`(redraw 요청)를 걸지 판단하는 데
    /// 쓴다.
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
        // modifier 는 surface 무관 — 루프 전에 1회 계산(차용 충돌 회피).
        let modifiers = self.mesh_modifiers();
        // 현재 resolved Theme 스냅샷을 1회 만든다(surface 무관). plugin 이 host 와 동일
        // Theme 으로 재구성하도록 색 집합+is_light+UI zoom 을 운반한다(ADR-0028 parity).
        let current_theme = self.mesh_theme_snapshot();

        // 대상 수집 (surface_id, plugin_id, 물리 rect, kind, file, display_name).
        // kind/file/display_name 은 bootstrap 시 plugin 에 보낼 surface.create params 용.
        let mut targets: Vec<MeshTarget> = Vec::new();
        for (_pane_id, _pane_rect, regions) in
            self.state.surface_regions(&self.core_state, terminal_rect)
        {
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

        // layout(전 workspace, 비활성 탭 포함)에서 사라진 surface 의 추적 상태 정리.
        // 가시성 기반이 아니라 존재 기반 — 비가시 surface 의 pending_full 요청을
        // 마지막 geom 으로 보낼 수 있도록 추적 상태를 보존한다.
        let existing = self.state.egui_mesh_surfaces_existing(&self.core_state);
        let live: HashSet<u32> = existing.iter().map(|e| e.0).collect();
        self.egui_mesh.retain(|sid, _| live.contains(sid));

        let visible: HashSet<u32> = targets.iter().map(|t| t.sid).collect();

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
            if has_frame {
                // 건강 상태 — 이후 crash 로 frame 이 사라지면 재bootstrap 하도록 무장.
                st.bootstrap_sent = false;
            }
            let is_focused = focused == Some(sid);
            let geom_changed = st.last_geom != Some(geom);
            let has_input = !st.events.is_empty();
            let need_bootstrap = !has_frame && !st.bootstrap_sent;
            let theme_changed = st.last_theme.as_ref() != Some(&current_theme);
            let need_full = st.pending_full;
            // 포커스 변화만으로도 재forward — 입력 없이 포커스만 잃는 경우(다른 surface
            // 클릭 등)에 markdown 배경이 focused 로 잔류하지 않도록 (B).
            let focus_changed = st.last_focused != Some(is_focused);
            // idle 상태에서 plugin 이 파일 변경을 알렸다(단계 06) — 입력/geom/theme/focus
            // 무변이어도 이 무입력 재-forward 로 다음 paint 의 poll_reload 가 돈다.
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
            st.last_geom = Some(geom);
            st.last_theme = Some(current_theme.clone());
            st.last_focused = Some(is_focused);
            st.pending_full = false;
            st.invalidated = false;
            if !has_frame {
                st.bootstrap_sent = true;
            }

            // 첫 bootstrap(아직 paint 못 받음): set_context 직전에 surface.create 를
            // 먼저 보낸다 — plugin 이 생성 params(file 등)를 받아 콘텐츠를 적재한 뒤
            // 같은 채널로 도착하는 set_context 로 렌더하게 한다(create→set_context 순서 보장).
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
                    time: None,
                    focused: is_focused,
                    modifiers,
                    events,
                },
                theme: Some(current_theme.clone()),
                need_full_textures: need_full,
            };
            mgr.send_surface_set_context(&plugin_id, &params);
        }

        // 비가시 surface 의 full 재전송 요청 — 렌더 prepare 가 비가시 디코드 중 체인
        // 단절을 감지한 경우다. 마지막으로 보낸 geom/theme 으로 빈 입력 set_context 를
        // 보내 plugin 이 전체 텍스처 상태를 동봉한 frame 을 재송신하게 한다. geom 이
        // 활성화 시점과 다르면 활성화가 다시 정규 set_context 를 보내므로 무해하다.
        for (sid, st) in self.egui_mesh.iter_mut() {
            if !st.pending_full || visible.contains(sid) {
                continue;
            }
            let (Some((w, h, ppp_bits)), Some(plugin_id)) = (st.last_geom, st.plugin_id.as_ref())
            else {
                // 아직 한 번도 forward 되지 않은 surface (frame 도 없음) — bootstrap 이
                // 첫 활성화에서 자연-full frame 을 만들므로 요청이 필요 없다.
                st.pending_full = false;
                continue;
            };
            st.pending_full = false;
            let params = SurfaceSetContextParams {
                surface_id: *sid,
                width_px: w,
                height_px: h,
                pixels_per_point: f32::from_bits(ppp_bits),
                raw_input: RawInputWire {
                    time: None,
                    focused: false,
                    modifiers: ModifiersWire::default(),
                    events: Vec::new(),
                },
                theme: st.last_theme.clone(),
                need_full_textures: true,
            };
            mgr.send_surface_set_context(plugin_id, &params);
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

/// forward 될 Key wire 이벤트를 만든다. 매핑 불가한 키는 `None`(forward 생략) —
/// wire 는 egui `Key::name()` 문자열을 나르고 plugin SDK 가 `Key::from_name` 으로
/// 복원한다(매핑 불가 키는 plugin 도 무시). `KeyEvent` 전체가 아니라 구성요소를
/// 받아 `KeyEvent` 생성 없이 단위테스트가 가능하게 한다.
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

    // 명명 키(Space/Enter/화살표)는 논리 키만으로 매핑된다.
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

    // 라틴 문자 키는 논리 키(`Key::from_name`)로 매핑된다.
    #[test]
    fn latin_char_maps_from_logical() {
        let a: SmolStr = "a".into();
        assert_eq!(
            winit_key_to_egui(&WinitKey::Character(a), phys(KeyCode::KeyA)),
            Some(egui::Key::A)
        );
    }

    // 비-라틴(한글) 논리 문자는 `from_name` 이 None → 물리 키로 폴백해 편집 단축키
    // (Ctrl+A select-all 등)가 물리 위치로 매칭된다.
    #[test]
    fn non_latin_char_falls_back_to_physical() {
        let hangul: SmolStr = "ㅁ".into();
        assert_eq!(
            winit_key_to_egui(&WinitKey::Character(hangul), phys(KeyCode::KeyA)),
            Some(egui::Key::A)
        );
    }

    // 논리·물리 모두 매핑 불가면 None → forward 생략.
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

    // key_wire_event: 매핑된 키는 egui `Key::name()` 문자열 + pressed/repeat/modifiers
    // 를 담은 Key wire 이벤트를 만든다. plugin 이 `Key::from_name` 으로 복원 가능해야 한다.
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

    // 매핑 불가한 키는 wire 이벤트를 만들지 않는다(forward 생략).
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
