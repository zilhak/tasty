//! 브랜드 정체성 색·워드마크 락업 — 테마(Mocha/Latte)와 무관한 고정값.
//!
//! 디자인 시스템 `tokens/colors.css` 의 `--brand-melon-*` 미러. 테마 전환에도
//! 바뀌지 않는 *정체성* 색이라 `Theme` 색이 아니라 const 로 둔다 (테마 색
//! 하드코딩 금지 정책의 대상이 아니다 — 이건 테마 색이 아니다).
//!
//! **단일 출처.** 부팅/종료 로딩 화면(본체 `src/gfx/gpu/loading.rs`)과 그 갤러리
//! specimen(`tasty-gallery` `chrome_loading`), 사이드바 헤더가 모두 이 모듈의
//! [`draw_wordmark`] 을 쓴다 — **형상이 한 벌**이라는 것이 이 모듈의 요점이고,
//! 크기는 호출부가 `Theme` 에서 읽어 넘긴다(로딩 화면 `loading_screen_*`,
//! 사이드바 `sidebar_logo_size`/`sidebar_wordmark_font_size`). 예전에는 본체 `src/adapters/ui/brand.rs` 와 갤러리가 같은 상수·워드마크
//! 렌더를 각자 복제했으나(크레이트 경계로 본체 bin 을 갤러리가 못 부름), 상태
//! 의존이 없는 순수 view 라 위젯 크레이트로 승격해 복제를 없앴다 (근거·절차:
//! `docs/dev-guide/gallery-first.md` "본체 전용 view 를 위젯 크레이트로 승격").

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// 수박 과육 (워드마크 `tasty.` 의 `.`, 로고 flesh).
#[allow(clippy::disallowed_methods)] // 모듈 doc 참고 — 브랜드 정체성 색은 의도된 예외.
pub const MELON_FLESH: HexColor = HexColor::from_rgb(0xf2, 0x5d, 0x6b);

/// 워드마크 락업의 수박 마크 PNG — 사이드바 헤더(collapsed 아이콘 포함)·부팅
/// 로딩 화면·갤러리 specimen 이 공유하는 단일 소스. PNG 디코딩에는 `egui_extras`
/// 의 `image` feature 가 필요하다(`egui_extras::install_image_loaders` 를 호스트가
/// 설치 — 본체는 `GpuState::new`, 갤러리는 자체 부트스트랩).
pub const LOGO_PNG: &[u8] = include_bytes!("../../../assets/icons/icon_256.png");
/// 위 PNG 의 egui bytes-loader URI(캐시 키). 본체·갤러리 공통.
pub const LOGO_URI: &str = "bytes://tasty_brand_logo_256.png";

// 로딩 화면 락업의 네 치수(마크 64 · 워드마크 38 · 스피너 32 · phase 슬롯 16)는
// 여기 있었으나 `Theme` 접근자 `loading_screen_*` 로 옮겼다. 값은 그대로다 — 옮긴
// 이유는 토큰 부재가 아니라 **배율**이다: 본체는 egui `zoom_factor` 를 1.0 으로 고정하고
// UI 배율을 `Theme::with_colors_and_zoom` 안에서만 적용하므로(ADR-0135), const 로 두면
// 같은 스택의 간격·문구만 커지고 이 넷은 고정된다. 소비처는 `&Theme` 를 이미 쥐고 있다.

/// 워드마크 락업 그리기 — 수박 마크 + `tasty.` mono(`.` 는 `MELON_FLESH`). 가로
/// 배치, 마크·텍스트 gap 은 `theme.spacing_sm`. 호출부가 배치(centering 등)와
/// 크기만 결정한다 — sidebar 헤더(22px 마크 + 17px 텍스트)와 부팅 로딩 화면
/// (64px + 38px, 브랜드 락업 sanctioned 14px 예외)이 공유하는 단일 소스.
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
    // 텍스트를 먼저 갤리로 확정해 락업의 실제 폭을 잰다. `ui.horizontal` 은 desired
    // width 로 가용 폭 전체를 요구해 centered 부모(`top_down(Center)`) 안에서 좌측
    // origin 에 붙어 버리므로, 내용 크기만큼만 영역을 잡아 부모가 중앙정렬(또는
    // 사이드바처럼 left_to_right 에서 좌측정렬)을 그대로 적용하게 한다.
    let galley = ui.fonts(|f| f.layout_job(job));
    // gap(add_space) + label 앞 item_spacing 을 함께 더해 desired 폭을 실제 내용 폭과
    // 일치시킨다 — 어긋나면 centered 부모가 그 차이의 절반만큼 락업을 밀어 스피너
    // 중심과 어긋난다.
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
