//! 내용 영역 대신 그리는 상태 화면(글리프 · 제목 · 보조 줄 · 이유 줄 · 버튼 줄).
//! 탐색기 내용 영역과 이미지 캔버스가 함께 쓴다. 시안 `ExpState` 를 따른다: 가운데 정렬한
//! 글리프 · 제목(body) · 선택 보조 줄(caption, text-muted) · 선택 이유 줄(mono caption, text-muted) ·
//! 선택 버튼 줄을 space-sm 간격으로 쌓고, 버튼 줄은 space-xs 를 더 띄운다.
//! 영역 높이가 `explorer_state_compact_below()` 미만이면 [`compact_state_row`] 한 줄로 바꾸고
//! 보조 줄과 이유 문구는 제목 툴팁으로 옮긴다. 블록이 잘리지 않게 하기 위해서다.
//! 글리프는 호출자가 그린다. 플러그인은 egui 이미지 대신 구운 폴리라인으로 아이콘을 그리기 때문이다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::{
    Button, ButtonVariant, CompactStateGlyph, CompactStateRow, ControlSize, Spinner,
    compact_state_row,
};

/// 시안 글리프 확대 비율(`transform: scale(1.6)`). 대응 토큰이 없다.
const GLYPH_SCALE: f32 = 1.6;
/// 시안 보조 줄·이유 줄 최대 폭(`maxWidth: 200`). Theme 역할에 연결하지 않은 화면 전용 고정 치수다(ADR-0035).
const SUB_MAX_W: LogicalPx = LogicalPx(200.0);
/// 시안 버튼 크기(`size="sm"`).
const ACTION_SIZE: ControlSize = ControlSize::Sm;

/// 받은 사각형에 글리프를 주어진 색으로 그리는 함수.
pub type GlyphPainter<'a> = &'a dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32);

/// 상태 화면의 글리프 — 호출자가 그리는 아이콘 또는 Spinner.
#[derive(Clone, Copy)]
pub enum StateGlyph<'a> {
    Paint(GlyphPainter<'a>),
    Spinner,
}

/// 상태 화면 하나의 내용.
pub struct StateScreenView<'a> {
    pub glyph: StateGlyph<'a>,
    pub glyph_color: egui::Color32,
    pub title: &'a str,
    pub title_color: egui::Color32,
    pub sub: Option<&'a str>,
    /// 번역하지 않은 OS·디코더 이유 문구(mono).
    pub reason: Option<&'a str>,
    /// 버튼(라벨, variant). 순서대로 가운데 정렬해 그린다.
    pub actions: &'a [(&'a str, ButtonVariant)],
}

/// `region` 전체를 상태 화면으로 쓰고 누른 버튼의 순번을 돌려준다.
pub fn state_screen(
    ui: &mut egui::Ui,
    theme: &Theme,
    region: egui::Rect,
    s: &StateScreenView<'_>,
) -> Option<usize> {
    if region.height() < theme.explorer_state_compact_below().value() {
        return show_compact(ui, theme, region, s);
    }
    let muted = theme.text_muted().to_egui();
    // transform: scale 은 배치에 영향이 없다. 배치는 원래 글리프 크기로 하고 그림만 확대한다.
    let glyph_box = theme.icon_glyph_size_md.value();
    let glyph = glyph_box * GLYPH_SCALE;
    let gap = theme.spacing_sm.value();
    let inner_w = (region.width() - theme.spacing_lg.value() * 2.0).max(0.0);
    let center = |text: &str, font: egui::FontId, color: egui::Color32, max_w: f32| {
        let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, max_w);
        job.halign = egui::Align::Center;
        ui.painter().layout_job(job)
    };
    let caption = theme.font_size_caption.value();
    let narrow = SUB_MAX_W.value().min(inner_w);
    let title = center(
        s.title,
        egui::FontId::proportional(theme.font_size_body.value()),
        s.title_color,
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
    let top = region.center().y - block_h * 0.5;
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(region.center().x, top + glyph_box * 0.5),
        egui::vec2(glyph, glyph),
    );
    match s.glyph {
        StateGlyph::Paint(paint) => paint(ui, glyph_rect, s.glyph_color),
        StateGlyph::Spinner => {
            let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(glyph_rect));
            Spinner::new().size(glyph).show(&mut slot, theme);
        }
    }
    // 가운데 정렬 job 의 원점은 줄 가운데다.
    let mut y = top + glyph_box + gap;
    let title_h = title.rect.height();
    ui.painter()
        .galley(egui::pos2(region.center().x, y), title, s.title_color);
    y += title_h;
    for g in [sub, reason].into_iter().flatten() {
        y += gap;
        let h = g.rect.height();
        ui.painter()
            .galley(egui::pos2(region.center().x, y), g, muted);
        y += h;
    }
    if s.actions.is_empty() {
        return None;
    }
    y += gap + theme.spacing_xs.value();
    action_row(ui, theme, region, y, s.actions)
}

/// 낮은 영역의 한 줄 상태. 보조 줄·이유 문구는 제목 툴팁이고 버튼은 줄에 남는다.
fn show_compact(
    ui: &mut egui::Ui,
    theme: &Theme,
    region: egui::Rect,
    s: &StateScreenView<'_>,
) -> Option<usize> {
    let row = CompactStateRow {
        glyph: match s.glyph {
            StateGlyph::Paint(paint) => CompactStateGlyph::Paint(paint),
            StateGlyph::Spinner => CompactStateGlyph::Spinner,
        },
        glyph_color: s.glyph_color,
        title: s.title,
        title_color: s.title_color,
        tooltip: s.reason.or(s.sub),
        actions: s.actions,
    };
    compact_state_row(ui, theme, region, &row)
}

/// 시안 버튼 줄(`display: flex; gap: space-sm`)을 가운데에 놓는다.
fn action_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    region: egui::Rect,
    top: f32,
    actions: &[(&str, ButtonVariant)],
) -> Option<usize> {
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
        egui::pos2(region.center().x - row_w * 0.5, top),
        egui::vec2(row_w, ACTION_SIZE.height(theme)),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = gap;
    let mut clicked = None;
    for (i, (label, variant)) in actions.iter().enumerate() {
        if Button::new(label)
            .variant(*variant)
            .size(ACTION_SIZE)
            .show(&mut child, theme)
            .clicked()
        {
            clicked = Some(i);
        }
    }
    clicked
}
