//! Gallery specimen 공통 헬퍼.
//!
//! catalog 의 여러 specimen 모듈에서 반복되던 라벨 helper 를 한곳에 모은다.
//! 본체 (`crate tasty`) 에 의존하지 않고, 색·폰트·치수는 모두 `Theme` 토큰을
//! 그대로 사용한다 (시각 무변경 dedup).

use tasty_type_appearance::theme::Theme;

/// specimen 케이스 위 caption(부가 설명) 라벨 — caption 폰트 크기 + subtext0 색.
pub fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(egui::Color32::from(theme.subtext0)),
    );
}

/// specimen 케이스 제목(강조) 라벨 — strong + text 색, 아래 2px 간격.
pub fn case_title(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(2.0);
}
