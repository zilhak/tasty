//! Plugin popup 인스턴스 렌더링.
//!
//! `PluginManager::popup_instances`에 등록된 popup을 매 프레임 그린다.
//! **egui-mesh popup (A2)**: plugin 이 자기 프로세스에서 egui mesh 를 tessellate 하고,
//! host 는 셸(scrim/bg/border/outside-click/Esc)만 그린 뒤 콘텐츠 영역에 plugin mesh 를
//! 합성한다(합성은 `gpu.render` 가 host egui pass 후 수행 — `egui_mesh_prepare`).
//!
//! 호스트 본문 popup(`PopupManager`)과는 별도 경로 — plugin popup은 동적 instance_id를
//! 가지고 `&'static str` 기반 `PopupId`/`PopupDef` 모델에 맞지 않기 때문.
//!
//! 사용자 입력은 `popup.set_context` 의 raw_input 으로 모은 뒤 plugin 에 forward 한다.
//! set_context 송신 자체는 host 렌더 파이프라인의 일부라 사용자 상태에 부수효과가
//! 없다(identity 원칙 1·3).

use std::collections::HashSet;

use egui::{Context, Event, Id, Order, Pos2, Rect, Stroke, Vec2};
use tasty_plugin_manifest::PopupRendering;
use tasty_plugin_protocol::{
    ModifiersWire, PointerButtonWire, PopupCloseReason, PopupSetContextParams, RawInputEventWire,
    RawInputWire, ThemeWire,
};

use crate::adapters::ui::popup;
use crate::model::{PhysicalPx, PhysicalRect};
use crate::plugin::PluginManager;
use crate::plugin::manifest::PopupAnchor;
use crate::state::AppState;

const DEFAULT_POPUP_SIZE: Vec2 = Vec2::new(360.0, 200.0);

