//! UI 문장 안의 CLI 조각(code run) 예제. "Hint text" 절의 두 번째 spec 으로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    UiCodeContainer, UiCodeTokens, center_ui_copy_rows, paint_ui_copy, ui_copy, ui_copy_in,
    ui_copy_job, ui_copy_size,
};

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

const SAMPLE: &str =
    "Port 7420 is in use. Start Tasty with `--webhook-port 7421` or close the other app.";

const WHEN: &str = "The proportional UI face joins two hyphens into one dash, so `--webhook-port` \
reads as an en dash. Any CLI command, option or argument inside a UI sentence (error screen, \
banner, settings caption, result card) is drawn as a code run: mono at the sentence's own size, \
a surface-raised fill, 4px side padding, no vertical padding (the line height does not change), \
radius-sm, text-primary ink. The run never breaks inside; the sentence wraps around it. The copy \
keeps the literal option, so it can be typed back. Terminal content is not affected.";

/// 크기 라벨과 예문 한 벌.
fn sample(ui: &mut egui::Ui, theme: &Theme, label: &str, size: LogicalPx, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new(label)
                .monospace()
                .size(theme.font_size_micro.value())
                .color(egui::Color32::from(theme.text_muted())),
        );
        ui_copy(ui, theme, SAMPLE, size, color);
    });
}

const FOLLOWUPS_TITLE: &str =
    "Code runs — on raised containers · info modal chips · line start · centred hints";

const FOLLOWUPS_WHEN: &str = "On a raised container (toast card, tooltip, any surface-raised box) \
the run's fill would equal the card, so it switches to ui-code-bg-on-raised (bg-panel — a recessed \
well). Rule: the fill is always one step off the container — surface-raised on panels and app bg, \
bg-panel on raised. Info modal command chips become ordinary code runs: sentence size, 4px padding, \
radius-sm. A run that starts a wrapped line keeps its text on the column and lets the fill extend \
4px outward. Centred hints centre every line, including the line the run moved to.";

const RAISED_SAMPLE: &str = "Copied. Run `tasty agent task-list` to see it.";
const MODAL_SAMPLE: &str = "Reset the permission with `tccutil reset`, then sign the build with \
`./scripts/macos-codesign-identity.sh --create`.";
const CENTRED_SAMPLE: &str = "Create one with: `tasty agent task-create --workspace-id 1 ...`";

/// `spec::spec`와 같은 제목·설명 배치. 설명 문장의 CLI 조각도 code run 으로 그린다.
fn heading(ui: &mut egui::Ui, theme: &Theme, title: &str, when: &str) {
    ui.add_space(theme.spacing_xl.value());
    ui.label(
        egui::RichText::new(title)
            .size(theme.font_size_term_lg.value())
            .strong()
            .color(egui::Color32::from(theme.text_primary())),
    );
    ui.add_space(theme.spacing_xs.value());
    ui_copy(
        ui,
        theme,
        when,
        theme.font_size_body,
        egui::Color32::from(theme.text_secondary()),
    );
    ui.add_space(theme.spacing_md.value());
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    heading(ui, theme, "CLI runs inside UI copy", WHEN);
    stage(ui, theme, StageVariant::Column, |ui| {
        ui.set_max_width(theme.measure_md.value());
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
        sample(
            ui,
            theme,
            "13 body",
            theme.font_size_body,
            egui::Color32::from(theme.text_secondary()),
        );
        sample(
            ui,
            theme,
            "11 caption, muted",
            theme.font_size_caption,
            egui::Color32::from(theme.text_muted()),
        );
    });
    meta(
        ui,
        theme,
        &[
            (
                "font",
                "ui-code-font → font-mono · size = the sentence (1em)",
            ),
            ("fill", "ui-code-bg → surface-raised"),
            ("ink", "ui-code-fg → text-primary"),
            ("padding", "ui-code-padding-x 4 · no vertical"),
            ("radius", "ui-code-radius → radius-sm"),
            ("wrap", "never inside the run"),
            (
                "scope",
                "UI copy only · not terminal content · i18n strings keep the literal option",
            ),
        ],
        &[
            TokenChip::without_color("ui-code-font", "mono"),
            TokenChip::new("ui-code-bg", "fill", UiCodeTokens::of(theme).bg),
            TokenChip::new("ui-code-fg", "ink", UiCodeTokens::of(theme).fg),
            TokenChip::without_color("ui-code-padding-x", "→ space-xs"),
            TokenChip::without_color("ui-code-radius", "→ radius-sm"),
        ],
    );
}

/// 토스트 카드처럼 surface-raised 용기에 담은 문장.
fn raised_card(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::new()
        .fill(theme.toast_bg().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(
            theme.spacing_md,
            theme.spacing_sm,
        ))
        .show(ui, |ui| {
            ui_copy_in(
                ui,
                theme,
                UiCodeContainer::Raised,
                RAISED_SAMPLE,
                theme.font_size_body,
                theme.text_primary().to_egui(),
            );
        });
}

/// DAG 빈 화면 안내처럼 measure-sm 폭에서 줄을 바꾸고 줄마다 가운데에 맞춘 caption.
fn centred_hint(ui: &mut egui::Ui, theme: &Theme) {
    let job = ui_copy_job(
        theme,
        CENTRED_SAMPLE,
        theme.font_size_caption,
        theme.text_muted().to_egui(),
        theme.measure_sm.value(),
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    let galley = std::sync::Arc::new(center_ui_copy_rows(theme, &galley));
    let size = ui_copy_size(theme, &galley);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size.y),
        egui::Sense::hover(),
    );
    let pos = egui::pos2(rect.center().x - size.x / 2.0, rect.min.y);
    paint_ui_copy(ui.painter(), theme, UiCodeContainer::Panel, pos, galley);
}

/// b12 후속 결정 spec. 같은 절의 세 번째 spec 으로 그린다.
pub fn draw_followups(ui: &mut egui::Ui, theme: &Theme) {
    heading(ui, theme, FOLLOWUPS_TITLE, FOLLOWUPS_WHEN);
    stage(ui, theme, StageVariant::Column, |ui| {
        ui.set_max_width(theme.measure_md.value());
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
        raised_card(ui, theme);
        ui_copy(
            ui,
            theme,
            MODAL_SAMPLE,
            theme.font_size_body,
            theme.text_secondary().to_egui(),
        );
        centred_hint(ui, theme);
    });
    meta(
        ui,
        theme,
        &[
            (
                "on raised",
                "ui-code-bg-on-raised → bg-panel (toast · tooltip · raised cards)",
            ),
            ("elsewhere", "ui-code-bg → surface-raised (unchanged)"),
            (
                "info modal",
                "command chips = code runs (sentence size · padding-x 4 · radius-sm)",
            ),
            (
                "line start",
                "text on the column · fill may extend 4 outside it",
            ),
            ("centred hint", "every line centred (text-align center)"),
        ],
        &[
            TokenChip::new(
                "ui-code-bg-on-raised",
                "→ bg-panel",
                UiCodeTokens::on(theme, UiCodeContainer::Raised).bg,
            ),
            TokenChip::new("ui-code-bg", "→ surface-raised", UiCodeTokens::of(theme).bg),
        ],
    );
}
