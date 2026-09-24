//! 플러그인 배너의 콘텐츠 영역과 입력을 전달하고 GPU 합성 영역을 기록한다.
//! 호스트 배너를 그린 뒤 호출하며, 실제 mesh는 호스트 egui 렌더 뒤에 합성한다.
//! 배너에는 키보드 포커스를 주지 않고 포인터·스크롤만 전달한다.

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

/// 호스트에서 닫힌 배너와 플러그인 매니저에서 사라진 배너를 양쪽에 반영한다.
pub fn draw_plugin_banners(
    ctx: &Context,
    state: &mut AppState,
    engine: &crate::core::CoreState,
    plugin_manager: Option<&PluginManager>,
) {
    state.plugin_mesh_banner_regions.clear();

    let slots = state.banners.take_plugin_mesh_slots();
    let closed = state.banners.drain_closed_plugin_banners();

    let Some(mgr) = plugin_manager else {
        state.plugin_mesh_banner_forward.clear();
        return;
    };

    // 렌더 중 매니저를 변경하지 않고, 메인 루프에 닫기 처리를 요청한다.
    for (instance_id, kind) in closed {
        let reason = match kind {
            PluginBannerCloseKind::Ttl => BannerCloseReason::Ttl,
            PluginBannerCloseKind::UserClose => BannerCloseReason::UserClose,
        };
        state.plugin_banner_closes.push((instance_id, reason));
    }

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

    let live_slots: std::collections::HashSet<u64> = slots.iter().map(|s| s.instance_id).collect();
    state
        .plugin_mesh_banner_forward
        .retain(|k, _| live_slots.contains(k));

    if slots.is_empty() {
        return;
    }

    let ppp = ctx.pixels_per_point().max(f32::EPSILON);
    let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
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

        let physical = crate::plugin_bridge::mesh_region_of(content_rect, ppp);
        let w_px = physical.width.value().round().max(1.0) as u32;
        let h_px = physical.height.value().round().max(1.0) as u32;
        let geom = (w_px, h_px, ppp.to_bits());
        let has_input = !raw_input.events.is_empty();
        let has_frame = mgr.banner_mesh_frame(slot.instance_id).is_some();
        // 입력이나 크기 변경 없이 요청한 repaint도 처리한다.
        // fwd를 가변 차용하기 전에 AppState의 요청 집합에서 꺼낸다.
        let need_repaint = state
            .plugin_mesh_banner_pending_repaint
            .remove(&slot.instance_id);
        let fwd = state
            .plugin_mesh_banner_forward
            .entry(slot.instance_id)
            .or_default();
        let need_bootstrap = fwd.need_bootstrap(has_frame);
        fwd.watch_blank(
            has_frame,
            format_args!(
                "banner instance {} (plugin '{}')",
                slot.instance_id, slot.plugin_id
            ),
            &slot.plugin_id,
        );
        let geom_changed = fwd.geom_changed(geom);
        let theme_changed = fwd.theme_changed(&current_theme);
        let need_full = fwd.take_pending_full();
        if geom_changed || has_input || need_bootstrap || theme_changed || need_full || need_repaint
        {
            fwd.record_sent(geom, &current_theme, has_frame);
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

        state
            .plugin_mesh_banner_regions
            .push((slot.instance_id, physical));
    }
}

/// 콘텐츠 기준 논리 좌표로 포인터·스크롤을 전달한다. 키·텍스트는 전달하지 않는다.
/// PointerGone은 콘텐츠 영역 포함 여부와 관계없이 전달해 이전 hover 상태를 지운다.
fn collect_mesh_banner_input(
    ctx: &Context,
    content_rect: Rect,
    pointer_pos: Option<Pos2>,
) -> RawInputWire {
    let origin = content_rect.min;
    let pointer_inside = pointer_pos.is_some_and(|p| content_rect.contains(p));
    // ctx.input 안에서 같은 컨텍스트의 다른 잠금을 잡지 않도록 먼저 읽는다.
    let line = wire_scroll::line_scroll(ctx);
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
                // wire Scroll은 논리 포인트이므로 Line 입력은 노치 거리로 환산한다.
                Event::MouseWheel { unit, delta, .. } if pointer_inside => {
                    let (dx, dy) = wire_scroll::wheel_delta_to_points(
                        *unit,
                        *delta,
                        LogicalPx(i.screen_rect().height()),
                        line,
                    );
                    events.push(RawInputEventWire::Scroll {
                        x: dx.value(),
                        y: dy.value(),
                    });
                }
                Event::PointerGone => events.push(RawInputEventWire::PointerGone),
                _ => {}
            }
        }
        RawInputWire {
            time: None,
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

    fn collected_scroll(unit: egui::MouseWheelUnit, delta: egui::Vec2) -> Option<(f32, f32)> {
        collected_scroll_with_notch(unit, delta, tasty_settings::DEFAULT_WHEEL_LINE_SCROLL)
    }

    fn collected_scroll_with_notch(
        unit: egui::MouseWheelUnit,
        delta: egui::Vec2,
        notch: f32,
    ) -> Option<(f32, f32)> {
        let ctx = Context::default();
        // 호스트의 노치 거리를 지정하지 않으면 egui 기본값 40이 남는다.
        ctx.options_mut(|o| o.line_scroll_speed = notch);
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
        // 실제 입력 처리를 실행하되 렌더 결과는 이 검사에 사용하지 않는다.
        let _full_output = ctx.run(input, |ctx| {
            wire = Some(collect_mesh_banner_input(ctx, content_rect, Some(pointer)));
        });
        wire?.events.iter().find_map(|e| match e {
            RawInputEventWire::Scroll { x, y } => Some((*x, *y)),
            _ => None,
        })
    }

    #[test]
    fn a_wheel_notch_reaches_the_wire_as_the_shared_line_scroll_distance() {
        let (dx, dy) = collected_scroll(egui::MouseWheelUnit::Line, egui::Vec2::new(0.0, -1.0))
            .expect("휠 이벤트가 와이어 Scroll 로 수집돼야 한다");
        assert_eq!(dx, 0.0);
        assert_eq!(dy, -tasty_settings::DEFAULT_WHEEL_LINE_SCROLL);
        assert_ne!(dy, -1.0);
    }

    #[test]
    fn the_wire_distance_follows_the_host_option() {
        let (_, slow) = collected_scroll_with_notch(
            egui::MouseWheelUnit::Line,
            egui::Vec2::new(0.0, -1.0),
            20.0,
        )
        .expect("휠 이벤트가 와이어 Scroll 로 수집돼야 한다");
        let (_, fast) = collected_scroll_with_notch(
            egui::MouseWheelUnit::Line,
            egui::Vec2::new(0.0, -1.0),
            120.0,
        )
        .expect("휠 이벤트가 와이어 Scroll 로 수집돼야 한다");
        assert_eq!(slow, -20.0);
        assert_eq!(fast, -120.0);
    }

    #[test]
    fn a_trackpad_delta_reaches_the_wire_unchanged() {
        let (dx, dy) = collected_scroll(egui::MouseWheelUnit::Point, egui::Vec2::new(1.5, -9.0))
            .expect("휠 이벤트가 와이어 Scroll 로 수집돼야 한다");
        assert_eq!((dx, dy), (1.5, -9.0));
    }
}
