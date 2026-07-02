//! `Tab` primitive specimen — 디자인(4) `components/nav/Tab` 카드.
//!
//! 디자인 nav 섹션은 Tab 을 독립 Spec 으로 노출한다(Layouts 의 Pane Tab Bar 와
//! 별개 — 여기선 단일 탭의 *상태* 만 보인다). 본체 binary 비의존이므로 Theme 토큰
//! 만으로 strip + 탭을 painter 로 직접 그린다.
//!
//! 상태 3종: active(accent top bar + panel fill) · idle · notification(badge dot).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::StatusKind;

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

struct TabSpec {
    label: &'static str,
    status: StatusKind,
    active: bool,
    notification: bool,
}

/// StatusKind → dot 색 (위젯 내부 매핑 미러 — `StatusKind::color` 는 비공개).
fn status_color(theme: &Theme, kind: StatusKind) -> egui::Color32 {
    match kind {
        StatusKind::Running => egui::Color32::from(theme.accent_success()),
        StatusKind::Idle => egui::Color32::from(theme.text_muted()),
        StatusKind::Agent => egui::Color32::from(theme.accent_agent()),
        StatusKind::Waiting => egui::Color32::from(theme.accent_warning()),
        StatusKind::Error => egui::Color32::from(theme.accent_danger()),
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        let strip_h = theme.item_height_tab.value();
        let tab_w = theme.tab_width.value();
        let tabs = [
            TabSpec {
                label: "build.sh",
                status: StatusKind::Running,
                active: true,
                notification: false,
            },
            TabSpec {
                label: "README.md",
                status: StatusKind::Idle,
                active: false,
                notification: false,
            },
            TabSpec {
                label: "server.log",
                status: StatusKind::Idle,
                active: false,
                notification: true,
            },
        ];
        let strip_w = tab_w * tabs.len() as f32;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(strip_w, strip_h), egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // strip 배경 = bg-sidebar, 하단 separator.
        painter.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
        painter.hline(
            rect.x_range(),
            rect.bottom(),
            egui::Stroke::new(
                theme.border_width.value(),
                egui::Color32::from(theme.separator),
            ),
        );

        for (i, tab) in tabs.iter().enumerate() {
            let x = rect.left() + tab_w * i as f32;
            let tab_rect =
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(tab_w, strip_h));
            draw_tab(&painter, theme, tab_rect, tab);
            // 탭 사이 separator.
            if i > 0 {
                painter.vline(
                    x,
                    tab_rect.y_range(),
                    egui::Stroke::new(
                        theme.border_width.value(),
                        egui::Color32::from(theme.separator),
                    ),
                );
            }
        }
    });

    meta(
        ui,
        theme,
        &[
            ("height", "24 control-height-tab"),
            ("width", "150 tab-width"),
            ("active", "accent top bar + panel fill"),
            ("close", "hover-revealed"),
        ],
        &[
            TokenChip::new(
                "bg-panel",
                "active fill",
                egui::Color32::from(theme.bg_panel()),
            ),
            TokenChip::new(
                "accent-primary",
                "top bar",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::new(
                "separator",
                "tab divider",
                egui::Color32::from(theme.separator),
            ),
        ],
    );
}

fn draw_tab(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, tab: &TabSpec) {
    if tab.active {
        // panel fill + accent top bar.
        painter.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_panel()));
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), theme.tab_indicator_width.value()),
        );
        painter.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
    }

    let pad = theme.spacing_sm.value();
    let dot_r = theme.status_dot_size.value() * 0.5;
    let cy = rect.center().y;

    // status dot.
    let dot_x = rect.left() + pad + dot_r;
    painter.circle_filled(
        egui::pos2(dot_x, cy),
        dot_r,
        status_color(theme, tab.status),
    );

    // label.
    let label_color = if tab.active {
        egui::Color32::from(theme.text_primary())
    } else {
        egui::Color32::from(theme.text_muted())
    };
    painter.text(
        egui::pos2(dot_x + dot_r + pad, cy),
        egui::Align2::LEFT_CENTER,
        tab.label,
        egui::FontId::proportional(theme.font_size_body.value()),
        label_color,
    );

    // 우측: notification badge dot 또는 close affordance(hover-revealed → 정적은 생략).
    if tab.notification {
        let nx = rect.right() - pad - dot_r;
        painter.circle_filled(
            egui::pos2(nx, cy),
            dot_r,
            egui::Color32::from(theme.accent_agent()),
        );
    }
}
