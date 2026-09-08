//! `PluginAvatar` — plugin 이름 머리글자 사각 아바타.
//!
//! 디자인 `ui_kits/terminal/overlays/plugins_window.jsx` 의 `PluginAvatar` 전사.
//! 네 자리에서 불린다 — Installed 목록 행 · Installed 상세 identity · Attention 목록
//! 행 · Attention 상세 identity.
//!
//! ## 디자인과 갈린 두 자리 (둘 다 의도)
//!
//! **색.** 디자인은 배경·보더·글자를 `CAT_COLOR[plugin.cat]` 으로 칠하고, 그 표에 없는
//! 카테고리면 `var(--tasty-accent-primary)` 로 떨어진다. tasty 매니페스트에는 카테고리
//! 필드가 없어서(`crates/tasty-plugin-manifest/src/types.rs` 의 `Manifest`) **그
//! fallback 갈래 하나만 도달 가능**하다. 색을 인자로 받게 해 두지 않은 것은 그래서다 —
//! 부를 수 있는 값이 하나뿐인 인자는 호출부마다 같은 상수를 다시 적게 만든다.
//!
//! **글자 크기.** 디자인은 `Math.round(size * 0.42)` 다. 목록(32)에서는 13 이 나와
//! `font_size_body` 와 값이 그대로 맞지만, 상세(46)에서는 19 가 나와 **UI 폰트 상한
//! 14 를 넘는다**(`docs/design/systems/theme.md` "UI 폰트 최대"). 토큰 축은 구조 축과
//! 함께 필수라(`CLAUDE.md` "갤러리 완전성 · gallery-first") 상한 쪽을 따르고 상세
//! 글리프를 `font_size_max` 로 자른다. 비율은 0.42 → 0.30 으로 바뀐다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::tokens::{PLUGIN_AVATAR_DETAIL_SIZE, PLUGIN_AVATAR_ROW_SIZE};

/// 아바타가 앉는 자리. 한 변과 글리프 크기가 함께 정해진다 — 둘을 따로 받으면 호출부
/// 넷에서 짝이 갈린다.
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

    /// 머리글자 글리프 크기. 모듈 doc "글자 크기" 참고 — 상세는 상한으로 잘린다.
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
    // 디자인은 mono 700 이다. egui 는 별도 bold family 없이 굵기를 못 낸다
    // (`design-parity-notes.md` "egui 세금") — 크기·색만 따른다.
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
