//! Pure view 함수 + props/action — Full / Collapsed sidebar 의 시각 / 입력 처리.
//!
//! 본 모듈은 AppState / CoreState / 글로벌 `theme::theme()` 에 접근하지 않는다.
//! 호출처 wrapper (`full.rs::draw_full_sidebar`, `collapsed.rs::draw_collapsed_sidebar`)
//! 가 props 추출 + action 매핑을 담당한다. gallery 는 같은 view 를 mock props
//! 로 호출해 시각 검증한다 — Tier 3 패턴
//! (`.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).

use crate::adapters::ui::{brand, icons};
use crate::theme::Theme;

/// 사이드바 헤더 (full / collapsed) 에 표시되는 수박 로고 PNG.
/// `egui_extras::install_image_loaders` (gpu.rs) 가 PNG 디코딩을 처리한다.
const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/icons/icon_256.png");
const LOGO_URI: &str = "bytes://tasty_sidebar_logo_256.png";

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
    /// 이 워크스페이스가 원격을 attach 한 client mirror 인지 (하늘색 인디케이터, 항상 켜짐).
    pub is_mirror: bool,
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
    pub workspaces_heading: &'a str,
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

// Sidebar 의 모든 zoom-sensitive 길이는 Theme 토큰에서 가져온다 (Z-1/Z-2 에서
// host UI zoom 곱셈이 토큰 자체에 박힘). 아래는 토큰에서 도출하는 헬퍼.
fn btn_height(th: &Theme) -> f32 {
    th.item_height_tab.value()
}
fn collapsed_icon_size(th: &Theme) -> egui::Vec2 {
    egui::vec2(
        th.sidebar_collapsed_slot_width.value(),
        th.sidebar_collapsed_icon_height.value(),
    )
}
fn collapsed_ws_size(th: &Theme) -> egui::Vec2 {
    egui::vec2(
        th.sidebar_collapsed_slot_width.value(),
        th.sidebar_collapsed_workspace_height.value(),
    )
}
fn card_inner_margin_x(th: &Theme) -> i8 {
    th.spacing_sm.value() as i8
}
fn card_inner_margin_y(th: &Theme) -> i8 {
    th.spacing_xs.value() as i8
}

