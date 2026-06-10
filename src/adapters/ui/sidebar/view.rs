//! Pure view 함수 + props/action — Full / Collapsed sidebar 의 시각 / 입력 처리.
//!
//! 본 모듈은 AppState / CoreState / 글로벌 `theme::theme()` 에 접근하지 않는다.
//! 호출처 wrapper (`full.rs::draw_full_sidebar`, `collapsed.rs::draw_collapsed_sidebar`)
//! 가 props 추출 + action 매핑을 담당한다. gallery 는 같은 view 를 mock props
//! 로 호출해 시각 검증한다 — Tier 3 패턴
//! (`.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).

use crate::theme::Theme;

/// Full / Collapsed 공통 — 사이드바 한 행 (workspace card / square) 에 들어가는
/// 데이터. AppState / CoreState 모두 비의존인 owned/snapshot 값.
#[derive(Debug, Clone)]
pub struct WorkspaceEntryView {
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub busy_count: usize,
    pub has_highlight: bool,
    /// 다른 client 가 해당 workspace 를 attach 한 상태 (빨간 인디케이터).
    pub attached: bool,
    pub is_active: bool,
}

/// Full sidebar 의 view 입력. labels 는 사전 번역.
pub struct SidebarFullProps<'a> {
    pub theme: &'a Theme,
    pub workspaces: &'a [WorkspaceEntryView],
    pub drag: Option<DragSnapshot>,
    pub tools_label: &'a str,
    pub collapse_label: &'a str,
    pub plugins_label: &'a str,
    pub settings_label: &'a str,
    pub new_workspace_label: &'a str,
    pub occupied_hover: &'a str,
}

/// 진행 중인 workspace drag-and-drop 의 스냅샷. 호출처가 매 프레임 view 에 전달.
#[derive(Debug, Clone, Copy)]
pub struct DragSnapshot {
    pub ws_idx: usize,
    pub current_y: f32,
}

/// Collapsed sidebar 의 view 입력.
pub struct SidebarCollapsedProps<'a> {
    pub theme: &'a Theme,
    pub workspaces: &'a [WorkspaceEntryView],
    pub tools_hover: &'a str,
}

/// Full sidebar view 가 보고하는 사용자 의도. wrapper 가 state mutation 으로 변환.
#[derive(Debug, Clone)]
pub enum SidebarFullAction {
    Collapse,
    Plugins,
    Settings,
    ToolsClicked(egui::Rect),
    WorkspaceClicked(usize),
    WorkspaceContextMenu {
        ws_idx: usize,
        x: f32,
        y: f32,
    },
    DragStart {
        ws_idx: usize,
        y: f32,
    },
    DragUpdate {
        y: f32,
    },
    /// 마우스 떼짐 — drop_target=None 이면 drop 위치가 from 과 동일 (이동 없음).
    DragReleased {
        drop_target: Option<usize>,
    },
    NewWorkspace,
    /// "New workspace" 버튼 우클릭 — 프리셋으로 새 워크스페이스 생성 진입점.
    NewWorkspaceContextMenu {
        x: f32,
        y: f32,
    },
}

/// Collapsed sidebar view 가 보고하는 사용자 의도.
#[derive(Debug, Clone)]
pub enum SidebarCollapsedAction {
    Expand,
    Plugins,
    Settings,
    ToolsClicked(egui::Rect),
    WorkspaceClicked(usize),
    NewWorkspace,
    /// "+" 아이콘 우클릭 — 프리셋으로 새 워크스페이스 생성 진입점.
    NewWorkspaceContextMenu {
        x: f32,
        y: f32,
    },
}

const BTN_HEIGHT: f32 = 28.0;
const COLLAPSED_ICON_SIZE: egui::Vec2 = egui::vec2(32.0, 22.0);
const COLLAPSED_WS_SIZE: egui::Vec2 = egui::vec2(32.0, 28.0);
const CARD_INNER_MARGIN_X: i8 = 8;
const CARD_INNER_MARGIN_Y: i8 = 6;

