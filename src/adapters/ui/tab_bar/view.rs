//! Pane strip assembly, scrolling geometry and drag overlay.

use super::{PaneTabBarView, PaneTabBarsOutput, PaneTabBarsProps, TabBarAction};
use crate::adapters::ui::icons;
use egui::emath::GuiRounding as _;
use tasty_type_geometry::length::LogicalPx;

/// 순수 시각 view. AppState/CoreState/`theme::theme()` 비의존.
#[allow(clippy::cognitive_complexity)] // complexity-exempt: egui 즉시모드 draw — pane별 탭바 horizontal 클로저 나열이 구조적(clippy 가 클로저를 과대계상)
pub fn draw_pane_tab_bars_view(
    ctx: &egui::Context,
    props: &PaneTabBarsProps<'_>,
) -> PaneTabBarsOutput {
    let th = props.theme;
    let scale_factor = props.scale_factor;
    let mut output = PaneTabBarsOutput::default();

    let tab_w = props.tab_width;
    let label_font_size = props.tab_font_size;
    // 탭바는 host UI zoom을 적용하지 않는 tab_bar_* 토큰을 사용한다.
    let bar_h = th.tab_bar_height.value();
    let plus_w: f32 = 28.0;
    let icon_btn_w: f32 = 28.0;
    let icon_glyph: f32 = 14.0;
    let arrow_w: f32 = 20.0;
    let separator_w: f32 = 1.0;
    let plus_font_size = th.tab_bar_label_font_size.value();
    let arrow_font_size = th.tab_bar_arrow_font_size.value();

    let dimensions = StripDimensions {
        tab_width: LogicalPx(tab_w),
        plus_width: LogicalPx(plus_w),
        icon_button_width: LogicalPx(icon_btn_w),
        arrow_width: LogicalPx(arrow_w),
        separator_width: LogicalPx(separator_w),
    };

    for info in props.panes {
        let geometry = strip_geometry(info, scale_factor, &dimensions);
        let logical_x = geometry.x.value();
        let logical_y = geometry.y.value();
        let logical_w = geometry.width.value();
        let n = info.tab_names.len();
        let needs_scroll = geometry.needs_scroll;
        let viewport_w = geometry.viewport_width.value();
        let max_scroll = geometry.max_scroll.value();
        let mut scroll = info.scroll_offset.clamp(0.0, max_scroll);

        // 활성 탭이나 pane 크기·탭 수가 바뀌었을 때만 필요한 스크롤을 보정한다.
        // 매 프레임 보정하면 사용자가 옮긴 스크롤을 덮어쓰므로 이전 값을 Context에 보관한다.
        let scroll_track_id = egui::Id::new("tab_bar_active_scroll_track").with(info.pane_id);
        let prev_track: Option<(usize, f32, usize)> = ctx.data(|d| d.get_temp(scroll_track_id));
        let active_changed = match prev_track {
            Some((active, _, _)) => active != info.active_tab,
            None => true,
        };
        let geometry_changed = match prev_track {
            Some((_, w, count)) => (w - logical_w).abs() > 0.5 || count != n,
            None => false,
        };
        ctx.data_mut(|d| d.insert_temp(scroll_track_id, (info.active_tab, logical_w, n)));

        if n > 0 && (active_changed || geometry_changed) {
            let active_start = info.active_tab as f32 * (tab_w + separator_w);
            let active_end = active_start + tab_w;
            if active_start < scroll {
                scroll = active_start;
            } else if active_end > scroll + viewport_w {
                scroll = active_end - viewport_w;
            }
            scroll = scroll.clamp(0.0, max_scroll);
            if (scroll - info.scroll_offset).abs() > f32::EPSILON {
                output.actions.push(TabBarAction::AutoScrollToActiveTab {
                    pane_id: info.pane_id,
                    offset: scroll,
                });
            }
        }

        let area_response = egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(logical_x, logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let bg = if info.is_focused {
                    th.surface_raised()
                } else {
                    th.bg_sidebar()
                };

                egui::Frame::new()
                    .fill(bg.into())
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_min_width(logical_w);
                        ui.set_max_width(logical_w);
                        ui.set_min_height(bar_h);
                        ui.set_max_height(bar_h);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;

                            if needs_scroll {
                                let can_left = scroll > 0.0;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let arrow_color = if can_left {
                                    th.text_muted()
                                } else {
                                    th.text_disabled()
                                };
                                if resp.hovered() && can_left {
                                    ui.painter().rect_filled(
                                        r,
                                        0.0,
                                        th.overlay_hover().to_egui_premultiplied(),
                                    );
                                }
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "<",
                                    egui::FontId::proportional(arrow_font_size),
                                    arrow_color.into(),
                                );
                                if resp.clicked() && can_left {
                                    output.actions.push(TabBarAction::ScrollLeft {
                                        pane_id: info.pane_id,
                                    });
                                }
                            }

                            let clip_start_x = ui.cursor().min.x;
                            let clip_rect = egui::Rect::from_min_size(
                                egui::pos2(clip_start_x, ui.cursor().min.y),
                                egui::vec2(viewport_w, bar_h),
                            );
                            let (_, viewport_resp) = ui.allocate_exact_size(
                                egui::vec2(viewport_w, bar_h),
                                egui::Sense::click(),
                            );
                            if viewport_resp.secondary_clicked() {
                                output.actions.push(TabBarAction::OpenPaneContextMenu {
                                    pane_id: info.pane_id,
                                    pos: viewport_resp.interact_pointer_pos().unwrap_or_default(),
                                });
                                ui.painter().rect_stroke(
                                    clip_rect,
                                    0.0,
                                    egui::Stroke::new(
                                        th.focus_ring_width.value(),
                                        th.accent_success(),
                                    ),
                                    egui::StrokeKind::Inside,
                                );
                            } else if viewport_resp.clicked() {
                                // 빈 영역 클릭은 탭 전환 없이 pane 포커스만 바꾼다.
                                output.actions.push(TabBarAction::FocusPane {
                                    pane_id: info.pane_id,
                                });
                            }

                            let painter = ui.painter().with_clip_rect(clip_rect);
                            let mut x = clip_start_x - scroll;

                            let tab_context = super::tab::TabRenderContext {
                                props,
                                info,
                                painter: &painter,
                                clip_rect,
                                bg,
                                separator_w: LogicalPx(separator_w),
                            };
                            for i in 0..info.tab_names.len() {
                                x = super::tab::draw_tab(
                                    ui,
                                    &tab_context,
                                    &mut output,
                                    i,
                                    LogicalPx(x),
                                )
                                .value();
                            }

                            {
                                let sep = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(separator_w, bar_h),
                                );
                                painter.rect_filled(
                                    sep,
                                    0.0,
                                    th.tab_separator().to_egui_premultiplied(),
                                );
                                x += separator_w;
                            }

                            {
                                let plus_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(plus_w, bar_h),
                                );
                                let plus_clip = plus_rect.intersect(clip_rect);
                                if !plus_clip.is_negative() {
                                    let resp = ui.interact(
                                        plus_clip,
                                        egui::Id::new(format!("tab_plus_{}", info.pane_id)),
                                        egui::Sense::click(),
                                    );
                                    if resp.hovered() {
                                        painter.rect_filled(
                                            plus_rect,
                                            0.0,
                                            th.overlay_hover().to_egui_premultiplied(),
                                        );
                                    }
                                    painter.text(
                                        plus_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "+",
                                        egui::FontId::proportional(plus_font_size),
                                        th.text_muted().into(),
                                    );
                                    if resp.clicked() {
                                        output.actions.push(TabBarAction::AddTab {
                                            pane_id: info.pane_id,
                                        });
                                    }
                                    if resp.secondary_clicked() {
                                        output.actions.push(
                                            TabBarAction::OpenNewTabButtonContextMenu {
                                                pane_id: info.pane_id,
                                                pos: resp
                                                    .interact_pointer_pos()
                                                    .unwrap_or_default(),
                                            },
                                        );
                                        painter.rect_stroke(
                                            plus_clip,
                                            0.0,
                                            egui::Stroke::new(
                                                th.focus_ring_width.value(),
                                                th.accent_success(),
                                            ),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                }
                            }

                            if needs_scroll {
                                let can_right = scroll < max_scroll;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let arrow_color = if can_right {
                                    th.text_muted()
                                } else {
                                    th.text_disabled()
                                };
                                if resp.hovered() && can_right {
                                    ui.painter().rect_filled(
                                        r,
                                        0.0,
                                        th.overlay_hover().to_egui_premultiplied(),
                                    );
                                }
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    ">",
                                    egui::FontId::proportional(arrow_font_size),
                                    arrow_color.into(),
                                );
                                if resp.clicked() && can_right {
                                    output.actions.push(TabBarAction::ScrollRight {
                                        pane_id: info.pane_id,
                                    });
                                }
                            }

                            for (icon, is_split) in [(icons::SPLIT, true), (icons::SEARCH, false)] {
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(icon_btn_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let color = if resp.hovered() {
                                    th.text_primary()
                                } else {
                                    th.text_muted()
                                };
                                if resp.hovered() {
                                    ui.painter().rect_filled(
                                        r,
                                        0.0,
                                        th.overlay_hover().to_egui_premultiplied(),
                                    );
                                }
                                let icon_rect = egui::Rect::from_center_size(
                                    r.center(),
                                    egui::vec2(icon_glyph, icon_glyph),
                                );
                                icon.image(icon_glyph, color.into()).paint_at(ui, icon_rect);
                                if resp.clicked() {
                                    output.actions.push(if is_split {
                                        TabBarAction::RequestSplit {
                                            pane_id: info.pane_id,
                                        }
                                    } else {
                                        TabBarAction::OpenSearch {
                                            pane_id: info.pane_id,
                                        }
                                    });
                                }
                            }
                        });
                    });
            });

        if output.measured_height_physical.is_none() {
            let logical_h = LogicalPx(area_response.response.rect.height());
            output.measured_height_physical = Some(logical_h.to_physical(scale_factor));
        }
    }

    if let Some(ref drag) = props.drag
        && let Some(pane_info) = props.panes.iter().find(|i| i.pane_id == drag.pane_id)
    {
        let geometry = strip_geometry(pane_info, scale_factor, &dimensions);
        let pane_logical_x = geometry.x.value();
        let pane_logical_y = geometry.y.value();
        let pane_logical_w = geometry.width.value();
        let viewport_start = geometry.viewport_start.value();

        let drop_idx = compute_drop_index(
            drag.current_x,
            pane_logical_x,
            pane_info.scroll_offset,
            pane_info.tab_names.len(),
            tab_w,
            separator_w,
            pane_logical_w,
        );

        let marker_x =
            viewport_start - pane_info.scroll_offset + drop_idx as f32 * (tab_w + separator_w);
        let marker_rect = egui::Rect::from_min_size(
            egui::pos2(marker_x - 1.0, pane_logical_y),
            egui::vec2(2.0, bar_h),
        );
        let overlay_painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("tab_drag_overlay"),
        ));
        overlay_painter.rect_filled(marker_rect, 0.0, th.accent_primary());

        let ghost_name = pane_info
            .tab_names
            .get(drag.tab_index)
            .cloned()
            .unwrap_or_default();
        let ghost_rect = egui::Rect::from_min_size(
            egui::pos2(drag.current_x - tab_w / 2.0, pane_logical_y),
            egui::vec2(tab_w, bar_h),
        );
        // 드래그 중 따라다니는 고스트는 반투명이다. 대응 토큰 없음.
        const DRAG_GHOST_ALPHA: u8 = 180;
        let ghost_bg = th.bg_panel().with_alpha(DRAG_GHOST_ALPHA).to_egui();
        let ghost_fg = th.text_primary().with_alpha(DRAG_GHOST_ALPHA).to_egui();
        overlay_painter.rect_filled(ghost_rect, 0.0, ghost_bg);
        overlay_painter.text(
            ghost_rect.center(),
            egui::Align2::CENTER_CENTER,
            &ghost_name,
            egui::FontId::proportional(label_font_size),
            ghost_fg,
        );
    }

    output
}

