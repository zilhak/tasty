//! Typography 데모: caption / body / heading 폰트 크기 비교.

use tasty_type_appearance::theme::Theme;

const SAMPLE: &str = "The quick brown fox jumps over the lazy dog";
const SAMPLE_KO: &str = "다람쥐 헌 쳇바퀴에 타고파";

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            row(ui, "caption", theme.font_size_caption.value(), theme);
            row(ui, "body", theme.font_size_body.value(), theme);
            row(ui, "heading", theme.font_size_heading.value(), theme);

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Monospace (body size)")
                    .color(egui::Color32::from(theme.subtext0)),
            );
            ui.label(
                egui::RichText::new(SAMPLE)
                    .monospace()
                    .size(theme.font_size_body.value()),
            );
        });
}

fn row(ui: &mut egui::Ui, name: &str, size: f32, theme: &Theme) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(format!("{name} — {size:.1}px"))
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.label(egui::RichText::new(SAMPLE).size(size));
    ui.label(egui::RichText::new(SAMPLE_KO).size(size));
}
