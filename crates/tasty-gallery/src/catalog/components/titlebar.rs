//! `titlebar` specimen — CSD 창 타이틀바 (Layouts).
//!
//! 본체 `src/adapters/ui/titlebar/view.rs::draw_titlebar_view` 의 구조 전사.
//! 본체는 `TopBottomPanel::top` 으로 `egui::Context` 에 직접 붙지만, 갤러리는
//! 부유 배치/패널을 넘겨받지 않으므로(`docs/dev-guide/gallery-first.md`) 넘겨받은
//! `Ui` 안에 바 rect 를 할당하고 **그 rect 기준**으로만 같은 순서로 그린다.
//!
//! 그리는 순서(본체와 동일):
//! 1. 바 배경 — `titlebar_bg()` / 비활성이면 `titlebar_bg_inactive()`.
//! 2. DE 가변 컨트롤 클러스터 — 지름 `window_button_size`(24), 측면 끝 여백
//!    `spacing_sm`, 버튼 간 `spacing_xs`. **Right 측면은 역순으로 그린다** — 그래야
//!    `[min, max, close]` 의 마지막(close)이 가장 바깥에 온다.
//! 3. 드래그 영역 — 좌측 inset(macOS 신호등)과 우측 strip 을 뺀 나머지. 버튼 rect
//!    와 겹치지 않아 버튼 클릭이 드래그로 새지 않는다(정적 specimen 이라 히트영역은
//!    그리지 않고 치수만 meta 에 적는다).
//! 4. 하단 1px 보더 — `border_width` × `titlebar_border()`.
//!
//! 글리프는 지름의 0.22 를 반경 extent 로 쓰는 painter 직선이다 — min=가로선,
//! max=정사각 stroke, close=×. close 만 hover/press 배경이 `accent_window_close`
//! (시스템 red)이고 글리프가 `text_on_window_close` 로 뒤집힌다.
//!
//! **macOS 변형은 무대에 행으로 두지 않는다.** 그 경로에서 tasty 는 버튼을 그리지
//! 않고 좌측 슬롯만 비우므로, 정적 specimen 으로 그리면 화면에는 빈 밴드만 남는다.
//! 갤러리는 사람이 눈으로 보고 판정하는 물건이라 "그릴 것이 없어서 비어 있는 것"과
//! "렌더가 실패해 비어 있는 것"을 화면만으로 구별할 수 없으면 정보가 0 이 아니라
//! 음수가 된다. 그래서 이 변형은 아래 note 로 서술한다(inset 폭은 본체
//! `titlebar/mod.rs::MACOS_TRAFFIC_LIGHT_INSET` = 78).

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 `draw_button_glyph` 의 글리프 extent 비율 (지름 대비).
const GLYPH_EXTENT_RATIO: f32 = 0.22;

/// 본체 `WindowButton` 과 같은 3종.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

