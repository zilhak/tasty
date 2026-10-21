//! 설정 Handler › File Detectors·File Handlers 서브탭의 부품. 본체와 갤러리가 함께 그린다.
//!
//! 목록 행은 두 줄이다. 첫 줄은 id · 출처 Tag · `prio N` · 대기 Tag · Switch · 끝 칸이고,
//! 둘째 줄은 규칙 또는 동작 요약 한 줄이다. 추가 카드는 라벨 열과 컨트롤 열로 된 필드 행을
//! 쌓고, 컨트롤 아래에는 오류 · 안내 · caption 가운데 하나만 둔다.
//! 상태 판단(대기·검증)은 호출자가 하고 여기서는 받은 값을 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::{
    Button, ButtonVariant, ControlSize, TagVariant, Tooltip, tag, tag_disabled, tag_width,
};

/// 행의 출처 Tag 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhOrigin {
    pub label: String,
    /// `plugin:<id>` 출처는 agent 변형 Tag를 쓴다.
    pub plugin: bool,
    /// 저장하면 사라질 기여. disabled Tag에 취소선을 긋는다.
    pub struck: bool,
}

/// 끝 칸. 사용자 기여가 없으면 자물쇠, 대기 중이면 Undo, 그 밖에는 Remove다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FhTrailing {
    Lock,
    Remove,
    Undo,
}

/// 요약 줄의 조각. 저장하면 사라질 사용자 규칙은 취소선과 removed 색으로 그린다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FhSummarySeg {
    pub text: String,
    pub struck: bool,
}

/// 요약 조각 사이 구분자. 시안 `"  ·  "`.
pub const FH_SUMMARY_SEP: &str = "  ·  ";

/// 목록 행 하나.
#[derive(Debug, Clone)]
pub struct FhRowView<'a> {
    pub id: &'a str,
    pub origins: Vec<FhOrigin>,
    pub prio: Option<i32>,
    pub pending: Option<&'a str>,
    /// 삭제 대기 — id에 취소선을 긋는다.
    pub strike_id: bool,
    /// false면 id와 요약을 흐리게 그린다(꺼짐 또는 삭제 대기).
    pub on: bool,
    pub switch_on: bool,
    pub switch_disabled: bool,
    pub trailing: FhTrailing,
    /// 자물쇠 툴팁 문구.
    pub locked_tooltip: &'a str,
    pub summary: Vec<FhSummarySeg>,
    pub undo_label: &'a str,
    pub remove_label: &'a str,
    /// 출처 Tag들이 id를 말줄임시킬 때 대신 그리는 한 Tag(`N sources`).
    pub sources_label: &'a str,
}

/// 행에서 일어난 조작.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FhRowEvents {
    /// Switch를 눌렀으면 새 값.
    pub toggled: Option<bool>,
    pub remove: bool,
    pub undo: bool,
}

/// 요약 조각을 이은 전체 문자열. 툴팁과 시험이 쓴다.
pub fn fh_summary_text(segs: &[FhSummarySeg]) -> String {
    segs.iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(FH_SUMMARY_SEP)
}

/// 출처 Tag들을 하나로 접을지. 여러 출처가 id를 말줄임시키면 접는다.
/// 같은 폭에서는 언제나 같은 답이라 프레임마다 흔들리지 않는다.
pub fn fh_origins_collapse(
    id_width: LogicalPx,
    id_room_with_all_tags: LogicalPx,
    origins: usize,
) -> bool {
    origins > 1 && id_width > id_room_with_all_tags
}

/// 목록 머리글 — mono micro caps, text-muted. 예 "Registered detectors (6)".
pub fn fh_list_head(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .monospace()
            .size(theme.font_size_micro.value())
            .extra_letter_spacing(theme.letter_spacing_caps(theme.font_size_micro).value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 빈 목록 한 줄 — settings-row-min-height, caption, empty-fg, 아래 구분선.
pub fn fh_empty(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let w = LogicalPx(ui.available_width());
    let h = theme.settings_row_min_height();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w.value(), h.value()), egui::Sense::hover());
    let text_w = (w - theme.spacing_xs * 2.0).max(LogicalPx(0.0));
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.file_handler_empty_fg().to_egui(),
        text_w.value(),
    );
    let text_h = LogicalPx(galley.rect.height());
    let pos = egui::pos2(
        rect.left() + theme.spacing_xs.value(),
        rect.center().y - (text_h * 0.5).value(),
    );
    ui.painter()
        .galley(pos, galley, theme.file_handler_empty_fg().to_egui());
    separator_under(ui, theme, rect);
}

