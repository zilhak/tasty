//! 테마와 무관한 브랜드 색상과 수박·tasty. 워드마크를 그린다.
//! 본체와 갤러리가 같은 함수를 사용하며 크기·배치는 호출자가 Theme 값으로 지정한다.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// 수박 과육 (워드마크 `tasty.` 의 `.`, 로고 flesh).
#[allow(clippy::disallowed_methods)] // 모듈 doc 참고 — 브랜드 정체성 색은 의도된 예외.
pub const MELON_FLESH: HexColor = HexColor::from_rgb(0xf2, 0x5d, 0x6b);

/// 본체와 갤러리가 공유하는 로고 PNG. 호스트에 egui_extras 이미지 로더가 설치돼 있어야 한다.
pub const LOGO_PNG: &[u8] = include_bytes!("../../../assets/icons/icon_256.png");
/// 위 PNG 의 egui bytes-loader URI(캐시 키). 본체·갤러리 공통.
pub const LOGO_URI: &str = "bytes://tasty_brand_logo_256.png";

/// 수박 마크와 tasty. 글자를 가로로 배치한다. 호출자가 크기와 정렬 위치를 정한다.
pub fn draw_wordmark(ui: &mut egui::Ui, theme: &Theme, icon_size: LogicalPx, font_size: LogicalPx) {
    let icon_vec = egui::vec2(icon_size.value(), icon_size.value());
    let gap = theme.spacing_sm.value();

    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(font_size.value());
    job.append(
        "tasty",
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            extra_letter_spacing: -0.5,
            color: theme.text_primary().into(),
            ..Default::default()
        },
    );
    job.append(
        ".",
        0.0,
        egui::TextFormat {
            font_id: font,
            extra_letter_spacing: -0.5,
            color: MELON_FLESH.into(),
            ..Default::default()
        },
    );
    // 전체 가용 폭 대신 내용의 실제 폭만 할당해야 부모의 중앙 정렬을 그대로 사용할 수 있다.
    let galley = ui.fonts(|f| f.layout_job(job));
    // label 앞의 item_spacing도 포함해 부모가 정렬하는 폭과 실제 폭을 맞춘다.
    let item_spacing = ui.spacing().item_spacing.x;
    let content = egui::vec2(
        icon_vec.x + gap + item_spacing + galley.size().x,
        icon_vec.y.max(galley.size().y),
    );
    ui.allocate_ui_with_layout(
        content,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let (icon_rect, _) = ui.allocate_exact_size(icon_vec, egui::Sense::hover());
            egui::Image::from_bytes(LOGO_URI, LOGO_PNG)
                .fit_to_exact_size(icon_vec)
                .paint_at(ui, icon_rect);
            ui.add_space(gap);
            ui.label(galley);
        },
    );
}
