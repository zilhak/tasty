//! CenterState 의 세 변형(loading·empty·error)과 액션이 달린 error 를 두 호스트 크기의 카드에
//! 나란히 보여준다. 본체 file picker·remote attach·Settings › Misc › Scripts 와 같은 공용 위젯을 호출한다.
//! 높이를 주지 않는 호스트(`show(…, None)`)의 대칭 자연 높이는 별도 예제가 보여준다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{CenterState, CenterStateVariant};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 디자인 무대 카드 폭(`--tasty-size-288`). 목록 영역을 흉내 내는 액자다.
const CARD_W: LogicalPx = LogicalPx(288.0);
/// file picker·remote attach 호스트 카드 높이(디자인 `--tasty-size-220`). 오류 글리프·액션 슬롯
/// 예제의 정사각 카드 한 변도 시안에서 같은 `--tasty-size-220` 이다.
const CARD_H_PICKER: LogicalPx = LogicalPx(220.0);
/// Settings › Misc › Scripts 호스트 카드 높이(디자인 `--tasty-size-160`).
const CARD_H_SCRIPTS: LogicalPx = LogicalPx(160.0);

#[derive(Clone, Copy)]
enum Host {
    Picker,
    Scripts,
}

/// 디자인 카드 열: loading · empty · error · error + action.
const COLUMNS: [(CenterStateVariant, bool); 4] = [
    (CenterStateVariant::Loading, false),
    (CenterStateVariant::Empty, false),
    (CenterStateVariant::Error, false),
    (CenterStateVariant::Error, true),
];

/// 디자인 `CS_COPY` 의 문구.
fn copy(host: Host, v: CenterStateVariant) -> (&'static str, Option<&'static str>) {
    match (host, v) {
        (Host::Picker, CenterStateVariant::Loading) => ("Loading folder", None),
        (Host::Picker, CenterStateVariant::Empty) => (
            "This folder is empty",
            Some("Files you add here appear in this list."),
        ),
        (Host::Picker, CenterStateVariant::Error) => (
            "Could not read this folder",
            Some("Permission denied (os error 13)"),
        ),
        (Host::Scripts, CenterStateVariant::Loading) => ("Loading scripts", None),
        (Host::Scripts, CenterStateVariant::Empty) => (
            "No scripts",
            Some("Add a Lua script to run it on a lifecycle event."),
        ),
        (Host::Scripts, CenterStateVariant::Error) => (
            "Could not load scripts",
            Some("~/.config/tasty/scripts is not readable"),
        ),
    }
}

fn variant_name(v: CenterStateVariant) -> &'static str {
    match v {
        CenterStateVariant::Loading => "loading",
        CenterStateVariant::Empty => "empty",
        CenterStateVariant::Error => "error",
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .inner_margin(theme.spacing_lg.value())
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
                host_row(ui, theme, "file picker · remote attach", Host::Picker);
                host_row(ui, theme, "Settings › Misc › Scripts", Host::Scripts);
            });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("variants", "loading · empty · error"),
            ("glyph / spinner", "24 — icon-size-lg (was 22 · 26)"),
            ("glyph → title", "8"),
            ("title → sub", "4 · sub slot always reserved"),
            ("title", "body 13 · text-secondary"),
            ("sub", "caption 11 · text-muted · wraps at 300"),
            (
                "height",
                "none — centres in the list region; unsized hosts (natural): block + 2 × 12, with action block + 2 × 48",
            ),
            ("ui_scale", "scales (tokens) — 20.4 / 24 / 28.8 glyph"),
        ],
        &[
            TokenChip::new(
                "center-state-glyph-size",
                "→ icon-size-lg 24",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "center-state-glyph-fg",
                "→ glyph-dim",
                theme.center_state_glyph_fg().to_egui(),
            ),
            TokenChip::new(
                "center-state-error-fg",
                "→ accent-danger",
                theme.center_state_error_fg().to_egui(),
            ),
            TokenChip::new("center-state-gap", "8", theme.accent_primary().to_egui()),
            TokenChip::new(
                "center-state-line-gap",
                "4",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "center-state-max-width",
                "→ measure-sm 300",
                theme.accent_primary().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Clipboard centre glyph 28 is a content glyph (T6) and stays outside this part.",
    );
}

fn host_row(ui: &mut egui::Ui, theme: &Theme, label: &str, host: Host) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
            for (v, action) in COLUMNS {
                card(ui, theme, host, v, action);
            }
        });
    });
}

fn card(ui: &mut egui::Ui, theme: &Theme, host: Host, v: CenterStateVariant, action: bool) {
    let h = match host {
        Host::Picker => CARD_H_PICKER,
        Host::Scripts => CARD_H_SCRIPTS,
    };
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(CARD_W.value(), h.value()), egui::Sense::hover());
    let bw = theme.border_width.value();
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.bg_panel().to_egui(),
    );

    let head = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width(), theme.item_height_interactive.value()),
    );
    let head_text = match host {
        Host::Picker => "~/work/tasty/assets".to_owned(),
        Host::Scripts if action => format!("Scripts · {} + action", variant_name(v)),
        Host::Scripts => format!("Scripts · {}", variant_name(v)),
    };
    painter.text(
        egui::pos2(head.left() + theme.spacing_sm.value(), head.center().y),
        egui::Align2::LEFT_CENTER,
        head_text,
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    painter.hline(
        head.x_range(),
        head.bottom() - bw * 0.5,
        egui::Stroke::new(bw, theme.border_default().to_egui()),
    );

    let region = egui::Rect::from_min_max(egui::pos2(rect.left(), head.bottom()), rect.max);
    state(host, v, action).show_in(ui, theme, region);

    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(bw, theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
}

/// 디자인 `CenterStateG` — 호스트 문구와 빈 상태 글리프, 선택 Retry 액션.
fn state(host: Host, v: CenterStateVariant, action: bool) -> CenterState<'static> {
    let (title, sub) = copy(host, v);
    let glyph = match host {
        Host::Picker => icons::FOLDER_OPEN,
        Host::Scripts => icons::SCRIPT,
    };
    let state = match v {
        CenterStateVariant::Loading => CenterState::loading(title),
        CenterStateVariant::Empty => CenterState::empty(glyph, title),
        CenterStateVariant::Error => CenterState::error(title),
    }
    .sub_line(sub);
    if action {
        state.action("Retry", Some(icons::REFRESH))
    } else {
        state
    }
}