/// Pure view: full sidebar 내부 (SidePanel 안쪽 ui) 를 그리고 action 리스트
/// 를 반환. 호출처는 SidePanel 을 직접 연다.
pub fn draw_full_sidebar_view(
    ui: &mut egui::Ui,
    props: &SidebarFullProps<'_>,
) -> Vec<SidebarFullAction> {
    let mut actions: Vec<SidebarFullAction> = Vec::new();
    let th = props.theme;

    // 헤더 — 워드마크 `tasty.` + 접기 (ui_kit Sidebar 상단).
    egui::TopBottomPanel::top("workspace_sidebar_header")
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ui.add_space(10.0);
            if draw_sidebar_header(ui, th, props.collapse_label) {
                actions.push(SidebarFullAction::Collapse);
            }
            ui.add_space(6.0);
        });

    // 바닥 고정 섹션 (Tools / Plugins / Settings). 접기는 헤더로 이동.
    egui::TopBottomPanel::bottom("workspace_sidebar_bottom")
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.separator();
            ui.add_space(2.0);

            // Tools
            let tools_resp = draw_ghost_block_button(ui, th, Some(icons::TOOLS), props.tools_label);
            if tools_resp.clicked() {
                actions.push(SidebarFullAction::ToolsClicked(tools_resp.rect));
            }
            ui.add_space(2.0);

            // Plugins
            if draw_ghost_block_button(ui, th, Some(icons::PLUG), props.plugins_label).clicked() {
                actions.push(SidebarFullAction::Plugins);
            }
            ui.add_space(2.0);

            // Settings
            if draw_ghost_block_button(ui, th, Some(icons::SETTINGS), props.settings_label)
                .clicked()
            {
                actions.push(SidebarFullAction::Settings);
            }
            ui.add_space(8.0);
        });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.add_space(8.0);
            draw_section_heading(ui, th, props.workspaces_heading);
            ui.add_space(4.0);
            let mut card_rects: Vec<(usize, egui::Rect)> = Vec::new();

            // 디자인 chrome.jsx:141-149 — 목록 블록 상단 보더 (separator).
            if !props.workspaces.is_empty() {
                draw_list_separator(ui, th, 0.0);
            }

            for (i, ws) in props.workspaces.iter().enumerate() {
                // 행 사이 1px 구분선, 좌측 32px 들여쓰기 (디자인 margin-left:32px).
                if i > 0 {
                    draw_list_separator(ui, th, 32.0);
                }
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
            }

            // 디자인 chrome.jsx:141-149 — 목록 블록 하단 보더 (separator).
            if !props.workspaces.is_empty() {
                draw_list_separator(ui, th, 0.0);
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
            let new_ws_resp =
                draw_ghost_block_button(ui, th, Some(icons::PLUS), props.new_workspace_label);
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

    // 헤더 — 로고 + 펼치기(») 버튼 (ui_kit CollapsedSidebar 상단).
    egui::TopBottomPanel::top("workspace_sidebar_collapsed_header")
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                // 로고 (collapsed) — 상단, expand 버튼 위.
                let logo_size = th.sidebar_logo_collapsed_size.value();
                let logo_vec = egui::vec2(logo_size, logo_size);
                let (logo_rect, _) = ui.allocate_exact_size(logo_vec, egui::Sense::hover());
                egui::Image::from_bytes(LOGO_URI, LOGO_PNG)
                    .fit_to_exact_size(logo_vec)
                    .paint_at(ui, logo_rect);
                ui.add_space(4.0);
                let (rect, resp) =
                    ui.allocate_exact_size(collapsed_icon_size(th), egui::Sense::click());
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
                }
                let color: egui::Color32 = if resp.hovered() {
                    th.subtext1.into()
                } else {
                    th.overlay0.into()
                };
                icons::CHEVRONS_RIGHT.image(16.0, color).paint_at(
                    ui,
                    egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
                );
                if resp.clicked() {
                    actions.push(SidebarCollapsedAction::Expand);
                }
            });
            ui.add_space(6.0);
        });

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
                    ui.allocate_exact_size(collapsed_icon_size(th), egui::Sense::click());
                paint_icon_button(ui, th, tools_btn_rect, &tools_resp, icons::TOOLS);
                let tools_resp = tools_resp.on_hover_text(props.tools_hover);
                if tools_resp.clicked() {
                    actions.push(SidebarCollapsedAction::ToolsClicked(tools_btn_rect));
                }
                ui.add_space(2.0);

                // Plugins
                let (rect, resp) =
                    ui.allocate_exact_size(collapsed_icon_size(th), egui::Sense::click());
                paint_icon_button(ui, th, rect, &resp, icons::PLUG);
                if resp.clicked() {
                    actions.push(SidebarCollapsedAction::Plugins);
                }
                ui.add_space(2.0);

                // Settings
                let (rect, resp) =
                    ui.allocate_exact_size(collapsed_icon_size(th), egui::Sense::click());
                paint_icon_button(ui, th, rect, &resp, icons::SETTINGS);
                if resp.clicked() {
                    actions.push(SidebarCollapsedAction::Settings);
                }
                ui.add_space(12.0);
            });
        });

    ui.vertical_centered(|ui| {
        ui.add_space(4.0);
        for (i, ws) in props.workspaces.iter().enumerate() {
            // 디자인 (chrome.jsx CollapsedSidebar): 워크스페이스 이름 첫 글자 대문자,
            // mono 13 bold. 빈 이름이면 라벨 생략 (한글/이모지도 안전하게 chars().next()).
            let label = ws
                .name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            // G3: 디자인 IconButton.active — active = bg overlay-active + 글자색
            // accent-primary(blue), 테두리 없음. inactive = bg 없음 + 글자색 text-muted.
            // G4: notif 의 글자색(yellow) 표현은 제거 — notif 는 우상단 dot 으로만.
            let text_color: egui::Color32 = if ws.is_active {
                th.accent_primary().into()
            } else {
                th.text_muted().into()
            };

            let (rect, resp) = ui.allocate_exact_size(collapsed_ws_size(th), egui::Sense::click());
            if ws.is_active {
                ui.painter()
                    .rect_filled(rect, 4.0, th.overlay_active().to_egui_premultiplied());
            }
            if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
            }
            if !label.is_empty() {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &label,
                    egui::FontId::monospace(th.font_size_body.value()),
                    text_color,
                );
            }
            // 우상단 dot — mirror(하늘색) > notif(blue+링) > running(초록). attached(빨강)는 우하단.
            let dot_radius = 3.0;
            let dot_pad = 4.0;
            let dot_center = egui::pos2(
                rect.max.x - dot_pad - dot_radius,
                rect.min.y + dot_pad + dot_radius,
            );
            if ws.is_mirror {
                ui.painter().circle_filled(dot_center, dot_radius, th.sky);
            } else if ws.has_highlight {
                // G4: notif → blue dot + bg-sidebar 링 (디자인 Badge dot variant, boxShadow 0 0 0 1.5px).
                ui.painter()
                    .circle_filled(dot_center, dot_radius + 1.5, th.mantle);
                ui.painter()
                    .circle_filled(dot_center, dot_radius, th.accent_primary());
            } else if ws.busy_count > 0 {
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
        let (rect, resp) = ui.allocate_exact_size(collapsed_icon_size(th), egui::Sense::click());
        paint_icon_button(ui, th, rect, &resp, icons::PLUS);
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

/// 디자인의 ghost variant block button — 사이드바 좌측 정렬 버튼 공통 (Full
/// New Workspace / Tools / Plugins / Settings). 평소 subtext1 (text-secondary),
/// hover 시 text (text-primary) + overlay_hover 배경, pressed 시 overlay_active.
fn draw_ghost_block_button(
    ui: &mut egui::Ui,
    th: &Theme,
    leading_icon: Option<icons::Icon>,
    label: &str,
) -> egui::Response {
    let full_width = ui.available_width();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(full_width, btn_height(th)), egui::Sense::click());
    let pressed = resp.is_pointer_button_down_on();
    if pressed {
        ui.painter()
            .rect_filled(rect, 4.0, th.active_overlay.to_egui_premultiplied());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
    }
    let color: egui::Color32 = if resp.hovered() || pressed {
        th.text.into()
    } else {
        th.subtext1.into()
    };
    let mut text_x = rect.min.x + 10.0;
    if let Some(icon) = leading_icon {
        let icon_size = 16.0;
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(text_x, rect.center().y - icon_size / 2.0),
            egui::vec2(icon_size, icon_size),
        );
        icon.image(icon_size, color).paint_at(ui, icon_rect);
        text_x = icon_rect.max.x + 8.0;
    }
    ui.painter().text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(th.sidebar_button_label_font_size.value()),
        color,
    );
    resp
}

