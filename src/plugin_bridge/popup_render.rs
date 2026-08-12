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
    // 매 frame mesh 합성 영역/셸 레이어 목록을 새로 수집한다 — 이전 frame 잔재가
    // 합성되거나 `enforce_host_plugin_popup_z_order`(egui_bridge.rs)에 남지 않게.
    state.plugin_mesh_popup_regions.clear();
    state.plugin_popup_layers.clear();

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
        z_seq: u64,
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
                z_seq: inst.z_seq,
            }),
        }
    }
    // `popup_instances` 는 HashMap 이라 순회 순서가 비결정적 — z_seq 오름차순으로 정렬해야
    // `plugin_mesh_popup_regions`(GPU 콘텐츠 합성 순서, 뒤에 push된 것이 위)에서 여러
    // plugin popup 이 동시에 열려 있을 때도 나중에 열리거나 클릭된 것이 콘텐츠 상 위에
    // 온다. 단, 이 정렬은 **셸(scrim/bg/border) 순서에는 영향이 없다** — 셸은
    // `ctx.layer_painter`로 직접 그리는 raw layer 라 `egui::Area`(`Areas::order`)를 거치지
    // 않으므로, 프레임 내 그리기 호출 순서가 최종 페인트 순서를 결정하지 않는다(egui
    // `GraphicLayers::drain` 소스 — order 밖 레이어는 별도 맵 순회로 덧붙여짐, 순서 보장
    // 없음). 여러 plugin popup 이 동시에 열렸을 때 그들끼리의 셸 순서까지 정확히
    // 강제하려면 host↔plugin 관계와 마찬가지로 `set_sublayer` 체인이 필요하지만
    // egui 는 1단 중첩만 지원해 N>2 개에서는 안전하지 않다 — 최소 설계 범위 밖으로 남긴다
    // (`gfx/gpu/egui_bridge.rs` 의 `enforce_host_plugin_popup_z_order` 문서 참고).
    mesh_snaps.sort_by_key(|s| s.z_seq);

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

        let content_rect = rect.shrink(popup::content_margin());

        // 셸(chrome)은 host 소유: scrim → bg_panel(content_rect 는 hole) → border. 내용은
        // plugin mesh 가 content_rect 에 합성된다(gpu.render → egui_mesh_prepare). host popup
        // 이 이 popup 보다 위여야 하는 프레임(z_seq 역전)에는 콘텐츠 합성이 host egui pass
        // *전*에 실행되므로, bg_panel 이 content_rect 까지 채우면 그 뒤(같은 pass 안, 이
        // 셸과 함께 그려지는) host popup 유무와 무관하게 방금 합성한 콘텐츠를 덮어버린다.
        // content_rect 를 비워 두면(hole) 어느 순서로 합성되든 셸이 콘텐츠를 가리지 않는다
        // (`gfx/gpu.rs` 의 `render_egui_pass`/`render_egui_mesh_popups` 순서 분기 참고).
        let layer_id = egui::LayerId::new(
            Order::Foreground,
            Id::new("plugin_mesh_popup").with(snap.instance_id),
        );
        state.plugin_popup_layers.push(layer_id);
        let painter = ctx.layer_painter(layer_id);
        let th = crate::theme::theme();
        painter.rect_filled(screen_rect, 0.0, th.scrim().to_egui());
        paint_shell_background_excluding_content(
            &painter,
            rect,
            content_rect,
            th.corner_radius.value(),
            th.bg_panel().to_egui(),
        );
        painter.rect_stroke(
            rect,
            th.corner_radius.value(),
            Stroke::new(th.border_width.value(), th.border_default().to_egui()),
            egui::StrokeKind::Outside,
        );

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
        // 렌더 prepare 의 textures_delta 체인 단절 감지 — full 재전송 요청을 소비해
        // need_full_textures 를 실어 보낸다(다른 트리거가 없어도 송신).
        let need_full = state
            .plugin_mesh_popup_full_requests
            .remove(&snap.instance_id);
        // (ADR-0056) 비동기 host→plugin push(예: 원격 git 조회 결과) 도착 후 강제
        // repaint — geom/input/theme 변경 없이도 plugin 이 새 내부 상태로 다시
        // 그리도록 이번 frame 에 set_context 를 보낸다.
        let need_repaint = state
            .plugin_mesh_popup_pending_repaint
            .remove(&snap.instance_id);
        if geom_changed || has_input || need_bootstrap || theme_changed || need_full || need_repaint
        {
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
                    need_full_textures: need_full,
                },
            );
        }

        // 합성 영역(물리 px) 적재 — gpu.render 가 이 popup 의 z_seq 와 현재 열린 host popup
        // 최대 z_seq 를 비교해 host egui pass 전/후 중 알맞은 시점에 mesh 를 합성한다.
        state.plugin_mesh_popup_regions.push((
            snap.instance_id,
            PhysicalRect {
                x: PhysicalPx(content_rect.min.x * ppp),
                y: PhysicalPx(content_rect.min.y * ppp),
                width: PhysicalPx(content_rect.width() * ppp),
                height: PhysicalPx(content_rect.height() * ppp),
            },
        ));

        // popup 내부 클릭 시 z-order 승격(규칙 7 "클릭된 것이 앞") — host popup 의
        // `bring_to_front`(click-to-front)와 동형. `mgr` 이 `&PluginManager` 불변
        // 참조라 여기서 직접 갱신할 수 없어 큐에 적재하고 App 메인 루프가 drain한다.
        if primary_pressed
            && let Some(p) = pointer_pos
            && rect.contains(p)
        {
            state.plugin_popup_focus_bumps.push(snap.instance_id);
        }

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

/// `rect` 를 `bg_fill` 로 채우되 `content_rect`(plugin mesh 콘텐츠가 합성될 영역)는 비워
/// 둔다("hole"). 4개의 축정렬 띠로 분해한다 — 상/하단 띠만 `rect`의 바깥쪽 모서리에 맞춰
/// 둥근 모서리를 적용하고(원래 단일 `rect_filled(rect, corner_radius, ..)` 와 동일한
/// 시각 결과), 좌/우 띠는 `content_rect` 상하 범위로 제한되어 둥글릴 모서리가 없다.
/// `content_margin() >= corner_radius` 라 모서리 곡선이 상/하단 띠 폭 안에 완전히
/// 들어온다(현재 테마 기본값: 둘 다 4px).
fn paint_shell_background_excluding_content(
    painter: &egui::Painter,
    rect: Rect,
    content_rect: Rect,
    corner_radius: f32,
    bg_fill: egui::Color32,
) {
    let cr = corner_radius as u8;
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, content_rect.min.y)),
        egui::CornerRadius {
            nw: cr,
            ne: cr,
            sw: 0,
            se: 0,
        },
        bg_fill,
    );
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.min.x, content_rect.max.y), rect.max),
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: cr,
            se: cr,
        },
        bg_fill,
    );
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.min.x, content_rect.min.y),
            Pos2::new(content_rect.min.x, content_rect.max.y),
        ),
        0.0,
        bg_fill,
    );
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(content_rect.max.x, content_rect.min.y),
            Pos2::new(rect.max.x, content_rect.max.y),
        ),
        0.0,
        bg_fill,
    );
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
