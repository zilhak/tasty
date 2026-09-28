//! CenterState 의 세 변형(loading·empty·error)을 두 호스트 크기의 카드에 나란히 보여준다.
//! 본체 file picker·remote attach·Settings › Misc › Scripts 와 같은 공용 위젯을 호출한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{CenterState, CenterStateVariant};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 디자인 무대 카드 폭(`--tasty-size-288`). 목록 영역을 흉내 내는 액자다.
const CARD_W: LogicalPx = LogicalPx(288.0);
/// file picker·remote attach 호스트 카드 높이(디자인 `--tasty-size-220`).
const CARD_H_PICKER: LogicalPx = LogicalPx(220.0);
/// Settings › Misc › Scripts 호스트 카드 높이(디자인 `--tasty-size-160`).
const CARD_H_SCRIPTS: LogicalPx = LogicalPx(160.0);

#[derive(Clone, Copy)]
enum Host {
    Picker,
    Scripts,
}

const VARIANTS: [CenterStateVariant; 3] = [
    CenterStateVariant::Loading,
    CenterStateVariant::Empty,
    CenterStateVariant::Error,
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
            ("height", "none — centres in the list region"),
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
            for v in VARIANTS {
                card(ui, theme, host, v);
            }
        });
    });
}

fn card(ui: &mut egui::Ui, theme: &Theme, host: Host, v: CenterStateVariant) {
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
    let (title, sub) = copy(host, v);
    let glyph = match host {
        Host::Picker => icons::FOLDER_OPEN,
        Host::Scripts => icons::SCRIPT,
    };
    let state = match v {
        CenterStateVariant::Loading => CenterState::loading(title),
        CenterStateVariant::Empty => CenterState::empty(glyph, title),
        CenterStateVariant::Error => CenterState::error(icons::ALERT_CIRCLE, title),
    };
    state.sub_line(sub).show_in(ui, theme, region);

    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(bw, theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
}
