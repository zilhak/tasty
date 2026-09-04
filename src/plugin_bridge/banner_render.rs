//! Plugin egui-mesh banner 합성 forward (A3).
//!
//! host banner manager([`crate::adapters::ui::BannerManager`])가 매 egui frame 셸(컨테이너/
//! border/close X/카운트다운)을 그리고 plugin 배너의 content_rect 를 슬롯으로 기록한다.
//! 이 모듈은 그 슬롯을 받아 `banner.set_context` 를 plugin 에 forward 하고, 합성 영역을
//! `state.plugin_mesh_banner_regions` 에 적재한다 — 실제 mesh 합성은 `gpu.render` 가 host
//! egui pass *후* content_rect 에 수행한다(`render_egui_mesh_banners`).
//!
//! popup([`super::popup_render`])과 평행하되, banner 는 non-modal 공지라 scrim/키보드
//! 포커스가 없어 content 영역 위 **포인터/스크롤 입력만** forward 한다(키/텍스트 없음, D3).
//! set_context 송신 자체는 host 렌더 파이프라인의 일부라 사용자 상태에 부수효과가 없다
//! (identity 원칙 1·3).

use egui::{Context, Event, Pos2, Rect};
use tasty_plugin_protocol::{
    BannerCloseReason, BannerSetContextParams, ModifiersWire, PointerButtonWire, RawInputEventWire,
    RawInputWire, ThemeWire,
};

use crate::adapters::ui::PluginBannerCloseKind;
use crate::model::LogicalPx;
use crate::plugin::PluginManager;
use crate::plugin_bridge::wire_scroll;
use crate::state::AppState;

