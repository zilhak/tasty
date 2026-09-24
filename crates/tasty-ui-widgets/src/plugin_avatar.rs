//! 플러그인 이름의 첫 글자로 사각 아바타를 그린다.
//! 매니페스트에 카테고리가 없어 기본 강조색을 사용한다.
//! 현재 목록은 font_size_body, 상세는 font_size_max를 사용한다.
//! 디자인의 별도 아바타 글꼴 토큰은 아직 적용하지 않았으며 이 파일에서 임의로 바꾸지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::tokens::{PLUGIN_AVATAR_DETAIL_SIZE, PLUGIN_AVATAR_ROW_SIZE};

/// 아바타가 표시되는 위치에 따라 크기와 글꼴을 함께 선택한다.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PluginAvatarSize {
    /// 목록 행 왼쪽 (디자인 `size={32}`).
    Row,
    /// 상세 identity 블록 왼쪽 (디자인 `size={46}`).
    Detail,
}

impl PluginAvatarSize {
    /// 사각형 한 변.
    #[inline]
    pub fn side(self) -> LogicalPx {
        match self {
            PluginAvatarSize::Row => PLUGIN_AVATAR_ROW_SIZE,
            PluginAvatarSize::Detail => PLUGIN_AVATAR_DETAIL_SIZE,
        }
    }

    /// 목록과 상세 위치에 맞는 글꼴 크기.
    #[inline]
    fn font(self, theme: &Theme) -> LogicalPx {
        match self {
            PluginAvatarSize::Row => theme.font_size_body,
            PluginAvatarSize::Detail => theme.font_size_max,
        }
    }
}

/// 이름의 머리글자 한 자를 대문자로. 빈 이름이면 빈 문자열 — 그때는 글리프 없이
/// 배경/보더만 그린다(디자인의 `charAt(0)` 도 빈 문자열을 낸다).
fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

/// 아바타를 `center` 기준으로 그린다. 행을 painter 로 직접 그리는 자리(목록)용.
pub fn paint_plugin_avatar(
    painter: &egui::Painter,
    theme: &Theme,
    center: egui::Pos2,
    name: &str,
    size: PluginAvatarSize,
) {
    let side = size.side().value();
    let rect = egui::Rect::from_center_size(center, egui::vec2(side, side));
    let radius = theme.corner_radius.value();
    painter.rect(
        rect,
        radius,
        theme.plugin_avatar_bg().to_egui(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.plugin_avatar_border().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
    let label = initial(name);
    if label.is_empty() {
        return;
    }
    // 별도 굵은 글꼴을 등록하지 않아 굵기는 재현하지 않는다.
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(size.font(theme).value()),
        theme.accent_primary().to_egui(),
    );
}

/// 아바타 한 칸을 `ui` 에 할당하고 그린다. 상세 identity 처럼 레이아웃에 얹는 자리용.
pub fn plugin_avatar(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    size: PluginAvatarSize,
) -> egui::Response {
    let side = size.side().value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    paint_plugin_avatar(ui.painter(), theme, rect.center(), name, size);
    resp
}
