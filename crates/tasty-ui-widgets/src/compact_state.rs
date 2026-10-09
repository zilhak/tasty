//! 낮은 내용 영역에서 상태 화면([`crate::state_screen`])을 대신하는 한 줄(compact state row).
//! 글리프 · 제목 · 버튼을 한 줄에 놓고 줄 전체를 받은 영역의 가운데에 둔다.
//! 제목은 남는 폭을 넘으면 끝에서 줄이고, 보조 줄과 OS 이유 문구는 제목의 툴팁으로 옮긴다.
//! 글리프는 확대하지 않은 `icon_glyph_size_md` 크기다. 상태 화면은 영역 높이가
//! `explorer_state_compact_below()` 미만일 때 이 줄로 바꾼다.

use tasty_type_appearance::theme::Theme;

use crate::{Button, ButtonVariant, ControlSize, Spinner};

/// 시안 버튼 크기(`size="sm"`).
const ACTION_SIZE: ControlSize = ControlSize::Sm;

/// 줄 앞의 글리프 — 아이콘, 호출자가 그리는 글리프 또는 Spinner.
#[derive(Clone, Copy)]
pub enum CompactStateGlyph<'a> {
    Icon(tasty_icons::Icon),
    Paint(crate::state_screen::GlyphPainter<'a>),
    Spinner,
}

/// compact 줄 하나의 내용.
pub struct CompactStateRow<'a> {
    pub glyph: CompactStateGlyph<'a>,
    pub glyph_color: egui::Color32,
    pub title: &'a str,
    pub title_color: egui::Color32,
    /// 제목 툴팁. 보조 줄이나 OS 이유 문구를 넣는다.
    pub tooltip: Option<&'a str>,
    /// 버튼(라벨, variant). 순서대로 그린다.
    pub actions: &'a [(&'a str, ButtonVariant)],
}

/// `region` 가운데에 compact 줄을 그리고 누른 버튼의 순번을 돌려준다.
pub fn compact_state_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    region: egui::Rect,
    row: &CompactStateRow<'_>,
) -> Option<usize> {
    let pad = theme.spacing_sm.value();
    let gap = theme.spacing_sm.value();
    let action_gap = theme.spacing_xs.value();
    let glyph = theme.icon_glyph_size_md.value();

    let font = egui::FontId::proportional(ACTION_SIZE.font_size(theme));
    let action_pad = ACTION_SIZE.pad_x(theme) * 2.0;
    let action_widths: Vec<f32> = row
        .actions
        .iter()
        .map(|(label, _)| {
            ui.painter()
                .layout_no_wrap(
                    (*label).to_owned(),
                    font.clone(),
                    egui::Color32::PLACEHOLDER,
                )
                .rect
                .width()
                + action_pad
        })
        .collect();
    let actions_w = if action_widths.is_empty() {
        0.0
    } else {
        action_widths.iter().sum::<f32>() + action_gap * (action_widths.len() as f32 - 1.0)
    };
    let fixed_w = glyph
        + gap
        + if actions_w > 0.0 {
            gap + actions_w
        } else {
            0.0
        };
    let title_max = (region.width() - pad * 2.0 - fixed_w).max(0.0);

    let mut job = egui::text::LayoutJob::simple_singleline(
        row.title.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        row.title_color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(title_max);
    let title = ui.painter().layout_job(job);
    let title_w = title.rect.width().min(title_max);
    let title_h = title.rect.height();

    let row_w = fixed_w + title_w;
    let mut x = region.center().x - row_w * 0.5;
    let cy = region.center().y;

    let glyph_rect =
        egui::Rect::from_center_size(egui::pos2(x + glyph * 0.5, cy), egui::vec2(glyph, glyph));
    match row.glyph {
        CompactStateGlyph::Icon(icon) => {
            icon.image(glyph, row.glyph_color).paint_at(ui, glyph_rect);
        }
        CompactStateGlyph::Paint(paint) => paint(ui, glyph_rect, row.glyph_color),
        CompactStateGlyph::Spinner => {
            let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(glyph_rect));
            Spinner::new().size(glyph).show(&mut slot, theme);
        }
    }
    x += glyph + gap;

    let title_rect = egui::Rect::from_min_size(
        egui::pos2(x, cy - title_h * 0.5),
        egui::vec2(title_w, title_h),
    );
    ui.painter().galley(title_rect.min, title, row.title_color);
    if let Some(tip) = row.tooltip {
        ui.interact(
            title_rect,
            ui.id().with("compact_state_title"),
            egui::Sense::hover(),
        )
        .on_hover_text(tip);
    }
    x += title_w;

    if row.actions.is_empty() {
        return None;
    }
    x += gap;
    let action_h = ACTION_SIZE.height(theme);
    let actions_rect = egui::Rect::from_min_size(
        egui::pos2(x, cy - action_h * 0.5),
        egui::vec2(actions_w, action_h),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(actions_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = action_gap;
    let mut clicked = None;
    for (i, (label, variant)) in row.actions.iter().enumerate() {
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
