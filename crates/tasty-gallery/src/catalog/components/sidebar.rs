//! Sidebar (Full / Collapsed) 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/sidebar/view.rs::draw_{full,collapsed}_sidebar_view`
//! 가 표현하는 시각 상태를 mock props 로 재현. 본체와 *시각 동일* 하지만
//! gallery 가 본체 binary 에 의존할 수 없으므로 view 로직은 로컬 미러
//! (POC 패턴 — `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 카탈로그 한 항목 안에서 Full ↔ Collapsed 모드 토글 + workspace 다양화.
//! 매 프레임 호출되는 catalog `draw` 함수는 stateless 라 mode 토글은 마지막
//! 클릭 시점의 thread-local 상태로 보존.
//!
//! 대표 상태:
//! - Full / 1 workspace
//! - Full / 5 workspaces (스크롤 + busy/highlight/attached 인디케이터)
//! - Full / 매우 긴 이름 + subtitle/description
//! - Collapsed / 5 workspaces
//! - Collapsed / busy/attached/highlight 인디케이터

use std::cell::RefCell;
use tasty_type_appearance::theme::Theme;

#[derive(Debug, Clone)]
struct WorkspaceEntryView {
    name: String,
    subtitle: String,
    description: String,
    busy_count: usize,
    has_highlight: bool,
    attached: bool,
    is_active: bool,
}

struct SidebarFullProps<'a> {
    theme: &'a Theme,
    workspaces: &'a [WorkspaceEntryView],
    tools_label: &'a str,
    collapse_label: &'a str,
    plugins_label: &'a str,
    settings_label: &'a str,
    new_workspace_label: &'a str,
    occupied_hover: &'a str,
}

struct SidebarCollapsedProps<'a> {
    theme: &'a Theme,
    workspaces: &'a [WorkspaceEntryView],
    tools_hover: &'a str,
}

const BTN_HEIGHT: f32 = 28.0;
const COLLAPSED_ICON_SIZE: egui::Vec2 = egui::vec2(32.0, 22.0);
const COLLAPSED_WS_SIZE: egui::Vec2 = egui::vec2(32.0, 28.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarMode {
    Full,
    Collapsed,
}

thread_local! {
    static CURRENT_MODE: RefCell<SidebarMode> = const { RefCell::new(SidebarMode::Full) };
}

/// Full sidebar view 미러 — embedded box. SidePanel 없이 sized frame 안에 그려
/// 카탈로그 패널에 맞춘다.
fn draw_full_sidebar_box(ui: &mut egui::Ui, props: &SidebarFullProps<'_>) {
    let th = props.theme;
    let width = 220.0;
    let height = 320.0;

    egui::Frame::new()
        .fill(egui::Color32::from(th.base))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from(th.surface0)))
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(width, height));
            ui.set_max_size(egui::vec2(width, height));

            // 바닥 버튼 4 개를 먼저 그려 시각 위계 유지.
            egui::TopBottomPanel::bottom("gallery_full_bottom")
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show_inside(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.separator();
                    ui.add_space(2.0);
                    full_bottom_button(ui, th, props.tools_label);
                    ui.add_space(2.0);
                    full_bottom_button(ui, th, props.collapse_label);
                    ui.add_space(2.0);
                    full_bottom_button(ui, th, props.plugins_label);
                    ui.add_space(2.0);
                    full_bottom_button(ui, th, props.settings_label);
                    ui.add_space(8.0);
                });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    for ws in props.workspaces {
                        draw_workspace_card(ui, th, ws, props.occupied_hover);
                        ui.add_space(2.0);
                    }
                    ui.add_space(4.0);
                    let full_width = ui.available_width();
                    // 카탈로그는 시각 검증 전용 — 버튼 click response 무시.
                    ui.add_sized(
                        [full_width, BTN_HEIGHT],
                        egui::Button::new(props.new_workspace_label),
                    );
                    ui.add_space(4.0);
                });
        });
}

