//! 탐색기 내용 영역의 상태 화면 네 개(빈 폴더 · 권한 거부 · 불러오는 중 · 읽기 오류)와
//! 즐겨찾기 추가·이름 변경 팝업을 한 무대에 놓은 예제.

mod popups;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Spinner};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 시안 상태 줄의 최대 폭(`maxWidth: 700`)과 상태 칸 높이(`height: 180`). 전시 치수다.
const ROW_W: LogicalPx = LogicalPx(700.0);
const STATE_H: LogicalPx = LogicalPx(180.0);
/// 시안 글리프 확대 비율(`transform: scale(1.6)`). 대응 토큰이 없다.
const GLYPH_SCALE: f32 = 1.6;
/// 시안 보조 줄·이유 줄 최대 폭(`maxWidth: 200`). 대응 토큰이 없다.
const SUB_MAX_W: LogicalPx = LogicalPx(200.0);
/// 시안 팝업 두 장 사이 간격(`gap: 18`). 대응 토큰이 없다.
const POPUP_GAP: LogicalPx = LogicalPx(18.0);

/// 상태 칸의 글리프 — 아이콘 또는 Spinner.
#[derive(Clone, Copy)]
enum StateGlyph {
    Icon(MockGlyph),
    Spinner,
}

/// 글리프와 제목의 색조. 중립은 글리프 text-muted · 제목 text-secondary 다.
#[derive(Clone, Copy)]
enum Tone {
    Neutral,
    /// 권한 거부 — accent-warning(peach).
    Warning,
    /// 읽기 오류 — explorer-error-fg(→ accent-danger).
    Error,
}

struct StateCell {
    glyph: StateGlyph,
    tone: Tone,
    title: &'static str,
    sub: Option<&'static str>,
    /// 번역하지 않은 OS 이유 문구(mono caption).
    reason: Option<&'static str>,
    /// 버튼 줄(라벨, variant). 시안 Retry(secondary sm) · Go up(ghost sm).
    actions: &'static [(&'static str, ButtonVariant)],
}

