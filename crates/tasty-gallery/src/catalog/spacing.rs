//! Spacing 데모: xs / sm / md / lg / xl 간격을 사각형 갭으로 시각화.

use tasty_type_appearance::theme::Theme;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            row(ui, "spacing_xs", theme.spacing_xs.value(), theme);
            row(ui, "spacing_sm", theme.spacing_sm.value(), theme);
            row(ui, "spacing_md", theme.spacing_md.value(), theme);
            row(ui, "spacing_lg", theme.spacing_lg.value(), theme);
            row(ui, "spacing_xl", theme.spacing_xl.value(), theme);
        });
}

fn row(ui: &mut egui::Ui, name: &str, gap: f32, theme: &Theme) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(format!("{name} — {gap:.0}px"))
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    let box_size = egui::vec2(48.0, 24.0);
    let total_w = box_size.x * 3.0 + gap * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, box_size.y), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // 두 블록 + 갭. 가운데에 4px grid 격자 가이드를 옅게.
    paint_grid(&painter, rect, theme);

    for i in 0..3 {
        let x = rect.left() + (box_size.x + gap) * i as f32;
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.top()), box_size);
        painter.rect_filled(r, 2.0, egui::Color32::from(theme.blue));
    }
}

fn paint_grid(painter: &egui::Painter, rect: egui::Rect, theme: &Theme) {
    let step = 4.0;
    let color = egui::Color32::from(theme.surface0);
    let mut x = rect.left();
    while x <= rect.right() {
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, color),
        );
        x += step;
    }
}
