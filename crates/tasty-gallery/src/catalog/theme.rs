//! Theme 색상 토큰 전체 스와치.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

const SWATCH_W: f32 = 88.0;
const SWATCH_H: f32 = 56.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.heading("Surface");
            grid(
                ui,
                &[
                    ("crust", theme.crust),
                    ("mantle", theme.mantle),
                    ("base", theme.base),
                    ("surface0", theme.surface0),
                    ("surface1", theme.surface1),
                    ("surface2", theme.surface2),
                    ("overlay0", theme.overlay0),
                    ("overlay1", theme.overlay1),
                    ("overlay2", theme.overlay2),
                ],
            );

            ui.add_space(12.0);
            ui.heading("Text");
            grid(
                ui,
                &[
                    ("text", theme.text),
                    ("subtext1", theme.subtext1),
                    ("subtext0", theme.subtext0),
                    ("placeholder", theme.placeholder),
                ],
            );

            ui.add_space(12.0);
            ui.heading("Accent");
            grid(
                ui,
                &[
                    ("blue", theme.blue),
                    ("green", theme.green),
                    ("red", theme.red),
                    ("yellow", theme.yellow),
                    ("peach", theme.peach),
                    ("mauve", theme.mauve),
                    ("teal", theme.teal),
                    ("sky", theme.sky),
                    ("lavender", theme.lavender),
                    ("flamingo", theme.flamingo),
                    ("pink", theme.pink),
                    ("maroon", theme.maroon),
                    ("rosewater", theme.rosewater),
                ],
            );

            ui.add_space(12.0);
            ui.heading("Semantic");
            grid(
                ui,
                &[
                    ("selection_bg", theme.selection_bg),
                    ("vi_cursor_bg", theme.vi_cursor_bg),
                    ("search_match_bg", theme.search_match_bg),
                    ("search_match_active_bg", theme.search_match_active_bg),
                ],
            );

            ui.add_space(12.0);
            ui.heading("ANSI (8)");
            grid(
                ui,
                &[
                    ("ansi_black", theme.ansi_black),
                    ("ansi_red", theme.ansi_red),
                    ("ansi_green", theme.ansi_green),
                    ("ansi_yellow", theme.ansi_yellow),
                    ("ansi_blue", theme.ansi_blue),
                    ("ansi_magenta", theme.ansi_magenta),
                    ("ansi_cyan", theme.ansi_cyan),
                    ("ansi_white", theme.ansi_white),
                ],
            );

            ui.add_space(8.0);
            ui.heading("ANSI Bright (8)");
            grid(
                ui,
                &[
                    ("ansi_bright_black", theme.ansi_bright_black),
                    ("ansi_bright_red", theme.ansi_bright_red),
                    ("ansi_bright_green", theme.ansi_bright_green),
                    ("ansi_bright_yellow", theme.ansi_bright_yellow),
                    ("ansi_bright_blue", theme.ansi_bright_blue),
                    ("ansi_bright_magenta", theme.ansi_bright_magenta),
                    ("ansi_bright_cyan", theme.ansi_bright_cyan),
                    ("ansi_bright_white", theme.ansi_bright_white),
                ],
            );
        });
}

fn grid(ui: &mut egui::Ui, items: &[(&str, HexColor)]) {
    let avail = ui.available_width().max(SWATCH_W);
    let per_row = ((avail / (SWATCH_W + 8.0)).floor() as usize).max(1);
    for chunk in items.chunks(per_row) {
        ui.horizontal(|ui| {
            for (name, c) in chunk {
                draw_swatch(ui, name, *c);
            }
        });
    }
}

fn draw_swatch(ui: &mut egui::Ui, name: &str, color: HexColor) {
    let (rect, _resp) =
        ui.allocate_exact_size(egui::vec2(SWATCH_W, SWATCH_H), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from(color));
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_black_alpha(40)),
        egui::StrokeKind::Inside,
    );

    let hex = format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
    let label = format!("{name}\n{hex}");
    painter.text(
        rect.left_top() + egui::vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(10.0),
        readable_on(color),
    );
}

/// 스와치 위 텍스트가 읽히도록 배경 휘도에 따라 흰/검 선택.
fn readable_on(c: HexColor) -> egui::Color32 {
    let luma = 0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32;
    if luma > 140.0 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    }
}