const STATES: &[StateCell] = &[
    StateCell {
        glyph: StateGlyph::Icon(icons::FOLDER_OPEN),
        tone: Tone::Neutral,
        title: "This folder is empty",
        sub: None,
        reason: None,
        actions: &[],
    },
    StateCell {
        glyph: StateGlyph::Icon(icons::LOCK),
        tone: Tone::Warning,
        title: "Permission denied",
        sub: Some("You don't have access to read this folder."),
        reason: None,
        actions: &[],
    },
    StateCell {
        glyph: StateGlyph::Spinner,
        tone: Tone::Neutral,
        title: "Loading…",
        sub: None,
        reason: None,
        actions: &[],
    },
    StateCell {
        glyph: StateGlyph::Icon(icons::ALERT_TRIANGLE),
        tone: Tone::Error,
        title: "Can't read this folder",
        sub: None,
        reason: Some("No such file or directory (os error 2)"),
        actions: &[
            ("Retry", ButtonVariant::Secondary),
            ("Go up", ButtonVariant::Ghost),
        ],
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
            ui.horizontal(|ui| {
                let gap = theme.spacing_md.value();
                ui.spacing_mut().item_spacing.x = gap;
                let n = STATES.len() as f32;
                let cell_w = (ROW_W.value() - gap * (n - 1.0)) / n;
                for s in STATES {
                    state_cell(ui, theme, s, cell_w);
                }
            });
            // 시안 팝업 줄은 `justifyContent: center` 라 상태 줄과 같은 축의 가운데에 둔다.
            ui.horizontal_top(|ui| {
                let row_w = theme.measure_sm * 2.0 + POPUP_GAP;
                ui.add_space(((ROW_W - row_w) * 0.5).value().max(0.0));
                ui.spacing_mut().item_spacing.x = POPUP_GAP.value();
                popups::favorite(ui, theme);
                popups::rename(ui, theme);
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("empty", "centered glyph + line"),
            ("permission", "warning (peach) tone"),
            ("loading", "Spinner — busy-indicator policy"),
            (
                "read error",
                "alertTriangle · explorer-error-fg · mono OS reason (max 200, untranslated)",
            ),
            (
                "error actions",
                "Retry secondary sm (re-read) · Go up ghost sm (parent, hidden at a root)",
            ),
            ("popups", "Popup language · ghost + primary footer"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "permission tone",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "explorer-error-fg",
                "read error tone",
                theme.explorer_error_fg().to_egui(),
            ),
            TokenChip::new(
                "spinner-indicator",
                "loading",
                theme.spinner_indicator().to_egui(),
            ),
            TokenChip::without_color("shadow-modal", "popup lift"),
        ],
    );

    spec::note(
        ui,
        theme,
        "Explorer font (family + size, 14px cap) gets an Appearance › Explorer sub-tab \
         mirroring the Terminal/Markdown font sections — same control layout, deferred to the \
         Settings specimen.",
    );
}

/// 내용 영역 상태 칸 하나 — 가운데 정렬한 글리프 · 제목 · 보조 줄 · 이유 줄 · 버튼 줄.
fn state_cell(ui: &mut egui::Ui, theme: &Theme, s: &StateCell, w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, STATE_H.value()), egui::Sense::hover());
    let radius = theme.corner_radius.value();
    ui.painter()
        .rect_filled(rect, radius, theme.bg_panel().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
        egui::StrokeKind::Inside,
    );

    let muted = theme.text_muted().to_egui();
    let (glyph_fg, title_fg) = match s.tone {
        Tone::Neutral => (muted, theme.text_secondary().to_egui()),
        Tone::Warning => {
            let c = theme.accent_warning().to_egui();
            (c, c)
        }
        Tone::Error => {
            let c = theme.explorer_error_fg().to_egui();
            (c, c)
        }
    };
    // transform: scale 은 배치에 영향이 없다. 배치는 원래 글리프 크기로 하고 확대해서 그린다.
    let glyph_box = theme.icon_glyph_size_md.value();
    let glyph = glyph_box * GLYPH_SCALE;
    let gap = theme.spacing_sm.value();
    let inner_w = (w - theme.spacing_lg.value() * 2.0).max(0.0);
    let narrow = SUB_MAX_W.value().min(inner_w);
    let caption = theme.font_size_caption.value();
    let center = |text: &str, font: egui::FontId, color: egui::Color32, max_w: f32| {
        let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, max_w);
        job.halign = egui::Align::Center;
        ui.painter().layout_job(job)
    };
    let title = center(
        s.title,
        egui::FontId::proportional(theme.font_size_body.value()),
        title_fg,
        inner_w,
    );
    let sub = s
        .sub
        .map(|t| center(t, egui::FontId::proportional(caption), muted, narrow));
    let reason = s
        .reason
        .map(|t| center(t, egui::FontId::monospace(caption), muted, narrow));
    let action_h = ACTION_SIZE.height(theme);
    let mut block_h = glyph_box + gap + title.rect.height();
    for g in [&sub, &reason].into_iter().flatten() {
        block_h += gap + g.rect.height();
    }
    if !s.actions.is_empty() {
        block_h += gap + theme.spacing_xs.value() + action_h;
    }
    let mut y = rect.center().y - block_h * 0.5;
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, y + glyph_box * 0.5),
        egui::vec2(glyph, glyph),
    );
    match s.glyph {
        StateGlyph::Icon(g) => {
            g.image(glyph, glyph_fg).paint_at(ui, glyph_rect);
        }
        StateGlyph::Spinner => {
            let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(glyph_rect));
            Spinner::new().size(glyph).show(&mut slot, theme);
        }
    }
    // 가운데 정렬 job 은 원점이 줄의 가운데이므로 칸 가운데에 둔다.
    y += glyph_box + gap;
    let title_h = title.rect.height();
    ui.painter()
        .galley(egui::pos2(rect.center().x, y), title, title_fg);
    y += title_h;
    for g in [sub, reason].into_iter().flatten() {
        y += gap;
        let h = g.rect.height();
        ui.painter()
            .galley(egui::pos2(rect.center().x, y), g, muted);
        y += h;
    }
    if !s.actions.is_empty() {
        action_row(
            ui,
            theme,
            rect,
            y + gap + theme.spacing_xs.value(),
            s.actions,
        );
    }
}

/// 시안 버튼 크기(`size="sm"`).
const ACTION_SIZE: ControlSize = ControlSize::Sm;

/// 시안 버튼 줄(`display: flex; gap: space-sm`)을 칸 가운데에 놓는다.
fn action_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    top: f32,
    actions: &[(&str, ButtonVariant)],
) {
    let font = egui::FontId::proportional(ACTION_SIZE.font_size(theme));
    let pad = ACTION_SIZE.pad_x(theme) * 2.0;
    let gap = theme.spacing_sm.value();
    let row_w = actions
        .iter()
        .map(|(l, _)| {
            ui.painter()
                .layout_no_wrap((*l).to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width()
                + pad
        })
        .sum::<f32>()
        + gap * (actions.len() as f32 - 1.0);
    let row = egui::Rect::from_min_size(
        egui::pos2(rect.center().x - row_w * 0.5, top),
        egui::vec2(row_w, ACTION_SIZE.height(theme)),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = gap;
    for (label, variant) in actions {
        Button::new(label)
            .variant(*variant)
            .size(ACTION_SIZE)
            .show(&mut child, theme);
    }
}
