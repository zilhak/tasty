//! 이미지 플러그인의 툴바와 이미지 유무에 따른 화면 예제, 편집 모드의 paint bar와 팝업 예제.
//! 실제 텍스처를 읽지 않으며 아이콘과 Theme 값으로 구성을 재현한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, IconButton};

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

// 아래 값은 시안 `plugins.jsx`의 ImgSurface(edit) · FloatingSelection 그림에서 옮긴 표본 치수다.
// 대응하는 토큰이 없다. 손잡이 · 팝업 · 입력 · zoom % 치수는 `image-*` 토큰을 읽는다.
/// paint bar 높이.
const PAINT_BAR_H: LogicalPx = LogicalPx(40.0);
/// paint bar 구분선 높이.
const PAINT_SEP_H: LogicalPx = LogicalPx(18.0);
/// brush 슬라이더 트랙 폭·높이와 손잡이 지름.
const BRUSH_TRACK_W: LogicalPx = LogicalPx(60.0);
const BRUSH_TRACK_H: LogicalPx = LogicalPx(4.0);
const BRUSH_KNOB: LogicalPx = LogicalPx(10.0);
/// 손잡이 위치(트랙 폭 대비 비율).
const BRUSH_KNOB_AT: f32 = 0.4;
/// 색 견본 한 변.
const COLOR_SWATCH: LogicalPx = LogicalPx(16.0);
/// zoom 그룹 안 간격.
const ZOOM_GROUP_GAP: LogicalPx = LogicalPx(2.0);
/// 캔버스 위 그림 자리.
const PICTURE_W: LogicalPx = LogicalPx(200.0);
const PICTURE_H: LogicalPx = LogicalPx(132.0);
/// 붙여 넣은 floating selection 크기.
const FLOAT_W: LogicalPx = LogicalPx(96.0);
const FLOAT_H: LogicalPx = LogicalPx(64.0);

/// Plugins 페이지의 image 섹션: 보기 화면과 편집 모드 두 spec.
pub fn section() -> crate::catalog::Section {
    crate::catalog::Section {
        id: "image-viewer",
        title: "Image surface / canvas",
        specs: vec![spec(), paint_spec()],
    }
}

fn spec() -> crate::catalog::Spec {
    crate::catalog::Spec {
        id: "image-viewer",
        title: "Image surface / canvas",
        when: Some("Toolbar + zoom · canvas=bg-sidebar · loaded / no-image fallback"),
        draw,
    }
}

fn paint_spec() -> crate::catalog::Spec {
    crate::catalog::Spec {
        id: "image-paint",
        title: "Image — paint bar · floating selection · popups",
        when: Some("edit mode · 8 handles · New Image · Save As"),
        draw: draw_paint,
    }
}

fn draw(ui: &mut egui::Ui, theme: &Theme) {
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
            egui::Stroke::new(
                theme.border_width.value(),
                theme.separator.to_egui_premultiplied(),
            ),
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
    // 비율은 mono caption, 최소 폭 칸 가운데에 둔다(시안 ZoomGroup).
    let pct_w = theme.image_zoom_min_width();
    let pct = egui::Rect::from_min_size(
        egui::pos2((LogicalPx(minus.left()) - gap - pct_w).value(), y.value()),
        egui::vec2(pct_w.value(), BTN_H.value()),
    );
    p.text(
        pct.center(),
        egui::Align2::CENTER_CENTER,
        "100%",
        egui::FontId::monospace(theme.image_zoom_font_size().value()),
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

/// 편집 모드: paint bar · floating selection, 그리고 New Image · Save As 팝업.
fn draw_paint(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "edit · floating selection", |ui| {
            paint_surface(ui, theme);
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
            new_image_popup(ui, theme);
            save_as_popup(ui, theme);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "paint bar",
                "Save · Cancel · undo redo · brush · color · zoom",
            ),
            ("undo/redo", "enabled / disabled (text-disabled)"),
            (
                "floating sel",
                "accent border + 8 handles · 6 · image-handle-size (on-scale), centred on the edge",
            ),
            (
                "popup card",
                "300 · pad 12 / 14 / 10 · title 14 semibold, 10 below · buttons gap 8 · image-popup-*",
            ),
            ("size field", "64 · image-size-input-width"),
            ("path row gap", "6 · image-path-row-gap"),
            ("zoom %", "mono 11 · min 40 · image-zoom-font-size"),
            ("commit", "click outside = composite · Esc = cancel"),
            ("New Image", "Width × Height (1–8192)"),
            ("Save As", "path input + browse · PNG"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "floating selection + handles",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "default brush color",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "surface-active",
                "brush slider track",
                theme.surface_active().to_egui(),
            ),
            TokenChip::without_color("shadow-modal", "popups"),
        ],
    );

    spec::note(
        ui,
        theme,
        "Defaults (brief §6): metadata status-bar, filmstrip, corrupt-image state, async \
         loading indicator, rotate/flip/crop tools, and transparency checkerboard are out \
         of scope this pass — viewer + brush/paste paint only. Load-fail / no-image share \
         one centered \"No image loaded\" (muted).",
    );
}

