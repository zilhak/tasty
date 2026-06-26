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
    /// 사용자 옵션 — 활성 탭 인디케이터 스타일 (Underline / Fill / Dot).
    pub active_tab_indicator: crate::settings::ActiveTabIndicator,
    /// 현재 drag 진행 상태 (None 이면 overlay 미표시).
    pub drag: Option<TabDragView>,
    /// switch-number overlay — 키캡을 그릴 **focused pane id**.
    /// 사용자가 `tab_switch_modifier`(대상=Tab)를 누르고 있는 동안만 `Some(focused_pane)`,
    /// 그 외엔 `None`. 이 pane 의 탭바에서만 leading 아이콘을 숫자 키캡(`Ctrl+1`…`0`)으로
    /// in-place 교체한다(비-focused pane 은 held 여도 아이콘 유지). release 시 `None` → 원복.
    pub switch_overlay_pane: Option<u32>,
}

/// View 가 발생시킨 사용자 의도. wrapper 가 state/engine 으로 반영.
#[derive(Clone, Debug, PartialEq)]
pub enum TabBarAction {
    SwitchTab {
        pane_id: u32,
        tab_index: usize,
    },
    CloseTab {
        pane_id: u32,
        tab_index: usize,
    },
    AddTab {
        pane_id: u32,
    },
    /// 탭스트립 우측 Split 아이콘 — 해당 pane 을 분할 (기존 split_pane 경로 재사용).
    RequestSplit {
        pane_id: u32,
    },
    /// 탭스트립 우측 Search 아이콘 — 해당 pane 활성 surface 검색 (기존 find 경로 재사용).
    OpenSearch {
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
    // 탭바는 host UI zoom 영향 받지 않는다 (사용자 제약). zoom-aware 토큰 (item_height_tab /
    // font_size_body / font_size_caption) 대신 zoom 미적용 tab_bar_* 토큰 사용.
    let bar_h = th.tab_bar_height.value();
    let plus_w: f32 = 28.0;
    // 우측 고정 IconButton (Split / Search) — 디자인 TabStrip 우측 클러스터.
    // 디자인 IconButton sm(control-height-tab) 에 맞춰 "+" 와 동일 폭.
    let icon_btn_w: f32 = 28.0;
    let right_icons_w: f32 = icon_btn_w * 2.0;
    let icon_glyph: f32 = 14.0;
    let arrow_w: f32 = 20.0;
    let separator_w: f32 = 1.0;
    let h_padding: f32 = 8.0;
    let dot_radius: f32 = 3.0;
    let dot_pad: f32 = 6.0;
    let active_indicator_h: f32 = 2.0;
    let plus_font_size = th.tab_bar_label_font_size.value();
    let arrow_font_size = th.tab_bar_arrow_font_size.value();

    for info in props.panes {
        let logical_x = (info.rect.x.value() / scale_factor).round_ui();
        let logical_y = (info.rect.y.value() / scale_factor).round_ui();
        let logical_w = (info.rect.width.value() / scale_factor).round_ui();
        let n = info.tab_names.len();
        let content_w =
            n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
        // 우측 IconButton 클러스터(Split/Search) 폭을 항상 확보한 뒤 남은 폭으로 탭/화살표 배치.
        let avail_w = (logical_w - right_icons_w).max(0.0);
        let needs_scroll = content_w > avail_w;
        let viewport_w = if needs_scroll {
            (avail_w - arrow_w * 2.0).max(0.0)
        } else {
            avail_w
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
                                    egui::Stroke::new(2.0, th.accent_success()),
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
                                // Fill 스타일만 활성 탭 배경을 채운다. Underline/Dot 은
                                // 배경을 비활성과 동일하게 두고 별도 마커로 표시.
                                let tab_bg = if is_active
                                    && props.active_tab_indicator
                                        == crate::settings::ActiveTabIndicator::Fill
                                {
                                    th.base
                                } else {
                                    bg
                                };
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
                                    use crate::settings::ActiveTabIndicator;
                                    match props.active_tab_indicator {
                                        ActiveTabIndicator::Underline => {
                                            let line_rect = egui::Rect::from_min_size(
                                                egui::pos2(tab_rect.min.x, tab_rect.min.y),
                                                egui::vec2(tab_w, active_indicator_h),
                                            );
                                            painter.rect_filled(
                                                line_rect,
                                                0.0,
                                                th.accent_primary(),
                                            );
                                        }
                                        // Fill: 배경은 위에서 이미 th.base 로 채움 — 추가 마커 없음.
                                        ActiveTabIndicator::Fill => {}
                                        ActiveTabIndicator::Dot => {
                                            // 탭 상단 중앙의 accent 점 마커.
                                            let r = active_indicator_h;
                                            let center = egui::pos2(
                                                tab_rect.center().x,
                                                tab_rect.min.y + r * 2.0,
                                            );
                                            painter.circle_filled(center, r, th.accent_primary());
                                        }
                                    }
                                }

                                // close 버튼 슬롯(우측 h_padding + 14px)을 비워두고 dot 은
                                // 그 왼쪽에 둔다 (close 와 겹치지 않게).
                                let dot_right = tab_rect.max.x - h_padding - 14.0;
                                if is_busy {
                                    let dot_center =
                                        egui::pos2(dot_right - dot_radius, tab_rect.center().y);
                                    let color: egui::Color32 = th.accent_success().into();
                                    painter.circle_filled(dot_center, dot_radius, color);
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
                                // switch-number overlay: tab_switch_modifier 홀드 + 단축키
                                // 있는 탭(1–9,0)은 아이콘 자리를 숫자 키캡으로 in-place 교체.
                                // focused pane(switch_overlay_pane) 의 탭바에서만 — 비-focused
                                // pane 은 held 여도 아이콘 유지(거짓 안내 방지).
                                // 폭/text_x 는 불변(아이콘 slot 중앙에 키캡) → 리플로 없음.
                                let switch_digit =
                                    crate::adapters::ui::switch_overlay::tab_keycap_for(
                                        props.switch_overlay_pane,
                                        info.pane_id,
                                        i,
                                    );
                                if let Some(digit) = switch_digit {
                                    crate::adapters::ui::switch_overlay::paint_keycap(
                                        &painter,
                                        th,
                                        icon_rect.center(),
                                        digit,
                                        is_active,
                                    );
                                } else {
                                    let kind = info.tab_kinds.get(i).copied().unwrap_or("terminal");
                                    kind_icon(kind)
                                        .image(icon_size, text_color.into())
                                        .paint_at(ui, icon_rect);
                                }

                                // 텍스트 — 아이콘 뒤, 좌측 정렬. 우측엔 dot 공간 확보.
                                let text_x = icon_rect.max.x + 6.0;
                                // 텍스트 우측 한계: dot/close 슬롯(dot_right) 왼쪽.
                                let mut text_right = dot_right - 4.0;
                                if is_busy {
                                    text_right -= dot_radius * 2.0 + dot_pad;
                                }
                                let available_w = (text_right - text_x).max(0.0);
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
                                    // close 버튼 (active or hover) — 우측 끝. 클릭은
                                    // SwitchTab 보다 우선.
                                    let show_close = is_active || resp.hovered();
                                    let close_clicked = if show_close {
                                        let cs = 14.0;
                                        let close_rect = egui::Rect::from_center_size(
                                            egui::pos2(
                                                tab_rect.max.x - h_padding - cs / 2.0,
                                                tab_rect.center().y,
                                            ),
                                            egui::vec2(cs, cs),
                                        );
                                        let cr = ui.interact(
                                            close_rect,
                                            egui::Id::new(("tabclose", info.pane_id, i)),
                                            egui::Sense::click(),
                                        );
                                        if cr.hovered() {
                                            painter.rect_filled(
                                                close_rect,
                                                2.0,
                                                th.active_overlay.to_egui_premultiplied(),
                                            );
                                        }
                                        let cc: egui::Color32 = if cr.hovered() {
                                            th.text.into()
                                        } else {
                                            th.subtext0.into()
                                        };
                                        icons::CLOSE.image(cs, cc).paint_at(ui, close_rect);
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
                                            egui::Stroke::new(2.0, th.accent_success()),
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
                                            egui::Stroke::new(2.0, th.accent_success()),
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

                            // 우측 IconButton 클러스터 — Split / Search (디자인 TabStrip).
                            // 탭바는 zoom 비적용 → 고정 px. "+" 와 동일 호버 스타일.
                            for (icon, is_split) in [(icons::SPLIT, true), (icons::SEARCH, false)] {
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(icon_btn_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let color = if resp.hovered() { th.text } else { th.subtext0 };
                                if resp.hovered() {
                                    ui.painter().rect_filled(r, 0.0, th.surface0);
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
        let avail_w = (pane_logical_w - right_icons_w).max(0.0);
        let needs_scroll_arrows = content_w > avail_w;
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

    // switch-number overlay — `switch_overlay()` 스냅샷(사용자 입력 ModifiersChanged 로만
    // 갱신)에서 Tab 대상 + 그릴 focused pane id 를 읽는다. 그 pane 의 탭바에서만 키캡을
    // 그리므로 비-focused pane 에는 거짓 안내가 뜨지 않는다. 스냅샷은 egui raw_input 의
    // 사용자 키 입력만 반영 → IPC/CLI/에이전트로는 강제 표시될 수 없다(순수 미리보기).
    let switch_overlay_pane = state.switch_overlay().and_then(|o| match o.target {
        crate::adapters::ui::switch_overlay::SwitchTarget::Tab => o.pane_id,
        crate::adapters::ui::switch_overlay::SwitchTarget::Workspace => None,
    });

    let props = PaneTabBarsProps {
        theme: &th,
        panes: &panes,
        scale_factor,
        tab_width: tab_w,
        tab_font_size,
        active_tab_indicator: appearance.active_tab_indicator,
        drag,
        switch_overlay_pane,
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
            TabBarAction::CloseTab { pane_id, tab_index } => {
                state.close_tab(engine, pane_id, tab_index);
            }
            TabBarAction::AddTab { pane_id } => {
                state.active_workspace_mut(engine).focused_pane = pane_id;
                if let Err(e) = state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
            }
            TabBarAction::RequestSplit { pane_id } => {
                // 단축키(`split_pane_vertical`)와 동일 경로. 사용자 클릭이므로 대상 pane
                // 으로 focus 이동 후 split (cascade 가 새 pane 으로 focus 이동).
                use crate::intent::Intent;
                use crate::model::SplitDirection;
                state.active_workspace_mut(engine).focused_pane = pane_id;
                state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_pane_vertical"),
                );
            }
            TabBarAction::OpenSearch { pane_id } => {
                // 단축키(`find`)와 동일 경로 — 대상 pane 활성 surface 에 검색창을 연다.
                use crate::adapters::ui::popup::PopupScope;
                use crate::intent::{OpenPopupMode, UiIntent};
                state.active_workspace_mut(engine).focused_pane = pane_id;
                if state.popups.is_open("search_bar") {
                    state.popups.set_focused("search_bar", true);
                } else if let Some(sid) = state.focused_surface_id(engine) {
                    state.search.surface_id = sid;
                    state.dispatch_intent(
                        UiIntent::OpenPopup {
                            id: "search_bar",
                            mode: OpenPopupMode::AtTopOfScope(PopupScope::Surface(sid)),
                        }
                        .from_user_shortcut("find"),
                    );
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
                active_tab_indicator: crate::settings::ActiveTabIndicator::default(),
                drag: drag.clone(),
                switch_overlay_pane: None,
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
