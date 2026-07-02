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

use tasty_plugin_protocol::{
    ModifiersWire, PointerButtonWire, RawInputEventWire, RawInputWire, SurfaceSetContextParams,
    ThemeWire,
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
}

impl MeshForwardState {
    /// 렌더 prepare 의 full 재전송 요청을 기록한다 — 다음 forward 에서 소비된다.
    /// (redraw 가 gpu 요청 대기열을 drain 하며 호출.)
    pub(crate) fn set_pending_full(&mut self) {
        self.pending_full = true;
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
    fn mesh_theme_snapshot(&self) -> ThemeWire {
        let theme = crate::theme::theme();
        ThemeWire {
            colors: theme.to_colors(),
            is_light: theme.is_light,
            ui_zoom: self.core_state.settings.appearance.ui_scale_factor(),
        }
    }

    /// 현재 modifier 상태를 wire 형태로.
    fn mesh_modifiers(&self) -> ModifiersWire {
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

    /// 물리 window 좌표를 surface-local 논리 포인트로 변환.
    fn mesh_local_point(&self, rect: PhysicalRect, x: f32, y: f32) -> (f32, f32) {
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

    /// 스크롤 델타(논리 포인트)를 egui-mesh surface 에 누적.
    pub(super) fn egui_mesh_push_scroll(&mut self, surface_id: u32, dx: f32, dy: f32) {
        let st = self.egui_mesh.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Scroll { x: dx, y: dy });
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
            let geom_changed = st.last_geom != Some(geom);
            let has_input = !st.events.is_empty();
            let need_bootstrap = !has_frame && !st.bootstrap_sent;
            let theme_changed = st.last_theme.as_ref() != Some(&current_theme);
            let need_full = st.pending_full;

            if !(geom_changed || has_input || need_bootstrap || theme_changed || need_full) {
                continue;
            }

            let events = std::mem::take(&mut st.events);
            st.last_geom = Some(geom);
            st.last_theme = Some(current_theme.clone());
            st.pending_full = false;
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
                    focused: focused == Some(sid),
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
fn map_button(button: MouseButton) -> Option<PointerButtonWire> {
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
