//! Divider 데모.
//!
//! 본체 `src/adapters/ui/divider.rs::draw_pane_dividers` 의 *시각 패턴* 재현.
//! 본 함수는 props 만 받는 view (Theme + divider rect 목록 + scale_factor) — Tier 2 확정.
//!
//! 본체 의존: 없음. `Theme.surface2` 색만 사용.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::PhysicalPx;
use tasty_type_geometry::rect::PhysicalRect;

/// 데모용 mock — 본체 PaneTree::collect_dividers 결과와 같은 형태 (`PhysicalRect` 리스트).
struct DemoCase {
    label: &'static str,
    dividers: Vec<PhysicalRect>,
}

fn cases() -> Vec<DemoCase> {
    vec![
        DemoCase {
            label: "vertical split (2 panes)",
            dividers: vec![PhysicalRect {
                x: PhysicalPx(160.0),
                y: PhysicalPx(0.0),
                width: PhysicalPx(2.0),
                height: PhysicalPx(120.0),
            }],
        },
        DemoCase {
            label: "horizontal split (2 panes)",
            dividers: vec![PhysicalRect {
                x: PhysicalPx(0.0),
                y: PhysicalPx(60.0),
                width: PhysicalPx(320.0),
                height: PhysicalPx(2.0),
            }],
        },
        DemoCase {
            label: "nested 2×2 grid",
            dividers: vec![
                PhysicalRect {
                    x: PhysicalPx(160.0),
                    y: PhysicalPx(0.0),
                    width: PhysicalPx(2.0),
                    height: PhysicalPx(120.0),
                },
                PhysicalRect {
                    x: PhysicalPx(0.0),
                    y: PhysicalPx(60.0),
                    width: PhysicalPx(320.0),
                    height: PhysicalPx(2.0),
                },
            ],
        },
    ]
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new("draw_pane_dividers(ctx, dividers: &[PhysicalRect], scale_factor)")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("border_color = theme.surface2")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    let scale_factor: f32 = 1.0;
    let border_color = egui::Color32::from(theme.surface2);
    let bg_color = egui::Color32::from(theme.surface0);
    let pane_color = egui::Color32::from(theme.surface1);

    for case in cases() {
        ui.label(
            egui::RichText::new(case.label)
                .small()
                .color(egui::Color32::from(theme.subtext0)),
        );
        ui.add_space(4.0);

        let canvas_size = egui::vec2(320.0, 120.0);
        let (canvas_rect, _) = ui.allocate_exact_size(canvas_size, egui::Sense::hover());
        let painter = ui.painter_at(canvas_rect);

        // 배경 + pane filler (divider 가 어떤 영역을 나누는지 보이게).
        painter.rect_filled(canvas_rect, 0.0, bg_color);
        painter.rect_filled(canvas_rect.shrink(2.0), 0.0, pane_color);

        // 본체와 동일 로직: divider 좌표 / scale_factor 후 painter.rect_filled.
        for div in &case.dividers {
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    canvas_rect.min.x + div.x.value() / scale_factor,
                    canvas_rect.min.y + div.y.value() / scale_factor,
                ),
                egui::vec2(
                    div.width.value() / scale_factor,
                    div.height.value() / scale_factor,
                ),
            );
            painter.rect_filled(rect, 0.0, border_color);
        }

        ui.add_space(16.0);
    }
}