fn separator_under(ui: &egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
    );
}

/// 서브탭 머리 — 설명과 출처 불릿(왼쪽) · "Add …" 버튼(오른쪽, secondary sm, plus 글리프).
/// 카드가 열린 동안 버튼은 disabled다. 좁으면 버튼이 설명 아래로 내려간다. 눌렸으면 true.
pub fn fh_intro(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &str,
    bullets: &[(&str, &str)],
    add_label: &str,
    adding: bool,
) -> bool {
    let gap = theme.spacing_md;
    let plus = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        tasty_icons::PLUS.image(rect.height(), c).paint_at(ui, rect);
    };
    let button = Button::new(add_label)
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .leading_icon(&plus)
        .enabled(!adding);
    let button_w = add_button_width(ui, theme, add_label);
    let total = LogicalPx(ui.available_width());
    // 시안 flex 1 1 240: 설명 열이 240보다 좁아지면 버튼이 아래로 내려간다.
    let min_text = crate::tokens::FH_INTRO_TEXT_MIN * theme.ui_zoom;
    let side = total >= min_text + gap + button_w;
    let text_w = if side {
        (total - gap - button_w).min(theme.measure_md)
    } else {
        total.min(theme.measure_md)
    };
    let mut clicked = false;
    let draw_text = |ui: &mut egui::Ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(text_w.value(), 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(text_w.value());
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                intro_paragraph(ui, theme, text);
                for (key, desc) in bullets {
                    intro_bullet(ui, theme, key, desc);
                }
            },
        );
    };
    if side {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap.value();
            draw_text(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                clicked = button.show(ui, theme).clicked();
            });
        });
    } else {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = gap.value();
            draw_text(ui);
            clicked = button.show(ui, theme).clicked();
        });
    }
    clicked
}

/// Add 버튼의 그려질 폭 — 라벨 폭 + plus 글리프 + gap + 좌우 여백.
fn add_button_width(ui: &egui::Ui, theme: &Theme, label: &str) -> LogicalPx {
    let size = ControlSize::Sm;
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(size.font_size(theme)),
        egui::Color32::PLACEHOLDER,
    );
    LogicalPx(galley.rect.width())
        + theme.icon_glyph_size_md
        + theme.button_gap()
        + LogicalPx(size.pad_x(theme)) * 2.0
}

fn intro_paragraph(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        )
        .wrap(),
    );
}

/// 불릿 한 줄 — `• <key mono text-secondary> — <desc>`.
fn intro_bullet(ui: &mut egui::Ui, theme: &Theme, key: &str, desc: &str) {
    let size = theme.font_size_caption.value();
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let muted = theme.text_muted().to_egui();
    let fmt = |font: egui::FontId, color: egui::Color32| egui::TextFormat {
        font_id: font,
        color,
        ..Default::default()
    };
    job.append("•  ", 0.0, fmt(egui::FontId::proportional(size), muted));
    job.append(
        key,
        0.0,
        fmt(
            egui::FontId::monospace(size),
            theme.text_secondary().to_egui(),
        ),
    );
    job.append(
        &format!(" — {desc}"),
        0.0,
        fmt(egui::FontId::proportional(size), muted),
    );
    ui.label(job);
}