fn draw_collapsed_sidebar_box(ui: &mut egui::Ui, props: &SidebarCollapsedProps<'_>) {
    let th = props.theme;
    let width = 56.0;
    let height = 320.0;

    egui::Frame::new()
        .fill(egui::Color32::from(th.base))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from(th.surface0)))
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(width, height));
            ui.set_max_size(egui::vec2(width, height));

            egui::TopBottomPanel::bottom("gallery_collapsed_bottom")
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show_inside(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.separator();
                        ui.add_space(2.0);
                        collapsed_icon(ui, th, "T", 12.0).on_hover_text(props.tools_hover);
                        ui.add_space(2.0);
                        collapsed_icon(ui, th, ">", 14.0);
                        ui.add_space(2.0);
                        collapsed_icon(ui, th, "\u{1F9E9}", 14.0);
                        ui.add_space(2.0);
                        collapsed_icon(ui, th, "\u{2699}", 14.0);
                        ui.add_space(12.0);
                    });
                });

            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                for (i, ws) in props.workspaces.iter().enumerate() {
                    draw_collapsed_ws(ui, th, ws, i + 1);
                }
                ui.add_space(2.0);
                collapsed_icon(ui, th, "+", 14.0);
            });
        });
}

fn full_bottom_button(ui: &mut egui::Ui, th: &Theme, label: &str) {
    let full_width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(full_width, BTN_HEIGHT),
        egui::Sense::click().union(egui::Sense::hover()),
    );
    if resp.hovered() {
        let hover = egui::Color32::from(th.surface1);
        ui.painter().rect_filled(rect, 4.0, hover);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        if resp.hovered() {
            egui::Color32::from(th.subtext1)
        } else {
            egui::Color32::from(th.overlay0)
        },
    );
}

fn collapsed_icon(ui: &mut egui::Ui, th: &Theme, glyph: &str, font_size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(COLLAPSED_ICON_SIZE, egui::Sense::click());
    if resp.hovered() {
        let hover = egui::Color32::from(th.surface1);
        ui.painter().rect_filled(rect, 4.0, hover);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(font_size),
        if resp.hovered() {
            egui::Color32::from(th.subtext1)
        } else {
            egui::Color32::from(th.overlay0)
        },
    );
    resp
}

fn draw_workspace_card(
    ui: &mut egui::Ui,
    th: &Theme,
    ws: &WorkspaceEntryView,
    occupied_hover: &str,
) {
    let bg = if ws.is_active {
        egui::Color32::from(th.surface0)
    } else {
        egui::Color32::TRANSPARENT
    };
    let border = if ws.is_active {
        egui::Color32::from(th.accent_primary())
    } else {
        egui::Color32::from(th.surface0)
    };

    let frame = egui::Frame::new()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 6));

    frame.show(ui, |ui| {
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
                    ui.painter().circle_filled(
                        dot_rect.center(),
                        dot_radius,
                        egui::Color32::from(th.accent_danger()),
                    );
                    resp.on_hover_text(occupied_hover);
                }

                if ws.has_highlight {
                    let badge_size = egui::vec2(18.0, 16.0);
                    let (rect, _) = ui.allocate_exact_size(badge_size, egui::Sense::hover());
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        egui::Stroke::new(1.0, egui::Color32::from(th.accent_primary())),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "!",
                        egui::FontId::proportional(10.0),
                        egui::Color32::from(th.accent_primary()),
                    );
                }

                if ws.busy_count > 0 {
                    let count_text = format!("{}", ws.busy_count);
                    ui.label(
                        egui::RichText::new(&count_text)
                            .small()
                            .color(egui::Color32::from(th.accent_success())),
                    );
                    let dot_radius = 3.0;
                    let (dot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(dot_radius * 2.0 + 2.0, dot_radius * 2.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().circle_filled(
                        dot_rect.center(),
                        dot_radius,
                        egui::Color32::from(th.accent_success()),
                    );
                }
            });
        });

        if !ws.subtitle.is_empty() {
            ui.label(
                egui::RichText::new(&ws.subtitle)
                    .small()
                    .color(egui::Color32::from(th.subtext0)),
            );
        }

        if !ws.description.is_empty() {
            ui.label(
                egui::RichText::new(&ws.description)
                    .small()
                    .color(egui::Color32::from(th.overlay0)),
            );
        }
    });
}

