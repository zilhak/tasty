//! Pane tab bars — pane 별 상단 탭 표시 + 사용자 입력 (focus / drag / context menu / 새 탭).
//!
//! ## Split: wrapper / view / action
//!
//! 순수 시각 `draw_pane_tab_bars_view` 는 [`PaneTabBarsProps`] 만 받고
//! [`PaneTabBarsOutput`] (collected actions + measured height) 만 반환한다.
//! AppState/CoreState/`theme::theme()` 비의존. Gallery (`tasty-gallery`) 는
//! view 를 mock props 로 mirror 해서 시각 검증.
//!
//! wrapper `draw_pane_tab_bars` 는 (a) state/engine 에서 props 추출,
//! (b) view 호출, (c) 반환된 [`TabBarAction`] 리스트를 state mutation 으로 변환,
//! (d) measured height 를 `state.tab_bar_height` 에 기록.

use egui::emath::GuiRounding as _;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::PhysicalPx;
use tasty_type_geometry::rect::PhysicalRect;

use crate::adapters::ui::icons;
use crate::state::AppState;
use crate::theme;

/// surface kind → 탭 leading 아이콘 (ui_kit tab strip).
fn kind_icon(kind: &str) -> icons::Icon {
    match kind {
        "markdown" => icons::MD,
        "explorer" => icons::FOLDER,
        "image" => icons::IMAGE,
        "terminal" | "attached" => icons::TERM,
        _ => icons::FILE,
    }
}

/// View 입력 — pane 한 개 분의 탭 데이터.
#[derive(Clone, Debug)]
pub struct PaneTabBarView {
    pub pane_id: u32,
    /// Pane 의 *물리* 좌표 사각형 (view 가 scale_factor 로 logical 변환).
    pub rect: PhysicalRect,
    pub tab_names: Vec<String>,
    /// 탭별 surface kind ("terminal"/"markdown"/...) — leading 아이콘 결정.
    pub tab_kinds: Vec<&'static str>,
    /// 탭별 알림(노란 라벨) 여부.
    pub tab_has_notification: Vec<bool>,
    /// 탭별 busy(녹색 점) 여부.
    pub tab_is_busy: Vec<bool>,
    /// 탭별 agent(IPC/CLI) 생성 여부 — mauve dot 으로 표시.
    pub tab_is_agent_created: Vec<bool>,
    pub active_tab: usize,
    /// 이 pane 이 현재 focus 인지 — 배경 (surface0 vs mantle) 결정.
    pub is_focused: bool,
    /// 가로 스크롤 오프셋 (logical px).
    pub scroll_offset: f32,
}

/// View 입력 — drag 진행 중인 탭의 상태. None 이면 drag overlay 미표시.
#[derive(Clone, Debug)]
pub struct TabDragView {
    pub pane_id: u32,
    pub tab_index: usize,
    /// 현재 마우스 x (logical pane 좌표).
    pub current_x: f32,
}

/// View 입력 — 전체 pane 의 탭 바 + drag 상태 + appearance 옵션.
pub struct PaneTabBarsProps<'a> {
    pub theme: &'a Theme,
    pub panes: &'a [PaneTabBarView],
    pub scale_factor: f32,
    /// 사용자 옵션 — 탭 1 개의 가로 너비 (logical px).
    pub tab_width: f32,
    /// 사용자 옵션 — 탭 라벨 폰트 크기 (logical px).
    pub tab_font_size: f32,
    /// 현재 drag 진행 상태 (None 이면 overlay 미표시).
    pub drag: Option<TabDragView>,
}