/// 필드 아래 한 줄. 오류가 안내보다, 안내가 caption보다 먼저다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FhLine<'a> {
    None,
    Caption(&'a str),
    Info(&'a str),
    Error(&'a str),
}

impl<'a> FhLine<'a> {
    /// 세 후보 가운데 보일 하나.
    pub fn pick(error: Option<&'a str>, info: Option<&'a str>, caption: Option<&'a str>) -> Self {
        match (error, info, caption) {
            (Some(e), _, _) => Self::Error(e),
            (None, Some(i), _) => Self::Info(i),
            (None, None, Some(c)) => Self::Caption(c),
            (None, None, None) => Self::None,
        }
    }
}

/// 카드 필드 행 하나. 라벨 열 settings-label-width(줄바꿈) · gap settings-label-gap · 컨트롤 열.
/// 카드가 라벨 열 + gap + field-width-lg보다 좁으면 컨트롤 열이 라벨 아래로 내려간다.
pub fn fh_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    line: FhLine<'_>,
    control: impl FnOnce(&mut egui::Ui),
) {
    let label_w = theme.settings_label_width();
    let gap = theme.settings_label_gap();
    let total = LogicalPx(ui.available_width());
    let side = total >= label_w + gap + theme.field_width_lg;
    let draw_label = |ui: &mut egui::Ui, w: LogicalPx| {
        ui.allocate_ui_with_layout(
            egui::vec2(w.value(), theme.input_height().value()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(w.value());
                ui.set_min_height(theme.input_height().value());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(theme.font_size_body.value())
                            .color(theme.file_handler_form_label_fg().to_egui()),
                    )
                    .wrap(),
                );
            },
        );
    };
    let draw_control = |ui: &mut egui::Ui, w: LogicalPx| {
        ui.allocate_ui_with_layout(
            egui::vec2(w.value(), 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(w.value());
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                control(ui);
                fh_line(ui, theme, line);
            },
        );
    };
    if side {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap.value();
            draw_label(ui, label_w);
            draw_control(ui, (total - label_w - gap).max(LogicalPx(0.0)));
        });
    } else {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            draw_label(ui, total);
            draw_control(ui, total);
        });
    }
}

/// 컨트롤 아래 한 줄을 그린다. 오류는 alertCircle + form-error-fg, 안내는 info + form-info-fg,
/// caption은 글리프 없이 form-caption-fg다. 모두 caption 크기이며 줄을 바꾼다.
pub fn fh_line(ui: &mut egui::Ui, theme: &Theme, line: FhLine<'_>) {
    let (text, glyph, color) = match line {
        FhLine::None => return,
        FhLine::Caption(t) => (t, None, theme.file_handler_form_caption_fg()),
        FhLine::Info(t) => (
            t,
            Some(tasty_icons::INFO),
            theme.file_handler_form_info_fg(),
        ),
        FhLine::Error(t) => (
            t,
            Some(tasty_icons::ALERT_CIRCLE),
            theme.file_handler_form_error_fg(),
        ),
    };
    let color = color.to_egui();
    let size = theme.font_size_caption.value();
    let Some(glyph) = glyph else {
        ui.add(egui::Label::new(egui::RichText::new(text).size(size).color(color)).wrap());
        return;
    };
    let icon = theme.icon_glyph_size_sm;
    let gap = theme.spacing_xs;
    let w = LogicalPx(ui.available_width());
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(size),
        color,
        (w - icon - gap).max(LogicalPx(0.0)).value(),
    );
    let row_h = LogicalPx(ui.fonts(|f| f.row_height(&egui::FontId::proportional(size))));
    let h = LogicalPx(galley.rect.height()).max(icon);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w.value(), h.value()), egui::Sense::hover());
    // 글리프는 첫 줄 가운데에 둔다.
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.left() + (icon * 0.5).value(),
            rect.top() + (row_h * 0.5).value(),
        ),
        egui::vec2(icon.value(), icon.value()),
    );
    glyph.image(icon.value(), color).paint_at(ui, glyph_rect);
    ui.painter().galley(
        egui::pos2(rect.left() + (icon + gap).value(), rect.top()),
        galley,
        color,
    );
}

/// 카드 아래 버튼 줄의 결과.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FhCardEvents {
    pub cancel: bool,
    pub add: bool,
}