/// 매 egui 프레임 호출. plugin popup_instances를 순회하면서:
///  - egui-mesh 인스턴스는 셸을 그리고 `popup.set_context` 를 forward, 합성 영역을
///    `state.plugin_mesh_popup_regions` 에 적재(실 합성은 `gpu.render`),
///  - 외부 클릭/Escape를 감지해 `state.plugin_popup_closes`에 적재.
///
/// `mgr`이 `None`이거나 popup_instances가 비어있으면 mesh 영역만 비우고 반환.
pub fn draw_plugin_popups(
    ctx: &Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    plugin_manager: Option<&PluginManager>,
) {
    // 매 frame mesh 합성 영역을 새로 수집한다 — 이전 frame 잔재가 합성되지 않게.
    state.plugin_mesh_popup_regions.clear();

    let Some(mgr) = plugin_manager else {
        state.plugin_mesh_popup_geom.clear();
        state.plugin_mesh_popup_theme.clear();
        return;
    };

    // popup_instances를 즉시 owned snapshot으로 복사. 이후 loop에서 `state`를 mutable로
    // borrow하기 위함.
    struct MeshSnap {
        instance_id: u64,
        plugin_id: String,
        anchor: PopupAnchor,
        size: Vec2,
        dismiss_on_outside_click: bool,
    }
    let mut mesh_snaps: Vec<MeshSnap> = Vec::new();
    for (id, inst) in mgr.popup_instances() {
        let size = inst
            .contribute
            .size_hint
            .map(|s| Vec2::new(s.width as f32, s.height as f32))
            .unwrap_or(DEFAULT_POPUP_SIZE);
        match inst.contribute.rendering {
            PopupRendering::EguiMesh => mesh_snaps.push(MeshSnap {
                instance_id: id,
                plugin_id: inst.plugin_id.clone(),
                anchor: inst.contribute.anchor,
                size,
                dismiss_on_outside_click: inst.contribute.dismiss_on_outside_click,
            }),
        }
    }

    // 닫힌 mesh popup 의 geom/bootstrap 추적 정리.
    let live_mesh: HashSet<u64> = mesh_snaps.iter().map(|s| s.instance_id).collect();
    state
        .plugin_mesh_popup_geom
        .retain(|k, _| live_mesh.contains(k));
    state
        .plugin_mesh_popup_bootstrapped
        .retain(|k| live_mesh.contains(k));

    if mesh_snaps.is_empty() {
        return;
    }

    let screen_rect = ctx.screen_rect();
    let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
    let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    let mut any_hovered = false;

    // ── egui-mesh popups (A2) ──
    let ppp = ctx.pixels_per_point().max(f32::EPSILON);
    // 현재 resolved Theme 스냅샷을 1회 만든다(popup 무관). plugin 이 host 와 동일 Theme 으로
    // 재구성하도록 색 집합+is_light+UI zoom 을 운반한다(surface forward 와 동형, ADR-0028 parity).
    let current_theme = {
        let th = crate::theme::theme();
        ThemeWire {
            colors: th.to_colors(),
            is_light: th.is_light,
            ui_zoom: _engine.settings.appearance.ui_scale_factor(),
        }
    };
    for snap in mesh_snaps {
        let pos = clamp_to_screen(
            anchor_pos(snap.anchor, snap.size, screen_rect, pointer_pos),
            snap.size,
            screen_rect,
        );
        let rect = egui::Rect::from_min_size(pos, snap.size);

        if let Some(p) = pointer_pos
            && rect.contains(p)
        {
            any_hovered = true;
        }

        // 셸(chrome)은 host 소유: scrim → bg_panel → border. 내용은 plugin mesh 가
        // content_rect 에 host egui pass 후 합성된다(gpu.render → egui_mesh_prepare).
        let layer_id = egui::LayerId::new(
            Order::Foreground,
            Id::new("plugin_mesh_popup").with(snap.instance_id),
        );
        let painter = ctx.layer_painter(layer_id);
        let th = crate::theme::theme();
        painter.rect_filled(screen_rect, 0.0, th.scrim().to_egui());
        painter.rect_filled(rect, th.corner_radius.value(), th.bg_panel().to_egui());
        painter.rect_stroke(
            rect,
            th.corner_radius.value(),
            Stroke::new(th.border_width.value(), th.border_default().to_egui()),
            egui::StrokeKind::Outside,
        );

        let content_rect = rect.shrink(popup::content_margin());
        let raw_input = collect_mesh_popup_input(ctx, content_rect, pointer_pos);

        // set_context forward — geom 변경 / 입력 / bootstrap(미paint) 일 때만 (surface 와 동형).
        // bootstrap 은 1회만: paint frame 이 도착하기 전 매 frame 스팸하면 plugin 이 여러 번
        // paint 하고(첫 frame 의 폰트 atlas delta 가 후속 frame 엔 없음) host 가 최신 frame 만
        // 보관해 atlas 를 못 받는다("Missing texture Managed(0)"). 1회 보내고 frame 을 기다린다.
        let w_px = (content_rect.width() * ppp).round().max(1.0) as u32;
        let h_px = (content_rect.height() * ppp).round().max(1.0) as u32;
        let geom = (w_px, h_px, ppp.to_bits());
        let has_input = !raw_input.events.is_empty();
        let has_frame = mgr.popup_mesh_frame(snap.instance_id).is_some();
        let bootstrapped = state
            .plugin_mesh_popup_bootstrapped
            .contains(&snap.instance_id);
        if has_frame {
            // 건강 상태 — crash 로 frame 이 사라지면 재bootstrap 하도록 무장 해제.
            state
                .plugin_mesh_popup_bootstrapped
                .remove(&snap.instance_id);
        }
        let geom_changed = state.plugin_mesh_popup_geom.get(&snap.instance_id) != Some(&geom);
        let theme_changed =
            state.plugin_mesh_popup_theme.get(&snap.instance_id) != Some(&current_theme);
        let need_bootstrap = !has_frame && !bootstrapped;
        if geom_changed || has_input || need_bootstrap || theme_changed {
            state.plugin_mesh_popup_geom.insert(snap.instance_id, geom);
            state
                .plugin_mesh_popup_theme
                .insert(snap.instance_id, current_theme.clone());
            if !has_frame {
                state
                    .plugin_mesh_popup_bootstrapped
                    .insert(snap.instance_id);
            }
            mgr.send_popup_set_context(
                &snap.plugin_id,
                &PopupSetContextParams {
                    instance_id: snap.instance_id,
                    width_px: w_px,
                    height_px: h_px,
                    pixels_per_point: ppp,
                    raw_input,
                    theme: Some(current_theme.clone()),
                },
            );
        }

        // 합성 영역(물리 px) 적재 — gpu.render 가 host egui pass 후 mesh 를 그린다.
        state.plugin_mesh_popup_regions.push((
            snap.instance_id,
            PhysicalRect {
                x: PhysicalPx(content_rect.min.x * ppp),
                y: PhysicalPx(content_rect.min.y * ppp),
                width: PhysicalPx(content_rect.width() * ppp),
                height: PhysicalPx(content_rect.height() * ppp),
            },
        ));

        if snap.dismiss_on_outside_click
            && primary_pressed
            && let Some(p) = pointer_pos
            && !rect.contains(p)
        {
            state
                .plugin_popup_closes
                .push((snap.instance_id, PopupCloseReason::OutsideClick));
        }
        if escape_pressed {
            state
                .plugin_popup_closes
                .push((snap.instance_id, PopupCloseReason::Escape));
        }
    }

    if any_hovered {
        state.popup_hovered = true;
    }
}

