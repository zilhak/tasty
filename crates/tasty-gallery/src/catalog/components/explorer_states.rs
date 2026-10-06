//! 탐색기 내용 영역의 상태 화면 세 개(빈 폴더 · 권한 거부 · 불러오는 중)와
//! 즐겨찾기 추가·이름 변경 팝업을 한 무대에 놓은 예제.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::Spinner;

use super::{explorer_favorite_popup, explorer_rename_popup};
use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip, wrap_item};

/// 시안 상태 줄의 최대 폭(`maxWidth: 700`)과 상태 칸 높이(`height: 180`). 전시 치수다.
const ROW_W: LogicalPx = LogicalPx(700.0);
const STATE_H: LogicalPx = LogicalPx(180.0);
/// 시안 글리프 확대 비율(`transform: scale(1.6)`). 대응 토큰이 없다.
const GLYPH_SCALE: f32 = 1.6;
/// 시안 보조 줄 최대 폭(`maxWidth: 200`). 대응 토큰이 없다.
const SUB_MAX_W: LogicalPx = LogicalPx(200.0);
/// 시안 팝업 두 장 사이 간격(`gap: 18`). 대응 토큰이 없다.
const POPUP_GAP: LogicalPx = LogicalPx(18.0);

/// 상태 칸의 글리프 — 아이콘 또는 Spinner.
#[derive(Clone, Copy)]
enum StateGlyph {
    Icon(MockGlyph),
    Spinner,
}

struct StateCell {
    glyph: StateGlyph,
    /// 권한 거부는 글리프와 제목을 경고 색(peach)으로 칠한다.
    warning: bool,
    title: &'static str,
    sub: Option<&'static str>,
}

const STATES: &[StateCell] = &[
    StateCell {
        glyph: StateGlyph::Icon(icons::FOLDER_OPEN),
        warning: false,
        title: "This folder is empty",
        sub: None,
    },
    StateCell {
        glyph: StateGlyph::Icon(icons::LOCK),
        warning: true,
        title: "Permission denied",
        sub: Some("You don't have access to read this folder."),
    },
    StateCell {
        glyph: StateGlyph::Spinner,
        warning: false,
        title: "Loading…",
        sub: None,
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
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = POPUP_GAP.value();
                wrap_item(ui, |ui| explorer_favorite_popup::card(ui, theme));
                wrap_item(ui, |ui| explorer_rename_popup::card(ui, theme));
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
            ("popups", "Popup language · ghost + primary footer"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "permission tone",
                theme.accent_warning().to_egui(),
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

/// 내용 영역 상태 칸 하나 — 가운데 정렬한 글리프 · 제목 · 보조 줄.
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

    let warning = theme.accent_warning().to_egui();
    let glyph_fg = if s.warning {
        warning
    } else {
        theme.text_muted().to_egui()
    };
    let title_fg = if s.warning {
        warning
    } else {
        theme.text_secondary().to_egui()
    };
    let glyph = theme.icon_glyph_size_md.value() * GLYPH_SCALE;
    let gap = theme.spacing_sm.value();
    let inner_w = (w - theme.spacing_lg.value() * 2.0).max(0.0);
    let title = ui.painter().layout(
        s.title.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        title_fg,
        inner_w,
    );
    let sub = s.sub.map(|t| {
        let mut job = egui::text::LayoutJob::simple(
            t.to_owned(),
            egui::FontId::proportional(theme.font_size_caption.value()),
            theme.text_muted().to_egui(),
            SUB_MAX_W.value().min(inner_w),
        );
        job.halign = egui::Align::Center;
        ui.painter().layout_job(job)
    });
    let block_h =
        glyph + gap + title.rect.height() + sub.as_ref().map_or(0.0, |g| gap + g.rect.height());
    let mut y = rect.center().y - block_h * 0.5;
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, y + glyph * 0.5),
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
    y += glyph + gap;
    let title_h = title.rect.height();
    ui.painter().galley(
        egui::pos2(rect.center().x - title.rect.width() * 0.5, y),
        title,
        title_fg,
    );
    y += title_h;
    if let Some(g) = sub {
        y += gap;
        // 가운데 정렬 job 은 원점이 줄의 가운데이므로 칸 가운데에 둔다.
        ui.painter().galley(
            egui::pos2(rect.center().x, y),
            g,
            theme.text_muted().to_egui(),
        );
    }
}