/// 추가 카드 문구.
#[derive(Debug, Clone, Copy)]
pub struct FhCardLabels<'a> {
    pub title: &'a str,
    pub cancel: &'a str,
    pub add: &'a str,
}

/// 추가 카드 — form-bg · 1px form-border · radius · form-padding · 행 간격 form-row-gap.
/// 제목은 mono micro caps form-label-fg. 필드 간 오류는 버튼 줄 바로 위에 둔다.
/// 버튼은 오른쪽 정렬이며 Cancel ghost sm · Add primary sm 순서다. Add는 `can_add`일 때만 눌린다.
pub fn fh_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    labels: FhCardLabels<'_>,
    cross_error: Option<&str>,
    can_add: bool,
    fields: impl FnOnce(&mut egui::Ui),
) -> FhCardEvents {
    let mut ev = FhCardEvents::default();
    egui::Frame::new()
        .fill(theme.file_handler_form_bg().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.file_handler_form_border().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(crate::margin_all(theme.file_handler_form_padding()))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme.file_handler_form_row_gap().value();
            ui.label(
                egui::RichText::new(labels.title.to_uppercase())
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .extra_letter_spacing(theme.letter_spacing_caps(theme.font_size_micro).value())
                    .color(theme.file_handler_form_label_fg().to_egui()),
            );
            fields(ui);
            if let Some(err) = cross_error {
                fh_line(ui, theme, FhLine::Error(err));
            }
            // 버튼 줄 높이를 고정한다. with_layout은 남은 세로 공간을 모두 차지해 카드가 늘어난다.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), theme.button_height_sm().value()),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ev.add = Button::new(labels.add)
                        .variant(ButtonVariant::Primary)
                        .size(ControlSize::Sm)
                        .enabled(can_add)
                        .show(ui, theme)
                        .clicked();
                    ev.cancel = Button::new(labels.cancel)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, theme)
                        .clicked();
                },
            );
        });
    ev
}

/// 목록 행. 위아래 space-sm, 좌우 space-xs, 두 줄 사이 space-xs, 아래 1px 구분선.
pub fn fh_row(ui: &mut egui::Ui, theme: &Theme, view: &FhRowView<'_>) -> FhRowEvents {
    let mut ev = FhRowEvents::default();
    let pad_x = theme.spacing_xs;
    let pad_y = theme.spacing_sm;
    let gap = theme.spacing_sm;
    let full_w = LogicalPx(ui.available_width());
    let inner_w = (full_w - pad_x * 2.0).max(LogicalPx(0.0));
    let top = ui.cursor().top();
    ui.add_space(pad_y.value());
    let dim = !view.on || view.strike_id;

    // 1줄: 오른쪽 묶음(끝 칸 · Switch)을 먼저 오른쪽부터 놓고 남은 폭에 id와 Tag를 둔다.
    let line_h = theme.button_height_sm();
    ui.allocate_ui_with_layout(
        egui::vec2(full_w.value(), line_h.value()),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_height(line_h.value());
            ui.add_space(pad_x.value());
            ui.spacing_mut().item_spacing.x = gap.value();
            match view.trailing {
                FhTrailing::Lock => lock_slot(ui, theme, view.locked_tooltip),
                FhTrailing::Undo => {
                    ev.undo = ghost_sm(ui, theme, view.undo_label);
                }
                FhTrailing::Remove => {
                    ev.remove = ghost_sm(ui, theme, view.remove_label);
                }
            }
            let mut on = view.switch_on;
            if crate::switch(ui, theme, &mut on, None, !view.switch_disabled).changed() {
                ev.toggled = Some(on);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // 좁은 열에서 Tag가 남은 폭을 넘으면 Switch와 끝 칸을 덮는다. 왼쪽 묶음은 남은 폭에서 자른다.
                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                ui.add_space(pad_x.value());
                ui.spacing_mut().item_spacing.x = gap.value();
                row_left(ui, theme, view, dim);
            });
        },
    );

    // 2줄: 요약 한 줄 말줄임, 전체 값은 툴팁.
    ui.add_space(theme.spacing_xs.value());
    ui.horizontal(|ui| {
        ui.add_space(pad_x.value());
        summary_line(ui, theme, view, dim, inner_w);
    });
    ui.add_space(pad_y.value());
    let rect = egui::Rect::from_min_max(
        egui::pos2(ui.min_rect().left(), top),
        egui::pos2(ui.min_rect().left() + full_w.value(), ui.cursor().top()),
    );
    separator_under(ui, theme, rect);
    ev
}