/// View 가 발생시킨 사용자 의도. wrapper 가 state/engine 으로 반영.
#[derive(Clone, Debug, PartialEq)]
pub enum TabBarAction {
    SwitchTab {
        pane_id: u32,
        tab_index: usize,
    },
    AddTab {
        pane_id: u32,
    },
    ScrollLeft {
        pane_id: u32,
    },
    ScrollRight {
        pane_id: u32,
    },
    OpenContextMenu {
        pane_id: u32,
        tab_index: usize,
        pos: egui::Pos2,
    },
    OpenPaneContextMenu {
        pane_id: u32,
        pos: egui::Pos2,
    },
    /// 탭 "+" 버튼 우클릭 — 프리셋으로 탭/페인 생성 진입점.
    OpenNewTabButtonContextMenu {
        pane_id: u32,
        pos: egui::Pos2,
    },
    DragStart {
        pane_id: u32,
        tab_index: usize,
    },
    DragUpdate {
        pane_id: u32,
        mouse_x: f32,
    },
    DragEnd {
        pane_id: u32,
    },
}

/// View 의 출력 — 사용자 의도 리스트 + 측정된 탭 바 높이.
#[derive(Default)]
pub struct PaneTabBarsOutput {
    pub actions: Vec<TabBarAction>,
    /// 첫 pane 의 탭 바 logical 높이 × scale_factor (physical px). 측정 못 했으면 None.
    pub measured_height_physical: Option<f32>,
}