fn draw_collapsed_ws(ui: &mut egui::Ui, th: &Theme, ws: &WorkspaceEntryView, number: usize) {
    let label = format!("{number}");
    let bg = if ws.is_active {
        egui::Color32::from(th.surface0)
    } else {
        egui::Color32::from(th.mantle)
    };
    let text_color = if ws.is_active {
        egui::Color32::from(th.text)
    } else if ws.has_highlight {
        egui::Color32::from(th.yellow)
    } else {
        egui::Color32::from(th.subtext0)
    };

    let (rect, resp) = ui.allocate_exact_size(COLLAPSED_WS_SIZE, egui::Sense::click());
    ui.painter().rect_filled(rect, 4.0, bg);
    if ws.is_active {
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, egui::Color32::from(th.accent_primary())),
            egui::StrokeKind::Inside,
        );
    }
    if resp.hovered() {
        let hover = egui::Color32::from(th.surface1);
        ui.painter().rect_filled(rect, 4.0, hover);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        &label,
        egui::FontId::proportional(12.0),
        text_color,
    );
    if ws.busy_count > 0 {
        let dot_radius = 3.0;
        let dot_pad = 4.0;
        let dot_center = egui::pos2(
            rect.max.x - dot_pad - dot_radius,
            rect.min.y + dot_pad + dot_radius,
        );
        ui.painter()
            .circle_filled(dot_center, dot_radius, egui::Color32::from(th.accent_success()));
    }
    if ws.attached {
        let dot_radius = 3.0;
        let dot_pad = 4.0;
        let dot_center = egui::pos2(
            rect.max.x - dot_pad - dot_radius,
            rect.max.y - dot_pad - dot_radius,
        );
        ui.painter()
            .circle_filled(dot_center, dot_radius, egui::Color32::from(th.accent_danger()));
    }
}

fn mock_single() -> Vec<WorkspaceEntryView> {
    vec![WorkspaceEntryView {
        name: "Default".into(),
        subtitle: String::new(),
        description: String::new(),
        busy_count: 0,
        has_highlight: false,
        attached: false,
        is_active: true,
    }]
}

fn mock_many() -> Vec<WorkspaceEntryView> {
    vec![
        WorkspaceEntryView {
            name: "main".into(),
            subtitle: "user shell".into(),
            description: String::new(),
            busy_count: 0,
            has_highlight: false,
            attached: false,
            is_active: true,
        },
        WorkspaceEntryView {
            name: "build".into(),
            subtitle: "cargo watch".into(),
            description: String::new(),
            busy_count: 2,
            has_highlight: false,
            attached: false,
            is_active: false,
        },
        WorkspaceEntryView {
            name: "docs".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 0,
            has_highlight: true,
            attached: false,
            is_active: false,
        },
        WorkspaceEntryView {
            name: "remote".into(),
            subtitle: "ssh prod-01".into(),
            description: String::new(),
            busy_count: 0,
            has_highlight: false,
            attached: true,
            is_active: false,
        },
        WorkspaceEntryView {
            name: "scratch".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 0,
            has_highlight: false,
            attached: false,
            is_active: false,
        },
    ]
}

fn mock_long_names() -> Vec<WorkspaceEntryView> {
    vec![
        WorkspaceEntryView {
            name: "Very long workspace name that probably overflows".into(),
            subtitle: "And here is a subtitle that is also pretty long".into(),
            description: "Optional description giving extra context.".into(),
            busy_count: 1,
            has_highlight: false,
            attached: false,
            is_active: true,
        },
        WorkspaceEntryView {
            name: "second".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 0,
            has_highlight: false,
            attached: false,
            is_active: false,
        },
    ]
}

