//! 분할선의 가로·세로·호버 상태를 정적으로 비교한다.
//! 가는 선보다 넓은 드래그 영역을 색으로 보여주며 실제 크기 조절은 실행하지 않는다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 한 분할 데모 — 두 pane + 가운데 divider(선 + hit-band).
/// `vertical=true` 면 좌우 분할(col-resize), false 면 상하 분할(row-resize).
/// `hover=true` 면 선을 accent-primary 로, 아니면 separator 로 그린다.
fn split(ui: &mut egui::Ui, theme: &Theme, vertical: bool, hover: bool) {
    let w = theme.field_width_lg.value();
    let h = theme.spacing_xl.value() * 5.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);

    let pane = egui::Color32::from(theme.bg_panel());
    let line = if hover {
        egui::Color32::from(theme.accent_primary())
    } else {
        egui::Color32::from(theme.separator)
    };
    const HIT_BAND_ALPHA: u8 = 36;
    let band = theme.accent_primary().with_alpha(HIT_BAND_ALPHA).to_egui();
    let band_w = theme.spacing_sm.value(); // ~8 hit-band
    let line_w = theme.border_width.value();

    p.rect_filled(rect, theme.corner_radius_sm.value(), pane);
    if vertical {
        let cx = rect.center().x;
        let band_rect = egui::Rect::from_center_size(
            egui::pos2(cx, rect.center().y),
            egui::vec2(band_w, rect.height()),
        );
        p.rect_filled(band_rect, 0.0, band);
        p.vline(cx, rect.y_range(), egui::Stroke::new(line_w, line));
    } else {
        let cy = rect.center().y;
        let band_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, cy),
            egui::vec2(rect.width(), band_w),
        );
        p.rect_filled(band_rect, 0.0, band);
        p.hline(rect.x_range(), cy, egui::Stroke::new(line_w, line));
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "vertical · col-resize", |ui| {
            split(ui, theme, true, false);
        });
        spec::cluster(ui, theme, "horizontal · row-resize", |ui| {
            split(ui, theme, false, false);
        });
        spec::cluster(ui, theme, "hover → accent-primary", |ui| {
            split(ui, theme, true, true);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("line", "1px separator"),
            ("hit-band", "~7px (8 token)"),
            ("hover", "line → accent-primary"),
            ("cursor", "col-resize / row-resize"),
            ("axes", "vertical + horizontal"),
        ],
        &[
            TokenChip::new("separator", "idle line", theme.separator.into()),
            TokenChip::new(
                "accent-primary",
                "hover line + band",
                theme.accent_primary().into(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "선은 1px 이지만 잡는 영역은 ~7px hit-band — 정밀하게 겨냥하지 않아도 \
         리사이즈가 시작된다. 양축 모두 같은 규칙.",
    );
}
