//! Typography 데모: UI type scale (micro~max) + markdown prose + terminal scale,
//! 그리고 weight 롤. line-height / letter-spacing 은 egui Label 이 직접 제어를
//! 노출하지 않아 토큰 값만 표기(아래 주석 섹션).

use tasty_type_appearance::theme::Theme;

const SAMPLE: &str = "The quick brown fox jumps over the lazy dog";
const SAMPLE_KO: &str = "다람쥐 헌 쳇바퀴에 타고파";

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            section(ui, theme, "UI type scale");
            row(ui, "micro", theme.font_size_micro.value(), theme);
            row(ui, "caption", theme.font_size_caption.value(), theme);
            row(ui, "body", theme.font_size_body.value(), theme);
            row(ui, "heading (body size + 600)", theme.font_size_heading.value(), theme);
            row(ui, "max (UI cap)", theme.font_size_max.value(), theme);

            section(ui, theme, "Markdown prose — rendered content, exempt from the UI cap");
            row(ui, "prose-h1", theme.font_size_prose_h1.value(), theme);
            row(ui, "prose-h2", theme.font_size_prose_h2.value(), theme);

            section(ui, theme, "Terminal scale");
            row(ui, "term-sm", theme.font_size_term_sm.value(), theme);
            row(ui, "term", theme.font_size_term.value(), theme);
            row(ui, "term-lg", theme.font_size_term_lg.value(), theme);

            section(ui, theme, "Monospace (body size)");
            ui.label(
                egui::RichText::new(SAMPLE)
                    .monospace()
                    .size(theme.font_size_body.value()),
            );

            section(ui, theme, "Font weight");
            ui.label(
                egui::RichText::new(format!("{SAMPLE} — normal (400)"))
                    .size(theme.font_size_body.value()),
            );
            ui.label(
                egui::RichText::new(format!("{SAMPLE} — strong / semibold (600)"))
                    .strong()
                    .size(theme.font_size_body.value()),
            );
            note(
                ui,
                theme,
                "egui RichText 는 normal / strong(bold) 만 노출 — medium(500)·bold(700) 세분화는 미지원.",
            );

            section(ui, theme, "Line-height / letter-spacing");
            note(
                ui,
                theme,
                "line-height(tight 1.0 / term 1.2 / ui 1.4 / prose 1.6) 와 \
                 letter-spacing(ui 0 / caps 0.04em) 은 egui Label 이 직접 제어를 \
                 노출하지 않아 토큰 값만 기록한다(터미널 surface 는 자체 셰이더로 행간 제어).",
            );
        });
}

fn section(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
}

fn note(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .small()
            .italics()
            .color(egui::Color32::from(theme.subtext0)),
    );
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