/// popup 콘텐츠 영역 위 egui 입력을 surface-local 논리 포인트(좌상단 0,0) 와이어로 변환.
///
/// host 가 받은 *실제* 사용자 입력만 forward 한다(identity 원칙 1·3). 포인터 이벤트는
/// 콘텐츠 영역 안의 것만 보내 — 영역 밖 클릭은 plugin 으로 새지 않고 outside-click
/// dismiss 로만 처리된다. 키/텍스트는 popup 이 포커스를 가진 동안 전달한다.
fn collect_mesh_popup_input(
    ctx: &Context,
    content_rect: Rect,
    pointer_pos: Option<Pos2>,
) -> RawInputWire {
    let origin = content_rect.min;
    let pointer_inside = pointer_pos.is_some_and(|p| content_rect.contains(p));
    ctx.input(|i| {
        let modifiers = map_modifiers(&i.modifiers);
        let mut events: Vec<RawInputEventWire> = Vec::new();
        for ev in &i.events {
            match ev {
                Event::PointerMoved(p) if content_rect.contains(*p) => {
                    events.push(RawInputEventWire::PointerMoved {
                        x: p.x - origin.x,
                        y: p.y - origin.y,
                    });
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: m,
                } if content_rect.contains(*pos) => {
                    if let Some(button) = map_button(*button) {
                        events.push(RawInputEventWire::PointerButton {
                            x: pos.x - origin.x,
                            y: pos.y - origin.y,
                            button,
                            pressed: *pressed,
                            modifiers: map_modifiers(m),
                        });
                    }
                }
                Event::MouseWheel { delta, .. } if pointer_inside => {
                    events.push(RawInputEventWire::Scroll {
                        x: delta.x,
                        y: delta.y,
                    });
                }
                Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers: m,
                    ..
                } => {
                    events.push(RawInputEventWire::Key {
                        key: key.name().to_string(),
                        pressed: *pressed,
                        repeat: *repeat,
                        modifiers: map_modifiers(m),
                    });
                }
                Event::Text(t) => events.push(RawInputEventWire::Text { text: t.clone() }),
                Event::PointerGone => events.push(RawInputEventWire::PointerGone),
                _ => {}
            }
        }
        RawInputWire {
            time: None,
            // popup 이 열려 있는 동안 콘텐츠는 포커스를 가진다(키 입력 라우팅).
            focused: true,
            modifiers,
            events,
        }
    })
}

fn map_modifiers(m: &egui::Modifiers) -> ModifiersWire {
    ModifiersWire {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        mac_cmd: m.mac_cmd,
        command: m.command,
    }
}

fn map_button(b: egui::PointerButton) -> Option<PointerButtonWire> {
    match b {
        egui::PointerButton::Primary => Some(PointerButtonWire::Primary),
        egui::PointerButton::Secondary => Some(PointerButtonWire::Secondary),
        egui::PointerButton::Middle => Some(PointerButtonWire::Middle),
        _ => None,
    }
}

/// 화면 경계 안으로 popup 좌상단을 clamp.
fn clamp_to_screen(pos: egui::Pos2, size: Vec2, screen_rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(
        pos.x.clamp(
            screen_rect.min.x,
            (screen_rect.max.x - size.x).max(screen_rect.min.x),
        ),
        pos.y.clamp(
            screen_rect.min.y,
            (screen_rect.max.y - size.y).max(screen_rect.min.y),
        ),
    )
}

fn anchor_pos(
    anchor: PopupAnchor,
    size: Vec2,
    screen_rect: egui::Rect,
    pointer_pos: Option<egui::Pos2>,
) -> egui::Pos2 {
    let centered = egui::pos2(
        screen_rect.center().x - size.x / 2.0,
        screen_rect.center().y - size.y / 2.0,
    );
    match anchor {
        PopupAnchor::ScreenCenter => centered,
        PopupAnchor::Cursor => pointer_pos.unwrap_or(centered),
        // ActiveSurfaceCenter는 현재 미구현 — 호스트 layout context 통합 필요.
        // 그동안은 ScreenCenter로 fallback.
        PopupAnchor::ActiveSurfaceCenter => centered,
    }
}