/// ui_kit 사이드바 헤더 — 워드마크 `tasty.` (`.` = 브랜드색) + 접기(«).
/// collapse 클릭 여부 반환.
fn draw_sidebar_header(ui: &mut egui::Ui, th: &Theme, collapse_hover: &str) -> bool {
    let mut collapse = false;
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        // 로고 (수박 PNG) — 워드마크 좌측, gap 8.
        let logo_size = th.sidebar_logo_size.value();
        let logo_vec = egui::vec2(logo_size, logo_size);
        let (logo_rect, _) = ui.allocate_exact_size(logo_vec, egui::Sense::hover());
        egui::Image::from_bytes(LOGO_URI, LOGO_PNG)
            .fit_to_exact_size(logo_vec)
            .paint_at(ui, logo_rect);
        ui.add_space(8.0);
        let mut job = egui::text::LayoutJob::default();
        let font = egui::FontId::monospace(th.sidebar_wordmark_font_size.value());
        // 워드마크 트래킹 -0.5px (디자인 정책: mono 17 bold).
        job.append(
            "tasty",
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                extra_letter_spacing: -0.5,
                color: th.text.into(),
                ..Default::default()
            },
        );
        job.append(
            ".",
            0.0,
            egui::TextFormat {
                font_id: font,
                extra_letter_spacing: -0.5,
                color: brand::MELON_FLESH.into(),
                ..Default::default()
            },
        );
        ui.label(job);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
            }
            // 평소: subtext1 (--text-secondary), hover: text (--text-primary). 톤 한 단계 상향.
            let color: egui::Color32 = if resp.hovered() {
                th.text.into()
            } else {
                th.subtext1.into()
            };
            icons::CHEVRONS_LEFT.image(16.0, color).paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
            );
            resp.clone().on_hover_text(collapse_hover);
            collapse = resp.clicked();
        });
    });
    collapse
}