/// Pure view: full sidebar 내부 (SidePanel 안쪽 ui) 를 그리고 action 리스트
/// 를 반환. 호출처는 SidePanel 을 직접 연다.
pub fn draw_full_sidebar_view(
    ui: &mut egui::Ui,
    props: &SidebarFullProps<'_>,
) -> Vec<SidebarFullAction> {
    let mut actions: Vec<SidebarFullAction> = Vec::new();
    let th = props.theme;

    // 바닥 고정 섹션 (Tools / Collapse / Plugins / Settings).
    egui::TopBottomPanel::bottom("workspace_sidebar_bottom")
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.separator();
            ui.add_space(2.0);

            // Tools
            if let Some(rect) = draw_full_bottom_button(ui, th, props.tools_label) {
                actions.push(SidebarFullAction::ToolsClicked(rect));
            }
            ui.add_space(2.0);

            // Collapse
            if draw_full_bottom_button(ui, th, props.collapse_label).is_some() {
                actions.push(SidebarFullAction::Collapse);
            }
            ui.add_space(2.0);

            // Plugins
            if draw_full_bottom_button(ui, th, props.plugins_label).is_some() {
                actions.push(SidebarFullAction::Plugins);
            }
            ui.add_space(2.0);

            // Settings
            if draw_full_bottom_button(ui, th, props.settings_label).is_some() {
                actions.push(SidebarFullAction::Settings);
            }
            ui.add_space(8.0);
        });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.add_space(4.0);
            let mut card_rects: Vec<(usize, egui::Rect)> = Vec::new();

            for (i, ws) in props.workspaces.iter().enumerate() {
                let card_rect = draw_workspace_card(ui, th, ws, props.occupied_hover);
                let card_response = ui.interact(
                    card_rect,
                    egui::Id::new(("ws_card", i)),
                    egui::Sense::click_and_drag(),
                );

                if card_response.clicked() {
                    actions.push(SidebarFullAction::WorkspaceClicked(i));
                }

                if card_response.secondary_clicked() {
                    let pos = card_response.interact_pointer_pos().unwrap_or_default();
                    actions.push(SidebarFullAction::WorkspaceContextMenu {
                        ws_idx: i,
                        x: pos.x,
                        y: pos.y,
                    });
                    ui.painter().rect_stroke(
                        card_rect,
                        4.0,
                        egui::Stroke::new(2.0, th.green),
                        egui::StrokeKind::Inside,
                    );
                }

                if card_response.drag_started_by(egui::PointerButton::Primary) {
                    let y = card_response
                        .interact_pointer_pos()
                        .map(|p| p.y)
                        .unwrap_or(0.0);
                    actions.push(SidebarFullAction::DragStart { ws_idx: i, y });
                }

                if card_response.dragged_by(egui::PointerButton::Primary)
                    && let Some(drag) = props.drag
                    && drag.ws_idx == i
                    && let Some(pos) = card_response.interact_pointer_pos()
                {
                    actions.push(SidebarFullAction::DragUpdate { y: pos.y });
                }

                card_rects.push((i, card_rect));
                ui.add_space(2.0);
            }

            // Drag release / drop marker / ghost preview.
            if let Some(drag) = props.drag {
                let released = !ui.input(|i| i.pointer.primary_down());
                if released {
                    let target = card_rects
                        .iter()
                        .position(|(_, rect)| drag.current_y < rect.center().y)
                        .unwrap_or(card_rects.len().saturating_sub(1));
                    let target = target.min(props.workspaces.len().saturating_sub(1));
                    let drop = (target != drag.ws_idx).then_some(target);
                    actions.push(SidebarFullAction::DragReleased { drop_target: drop });
                } else {
                    // Insert marker.
                    let insert_idx = card_rects
                        .iter()
                        .position(|(_, rect)| drag.current_y < rect.center().y)
                        .unwrap_or(card_rects.len());
                    if let Some(marker_rect) = if insert_idx < card_rects.len() {
                        Some(card_rects[insert_idx].1)
                    } else {
                        card_rects.last().map(|(_, r)| *r)
                    } {
                        let marker_y = if insert_idx < card_rects.len() {
                            marker_rect.min.y - 1.0
                        } else {
                            marker_rect.max.y + 1.0
                        };
                        let line = egui::Rect::from_min_size(
                            egui::pos2(marker_rect.min.x, marker_y),
                            egui::vec2(marker_rect.width(), 2.0),
                        );
                        ui.painter().rect_filled(line, 0.0, th.blue);
                    }

                    // Ghost card.
                    if let Some(ws) = props.workspaces.get(drag.ws_idx)
                        && let Some((_, first_rect)) = card_rects.first()
                    {
                        let ghost_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                first_rect.min.x,
                                drag.current_y - first_rect.height() / 2.0,
                            ),
                            first_rect.size(),
                        );
                        let ghost_bg = th.surface0.with_alpha(180).to_egui();
                        let ghost_fg = th.text.with_alpha(180).to_egui();
                        ui.painter().rect_filled(ghost_rect, 4.0, ghost_bg);
                        ui.painter().text(
                            ghost_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            &ws.name,
                            egui::FontId::proportional(12.0),
                            ghost_fg,
                        );
                    }
                }
            }

            ui.add_space(4.0);
            let full_width = ui.available_width();
            let new_ws_resp = ui.add_sized(
                [full_width, BTN_HEIGHT],
                egui::Button::new(props.new_workspace_label),
            );
            if new_ws_resp.clicked() {
                actions.push(SidebarFullAction::NewWorkspace);
            }
            if new_ws_resp.secondary_clicked() {
                let pos = new_ws_resp.interact_pointer_pos().unwrap_or_default();
                actions.push(SidebarFullAction::NewWorkspaceContextMenu { x: pos.x, y: pos.y });
                ui.painter().rect_stroke(
                    new_ws_resp.rect,
                    4.0,
                    egui::Stroke::new(2.0, th.green),
                    egui::StrokeKind::Inside,
                );
            }
            ui.add_space(4.0);
        });

    actions
}