/// 매 egui frame, host banner draw *후* 호출. banner manager 가 기록한 plugin mesh 슬롯을
/// 받아 set_context forward + 합성 영역 적재. host 측 생명주기(TTL/close X)로 닫힌 배너는
/// plugin 에 `banner.closed` 로 전파하고, plugin 이 죽어 mgr 에서 사라진 배너는 host UI 에서
/// 정리한다(양방향 reconcile).
pub fn draw_plugin_banners(
    ctx: &Context,
    state: &mut AppState,
    engine: &crate::core::CoreState,
    plugin_manager: Option<&PluginManager>,
) {
    // 매 frame 합성 영역을 새로 수집한다 — 이전 frame 잔재가 합성되지 않게.
    state.plugin_mesh_banner_regions.clear();

    // banner manager 가 이번 frame 그린 슬롯 + host 측 close 이벤트를 가져온다.
    let slots = state.banners.take_plugin_mesh_slots();
    let closed = state.banners.drain_closed_plugin_banners();

    let Some(mgr) = plugin_manager else {
        // plugin manager 부재 — 추적 상태 정리.
        state.plugin_mesh_banner_geom.clear();
        state.plugin_mesh_banner_bootstrapped.clear();
        state.plugin_mesh_banner_theme.clear();
        return;
    };

    // 1) host 측 생명주기(TTL/close X)로 닫힌 plugin 배너 → close 큐에 적재. 실제
    //    close_banner_instance(banner.closed 송신 + frame 정리)는 App 메인 루프가 drain 해
    //    호출한다 — 렌더 경로가 manager 를 직접 mutate 하지 않게(popup closes 와 동형).
    for (instance_id, kind) in closed {
        let reason = match kind {
            PluginBannerCloseKind::Ttl => BannerCloseReason::Ttl,
            PluginBannerCloseKind::UserClose => BannerCloseReason::UserClose,
        };
        state.plugin_banner_closes.push((instance_id, reason));
    }

    // 2) reconcile: host UI 에 있으나 mgr 에서 사라진(=plugin 종료) 배너는 UI 에서 제거.
    let live_in_mgr: std::collections::HashSet<u64> =
        mgr.banner_instances().map(|(iid, _)| iid).collect();
    let orphan_ui: Vec<u64> = state
        .banners
        .plugin_instances()
        .filter(|iid| !live_in_mgr.contains(iid))
        .collect();
    for iid in orphan_ui {
        state.banners.close_by_instance(iid);
    }

    // 닫힌 banner 의 추적 상태 정리.
    let live_slots: std::collections::HashSet<u64> = slots.iter().map(|s| s.instance_id).collect();
    state
        .plugin_mesh_banner_geom
        .retain(|k, _| live_slots.contains(k));
    state
        .plugin_mesh_banner_bootstrapped
        .retain(|k| live_slots.contains(k));
    state
        .plugin_mesh_banner_theme
        .retain(|k, _| live_slots.contains(k));

    if slots.is_empty() {
        return;
    }

    let ppp = ctx.pixels_per_point().max(f32::EPSILON);
    let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
    // 현재 resolved Theme 스냅샷 1회 (배너 무관). plugin 이 host 와 동일 Theme 으로
    // 재구성하도록 색 집합+is_light+UI zoom 을 운반한다(popup forward 와 동형, ADR-0028).
    let current_theme = {
        let th = crate::theme::theme();
        ThemeWire {
            colors: th.to_colors(),
            is_light: th.is_light,
            ui_zoom: engine.settings.appearance.ui_scale_factor(),
        }
    };

    for slot in &slots {
        let content_rect = slot.content_rect;
        let raw_input = collect_mesh_banner_input(ctx, content_rect, pointer_pos);

        // set_context forward — geom 변경 / 입력 / bootstrap(미paint) / theme 변경 시만.
        // bootstrap 은 1회만 (popup 과 동일: 첫 frame 폰트 atlas delta 를 host 가 반드시 decode).
        let physical = crate::plugin_bridge::mesh_region_of(content_rect, ppp);
        let w_px = physical.width.value().round().max(1.0) as u32;
        let h_px = physical.height.value().round().max(1.0) as u32;
        let geom = (w_px, h_px, ppp.to_bits());
        let has_input = !raw_input.events.is_empty();
        let has_frame = mgr.banner_mesh_frame(slot.instance_id).is_some();
        let bootstrapped = state
            .plugin_mesh_banner_bootstrapped
            .contains(&slot.instance_id);
        if has_frame {
            // 건강 상태 — crash 로 frame 이 사라지면 재bootstrap 하도록 무장 해제.
            state
                .plugin_mesh_banner_bootstrapped
                .remove(&slot.instance_id);
        }
        let geom_changed = state.plugin_mesh_banner_geom.get(&slot.instance_id) != Some(&geom);
        let theme_changed =
            state.plugin_mesh_banner_theme.get(&slot.instance_id) != Some(&current_theme);
        let need_bootstrap = !has_frame && !bootstrapped;
        // 렌더 prepare 의 textures_delta 체인 단절 감지 — full 재전송 요청을 소비해
        // need_full_textures 를 실어 보낸다(popup 과 동형).
        let need_full = state
            .plugin_mesh_banner_full_requests
            .remove(&slot.instance_id);
        if geom_changed || has_input || need_bootstrap || theme_changed || need_full {
            state.plugin_mesh_banner_geom.insert(slot.instance_id, geom);
            state
                .plugin_mesh_banner_theme
                .insert(slot.instance_id, current_theme.clone());
            if !has_frame {
                state
                    .plugin_mesh_banner_bootstrapped
                    .insert(slot.instance_id);
            }
            mgr.send_banner_set_context(
                &slot.plugin_id,
                &BannerSetContextParams {
                    instance_id: slot.instance_id,
                    width_px: w_px,
                    height_px: h_px,
                    pixels_per_point: ppp,
                    raw_input,
                    theme: Some(current_theme.clone()),
                    need_full_textures: need_full,
                },
            );
        }

        // 합성 영역(물리 px) 적재 — gpu.render 가 host egui pass 후 mesh 를 그린다.
        state
            .plugin_mesh_banner_regions
            .push((slot.instance_id, physical));
    }
}

