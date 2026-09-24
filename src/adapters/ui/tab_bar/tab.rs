//! Painting and interaction for one tab slot.

use super::{PaneTabBarView, PaneTabBarsOutput, PaneTabBarsProps, TabBarAction};
use crate::adapters::ui::{icons, zoomed_px};
use crate::core::AttentionKind;
use tasty_type_geometry::length::LogicalPx;

/// 점 형태의 활성 표시 지름. 대응 역할 토큰이 없어 별도로 두며 밑줄 두께와 구분한다.
const TAB_ACTIVE_DOT_SIZE: LogicalPx = LogicalPx(4.0);

/// busy 점과 탭 이름 사이 간격. 점 지름과는 별도 치수다.
const TAB_BUSY_DOT_PAD: LogicalPx = LogicalPx(6.0);

/// Inputs shared by the tab slots in one clipped pane strip.
pub(super) struct TabRenderContext<'a, 'props> {
    pub props: &'a PaneTabBarsProps<'props>,
    pub info: &'a PaneTabBarView,
    pub painter: &'a egui::Painter,
    pub clip_rect: egui::Rect,
    pub bg: tasty_type_appearance::color::HexColor,
    pub separator_w: LogicalPx,
}

pub(super) fn draw_tab(
    ui: &mut egui::Ui,
    context: &TabRenderContext<'_, '_>,
    output: &mut PaneTabBarsOutput,
    i: usize,
    start_x: LogicalPx,
) -> LogicalPx {
    let TabRenderContext {
        props,
        info,
        painter,
        clip_rect,
        bg,
        separator_w,
    } = *context;
    let th = props.theme;
    let name = &info.tab_names[i];
    let mut x = start_x.value();
    let separator_w = separator_w.value();
    let tab_w = props.tab_width;
    let bar_h = th.tab_bar_height.value();
    let label_font_size = props.tab_font_size;
    let h_padding: f32 = 8.0;
    let active_indicator_h = th.tab_indicator_width.value();
    // 점·여백은 같은 탭의 다른 요소와 배율을 맞춘다.
    let dot_radius = th.tab_dot_size().scaled(0.5);
    let dot_reserve = (th.tab_dot_size() + zoomed_px(th, TAB_BUSY_DOT_PAD)).value();
    if i > 0 {
        let sep = egui::Rect::from_min_size(
            egui::pos2(x, clip_rect.min.y),
            egui::vec2(separator_w, bar_h),
        );
        // separator는 알파가 이미 곱해진 색이므로 premultiplied로 읽는다.
        painter.rect_filled(sep, 0.0, th.tab_separator().to_egui_premultiplied());
        x += separator_w;
    }

    let is_active = i == info.active_tab;
    let tab_kind = info.tab_attention_kind.get(i).copied().flatten();
    let is_busy = info.tab_is_busy.get(i).copied().unwrap_or(false);
    // Fill 스타일만 활성 탭 배경을 채운다. Underline/Dot 은
    // 배경을 비활성과 동일하게 두고 별도 마커로 표시.
    let tab_bg =
        if is_active && props.active_tab_indicator == crate::settings::ActiveTabIndicator::Fill {
            th.bg_panel()
        } else {
            bg
        };
    // 제목 색 우선순위: NeedsInput > Completion > 활성 탭 > 기본.
    let text_color = match tab_kind {
        Some(AttentionKind::NeedsInput) => th.accent_warning(),
        Some(AttentionKind::Completion) => th.accent_primary(),
        None if is_active => th.text_primary(),
        None => th.text_muted(),
    };

    let tab_rect =
        egui::Rect::from_min_size(egui::pos2(x, clip_rect.min.y), egui::vec2(tab_w, bar_h));

    painter.rect_filled(tab_rect, 0.0, tab_bg);

    if is_active {
        use crate::settings::ActiveTabIndicator;
        match props.active_tab_indicator {
            ActiveTabIndicator::Underline => {
                let line_rect = egui::Rect::from_min_size(
                    egui::pos2(tab_rect.min.x, tab_rect.min.y),
                    egui::vec2(tab_w, active_indicator_h),
                );
                painter.rect_filled(line_rect, 0.0, th.accent_primary());
            }
            ActiveTabIndicator::Fill => {}
            ActiveTabIndicator::Dot => {
                let r = zoomed_px(th, TAB_ACTIVE_DOT_SIZE).value() * 0.5;
                let center = egui::pos2(tab_rect.center().x, tab_rect.min.y + r * 2.0);
                painter.circle_filled(center, r, th.accent_primary());
            }
        }
    }

    // close 버튼 슬롯(우측 h_padding + 14px)을 비워두고 dot 은
    // 그 왼쪽에 둔다 (close 와 겹치지 않게).
    let dot_right = tab_rect.max.x - h_padding - 14.0;
    if is_busy {
        let dot_center = egui::pos2(dot_right - dot_radius.value(), tab_rect.center().y);
        let color: egui::Color32 = th.accent_success().into();
        painter.circle_filled(dot_center, dot_radius.value(), color);
    }

    let icon_size = 14.0;
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(
            tab_rect.min.x + h_padding,
            tab_rect.center().y - icon_size / 2.0,
        ),
        egui::vec2(icon_size, icon_size),
    );
    // 포커스된 pane에서 슬롯 키가 있는 탭만 아이콘 대신 키캡을 표시한다. 슬롯 폭은 유지한다.
    let switch_digit = crate::adapters::ui::switch_overlay::tab_keycap_for(
        props.kb,
        props.switch_overlay_pane,
        info.pane_id,
        i,
    );
    // 키캡이 없는 프레임에도 페이드 상태를 갱신한다.
    let overlay_active = props.switch_overlay_pane == Some(info.pane_id);
    let fade = crate::adapters::ui::switch_overlay::appear_fade(
        ui.ctx(),
        th,
        info.pane_id,
        overlay_active,
    );
    if let Some(digit) = switch_digit {
        crate::adapters::ui::switch_overlay::paint_keycap(
            painter,
            th,
            icon_rect.center(),
            digit,
            is_active,
            fade,
        );
    } else {
        let icon = info.tab_icons.get(i).copied().unwrap_or(icons::FILE);
        // 아이콘이 화살표·버튼 위로 넘치지 않게 그리는 동안 clip을 좁힌다.
        let prev_clip = ui.clip_rect();
        ui.set_clip_rect(clip_rect.intersect(prev_clip));
        icon.image(icon_size, text_color.into())
            .paint_at(ui, icon_rect);
        ui.set_clip_rect(prev_clip);
    }

    let text_x = icon_rect.max.x + 6.0;
    let mut text_right = dot_right - 4.0;
    if is_busy {
        text_right -= dot_reserve;
    }
    let available_w = (text_right - text_x).max(0.0);
    let font_id = egui::FontId::proportional(label_font_size);
    let final_galley = layout_tab_label(painter, name, font_id, text_color, LogicalPx(available_w));
    let text_y = tab_rect.center().y - final_galley.size().y / 2.0;
    painter.galley(egui::pos2(text_x, text_y), final_galley, text_color.into());

    let tab_clip = tab_rect.intersect(clip_rect);
    if !tab_clip.is_negative() {
        let resp = ui.interact(
            tab_clip,
            egui::Id::new(format!("tab_{}_{}", info.pane_id, i)),
            egui::Sense::click_and_drag(),
        );
        // close 버튼 (active or hover) — 우측 끝. 클릭은
        // SwitchTab 보다 우선.
        let show_close = is_active || resp.hovered();
        let close_clicked = if show_close {
            let cs = 14.0;
            let close_rect = egui::Rect::from_center_size(
                egui::pos2(tab_rect.max.x - h_padding - cs / 2.0, tab_rect.center().y),
                egui::vec2(cs, cs),
            );
            let cr = ui.interact(
                close_rect,
                egui::Id::new(("tabclose", info.pane_id, i)),
                egui::Sense::click(),
            );
            if cr.hovered() {
                painter.rect_filled(close_rect, 2.0, th.active_overlay.to_egui_premultiplied());
            }
            let cc: egui::Color32 = if cr.hovered() {
                th.text_primary().into()
            } else {
                th.text_muted().into()
            };
            // 닫기 아이콘도 뷰포트 안에서만 그린다.
            let prev_clip = ui.clip_rect();
            ui.set_clip_rect(clip_rect.intersect(prev_clip));
            icons::CLOSE.image(cs, cc).paint_at(ui, close_rect);
            ui.set_clip_rect(prev_clip);
            cr.clicked()
        } else {
            false
        };
        if close_clicked {
            output.actions.push(TabBarAction::CloseTab {
                pane_id: info.pane_id,
                tab_index: i,
            });
        } else if resp.clicked() {
            output.actions.push(TabBarAction::SwitchTab {
                pane_id: info.pane_id,
                tab_index: i,
            });
        }
        if resp.secondary_clicked() {
            output.actions.push(TabBarAction::OpenContextMenu {
                pane_id: info.pane_id,
                tab_index: i,
                pos: resp.interact_pointer_pos().unwrap_or_default(),
            });
            painter.rect_stroke(
                tab_clip,
                0.0,
                egui::Stroke::new(th.focus_ring_width.value(), th.accent_success()),
                egui::StrokeKind::Inside,
            );
        }
        if resp.drag_started_by(egui::PointerButton::Primary) {
            output.actions.push(TabBarAction::DragStart {
                pane_id: info.pane_id,
                tab_index: i,
            });
        }
        if resp.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = resp.interact_pointer_pos()
        {
            output.actions.push(TabBarAction::DragUpdate {
                pane_id: info.pane_id,
                mouse_x: pos.x,
            });
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            output.actions.push(TabBarAction::DragEnd {
                pane_id: info.pane_id,
            });
        }
    }

    x += tab_w;
    LogicalPx(x)
}

/// Fit the label into the space left by the leading icon, busy dot and close slot.
fn layout_tab_label(
    painter: &egui::Painter,
    name: &str,
    font_id: egui::FontId,
    text_color: tasty_type_appearance::color::HexColor,
    available_width: LogicalPx,
) -> std::sync::Arc<egui::Galley> {
    let available_w = available_width.value();
    let galley = painter.layout_no_wrap(name.to_owned(), font_id.clone(), text_color.into());
    if galley.size().x > available_w {
        let mut truncated = name.to_owned();
        loop {
            truncated.pop();
            let candidate = format!("{truncated}…");
            let g = painter.layout_no_wrap(candidate.clone(), font_id.clone(), text_color.into());
            if g.size().x <= available_w || truncated.is_empty() {
                break g;
            }
        }
    } else {
        galley
    }
}