/// Pure view: collapsed sidebar 내부.
pub fn draw_collapsed_sidebar_view(
    ui: &mut egui::Ui,
    props: &SidebarCollapsedProps<'_>,
) -> Vec<SidebarCollapsedAction> {
    let mut actions: Vec<SidebarCollapsedAction> = Vec::new();
    let th = props.theme;

    egui::TopBottomPanel::bottom("workspace_sidebar_collapsed_bottom")
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.separator();
                ui.add_space(2.0);

                // Tools
                let (tools_btn_rect, tools_resp) =
                    ui.allocate_exact_size(COLLAPSED_ICON_SIZE, egui::Sense::click());
                paint_icon_button(ui, th, tools_btn_rect, &tools_resp, "T", 12.0);
                let tools_resp = tools_resp.on_hover_text(props.tools_hover);
                if tools_resp.clicked() {
                    actions.push(SidebarCollapsedAction::ToolsClicked(tools_btn_rect));
                }
                ui.add_space(2.0);

                // Expand
                let (rect, resp) =
                    ui.allocate_exact_size(COLLAPSED_ICON_SIZE, egui::Sense::click());
                paint_icon_button(ui, th, rect, &resp, ">", 14.0);
                if resp.clicked() {
                    actions.push(SidebarCollapsedAction::Expand);
                }
                ui.add_space(2.0);

                // Plugins
                let (rect, resp) =
                    ui.allocate_exact_size(COLLAPSED_ICON_SIZE, egui::Sense::click());
                paint_icon_button(ui, th, rect, &resp, "\u{1F9E9}", 14.0);
                if resp.clicked() {
                    actions.push(SidebarCollapsedAction::Plugins);
                }
                ui.add_space(2.0);

                // Settings
                let (rect, resp) =
                    ui.allocate_exact_size(COLLAPSED_ICON_SIZE, egui::Sense::click());
                paint_icon_button(ui, th, rect, &resp, "\u{2699}", 14.0);
                if resp.clicked() {
                    actions.push(SidebarCollapsedAction::Settings);
                }
                ui.add_space(12.0);
            });
        });

    ui.vertical_centered(|ui| {
        ui.add_space(4.0);
        for (i, ws) in props.workspaces.iter().enumerate() {
            let label = format!("{}", i + 1);
            let bg = if ws.is_active { th.surface0 } else { th.mantle };
            let text_color = if ws.is_active {
                th.text
            } else if ws.has_highlight {
                th.yellow
            } else {
                th.subtext0
            };

            let (rect, resp) = ui.allocate_exact_size(COLLAPSED_WS_SIZE, egui::Sense::click());
            ui.painter().rect_filled(rect, 4.0, bg);
            if ws.is_active {
                ui.painter().rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.0, th.blue),
                    egui::StrokeKind::Inside,
                );
            }
            if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &label,
                egui::FontId::proportional(12.0),
                text_color.into(),
            );
            if ws.busy_count > 0 {
                let dot_radius = 3.0;
                let dot_pad = 4.0;
                let dot_center = egui::pos2(
                    rect.max.x - dot_pad - dot_radius,
                    rect.min.y + dot_pad + dot_radius,
                );
                ui.painter().circle_filled(dot_center, dot_radius, th.green);
            }
            if ws.attached {
                let dot_radius = 3.0;
                let dot_pad = 4.0;
                let dot_center = egui::pos2(
                    rect.max.x - dot_pad - dot_radius,
                    rect.max.y - dot_pad - dot_radius,
                );
                ui.painter().circle_filled(dot_center, dot_radius, th.red);
            }
            if resp.clicked() {
                actions.push(SidebarCollapsedAction::WorkspaceClicked(i));
            }
        }

        ui.add_space(2.0);
        let (rect, resp) = ui.allocate_exact_size(COLLAPSED_ICON_SIZE, egui::Sense::click());
        paint_icon_button(ui, th, rect, &resp, "+", 14.0);
        if resp.clicked() {
            actions.push(SidebarCollapsedAction::NewWorkspace);
        }
        if resp.secondary_clicked() {
            let pos = resp.interact_pointer_pos().unwrap_or_default();
            actions.push(SidebarCollapsedAction::NewWorkspaceContextMenu { x: pos.x, y: pos.y });
            ui.painter().rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(2.0, th.green),
                egui::StrokeKind::Inside,
            );
        }
    });

    actions
}