fn ghost_sm(ui: &mut egui::Ui, theme: &Theme, label: &str) -> bool {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(ui, theme)
        .clicked()
}

/// 자물쇠 — icon-size-sm, glyph-dim. 누르는 것이 아니라 읽는 표시이며 툴팁에 출처를 적는다.
fn lock_slot(ui: &mut egui::Ui, theme: &Theme, tooltip: &str) {
    let s = theme.icon_glyph_size_sm;
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(s.value(), s.value()), egui::Sense::hover());
    tasty_icons::LOCK
        .image(s.value(), theme.glyph_dim().to_egui())
        .paint_at(ui, rect);
    if crate::tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
        Tooltip::new(tooltip).show(ui, theme, rect);
    }
}

/// id · 출처 Tag(들) · prio · 대기 Tag. id는 남은 폭에서 말줄임한다.
fn row_left(ui: &mut egui::Ui, theme: &Theme, view: &FhRowView<'_>, dim: bool) {
    let gap = theme.spacing_sm;
    let zero = LogicalPx(0.0);
    let avail = LogicalPx(ui.available_width());
    let id_font = egui::FontId::monospace(theme.font_size_body.value());
    let id_natural = LogicalPx(
        ui.painter()
            .layout_no_wrap(
                view.id.to_owned(),
                id_font.clone(),
                egui::Color32::PLACEHOLDER,
            )
            .rect
            .width(),
    );
    let prio_text = view.prio.map(|p| format!("prio {p}"));
    let prio_font = egui::FontId::monospace(theme.font_size_micro.value());
    let prio_w = prio_text.as_ref().map_or(zero, |t| {
        LogicalPx(
            ui.painter()
                .layout_no_wrap(t.clone(), prio_font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width(),
        ) + gap
    });
    // tag_width는 f32를 돌려주는 기존 helper라 받은 자리에서 감싼다.
    let tag_w = |label: &str| LogicalPx(tag_width(ui, theme, label)) + gap;
    let pending_w = view.pending.map_or(zero, &tag_w);
    let all_tags_w = view
        .origins
        .iter()
        .fold(zero, |sum, o| sum + tag_w(&o.label));
    let fixed = prio_w + pending_w;
    let collapse = fh_origins_collapse(
        id_natural,
        (avail - fixed - all_tags_w).max(zero),
        view.origins.len(),
    );
    let tags_w = if collapse {
        tag_w(view.sources_label)
    } else {
        all_tags_w
    };
    let id_max = (avail - fixed - tags_w).max(zero);

    let id_color = if dim {
        theme.text_secondary()
    } else {
        theme.text_primary()
    }
    .to_egui();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        view.id,
        0.0,
        egui::TextFormat {
            font_id: id_font,
            color: id_color,
            strikethrough: if view.strike_id {
                egui::Stroke::new(theme.border_width.value(), id_color)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(id_max.value());
    let galley = ui.painter().layout_job(job);
    let id_resp = ui.add(egui::Label::new(galley.clone()).selectable(false));
    // 시안은 bold(semibold) id를 쓰지만 egui mono 스택에는 굵은 면이 없다. 같은 자리에
    // 한 번 더 겹쳐 굵기를 낸다.
    ui.painter().galley(
        id_resp.rect.min + egui::vec2((theme.border_width * 0.5).value(), 0.0),
        galley,
        id_color,
    );
    if crate::tooltip_hover_delay_elapsed(ui.ctx(), theme, id_resp.id, id_resp.hovered()) {
        Tooltip::new(view.id).show(ui, theme, id_resp.rect);
    }

    if collapse {
        let full = view
            .origins
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let r = tag(ui, theme, view.sources_label, TagVariant::Default, false);
        if crate::tooltip_hover_delay_elapsed(ui.ctx(), theme, r.id, r.hovered()) {
            Tooltip::new(&full).show(ui, theme, r.rect);
        }
    } else {
        for o in &view.origins {
            if o.struck {
                struck_tag(ui, theme, &o.label);
            } else {
                let variant = if o.plugin {
                    TagVariant::Agent
                } else {
                    TagVariant::Default
                };
                tag(ui, theme, &o.label, variant, false);
            }
        }
    }
    if let Some(t) = prio_text {
        ui.label(
            egui::RichText::new(t)
                .font(prio_font)
                .color(theme.file_handler_prio_fg().to_egui()),
        );
    }
    if let Some(p) = view.pending {
        tag_disabled(ui, theme, p, false);
    }
}

/// 저장하면 사라질 출처 — disabled Tag 위에 라벨 폭만큼 취소선을 긋는다.
fn struck_tag(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    let r = tag_disabled(ui, theme, label, false);
    let pad = theme.tag_padding_x();
    let y = r.rect.center().y;
    ui.painter().hline(
        (r.rect.left() + pad.value())..=(r.rect.right() - pad.value()),
        y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.tag_disabled_fg().to_egui(),
        ),
    );
}

fn summary_line(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &FhRowView<'_>,
    dim: bool,
    max_w: LogicalPx,
) {
    let font = egui::FontId::monospace(theme.font_size_term_sm.value());
    let base = if dim {
        theme.text_muted()
    } else {
        theme.file_handler_summary_fg()
    }
    .to_egui();
    let removed = theme.file_handler_summary_removed_fg().to_egui();
    let mut job = egui::text::LayoutJob::default();
    for (i, seg) in view.summary.iter().enumerate() {
        let plain = egui::TextFormat {
            font_id: font.clone(),
            color: base,
            ..Default::default()
        };
        if i > 0 {
            job.append(FH_SUMMARY_SEP, 0.0, plain.clone());
        }
        let fmt = if seg.struck {
            egui::TextFormat {
                color: removed,
                strikethrough: egui::Stroke::new(theme.border_width.value(), removed),
                ..plain
            }
        } else {
            plain
        };
        job.append(&seg.text, 0.0, fmt);
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.value());
    // Label에 job을 넘기면 가로 레이아웃의 Extend 모드가 말줄임을 덮어쓴다. galley로 먼저 잰다.
    let galley = ui.painter().layout_job(job);
    let resp = ui.add(egui::Label::new(galley).selectable(false));
    if crate::tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
        Tooltip::new(&fh_summary_text(&view.summary)).show(ui, theme, resp.rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_under_a_field_prefers_error_then_info_then_caption() {
        assert_eq!(
            FhLine::pick(Some("e"), Some("i"), Some("c")),
            FhLine::Error("e")
        );
        assert_eq!(FhLine::pick(None, Some("i"), Some("c")), FhLine::Info("i"));
        assert_eq!(FhLine::pick(None, None, Some("c")), FhLine::Caption("c"));
        assert_eq!(FhLine::pick(None, None, None), FhLine::None);
    }

    #[test]
    fn one_origin_never_collapses_and_several_collapse_only_when_the_id_would_ellipsize() {
        assert!(!fh_origins_collapse(LogicalPx(500.0), LogicalPx(10.0), 1));
        assert!(!fh_origins_collapse(LogicalPx(80.0), LogicalPx(120.0), 3));
        assert!(fh_origins_collapse(LogicalPx(130.0), LogicalPx(120.0), 3));
    }

    #[test]
    fn the_summary_tooltip_joins_every_segment_with_the_design_separator() {
        let segs = vec![
            FhSummarySeg {
                text: "ext md, markdown".into(),
                struck: false,
            },
            FhSummarySeg {
                text: "ext mdx".into(),
                struck: true,
            },
        ];
        assert_eq!(fh_summary_text(&segs), "ext md, markdown  ·  ext mdx");
    }
}
