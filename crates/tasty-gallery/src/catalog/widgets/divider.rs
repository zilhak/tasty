//! `divider` specimen — Pane divider (research §2.5 Layouts).
//!
//! 분할된 pane 사이의 리사이즈 핸들. 1px `separator` 선 + 그 위에 얹힌
//! ~7px hit-band(드래그 잡는 영역). 포인터가 band 위에 오면 선이 accent-primary 로
//! 바뀌고 커서가 col/row-resize 로 전환된다. 가로·세로 양축 동일.
//!
//! 본체 `src/adapters/ui/divider.rs::draw_pane_dividers` 의 시각 패턴을 Theme
//! 토큰만으로 재현 (binary 미의존).

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 한 분할 데모 — 두 pane + 가운데 divider(선 + hit-band).
/// `vertical=true` 면 좌우 분할(col-resize), false 면 상하 분할(row-resize).
/// `hover=true` 면 선을 accent-primary 로, 아니면 separator 로 그린다.
fn split(ui: &mut egui::Ui, theme: &Theme, vertical: bool, hover: bool) {
    // 캔버스: 폭 field_width_lg(200), 높이 spacing_xl×5(120) — 디자인 데모 비율.
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
    // hit-band: accent-primary 저알파 tint (드래그 영역 가시화).
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
