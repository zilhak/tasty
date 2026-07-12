//! 브랜드 정체성 색·워드마크 락업 — 테마(Mocha/Latte)와 무관한 고정값.
//!
//! 디자인 시스템 `tokens/colors.css` 의 `--brand-melon-*` 미러. 테마 전환에도
//! 바뀌지 않는 *정체성* 색이라 `Theme` 색이 아니라 const 로 둔다 (테마 색
//! 하드코딩 금지 정책의 대상이 아니다 — 이건 테마 색이 아니다).

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

use crate::model::LogicalPx;

/// 수박 과육 (워드마크 `tasty.` 의 `.`, 로고 flesh).
#[allow(clippy::disallowed_methods)] // 모듈 doc 참고 — 브랜드 정체성 색은 의도된 예외.
pub const MELON_FLESH: HexColor = HexColor::from_rgb(0xf2, 0x5d, 0x6b);

/// 워드마크 락업의 수박 마크 PNG — 사이드바 헤더(collapsed 아이콘 포함)·부팅
/// 로딩 화면이 공유하는 단일 소스. PNG 디코딩에는 `egui_extras` 의 `image`
/// feature 가 필요하다(`egui_extras::install_image_loaders` 가 `GpuState::new`
/// 에서 설치).
pub(crate) const LOGO_PNG: &[u8] = include_bytes!("../../../assets/icons/icon_256.png");
pub(crate) const LOGO_URI: &str = "bytes://tasty_brand_logo_256.png";

/// 워드마크 락업 그리기 — 수박 마크 + `tasty.` mono(`.` 는 `MELON_FLESH`). 가로
/// 배치, 마크·텍스트 gap 은 `theme.spacing_sm`. 호출부가 배치(centering 등)와
/// 크기만 결정한다 — sidebar 헤더(22px 마크 + 17px 텍스트)와 부팅 로딩 화면
/// (64px + 38px, 브랜드 락업 sanctioned 14px 예외)이 공유하는 단일 소스.
pub fn draw_wordmark(ui: &mut egui::Ui, theme: &Theme, icon_size: LogicalPx, font_size: LogicalPx) {
    ui.horizontal(|ui| {
        let icon_vec = egui::vec2(icon_size.value(), icon_size.value());
        let (icon_rect, _) = ui.allocate_exact_size(icon_vec, egui::Sense::hover());
        egui::Image::from_bytes(LOGO_URI, LOGO_PNG)
            .fit_to_exact_size(icon_vec)
            .paint_at(ui, icon_rect);
        ui.add_space(theme.spacing_sm.value());
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
        ui.label(job);
    });
}