/// 편집 모드 surface = paint bar + 캔버스(그림 자리 + floating selection).
fn paint_surface(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card_flat(ui, theme, PANE_W, kit::panel_fill(theme), |ui| {
        let pad = theme.spacing_sm;
        let (bar, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), PAINT_BAR_H.value()),
            egui::Sense::hover(),
        );
        let inner = bar.shrink2(egui::vec2(pad.value(), 0.0));
        let mut bar_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        paint_bar(&mut bar_ui, theme);
        ui.painter().hline(
            bar.x_range(),
            bar.bottom(),
            egui::Stroke::new(
                theme.border_width.value(),
                theme.separator.to_egui_premultiplied(),
            ),
        );

        let (canvas, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), CANVAS_H.value()),
            egui::Sense::hover(),
        );
        let p = ui.painter_at(canvas);
        p.rect_filled(
            canvas,
            egui::CornerRadius::ZERO,
            theme.bg_sidebar().to_egui(),
        );
        let pic = egui::Rect::from_center_size(
            canvas.center(),
            egui::vec2(PICTURE_W.value(), PICTURE_H.value()),
        );
        p.rect_filled(
            pic,
            theme.corner_radius_sm.value(),
            theme.surface_raised().to_egui(),
        );
        p.rect_stroke(
            pic,
            theme.corner_radius_sm.value(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
        glyph(
            ui,
            pic.center(),
            theme.icon_glyph_size_md.value(),
            theme.text_disabled().to_egui(),
        );
        floating_selection(&p, theme, pic.center());
    });
}

/// Save · Cancel · Undo · Redo(비활성) | Brush 슬라이더 · Color 견본 · 오른쪽 zoom 그룹.
fn paint_bar(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    Button::new("Save")
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .show(ui, theme);
    Button::new("Cancel")
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(ui, theme);
    icon_button(ui, theme, icons::UNDO, true);
    icon_button(ui, theme, icons::REDO, false);

    let (sep, _) = ui.allocate_exact_size(
        egui::vec2(theme.border_width.value(), PAINT_SEP_H.value()),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(sep, 0.0, theme.separator.to_egui_premultiplied());

    bar_caption(ui, theme, "Brush");
    let (track_slot, _) = ui.allocate_exact_size(
        egui::vec2(BRUSH_TRACK_W.value(), BRUSH_KNOB.value()),
        egui::Sense::hover(),
    );
    let track = egui::Rect::from_center_size(
        track_slot.center(),
        egui::vec2(BRUSH_TRACK_W.value(), BRUSH_TRACK_H.value()),
    );
    ui.painter().rect_filled(
        track,
        BRUSH_TRACK_H.scaled(0.5).value(),
        theme.surface_active().to_egui(),
    );
    let knob_x = track.left() + track.width() * BRUSH_KNOB_AT + BRUSH_KNOB.scaled(0.5).value();
    ui.painter().circle_filled(
        egui::pos2(knob_x, track.center().y),
        BRUSH_KNOB.scaled(0.5).value(),
        theme.text_muted().to_egui(),
    );

    bar_caption(ui, theme, "Color");
    let (swatch, _) = ui.allocate_exact_size(
        egui::vec2(COLOR_SWATCH.value(), COLOR_SWATCH.value()),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        swatch,
        theme.corner_radius_sm.value(),
        theme.accent_danger().to_egui(),
    );
    ui.painter().rect_stroke(
        swatch,
        theme.corner_radius_sm.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // 오른쪽부터 역순: − · % · + · Fit.
        ui.spacing_mut().item_spacing.x = ZOOM_GROUP_GAP.value();
        icon_button(ui, theme, icons::MINUS, true);
        let (pct, _) = ui.allocate_exact_size(
            egui::vec2(theme.image_zoom_min_width().value(), PAINT_SEP_H.value()),
            egui::Sense::hover(),
        );
        ui.painter().text(
            pct.center(),
            egui::Align2::CENTER_CENTER,
            "100%",
            egui::FontId::monospace(theme.image_zoom_font_size().value()),
            theme.text_muted().to_egui(),
        );
        icon_button(ui, theme, icons::PLUS, true);
        Button::new("Fit")
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .show(ui, theme);
    });
}

