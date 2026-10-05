//! 서피스 타입 전환 팝업의 정적 예제.
//!
//! 본체 `popup/convert.rs` 와 같은 작은 목록 popup 이다 — 공용 타이틀바(× 만) 아래에 바꿀 수
//! 있는 kind 마다 `MenuItem` 하나. 폭은 `convert-popup-width` 에 UI 배율을 곱한 값이다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, menu_item};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::popup_frame::{self, TitleButtons};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체가 터미널 서피스에서 여는 목록 — (아이콘, 라벨). 현재 kind 는 빠진다. 첫 글자 단축키는
/// 동작만 하고 행에 글자를 표시하지 않는다.
const KINDS: &[(MockGlyph, &str)] = &[
    (icons::MARKDOWN, "Markdown"),
    (icons::HTML, "HTML"),
    (icons::FOLDER, "Explorer"),
    (icons::IMAGE, "Image"),
];

/// 배율을 따르지 않던 옛 convert popup 폭. 1.2 배 전후 비교 specimen 의 '전' 쪽에서만 쓴다.
const LITERAL_WIDTH_BEFORE: LogicalPx = LogicalPx(200.0);

const JA_TITLE: &str = "サーフェスタイプ切替";

/// 배율을 적용한 테마. 갤러리 기본 테마는 egui 전역 배율을 쓰므로 1.0 이다.
fn scaled(theme: &Theme, zoom: f32) -> Theme {
    Theme::with_colors_and_zoom(theme.to_colors(), theme.is_light, zoom)
}

/// 본체 convert popup 한 장. 높이 = 타이틀바 + 콘텐츠 여백 × 2 + 행 수 × MenuItem 높이.
/// `open_tip`이면 잘린 제목의 Tooltip을 호버 없이 띄운다(specimen).
fn popup(ui: &mut egui::Ui, th: &Theme, title: &str, width: LogicalPx, open_tip: bool) {
    let title_h = th.item_height_interactive;
    let margin = th.spacing_xs;
    let rows = th.menu_item_height().scaled(KINDS.len() as f32);
    let (frame, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), (title_h + margin.scaled(2.0) + rows).value()),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(frame.expand(th.spacing_lg.value()));
    let radius = th.corner_radius.value();
    painter.add(th.shadow_modal().to_egui().as_shape(frame, radius));
    painter.rect_filled(frame, radius, th.bg_panel().to_egui());
    painter.rect_stroke(
        frame,
        radius,
        egui::Stroke::new(th.border_width.value(), th.border_frame().to_egui()),
        egui::StrokeKind::Outside,
    );
    let title_rect =
        egui::Rect::from_min_size(frame.min, egui::vec2(frame.width(), title_h.value()));
    let cr = radius as u8;
    painter.rect_filled(
        title_rect,
        egui::CornerRadius {
            nw: cr,
            ne: cr,
            sw: 0,
            se: 0,
        },
        th.bg_sidebar().to_egui(),
    );
    painter.hline(
        title_rect.x_range(),
        title_rect.max.y,
        egui::Stroke::new(th.border_width.value(), th.border_frame().to_egui()),
    );
    let buttons_left =
        popup_frame::draw_title_buttons(ui.ctx(), &painter, th, title_rect, TitleButtons::CLOSE);
    let cut_band = popup_frame::draw_title_text(&painter, th, title_rect, buttons_left, title);
    popup_frame::title_tooltip(ui, th, title, cut_band, open_tip);

    let content = egui::Rect::from_min_max(
        egui::pos2(
            frame.min.x + margin.value(),
            title_rect.max.y + margin.value(),
        ),
        frame.max - egui::vec2(margin.value(), margin.value()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 0.0;
    for (i, (glyph, label)) in KINDS.iter().enumerate() {
        let glyph = *glyph;
        menu_item(
            &mut child,
            th,
            Some(&|ui: &mut egui::Ui, r: egui::Rect, c: egui::Color32| {
                glyph.image(r.height(), c).paint_at(ui, r);
            }),
            label,
            None,
            MenuItemVariant::Normal,
            i == 0,
            true,
        );
    }
}

/// 목록 popup — 바꿀 kind 를 고른다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for (label, th) in [
            ("Mocha · en · ui_scale 1.0", theme),
            ("Latte · en · ui_scale 1.0", &latte),
        ] {
            spec::cluster(ui, th, label, |ui| {
                popup(ui, th, "Surface Type", th.convert_popup_width(), false);
            });
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "width",
                "240 × ui_scale · convert-popup-width (was literal 200)",
            ),
            (
                "title",
                "shared popup title bar (× only, reserve 32 per side)",
            ),
            (
                "rows",
                "MenuItem 28 · icon + kind label · one per kind the surface can become",
            ),
            (
                "actions",
                "click / Enter converts · first letter converts · × / Esc close",
            ),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("border-frame", "edge", theme.border_frame().to_egui()),
            TokenChip::new(
                "surface-active",
                "keyboard row",
                theme.surface_active().to_egui(),
            ),
            TokenChip::without_color("convert-popup-width", "→ size-240, × ui_scale"),
        ],
    );

    spec::note(
        ui,
        theme,
        "Only the renderer changes — the running process keeps its scrollback. The current kind is \
         not listed. The earlier 400-wide From / To dialog is retired.",
    );
}

/// 좁은 popup 과 긴 제목 — 폭은 배율을 따르고, 그래도 넘치는 제목은 띠 안에서 말줄임한다.
pub fn draw_narrow(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let mocha_12 = scaled(theme, 1.2);
    let latte_12 = scaled(&latte, 1.2);
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(
            ui,
            theme,
            "Mocha · ja · 1.2 — before: literal 200 (cuts)",
            |ui| {
                popup(ui, &mocha_12, JA_TITLE, LITERAL_WIDTH_BEFORE, false);
            },
        );
        spec::cluster(
            ui,
            theme,
            "Mocha · ja · 1.2 — convert-popup-width 240 × 1.2 (fits)",
            |ui| {
                popup(
                    ui,
                    &mocha_12,
                    JA_TITLE,
                    mocha_12.convert_popup_width(),
                    false,
                );
            },
        );
        spec::cluster(
            ui,
            &latte,
            "Latte · ja · 1.2 — convert-popup-width 240 × 1.2 (fits)",
            |ui| {
                popup(
                    ui,
                    &latte_12,
                    JA_TITLE,
                    latte_12.convert_popup_width(),
                    false,
                );
            },
        );
        spec::cluster(
            ui,
            theme,
            "Mocha · still too long — ellipsis + full title tooltip (forced open)",
            |ui| {
                popup(
                    ui,
                    theme,
                    "Convert this surface to another type",
                    theme.convert_popup_width(),
                    true,
                );
            },
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "width",
                "token × ui_scale — never a literal · base sized for the longest locale title at 1.0",
            ),
            ("band", "width − 2 × reserve(N) · convert 240 → 176"),
            ("overflow", "ellipsis inside the band"),
            (
                "tooltip",
                "only when cut · full title on hover after tooltip-delay · top, then bottom",
            ),
            ("widen to fit", "no"),
        ],
        &[
            TokenChip::new("bg-sidebar", "title bar", theme.bg_sidebar().to_egui()),
            TokenChip::without_color("convert-popup-width", "240 × ui_scale"),
            TokenChip::without_color("popup-title-btn-size", "reserve part"),
        ],
    );

    spec::dont(
        ui,
        theme,
        "Don't keep a popup width as a literal outside the scale, and don't widen a popup to its \
         title — the band follows the token width.",
    );
}