/// 순수 시각 view. AppState/CoreState/`theme::theme()` 비의존.
pub fn draw_pane_tab_bars_view(
    ctx: &egui::Context,
    props: &PaneTabBarsProps<'_>,
) -> PaneTabBarsOutput {
    let th = props.theme;
    let scale_factor = props.scale_factor;
    let mut output = PaneTabBarsOutput::default();

    let tab_w = props.tab_width;
    let label_font_size = props.tab_font_size;
    let bar_h = th.item_height_tab.value();
    let plus_w: f32 = 28.0;
    let arrow_w: f32 = 20.0;
    let separator_w: f32 = 1.0;
    let h_padding: f32 = 8.0;
    let dot_radius: f32 = 3.0;
    let dot_pad: f32 = 6.0;
    let active_indicator_h: f32 = 2.0;
    let plus_font_size = th.font_size_body.value();
    let arrow_font_size = th.font_size_caption.value();

    for info in props.panes {
        let logical_x = (info.rect.x.value() / scale_factor).round_ui();
        let logical_y = (info.rect.y.value() / scale_factor).round_ui();
        let logical_w = (info.rect.width.value() / scale_factor).round_ui();
        let n = info.tab_names.len();
        let content_w =
            n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
        let needs_scroll = content_w > logical_w;
        let viewport_w = if needs_scroll {
            (logical_w - arrow_w * 2.0).max(0.0)
        } else {
            logical_w.max(0.0)
        };
        let max_scroll = (content_w - viewport_w).max(0.0);
        let scroll = info.scroll_offset.clamp(0.0, max_scroll);

        let area_response = egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(logical_x, logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let bg = if info.is_focused {
                    th.surface0
                } else {
                    th.mantle
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

                            // Left arrow
                            if needs_scroll {
                                let can_left = scroll > 0.0;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let arrow_color = if can_left { th.subtext0 } else { th.surface1 };
                                if resp.hovered() && can_left {
                                    ui.painter().rect_filled(r, 0.0, th.surface0);
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

                            // Clipped tab area
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
                                    egui::Stroke::new(2.0, th.green),
                                    egui::StrokeKind::Inside,
                                );
                            }

                            let painter = ui.painter().with_clip_rect(clip_rect);
                            let mut x = clip_start_x - scroll;

                            for (i, name) in info.tab_names.iter().enumerate() {
                                if i > 0 {
                                    let sep = egui::Rect::from_min_size(
                                        egui::pos2(x, clip_rect.min.y),
                                        egui::vec2(separator_w, bar_h),
                                    );
                                    painter.rect_filled(sep, 0.0, th.surface1);
                                    x += separator_w;
                                }

                                let is_active = i == info.active_tab;
                                let has_notif =
                                    info.tab_has_notification.get(i).copied().unwrap_or(false);
                                let is_busy = info.tab_is_busy.get(i).copied().unwrap_or(false);
                                let tab_bg = if is_active { th.base } else { bg };
                                let text_color = if is_active {
                                    th.text
                                } else if has_notif {
                                    th.yellow
                                } else {
                                    th.subtext0
                                };

                                let tab_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(tab_w, bar_h),
                                );

                                painter.rect_filled(tab_rect, 0.0, tab_bg);

                                if is_active {
                                    let line_rect = egui::Rect::from_min_size(
                                        egui::pos2(tab_rect.min.x, tab_rect.min.y),
                                        egui::vec2(tab_w, active_indicator_h),
                                    );
                                    painter.rect_filled(line_rect, 0.0, th.blue);
                                }

                                if is_busy {
                                    let dot_center = egui::pos2(
                                        tab_rect.max.x - dot_pad - dot_radius,
                                        tab_rect.center().y,
                                    );
                                    let dot_color: egui::Color32 = if is_active {
                                        th.green.into()
                                    } else {
                                        th.green.with_alpha(180).to_egui()
                                    };
                                    painter.circle_filled(dot_center, dot_radius, dot_color);
                                }

                                // agent(IPC/CLI) 생성 surface → mauve dot.
                                // busy(녹색) dot 과 겹치지 않게, busy 있으면 그 왼쪽 슬롯에.
                                let is_agent_created = info
                                    .tab_is_agent_created
                                    .get(i)
                                    .copied()
                                    .unwrap_or(false);
                                if is_agent_created {
                                    let base_x = tab_rect.max.x - dot_pad - dot_radius;
                                    let agent_x = if is_busy {
                                        base_x - dot_radius * 2.0 - dot_pad
                                    } else {
                                        base_x
                                    };
                                    let dot_color: egui::Color32 = if is_active {
                                        th.mauve.into()
                                    } else {
                                        th.mauve.with_alpha(180).to_egui()
                                    };
                                    painter.circle_filled(
                                        egui::pos2(agent_x, tab_rect.center().y),
                                        dot_radius,
                                        dot_color,
                                    );
                                }

                                // kind 아이콘 (leading) — ui_kit tab strip.
                                let icon_size = 14.0;
                                let icon_rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        tab_rect.min.x + h_padding,
                                        tab_rect.center().y - icon_size / 2.0,
                                    ),
                                    egui::vec2(icon_size, icon_size),
                                );
                                let kind = info.tab_kinds.get(i).copied().unwrap_or("terminal");
                                kind_icon(kind)
                                    .image(icon_size, text_color.into())
                                    .paint_at(ui, icon_rect);

                                // 텍스트 — 아이콘 뒤, 좌측 정렬. 우측엔 dot 공간 확보.
                                let text_x = icon_rect.max.x + 6.0;
                                let right_reserve = if is_busy || is_agent_created {
                                    dot_pad + dot_radius * 2.0 + 4.0
                                } else {
                                    h_padding
                                };
                                let available_w = (tab_rect.max.x - right_reserve - text_x).max(0.0);
                                let font_id = egui::FontId::proportional(label_font_size);
                                let galley = painter.layout_no_wrap(
                                    name.clone(),
                                    font_id.clone(),
                                    text_color.into(),
                                );
                                let final_galley = if galley.size().x > available_w {
                                    let mut truncated = name.clone();
                                    loop {
                                        truncated.pop();
                                        let candidate = format!("{truncated}…");
                                        let g = painter.layout_no_wrap(
                                            candidate.clone(),
                                            font_id.clone(),
                                            text_color.into(),
                                        );
                                        if g.size().x <= available_w || truncated.is_empty() {
                                            break g;
                                        }
                                    }
                                } else {
                                    galley
                                };
                                let text_y = tab_rect.center().y - final_galley.size().y / 2.0;
                                painter.galley(
                                    egui::pos2(text_x, text_y),
                                    final_galley,
                                    text_color.into(),
                                );

                                let tab_clip = tab_rect.intersect(clip_rect);
                                if !tab_clip.is_negative() {
                                    let resp = ui.interact(
                                        tab_clip,
                                        egui::Id::new(format!("tab_{}_{}", info.pane_id, i)),
                                        egui::Sense::click_and_drag(),
                                    );
                                    if resp.clicked() {
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
                                            egui::Stroke::new(2.0, th.green),
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
                            }

                            // Separator before "+"
                            {
                                let sep = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(separator_w, bar_h),
                                );
                                painter.rect_filled(sep, 0.0, th.surface1);
                                x += separator_w;
                            }

                            // "+" button
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
                                        painter.rect_filled(plus_rect, 0.0, th.surface0);
                                    }
                                    painter.text(
                                        plus_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "+",
                                        egui::FontId::proportional(plus_font_size),
                                        th.subtext0.into(),
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
                                            egui::Stroke::new(2.0, th.green),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                }
                            }

                            // Right arrow
                            if needs_scroll {
                                let can_right = scroll < max_scroll;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let arrow_color = if can_right { th.subtext0 } else { th.surface1 };
                                if resp.hovered() && can_right {
                                    ui.painter().rect_filled(r, 0.0, th.surface0);
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
                        });
                    });
            });

        if output.measured_height_physical.is_none() {
            let logical_h = area_response.response.rect.height();
            output.measured_height_physical = Some(logical_h * scale_factor);
        }
    }

    // Drag overlay (ghost tab + insert marker)
    if let Some(ref drag) = props.drag
        && let Some(pane_info) = props.panes.iter().find(|i| i.pane_id == drag.pane_id)
    {
        let pane_logical_x = (pane_info.rect.x.value() / scale_factor).round_ui();
        let pane_logical_y = (pane_info.rect.y.value() / scale_factor).round_ui();
        let pane_logical_w = (pane_info.rect.width.value() / scale_factor).round_ui();
        let n = pane_info.tab_names.len();
        let content_w =
            n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
        let needs_scroll_arrows = content_w > pane_logical_w;
        let viewport_start = if needs_scroll_arrows {
            pane_logical_x + arrow_w
        } else {
            pane_logical_x
        };

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
        overlay_painter.rect_filled(marker_rect, 0.0, th.blue);

        let ghost_name = pane_info
            .tab_names
            .get(drag.tab_index)
            .cloned()
            .unwrap_or_default();
        let ghost_rect = egui::Rect::from_min_size(
            egui::pos2(drag.current_x - tab_w / 2.0, pane_logical_y),
            egui::vec2(tab_w, bar_h),
        );
        let ghost_bg = th.base.with_alpha(180).to_egui();
        let ghost_fg = th.text.with_alpha(180).to_egui();
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

/// Wrapper — state/engine 에서 props 추출 → view 호출 → action 적용.
///
/// 시그니처는 기존과 동일 (외부 호출처 무영향).
pub fn draw_pane_tab_bars(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    let th = theme::theme();
    let focused_pane_id = state.focused_pane_id(engine);

    let mut panes: Vec<PaneTabBarView> = Vec::new();
    {
        let ws = state.active_workspace(engine);
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let tab_has_notification: Vec<bool> = pane
                .tabs
                .iter()
                .map(|t| {
                    let sids = t.all_surface_ids();
                    engine.notifications.has_highlighted_surface(&sids)
                })
                .collect();
            let tab_is_busy: Vec<bool> = pane
                .tabs
                .iter()
                .map(|t| {
                    let sids = t.all_surface_ids();
                    sids.iter().any(|sid| engine.busy_surfaces.contains(sid))
                })
                .collect();
            // created_by=agent 메타는 영속(memory.db). pane 당 lock 1 회로 묶어 조회.
            let tab_is_agent_created: Vec<bool> = state.with_memory(|m| {
                pane.tabs
                    .iter()
                    .map(|t| {
                        t.all_surface_ids().iter().any(|&sid| {
                            crate::surface_meta::SurfaceMetaStore::get(
                                m,
                                sid,
                                crate::surface_meta::META_CREATED_BY,
                            )
                            .as_deref()
                                == Some(crate::surface_meta::CREATED_BY_AGENT)
                        })
                    })
                    .collect()
            });
            panes.push(PaneTabBarView {
                pane_id,
                rect: pane_rect,
                tab_names: pane.tabs.iter().map(|t| t.display_name()).collect(),
                tab_kinds: pane
                    .tabs
                    .iter()
                    .map(|t| {
                        engine
                            .find_surface_by_id(t.focused_surface)
                            .map(|s| s.kind())
                            .unwrap_or("terminal")
                    })
                    .collect(),
                tab_has_notification,
                tab_is_busy,
                tab_is_agent_created,
                active_tab: pane.active_tab,
                is_focused: pane_id == focused_pane_id,
                scroll_offset: pane.tab_scroll_offset,
            });
        }
    }

    let appearance = &engine.settings.appearance;
    let tab_w = appearance.tab_width;
    let tab_font_size = appearance.tab_font_size;

    let drag = state.dialogs.tab_drag.as_ref().map(|d| TabDragView {
        pane_id: d.pane_id,
        tab_index: d.tab_index,
        current_x: d.current_x,
    });

    let props = PaneTabBarsProps {
        theme: &th,
        panes: &panes,
        scale_factor,
        tab_width: tab_w,
        tab_font_size,
        drag,
    };

    let output = draw_pane_tab_bars_view(ctx, &props);

    if let Some(h_phys) = output.measured_height_physical {
        state.tab_bar_height = PhysicalPx(h_phys);
    }

    let separator_w: f32 = 1.0;

    for action in output.actions {
        match action {
            TabBarAction::SwitchTab { pane_id, tab_index } => {
                let mut to_wake: Vec<u32> = Vec::new();
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.active_tab = tab_index;
                    if let Some(tab) = pane.tabs.get(tab_index) {
                        to_wake = tab.deferred_surface_ids();
                    }
                }
                for sid in to_wake {
                    engine.ensure_surface_initialized(sid);
                }
            }
            TabBarAction::AddTab { pane_id } => {
                state.active_workspace_mut(engine).focused_pane = pane_id;
                if let Err(e) = state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
            }
            TabBarAction::ScrollLeft { pane_id } => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset = (pane.tab_scroll_offset - tab_w).max(0.0);
                }
            }
            TabBarAction::ScrollRight { pane_id } => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset += tab_w;
                }
            }
            TabBarAction::OpenContextMenu {
                pane_id,
                tab_index,
                pos,
            } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Tab {
                    pane_id,
                    tab_index,
                    x: pos.x,
                    y: pos.y,
                });
            }
            TabBarAction::OpenPaneContextMenu { pane_id, pos } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Pane {
                    pane_id,
                    x: pos.x,
                    y: pos.y,
                });
            }
            TabBarAction::OpenNewTabButtonContextMenu { pane_id, pos } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::NewTabButton {
                        pane_id,
                        x: pos.x,
                        y: pos.y,
                    });
            }
            TabBarAction::DragStart { pane_id, tab_index } => {
                state.dialogs.tab_drag = Some(crate::state::TabDragState {
                    pane_id,
                    tab_index,
                    current_x: 0.0,
                });
            }
            TabBarAction::DragUpdate { pane_id, mouse_x } => {
                if let Some(ref mut drag) = state.dialogs.tab_drag
                    && drag.pane_id == pane_id
                {
                    drag.current_x = mouse_x;
                }
            }
            TabBarAction::DragEnd { pane_id } => {
                if let Some(drag) = state.dialogs.tab_drag.take()
                    && drag.pane_id == pane_id
                    && let Some(pane_info) = panes.iter().find(|i| i.pane_id == pane_id)
                {
                    let pane_logical_x = (pane_info.rect.x.value() / scale_factor).round_ui();
                    let pane_logical_w = (pane_info.rect.width.value() / scale_factor).round_ui();
                    let target = compute_drop_index(
                        drag.current_x,
                        pane_logical_x,
                        pane_info.scroll_offset,
                        pane_info.tab_names.len(),
                        tab_w,
                        separator_w,
                        pane_logical_w,
                    );
                    if target != drag.tab_index
                        && let Some(pane) = state
                            .active_workspace_mut(engine)
                            .pane_layout_mut()
                            .find_pane_mut(pane_id)
                    {
                        pane.move_tab(drag.tab_index, target);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_drop_index_first_slot() {
        let idx = compute_drop_index(100.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 0);
    }

    #[test]
    fn compute_drop_index_middle_slot() {
        // mouse_x=281, pane_x=100 → content_x=181 → slot=181/121 ≈ 1.496 → round 1
        let idx = compute_drop_index(281.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 1);
    }

    #[test]
    fn compute_drop_index_last_slot_clamped() {
        let idx = compute_drop_index(10_000.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 2);
    }

    #[test]
    fn compute_drop_index_accounts_for_scroll() {
        let idx0 = compute_drop_index(100.0, 100.0, 0.0, 5, 120.0, 1.0, 400.0);
        let idx_scroll = compute_drop_index(100.0, 100.0, 121.0, 5, 120.0, 1.0, 400.0);
        assert_eq!(idx0, 0);
        assert_eq!(idx_scroll, 1);
    }

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_view(panes: Vec<PaneTabBarView>, drag: Option<TabDragView>) -> PaneTabBarsOutput {
        let ctx = egui::Context::default();
        let theme = test_theme();
        let mut out = PaneTabBarsOutput::default();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            let props = PaneTabBarsProps {
                theme: &theme,
                panes: &panes,
                scale_factor: 1.0,
                tab_width: 160.0,
                tab_font_size: 12.0,
                drag: drag.clone(),
            };
            out = draw_pane_tab_bars_view(ctx, &props);
        }));
        out
    }

    fn mk_pane(pane_id: u32, names: &[&str], active: usize, focused: bool) -> PaneTabBarView {
        let n = names.len();
        PaneTabBarView {
            pane_id,
            rect: PhysicalRect {
                x: PhysicalPx(0.0),
                y: PhysicalPx(0.0),
                width: PhysicalPx(800.0),
                height: PhysicalPx(600.0),
            },
            tab_names: names.iter().map(|s| s.to_string()).collect(),
            tab_kinds: vec!["terminal"; n],
            tab_has_notification: vec![false; n],
            tab_is_busy: vec![false; n],
            tab_is_agent_created: vec![false; n],
            active_tab: active,
            is_focused: focused,
            scroll_offset: 0.0,
        }
    }

    #[test]
    fn view_idle_emits_no_actions() {
        let panes = vec![mk_pane(1, &["A", "B"], 0, true)];
        let out = run_view(panes, None);
        assert!(out.actions.is_empty());
        assert!(out.measured_height_physical.is_some());
    }

    #[test]
    fn view_measures_bar_height_for_first_pane() {
        let panes = vec![
            mk_pane(1, &["A"], 0, true),
            mk_pane(2, &["X", "Y"], 0, false),
        ];
        let out = run_view(panes, None);
        assert!(out.measured_height_physical.unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn view_empty_panes_returns_default_output() {
        let out = run_view(vec![], None);
        assert!(out.actions.is_empty());
        assert!(out.measured_height_physical.is_none());
    }

    #[test]
    fn view_with_drag_does_not_panic() {
        let panes = vec![mk_pane(1, &["A", "B", "C"], 1, true)];
        let drag = Some(TabDragView {
            pane_id: 1,
            tab_index: 1,
            current_x: 240.0,
        });
        let out = run_view(panes, drag);
        // drag overlay 자체는 actions 를 추가하지 않음
        assert!(out.actions.is_empty());
    }
}