/// 본체 `draw_button_glyph` 전사.
fn button_glyph(
    p: &egui::Painter,
    center: egui::Pos2,
    d: f32,
    button: WindowButton,
    stroke: egui::Stroke,
) {
    let g = d * GLYPH_EXTENT_RATIO;
    match button {
        WindowButton::Minimize => {
            p.line_segment(
                [
                    egui::pos2(center.x - g, center.y),
                    egui::pos2(center.x + g, center.y),
                ],
                stroke,
            );
        }
        WindowButton::Maximize => {
            p.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(g * 2.0, g * 2.0)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        WindowButton::Close => {
            p.line_segment(
                [
                    egui::pos2(center.x - g, center.y - g),
                    egui::pos2(center.x + g, center.y + g),
                ],
                stroke,
            );
            p.line_segment(
                [
                    egui::pos2(center.x - g, center.y + g),
                    egui::pos2(center.x + g, center.y - g),
                ],
                stroke,
            );
        }
    }
}

/// 컨트롤 클러스터를 우측에 그리고 strip 폭을 돌려준다 (본체 `draw_window_buttons`).
/// `hovered_close` 면 close 버튼만 hover 상태로 그린다(정적 재현).
fn window_buttons(
    p: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    active: bool,
    hovered_close: bool,
) -> f32 {
    const BUTTONS: [WindowButton; 3] = [
        WindowButton::Minimize,
        WindowButton::Maximize,
        WindowButton::Close,
    ];
    let d = theme.window_button_size.value();
    let edge_pad = theme.spacing_sm.value();
    let gap = theme.spacing_xs.value();
    let n = BUTTONS.len() as f32;
    let strip_w = edge_pad * 2.0 + d * n + gap * (n - 1.0);

    let cy = rect.center().y;
    let mut cx = rect.right() - edge_pad - d * 0.5;
    let step = -(d + gap);

    // Right 측면 — 역순 순회라 close 가 가장 우측.
    for button in BUTTONS.iter().rev() {
        let center = egui::pos2(cx, cy);
        let is_close = *button == WindowButton::Close;
        let hovered = is_close && hovered_close;
        if hovered {
            p.circle_filled(center, d * 0.5, theme.accent_window_close().to_egui());
        }
        let fg = if hovered {
            theme.text_on_window_close()
        } else if active {
            theme.titlebar_fg()
        } else {
            theme.titlebar_fg_inactive()
        };
        button_glyph(
            p,
            center,
            d,
            *button,
            egui::Stroke::new(theme.border_width.value(), fg.to_egui()),
        );
        cx += step;
    }
    strip_w
}

/// 타이틀바 1줄 — tasty 가 컨트롤을 그리는 경로(Linux DE / Windows).
fn bar(ui: &mut egui::Ui, theme: &Theme, active: bool, hovered_close: bool) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(theme.measure_lg.value(), theme.titlebar_height.value()),
        egui::Sense::hover(),
    );
    let p = ui.painter_at(rect);

    // ① 배경.
    let bg = if active {
        theme.titlebar_bg()
    } else {
        theme.titlebar_bg_inactive()
    };
    p.rect_filled(rect, 0.0, bg.to_egui());

    // ② 컨트롤 클러스터.
    window_buttons(&p, theme, rect, active, hovered_close);

    // ③ 하단 1px 보더.
    p.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.titlebar_border().to_egui(),
        ),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        spec::cluster(ui, theme, "Active — DE buttons (right)", |ui| {
            bar(ui, theme, true, false)
        });
        spec::cluster(ui, theme, "Inactive (dimmed)", |ui| {
            bar(ui, theme, false, false)
        });
        spec::cluster(ui, theme, "Close hovered", |ui| bar(ui, theme, true, true));
    });

    spec::meta(
        ui,
        theme,
        &[
            ("height", "titlebar-height(36) · 하단 1px titlebar-border"),
            (
                "button",
                "window-button-size(24) 원형 · gap spacing-xs · edge spacing-sm",
            ),
            ("glyph", "지름 × 0.22 extent · 1px border-width stroke"),
            (
                "close",
                "hover/press 시 accent-window-close 배경 + 반전 글리프",
            ),
            ("drag", "좌 inset · 우 strip 을 뺀 나머지 (버튼과 비중첩)"),
        ],
        &[
            TokenChip::new("titlebar-bg", "bar", theme.titlebar_bg().to_egui()),
            TokenChip::new(
                "titlebar-fg",
                "glyph (active)",
                theme.titlebar_fg().to_egui(),
            ),
            TokenChip::new(
                "titlebar-border",
                "bottom hairline",
                theme.titlebar_border().to_egui(),
            ),
            TokenChip::new(
                "accent-window-close",
                "close hover",
                theme.accent_window_close().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "버튼 집합·순서·측면은 DE 마다 달라 데이터로 받는다(`TitlebarControls`). Right 측면은 \
         목록의 마지막이 가장 바깥에 오도록 역순으로 그린다. Windows 캡션은 전용 경로 \
         (46px · close hover red)로 그린다. macOS 는 네이티브 신호등을 유지하므로 tasty 가 \
         버튼을 하나도 그리지 않고 좌측 78px 슬롯을 드래그 대상에서만 빼둔다 — 그 변형은 \
         정적으로 그리면 빈 밴드와 렌더 실패가 화면상 구별되지 않아 무대에 행으로 두지 \
         않고 여기 글로 남긴다.",
    );
}