fn mock_indicators() -> Vec<WorkspaceEntryView> {
    vec![
        WorkspaceEntryView {
            name: "a".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 5,
            has_highlight: false,
            attached: false,
            is_active: true,
        },
        WorkspaceEntryView {
            name: "b".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 0,
            has_highlight: true,
            attached: false,
            is_active: false,
        },
        WorkspaceEntryView {
            name: "c".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 0,
            has_highlight: false,
            attached: true,
            is_active: false,
        },
        WorkspaceEntryView {
            name: "d".into(),
            subtitle: String::new(),
            description: String::new(),
            busy_count: 3,
            has_highlight: true,
            attached: true,
            is_active: false,
        },
    ]
}

fn case_title(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(2.0);
}

fn full_props<'a>(theme: &'a Theme, workspaces: &'a [WorkspaceEntryView]) -> SidebarFullProps<'a> {
    SidebarFullProps {
        theme,
        workspaces,
        tools_label: "Tools",
        collapse_label: "<  Collapse",
        plugins_label: "Plugins",
        settings_label: "Settings",
        new_workspace_label: "+ New workspace",
        occupied_hover: "Held by another client",
    }
}

fn collapsed_props<'a>(
    theme: &'a Theme,
    workspaces: &'a [WorkspaceEntryView],
) -> SidebarCollapsedProps<'a> {
    SidebarCollapsedProps {
        theme,
        workspaces,
        tools_hover: "Tools menu",
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "draw_full_sidebar_view / draw_collapsed_sidebar_view — workspace navigation",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Wrappers: src/adapters/ui/sidebar/{full,collapsed}.rs::draw_{full,collapsed}_sidebar",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    // Mode toggle.
    let mut mode = CURRENT_MODE.with(|m| *m.borrow());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Mode:").color(egui::Color32::from(theme.subtext0)));
        if ui
            .selectable_label(mode == SidebarMode::Full, "Full")
            .clicked()
        {
            mode = SidebarMode::Full;
        }
        if ui
            .selectable_label(mode == SidebarMode::Collapsed, "Collapsed")
            .clicked()
        {
            mode = SidebarMode::Collapsed;
        }
    });
    CURRENT_MODE.with(|m| *m.borrow_mut() = mode);
    ui.add_space(12.0);

    match mode {
        SidebarMode::Full => {
            case_title(ui, theme, "Case 1 — Single workspace");
            let ws = mock_single();
            draw_full_sidebar_box(ui, &full_props(theme, &ws));
            ui.add_space(16.0);

            case_title(
                ui,
                theme,
                "Case 2 — Five workspaces (busy / highlight / attached indicators)",
            );
            let ws = mock_many();
            draw_full_sidebar_box(ui, &full_props(theme, &ws));
            ui.add_space(16.0);

            case_title(ui, theme, "Case 3 — Long names + subtitle + description");
            let ws = mock_long_names();
            draw_full_sidebar_box(ui, &full_props(theme, &ws));
        }
        SidebarMode::Collapsed => {
            case_title(ui, theme, "Case 1 — Five workspaces");
            let ws = mock_many();
            draw_collapsed_sidebar_box(ui, &collapsed_props(theme, &ws));
            ui.add_space(16.0);

            case_title(
                ui,
                theme,
                "Case 2 — Mixed indicators (busy / highlight / attached / combined)",
            );
            let ws = mock_indicators();
            draw_collapsed_sidebar_box(ui, &collapsed_props(theme, &ws));
        }
    }

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "Note: hover overlay 색은 본체의 theme.hover_overlay (premultiplied) 대신 \
             surface1 로 미러. 본체 wrapper 는 workspace tree + attach holder + busy \
             count + notification highlight 를 매 프레임 snapshot 으로 만들어 view 에 \
             넘기고, action 을 state.switch_workspace / Intent::NewWorkspace / \
             state.dialogs.ws_drag 로 번역한다.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