/// banner content 영역 위 egui 입력을 content-local 논리 포인트(좌상단 0,0) 와이어로 변환.
///
/// host 가 받은 *실제* 사용자 입력만 forward 한다(identity 원칙 1·3). banner 는 non-modal
/// 이라 키보드 포커스가 없다 — 포인터/스크롤만 보내고 focused=false (키/텍스트 없음, D3).
/// 포인터 이벤트는 content 영역 안의 것만 보낸다.
fn collect_mesh_banner_input(
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
                // 와이어 `Scroll` 은 논리 포인트 단위다 — 물리 마우스 휠이 싣고 오는
                // `Line` 단위를 여기서 환산하지 않으면 notch 당 1pt 만 도착해 egui-mesh
                // surface 와 이동량이 갈린다(`wire_scroll` 모듈 문서).
                Event::MouseWheel { unit, delta, .. } if pointer_inside => {
                    let (dx, dy) = wire_scroll::wheel_delta_to_points(
                        *unit,
                        *delta,
                        LogicalPx(i.screen_rect().height()),
                    );
                    events.push(RawInputEventWire::Scroll {
                        x: dx.value(),
                        y: dy.value(),
                    });
                }
                Event::PointerGone => events.push(RawInputEventWire::PointerGone),
                // 키/텍스트는 forward 하지 않는다 — banner 는 키보드 포커스를 안 받는다(D3).
                _ => {}
            }
        }
        RawInputWire {
            time: None,
            // banner 는 키보드 포커스가 없다.
            focused: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// banner content 위 휠 이벤트 하나가 와이어에 싣는 `Scroll` 값(논리 포인트)을
    /// **실제 수집 함수**로 잰다.
    fn collected_scroll(unit: egui::MouseWheelUnit, delta: egui::Vec2) -> Option<(f32, f32)> {
        let ctx = Context::default();
        let content_rect =
            Rect::from_min_size(Pos2::new(40.0, 40.0), egui::Vec2::new(400.0, 120.0));
        let pointer = Pos2::new(100.0, 80.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                egui::Vec2::new(1280.0, 800.0),
            )),
            events: vec![Event::MouseWheel {
                unit,
                delta,
                modifiers: egui::Modifiers::default(),
            }],
            ..Default::default()
        };
        let mut wire = None;
        // 한 pass 를 실제로 돌려 `InputState` 를 채운다. 렌더 산출물(`FullOutput`)은
        // 이 측정에 쓰지 않는다 — 필요한 것은 수집 함수가 만든 와이어 입력뿐이다.
        let _full_output = ctx.run(input, |ctx| {
            wire = Some(collect_mesh_banner_input(ctx, content_rect, Some(pointer)));
        });
        wire?.events.iter().find_map(|e| match e {
            RawInputEventWire::Scroll { x, y } => Some((*x, *y)),
            _ => None,
        })
    }

    /// banner 도 popup·surface 와 같은 배율을 쓴다 — 세 표면이 갈리지 않아야 한다.
    #[test]
    fn a_wheel_notch_reaches_the_wire_as_the_shared_line_scroll_distance() {
        let (dx, dy) = collected_scroll(egui::MouseWheelUnit::Line, egui::Vec2::new(0.0, -1.0))
            .expect("휠 이벤트가 와이어 Scroll 로 수집돼야 한다");
        assert_eq!(dx, 0.0);
        assert_eq!(dy, (wire_scroll::LINE_SCROLL * -1.0).value());
        assert_ne!(dy, -1.0);
    }

    /// 트랙패드 델타는 그대로 실린다(회귀 방지).
    #[test]
    fn a_trackpad_delta_reaches_the_wire_unchanged() {
        let (dx, dy) = collected_scroll(egui::MouseWheelUnit::Point, egui::Vec2::new(1.5, -9.0))
            .expect("휠 이벤트가 와이어 Scroll 로 수집돼야 한다");
        assert_eq!((dx, dy), (1.5, -9.0));
    }
}