/// ui_kit 섹션 헤딩 — 모노 대문자, muted, 좌측 패딩. 트래킹 0.07em (=0.7px @ 10px).
fn draw_section_heading(ui: &mut egui::Ui, th: &Theme, text: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::monospace(th.sidebar_section_heading_font_size.value()),
            extra_letter_spacing: 0.7,
            color: th.subtext0.into(),
            ..Default::default()
        },
    );
    let galley = ui.painter().layout_job(job);
    let pos = egui::pos2(rect.min.x + 10.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, th.subtext0.into());
}

/// 워크스페이스 목록의 1px 수평 구분선 (디자인 `separator` 토큰).
/// 블록 상하 보더는 `left_inset=0`, 행 사이 구분선은 `left_inset=32`(디자인
/// `margin-left:32px`). `separator` 는 premultiplied 반투명 바이트로 저장돼 있어
/// `to_egui_premultiplied()` 로 변환한다.
fn draw_list_separator(ui: &mut egui::Ui, th: &Theme, left_inset: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    let line = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + left_inset, rect.min.y),
        egui::vec2((rect.width() - left_inset).max(0.0), 1.0),
    );
    ui.painter()
        .rect_filled(line, 0.0, th.separator.to_egui_premultiplied());
}

/// Collapsed 측 IconButton — hover 배경 + SVG icon 그리기 helper.
fn paint_icon_button(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    resp: &egui::Response,
    icon: icons::Icon,
) {
    // pressed (마우스 누른 채 위) > hover > idle. pressed 가 우선, 배경만 강화.
    let pressed = resp.is_pointer_button_down_on();
    if pressed {
        ui.painter()
            .rect_filled(rect, 4.0, th.active_overlay.to_egui_premultiplied());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
    }
    let color: egui::Color32 = if resp.hovered() || pressed {
        th.subtext1.into()
    } else {
        th.overlay0.into()
    };
    let icon_size = 16.0;
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(icon_size, icon_size));
    icon.image(icon_size, color).paint_at(ui, icon_rect);
}

