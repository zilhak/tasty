//! 이미지 플러그인의 툴바와 이미지 유무에 따른 화면 예제.
//! 실제 텍스처를 읽지 않으며 아이콘과 Theme 값으로 구성을 재현한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 본문 폭(전시 박스).
const PANE_W: LogicalPx = LogicalPx(560.0);
/// 캔버스 영역 높이(전시 박스).
const CANVAS_H: LogicalPx = LogicalPx(300.0);
/// control 버튼 폭 (host `add_sized([24,20])`).
const BTN_W: LogicalPx = LogicalPx(24.0);
/// control 버튼 높이.
const BTN_H: LogicalPx = LogicalPx(20.0);
/// "Fit" zoom 버튼 폭 (host `add_sized([30,20])`).
const FIT_W: LogicalPx = LogicalPx(30.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        spec::cluster(ui, theme, "viewer — image loaded", |ui| {
            surface(ui, theme, true);
        });
        spec::cluster(ui, theme, "no image — fallback", |ui| {
            surface(ui, theme, false);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "toolbar",
                "prev / next / refresh / edit / new · surface-raised",
            ),
            ("filename", "caption · text-muted"),
            ("zoom", "right · Fit / + / % / -"),
            ("canvas", "bg-sidebar (mantle) fill"),
            ("loaded", "fit-to-window · centered"),
            ("empty", "fallback glyph + No image"),
        ],
        &[
            TokenChip::new("bg-sidebar", "canvas", theme.bg_sidebar().to_egui()),
            TokenChip::new(
                "surface-raised",
                "buttons",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "text-muted",
                "filename · zoom %",
                theme.text_muted().to_egui(),
            ),
            TokenChip::new(
                "border-default",
                "button / frame",
                theme.border_default().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "An image surface — a control bar (navigation / refresh / edit / new + a \
         right-aligned zoom group) over a canvas filled with the sidebar tone. When a \
         picture is loaded it fits to the window; with none, the surface shows a fallback \
         glyph and the no-image caption. New images open a blank canvas. This specimen \
         transcribes both states with tokens only.",
    );
}

/// surface = control bar + canvas. `loaded`=true 면 그림, false 면 fallback.
fn surface(ui: &mut egui::Ui, theme: &Theme, loaded: bool) {
    // 탭 내부 콘텐츠이므로 팝업 그림자를 그리지 않는다.
    kit::frame_card_flat(ui, theme, PANE_W, kit::panel_fill(theme), |ui| {
        let w = ui.available_width();
        let pad = theme.spacing_sm;
        let bar_h = BTN_H + pad.scaled(2.0);
        let total_h = bar_h + CANVAS_H;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(w, total_h.value()), egui::Sense::hover());
        let p = ui.painter_at(rect);

        let by = LogicalPx(rect.top()) + pad;
        let mut x = LogicalPx(rect.left()) + pad;
        // 이미지가 없으면 새 이미지·새로고침 버튼만 표시한다.
        let glyphs: &[icons::Icon] = if loaded {
            &[
                icons::CHEVRON_LEFT,
                icons::CHEVRON_RIGHT,
                icons::REFRESH,
                icons::EDIT,
                icons::PLUS,
            ]
        } else {
            &[icons::REFRESH, icons::PLUS]
        };
        for g in glyphs {
            x = button(&p, ui, theme, x, by, BTN_W, *g);
        }
        x += pad;
        let name = if loaded { "diagram.png (2/5)" } else { "—" };
        p.text(
            egui::pos2(x.value(), (by + BTN_H.scaled(0.5)).value()),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(theme.font_size_caption.value()),
            theme.text_muted().to_egui(),
        );
        // zoom 그룹 (우측 정렬): Fit + % - 를 오른쪽부터 역순 배치.
        zoom_group(&p, theme, LogicalPx(rect.right()) - pad, by);

        let canvas_top = rect.top() + bar_h.value();
        p.hline(
            rect.x_range(),
            canvas_top,
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );

        let canvas = egui::Rect::from_min_max(
            egui::pos2(rect.left(), canvas_top),
            egui::pos2(rect.right(), rect.bottom()),
        );
        p.rect_filled(
            canvas,
            egui::CornerRadius::ZERO,
            theme.bg_sidebar().to_egui(),
        );

        if loaded {
            // 실제 텍스처 대신 아이콘으로 이미지 영역을 표시한다.
            let pic = egui::Rect::from_center_size(
                canvas.center(),
                egui::vec2(canvas.height() * 1.3, canvas.height() * 0.78),
            );
            p.rect_filled(
                pic,
                theme.corner_radius_sm.value(),
                theme.bg_panel().to_egui(),
            );
            p.rect_stroke(
                pic,
                theme.corner_radius_sm.value(),
                egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
            glyph(
                ui,
                canvas.center(),
                theme.icon_glyph_size_md.value(),
                theme.text_muted().to_egui(),
            );
        } else {
            let g = canvas.center() - egui::vec2(0.0, theme.spacing_lg.value());
            glyph(
                ui,
                g,
                theme.icon_glyph_size_md.value(),
                theme.text_disabled().to_egui(),
            );
            p.text(
                egui::pos2(canvas.center().x, g.y + theme.icon_glyph_size_md.value()),
                egui::Align2::CENTER_TOP,
                "No image",
                egui::FontId::proportional(theme.font_size_body.value()),
                theme.text_muted().to_egui(),
            );
        }
    });
}

/// control 버튼 한 칸. surface-raised 채움 + 1px border + 중앙 tasty-icons 글리프.
/// 다음 x 반환. rect 는 painter `p` 로, 글리프는 `ui`(egui_extras 로더)로 그린다.
fn button(
    p: &egui::Painter,
    ui: &egui::Ui,
    theme: &Theme,
    x: LogicalPx,
    y: LogicalPx,
    width: LogicalPx,
    icon: icons::Icon,
) -> LogicalPx {
    let r = egui::Rect::from_min_size(
        egui::pos2(x.value(), y.value()),
        egui::vec2(width.value(), BTN_H.value()),
    );
    p.rect_filled(
        r,
        theme.corner_radius_sm.value(),
        theme.surface_raised().to_egui(),
    );
    p.rect_stroke(
        r,
        theme.corner_radius_sm.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    let gs = theme.icon_glyph_size_sm.value();
    let gr = egui::Rect::from_center_size(r.center(), egui::vec2(gs, gs));
    icon.image(gs, theme.text_primary().to_egui())
        .paint_at(ui, gr);
    x + width + theme.spacing_xs
}

/// 우측 정렬 zoom 그룹 — 오른쪽 끝 `right_x` 에서 `-`, `%`, `+`, `Fit` 순으로 역배치.
fn zoom_group(p: &egui::Painter, theme: &Theme, right_x: LogicalPx, y: LogicalPx) {
    let gap = theme.spacing_xs;
    let minus = egui::Rect::from_min_size(
        egui::pos2((right_x - BTN_W).value(), y.value()),
        egui::vec2(BTN_W.value(), BTN_H.value()),
    );
    btn_box(p, theme, minus, "-");
    let pct_x = LogicalPx(minus.left()) - gap;
    let pct = p.text(
        egui::pos2(pct_x.value(), (y + BTN_H.scaled(0.5)).value()),
        egui::Align2::RIGHT_CENTER,
        "100%",
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    let plus = egui::Rect::from_min_size(
        egui::pos2(pct.left() - (gap + BTN_W).value(), y.value()),
        egui::vec2(BTN_W.value(), BTN_H.value()),
    );
    btn_box(p, theme, plus, "+");
    let fit = egui::Rect::from_min_size(
        egui::pos2(plus.left() - (gap + FIT_W).value(), y.value()),
        egui::vec2(FIT_W.value(), BTN_H.value()),
    );
    btn_box(p, theme, fit, "Fit");
}

/// 고정 rect 버튼(zoom 그룹용).
fn btn_box(p: &egui::Painter, theme: &Theme, r: egui::Rect, label: &str) {
    p.rect_filled(
        r,
        theme.corner_radius_sm.value(),
        theme.surface_raised().to_egui(),
    );
    p.rect_stroke(
        r,
        theme.corner_radius_sm.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    p.text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_primary().to_egui(),
    );
}

/// IMAGE fallback glyph 를 `center` 에 size 정사각·tint 로 그린다.
fn glyph(ui: &egui::Ui, center: egui::Pos2, size: f32, color: egui::Color32) {
    let rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
    icons::IMAGE.image(size, color).paint_at(ui, rect);
}