/// Dimensions shared by strip assembly and overlay geometry.
struct StripDimensions {
    tab_width: LogicalPx,
    plus_width: LogicalPx,
    icon_button_width: LogicalPx,
    arrow_width: LogicalPx,
    separator_width: LogicalPx,
}

/// Shared pane strip geometry for normal painting and the drag overlay.
struct StripGeometry {
    x: LogicalPx,
    y: LogicalPx,
    width: LogicalPx,
    viewport_start: LogicalPx,
    viewport_width: LogicalPx,
    max_scroll: LogicalPx,
    needs_scroll: bool,
}

fn strip_geometry(
    info: &PaneTabBarView,
    scale_factor: f32,
    dimensions: &StripDimensions,
) -> StripGeometry {
    let x = info.rect.x.to_logical(scale_factor).value().round_ui();
    let y = info.rect.y.to_logical(scale_factor).value().round_ui();
    let width = info.rect.width.to_logical(scale_factor).value().round_ui();
    let n = info.tab_names.len();
    let content_w = n as f32 * dimensions.tab_width.value()
        + (n.max(1) - 1) as f32 * dimensions.separator_width.value()
        + dimensions.separator_width.value()
        + dimensions.plus_width.value();
    let avail_w = (width - dimensions.icon_button_width.value() * 2.0).max(0.0);
    let needs_scroll = content_w > avail_w;
    let viewport_w = if needs_scroll {
        (avail_w - dimensions.arrow_width.value() * 2.0).max(0.0)
    } else {
        avail_w
    };
    StripGeometry {
        x: LogicalPx(x),
        y: LogicalPx(y),
        width: LogicalPx(width),
        viewport_start: LogicalPx(if needs_scroll {
            x + dimensions.arrow_width.value()
        } else {
            x
        }),
        viewport_width: LogicalPx(viewport_w),
        max_scroll: LogicalPx((content_w - viewport_w).max(0.0)),
        needs_scroll,
    }
}

/// Mouse x → drop target tab index. Pure 함수 — view/wrapper 양쪽에서 호출.
pub fn compute_drop_index(
    mouse_x: f32,
    pane_logical_x: f32,
    scroll_offset: f32,
    tab_count: usize,
    tab_w: f32,
    separator_w: f32,
    _pane_w: f32,
) -> usize {
    let content_x = mouse_x - pane_logical_x + scroll_offset;
    let slot = content_x / (tab_w + separator_w);
    slot.round()
        .clamp(0.0, (tab_count.saturating_sub(1)) as f32) as usize
}