/// Full 사이드바의 workspace card 1 장 — Frame::show 로 직접 그리고 점유한 rect 반환.
fn draw_workspace_card(
    ui: &mut egui::Ui,
    th: &Theme,
    ws: &WorkspaceEntryView,
    occupied_hover: &str,
) -> egui::Rect {
    // ui_kit WorkspaceRow — 테두리 없는 플랫 행. active 만 배경 채움 (`--surface-active`
    // = catppuccin surface2).
    let bg = if ws.is_active {
        th.surface2.to_egui()
    } else {
        egui::Color32::TRANSPARENT
    };

    let frame = egui::Frame::new()
        .fill(bg)
        .corner_radius(2.0)
        .outer_margin(egui::Margin::symmetric(6, 0))
        .inner_margin(egui::Margin::symmetric(
            card_inner_margin_x(th),
            card_inner_margin_y(th),
        ));

    let response = frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // 좌측 상태 dot — 디자인 StatusDot (running/idle/agent/waiting/error)
            // 중 ws-level 데이터로 결정 가능한 case 만 표시. dot 은 항상 렌더하고
            // 색만 상태별로 분기한다 (디자인 StatusDot 은 idle 에도 점을 그림).
            // 우선순위: mirror (원격 attach client mirror) → sky
            //          > running(busy_count>0) → green (accent-success)
            //          > attached (다른 client 점유) → red (accent-danger)
            //          > idle → text-muted (회색)
            // 디자인의 agent / waiting case 는 ws-level 데이터 부재로 보류.
            // 폴더 아이콘(16px) 슬롯과 동일 폭을 차지해 라벨 위치가 흔들리지 않게 한다.
            let dot_slot = egui::vec2(16.0, 16.0);
            let (dot_rect, dot_resp) = ui.allocate_exact_size(dot_slot, egui::Sense::hover());
            // 디자인 StatusDot: 활성/비활성 무관하게 같은 색 (alpha 조정 없음).
            let dot_color: egui::Color32 = if ws.is_mirror {
                th.sky.into()
            } else if ws.busy_count > 0 {
                th.accent_success().into()
            } else if ws.attached {
                th.accent_danger().into()
            } else {
                th.text_muted().into()
            };
            ui.painter()
                .circle_filled(dot_rect.center(), 4.0, dot_color);
            if ws.attached && ws.busy_count == 0 {
                dot_resp.on_hover_text(occupied_hover);
            }
            // G5/J4: active 이름 text-primary, inactive 이름 text-secondary (한 단계
            // 어두움). 강조는 색으로만 — 디자인엔 굵기 차이가 없어 .strong() 미사용.
            let name_color = if ws.is_active {
                th.text_primary()
            } else {
                th.text_secondary()
            };
            ui.label(egui::RichText::new(&ws.name).color(name_color));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ws.has_highlight {
                    // 디자인 Badge variant="primary" — accent-primary 채움 pill.
                    // notif count 데이터가 ws-level 에 없어(props=bool) 숫자는 생략하고
                    // pill 형태(채움 원형)만 표현한다. count 배선은 별도 TODO.
                    let badge_size = egui::vec2(10.0, 10.0);
                    let (rect, _) = ui.allocate_exact_size(badge_size, egui::Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 5.0, th.accent_primary());
                }
            });
        });

        if !ws.subtitle.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                // 디자인 chrome.jsx:62 — subtitle 은 font-mono, text-muted.
                ui.label(
                    egui::RichText::new(&ws.subtitle)
                        .small()
                        .monospace()
                        .color(th.text_muted()),
                );
            });
        }

        if !ws.description.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new(&ws.description)
                        .small()
                        .color(th.overlay0),
                );
            });
        }
    });

    let card_rect = response.response.rect;

    if !ws.is_active && response.response.hovered() {
        ui.painter()
            .rect_filled(card_rect, 2.0, th.hover_overlay.to_egui_premultiplied());
    }

    // Active 좌측 2px inset accent bar (디자인 `boxShadow: inset 2px 0 0 var(--accent-primary)`).
    // 카드 좌측 가장 안쪽 모서리, 좌측 inner_margin(8px) 안에 위치 → dot 슬롯(좌측 8px 부터)
    // 과 겹치지 않는다.
    if ws.is_active {
        let bar = egui::Rect::from_min_size(card_rect.min, egui::vec2(2.0, card_rect.height()));
        ui.painter().rect_filled(bar, 0.0, th.blue);
    }

    card_rect
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
            is_mirror: false,
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
                    workspaces_heading: "WORKSPACES",
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
            is_mirror: false,
            is_active: true,
        }];
        let actions = run_collapsed(ws);
        assert!(actions.is_empty());
    }
}