/// 풀 사이드바 바닥 버튼 (Tools/Collapse/Plugins/Settings) 1 행. 클릭되면 rect 반환.
fn draw_full_bottom_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> Option<egui::Rect> {
    let full_width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(full_width, BTN_HEIGHT),
        egui::Sense::click().union(egui::Sense::hover()),
    );
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        if resp.hovered() {
            th.subtext1.into()
        } else {
            th.overlay0.into()
        },
    );
    resp.clicked().then_some(rect)
}

/// Collapsed 측 아이콘 버튼의 hover 배경 + 텍스트 그리기 helper.
fn paint_icon_button(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    resp: &egui::Response,
    glyph: &str,
    font_size: f32,
) {
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(font_size),
        if resp.hovered() {
            th.subtext1.into()
        } else {
            th.overlay0.into()
        },
    );
}

/// Full 사이드바의 workspace card 1 장 — Frame::show 로 직접 그리고 점유한 rect 반환.
fn draw_workspace_card(
    ui: &mut egui::Ui,
    th: &Theme,
    ws: &WorkspaceEntryView,
    occupied_hover: &str,
) -> egui::Rect {
    let bg = if ws.is_active {
        th.surface0.to_egui()
    } else {
        egui::Color32::TRANSPARENT
    };
    let border = if ws.is_active {
        th.blue.to_egui()
    } else {
        th.surface0.to_egui()
    };

    let frame = egui::Frame::new()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(
            CARD_INNER_MARGIN_X,
            CARD_INNER_MARGIN_Y,
        ));

    let response = frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            let title_text = if ws.is_active {
                egui::RichText::new(&ws.name).strong()
            } else {
                egui::RichText::new(&ws.name)
            };
            ui.label(title_text);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ws.attached {
                    let dot_radius = 3.0;
                    let (dot_rect, resp) = ui.allocate_exact_size(
                        egui::vec2(dot_radius * 2.0 + 2.0, dot_radius * 2.0),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .circle_filled(dot_rect.center(), dot_radius, th.red);
                    resp.on_hover_text(occupied_hover);
                }

                if ws.has_highlight {
                    let badge_size = egui::vec2(18.0, 16.0);
                    let (rect, _) = ui.allocate_exact_size(badge_size, egui::Sense::hover());
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        egui::Stroke::new(1.0, th.blue),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "!",
                        egui::FontId::proportional(10.0),
                        th.blue.into(),
                    );
                }

                if ws.busy_count > 0 {
                    let count_text = format!("{}", ws.busy_count);
                    ui.label(egui::RichText::new(&count_text).small().color(th.green));
                    let dot_radius = 3.0;
                    let (dot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(dot_radius * 2.0 + 2.0, dot_radius * 2.0),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .circle_filled(dot_rect.center(), dot_radius, th.green);
                }
            });
        });

        if !ws.subtitle.is_empty() {
            ui.label(egui::RichText::new(&ws.subtitle).small().color(th.subtext0));
        }

        if !ws.description.is_empty() {
            ui.label(
                egui::RichText::new(&ws.description)
                    .small()
                    .color(th.overlay0),
            );
        }
    });

    response.response.rect
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn mock_ws(name: &str, is_active: bool) -> WorkspaceEntryView {
        WorkspaceEntryView {
            name: name.to_string(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 0,
            has_highlight: false,
            attached: false,
            is_active,
        }
    }

    fn run_full(workspaces: Vec<WorkspaceEntryView>) -> Vec<SidebarFullAction> {
        let ctx = egui::Context::default();
        let mut out: Vec<SidebarFullAction> = Vec::new();
        let theme = test_theme();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::SidePanel::left("test_full").show(ctx, |ui| {
                let props = SidebarFullProps {
                    theme: &theme,
                    workspaces: &workspaces,
                    drag: None,
                    tools_label: "Tools",
                    collapse_label: "Collapse",
                    plugins_label: "Plugins",
                    settings_label: "Settings",
                    new_workspace_label: "New Workspace",
                    occupied_hover: "Held by another client",
                };
                out = draw_full_sidebar_view(ui, &props);
            });
        }));
        out
    }

    fn run_collapsed(workspaces: Vec<WorkspaceEntryView>) -> Vec<SidebarCollapsedAction> {
        let ctx = egui::Context::default();
        let mut out: Vec<SidebarCollapsedAction> = Vec::new();
        let theme = test_theme();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::SidePanel::left("test_collapsed").show(ctx, |ui| {
                let props = SidebarCollapsedProps {
                    theme: &theme,
                    workspaces: &workspaces,
                    tools_hover: "Tools menu",
                };
                out = draw_collapsed_sidebar_view(ui, &props);
            });
        }));
        out
    }

    #[test]
    fn full_view_no_input_yields_no_actions() {
        let ws = vec![mock_ws("Default", true)];
        let actions = run_full(ws);
        assert!(actions.is_empty(), "expected no actions, got {actions:?}");
    }

    #[test]
    fn full_view_renders_many_without_panic() {
        let ws: Vec<_> = (0..10)
            .map(|i| mock_ws(&format!("ws-{i}"), i == 0))
            .collect();
        let actions = run_full(ws);
        assert!(actions.is_empty());
    }

    #[test]
    fn collapsed_view_no_input_yields_no_actions() {
        let ws = vec![mock_ws("Default", true), mock_ws("Other", false)];
        let actions = run_collapsed(ws);
        assert!(actions.is_empty());
    }

    #[test]
    fn collapsed_view_renders_busy_and_attached_without_panic() {
        let ws = vec![WorkspaceEntryView {
            name: "active".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 3,
            has_highlight: true,
            attached: true,
            is_active: true,
        }];
        let actions = run_collapsed(ws);
        assert!(actions.is_empty());
    }
}