/// 붙여 넣은 이미지: 1px accent 테두리 + 모서리·변 중앙 손잡이 8개. 손잡이는 변 위에 가운데를 맞춘다.
fn floating_selection(p: &egui::Painter, theme: &Theme, center: egui::Pos2) {
    let sel = egui::Rect::from_center_size(center, egui::vec2(FLOAT_W.value(), FLOAT_H.value()));
    let accent = theme.accent_primary().to_egui();
    p.rect_filled(sel, 0.0, theme.bg_panel().to_egui());
    p.rect_stroke(
        sel,
        0.0,
        egui::Stroke::new(theme.border_width.value(), accent),
        egui::StrokeKind::Inside,
    );
    p.text(
        sel.center(),
        egui::Align2::CENTER_CENTER,
        "pasted",
        egui::FontId::monospace(theme.font_size_micro.value()),
        theme.text_muted().to_egui(),
    );
    let hs = theme.image_handle_size().value();
    for x in [sel.left(), sel.center().x, sel.right()] {
        for y in [sel.top(), sel.center().y, sel.bottom()] {
            if x == sel.center().x && y == sel.center().y {
                continue;
            }
            p.rect_filled(
                egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(hs, hs)),
                0.0,
                accent,
            );
        }
    }
}

/// New Image: Width × Height 입력 두 칸 + Cancel · OK.
fn new_image_popup(ui: &mut egui::Ui, theme: &Theme) {
    popup(ui, theme, "New Image", "OK", |ui| {
        ui.horizontal(|ui| {
            // 라벨을 입력칸 높이의 가운데에 맞추려면 줄 높이를 먼저 정한다.
            ui.set_min_height(theme.input_height().value());
            ui.spacing_mut().item_spacing.x = theme.image_popup_btn_gap().value();
            let input_w = theme.image_size_input_width();
            kit::body(ui, theme, "Width");
            kit::field(ui, theme, Some(input_w), "800", false, false);
            kit::caption(ui, theme, "×", false);
            kit::body(ui, theme, "Height");
            kit::field(ui, theme, Some(input_w), "600", false, false);
        });
    });
}

/// Save As: 경로 입력(placeholder) + 찾아보기 IconButton + Cancel · Save.
fn save_as_popup(ui: &mut egui::Ui, theme: &Theme) {
    popup(ui, theme, "Save As", "Save", |ui| {
        ui.horizontal(|ui| {
            let gap = theme.image_path_row_gap().value();
            ui.spacing_mut().item_spacing.x = gap;
            let browse_w = ControlSize::Sm.height(theme);
            let field_w = LogicalPx(ui.available_width() - browse_w - gap);
            kit::field(ui, theme, Some(field_w), "path/to/image.png", true, false);
            icon_button(ui, theme, icons::FOLDER_OPEN, true);
        });
    });
}

/// 두 팝업이 공유하는 카드: 제목 · 본문 · 오른쪽 정렬 Ghost Cancel + Primary 확인.
fn popup(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    confirm: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let pad_x = theme.image_popup_pad_x().value() as i8;
    let pad_top = theme.image_popup_pad_top().value() as i8;
    let gap = theme.image_popup_gap();
    kit::frame_card(
        ui,
        theme,
        theme.image_popup_width(),
        kit::panel_fill(theme),
        |ui| {
            kit::region(
                ui,
                egui::Margin {
                    left: pad_x,
                    right: pad_x,
                    top: pad_top,
                    bottom: gap.value() as i8,
                },
                |ui| {
                    // 시안 제목은 14 / semibold 다. egui 에 semibold 글꼴이 없어 굵기는 재현하지 않는다.
                    ui.label(
                        egui::RichText::new(title)
                            .size(theme.image_popup_title_font_size().value())
                            .color(theme.text_primary().to_egui()),
                    );
                    ui.add_space(gap.value());
                    body(ui);
                },
            );
            kit::region(
                ui,
                egui::Margin {
                    left: pad_x,
                    right: pad_x,
                    top: 0,
                    bottom: pad_top,
                },
                |ui| {
                    // 버튼 높이만큼만 차지해야 카드가 세로로 늘어나지 않는다.
                    let row = egui::vec2(ui.available_width(), ControlSize::Sm.height(theme));
                    ui.allocate_ui_with_layout(
                        row,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = theme.image_popup_btn_gap().value();
                            Button::new(confirm)
                                .variant(ButtonVariant::Primary)
                                .size(ControlSize::Sm)
                                .show(ui, theme);
                            Button::new("Cancel")
                                .variant(ButtonVariant::Ghost)
                                .size(ControlSize::Sm)
                                .show(ui, theme);
                        },
                    );
                },
            );
        },
    );
}

/// 크기 sm IconButton에 tasty-icons 글리프를 그린다.
fn icon_button(ui: &mut egui::Ui, theme: &Theme, icon: icons::Icon, enabled: bool) {
    IconButton::new()
        .size(ControlSize::Sm)
        .enabled(enabled)
        .show(ui, theme, &|ui, rect, c| {
            icon.image(rect.height(), c).paint_at(ui, rect)
        });
}

/// paint bar의 caption 라벨(11 · text-muted).
fn bar_caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}