/// 오류 글리프는 부품 소유, 액션은 가운데 정렬 밖에 매달린다 — 액션 유무로 글리프가 움직이지 않는다.
pub fn draw_action_slot(ui: &mut egui::Ui, theme: &Theme) {
    let palettes = [
        ("Mocha", tasty_themes::mocha_fallback()),
        ("Latte", crate::host_shell::latte_theme()),
    ];
    for (palette, base) in &palettes {
        let th = Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
        spec::stage(ui, &th, StageVariant::Wrap, |ui| {
            for (action, label) in [(false, "error"), (true, "error + action")] {
                spec::cluster(ui, &th, &format!("{palette} · {label}"), |ui| {
                    slot_card(ui, &th, action)
                });
            }
        });
    }

    spec::meta(
        ui,
        theme,
        &[
            ("error glyph", "alertTriangle — part-owned, every host"),
            ("empty glyph", "host-chosen"),
            ("action", "Button secondary · sm (24)"),
            ("sub → action", "12 · center-state-action-gap"),
            ("centring", "glyph · title · sub only — action hangs below"),
            (
                "short region",
                "action may reach the region's bottom padding; never pushes the glyph",
            ),
        ],
        &[
            TokenChip::new(
                "center-state-action-gap",
                "→ space-md 12",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("button-height-sm", "24", theme.accent_primary().to_egui()),
            TokenChip::new(
                "center-state-error-fg",
                "→ accent-danger",
                theme.center_state_error_fg().to_egui(),
            ),
        ],
    );
    spec::dont(
        ui,
        theme,
        "Don't put the button in the centred column. Loading → error + Retry would jump the glyph up by half the button, and with no animation that reads as a glitch.",
    );
}

/// 디자인 `--tasty-size-220` 정사각 카드에 file picker 문구의 error 를 그린다.
fn slot_card(ui: &mut egui::Ui, theme: &Theme, action: bool) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(CARD_H_PICKER.value(), CARD_H_PICKER.value()),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.bg_panel().to_egui(),
    );
    state(Host::Picker, CenterStateVariant::Error, action).show_in(ui, theme, rect);
    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
}

/// 높이를 주지 않는 호스트 — 부품이 자기 높이를 정한다. 액션이 있으면 위아래에 액션 띠를 둔다.
pub fn draw_unsized(ui: &mut egui::Ui, theme: &Theme) {
    let palettes = [
        ("Mocha", tasty_themes::mocha_fallback()),
        ("Latte", crate::host_shell::latte_theme()),
    ];
    for (palette, base) in &palettes {
        let th = Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
        spec::stage(ui, &th, StageVariant::Wrap, |ui| {
            for (action, label) in [(false, "empty"), (true, "error + action")] {
                spec::cluster(ui, &th, &format!("{palette} · {label}"), |ui| {
                    unsized_card(ui, &th, action)
                });
            }
        });
    }

    spec::meta(
        ui,
        theme,
        &[
            ("prop", "natural — host gives no height"),
            ("no action", "padding-block space-md (12)"),
            ("with action", "padding-block 12 + 12 + 24 = 48, both sides"),
            (
                "block",
                "stays centred — glyph position independent of the action",
            ),
            ("below the action", "space-md (12) — no new token"),
            (
                "rejected",
                "top-pinned block (glyph moves) · current overflow (6px into the next widget)",
            ),
        ],
        &[
            TokenChip::new("space-md", "12 outer pad", theme.accent_primary().to_egui()),
            TokenChip::new(
                "center-state-action-gap",
                "→ space-md 12",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("button-height-sm", "24", theme.accent_primary().to_egui()),
        ],
    );
}

/// 디자인 `--tasty-size-220` 폭 카드. 높이는 CenterState 가 정하고 아래에 "next widget" 줄을 둔다.
fn unsized_card(ui: &mut egui::Ui, theme: &Theme, action: bool) {
    let bw = theme.border_width.value();
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(bw, theme.border_strong().to_egui()))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            // cluster 는 내용을 가로 줄바꿈 배치로 받는다. 카드 안은 위에서 아래로 쌓는다.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(CARD_H_PICKER.value());
                ui.spacing_mut().item_spacing.y = 0.0;
                let v = if action {
                    CenterStateVariant::Error
                } else {
                    CenterStateVariant::Empty
                };
                state(Host::Scripts, v, action).show(ui, theme, None);
                let (row, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), theme.item_height_interactive.value()),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(row);
                painter.hline(
                    row.x_range(),
                    row.top() + bw * 0.5,
                    egui::Stroke::new(bw, theme.border_default().to_egui()),
                );
                painter.text(
                    egui::pos2(row.left() + theme.spacing_sm.value(), row.center().y),
                    egui::Align2::LEFT_CENTER,
                    "next widget",
                    egui::FontId::proportional(theme.font_size_caption.value()),
                    theme.text_muted().to_egui(),
                );
            });
        });
}
