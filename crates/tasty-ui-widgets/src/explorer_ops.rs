//! 탐색기 파일 작업의 진행·충돌·결과·드래그 표시. 본체와 갤러리가 같은 함수를 부른다.
//! 문구는 호출자가 번역해 넘기고, 배치와 색은 Theme 토큰에서 읽는다.
//! 디자인의 semibold(충돌 제목, 드래그 대상 폴더)는 굵은 UI 글꼴이 없어 text-primary 색으로 근사한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::button::{Button, ButtonVariant};
use crate::chip::{TagVariant, tag};
use crate::control::ControlSize;
use crate::icon_button::{IconButton, IconPainter};
use crate::spinner::Spinner;
use crate::tokens::{STRUCT_GAP_2, TOAST_PADDING_X, TOAST_PADDING_Y};
use tasty_type_appearance::toast_kind::ToastKind;

/// 대기열 팝오버 폭. 디자인 `explorer-ops.jsx` 의 `--tasty-size-288` 이며 역할 토큰이 없다.
pub fn op_queue_width(theme: &Theme) -> LogicalPx {
    LogicalPx((288.0 * theme.ui_zoom).round())
}

/// 충돌 카드의 비교 줄 라벨 열 폭. 디자인 `--tasty-size-64` 이며 역할 토큰이 없다.
fn conflict_label_width(theme: &Theme) -> f32 {
    (64.0 * theme.ui_zoom).round()
}

/// 충돌 카드 좌우 안쪽 여백. 디자인 `--tasty-size-14` 로 4px 그리드 밖이며 역할 토큰이 없다.
fn conflict_pad_x(theme: &Theme) -> f32 {
    (14.0 * theme.ui_zoom).round()
}

/// 상태줄 오른쪽 끝 여백. 상태줄 왼쪽 글자와 같은 `spacing_md` 다.
fn status_pad(theme: &Theme) -> f32 {
    theme.spacing_md.value()
}

fn caption(theme: &Theme) -> egui::FontId {
    egui::FontId::proportional(theme.font_size_caption.value())
}
fn caption_mono(theme: &Theme) -> egui::FontId {
    egui::FontId::monospace(theme.font_size_caption.value())
}
fn body(theme: &Theme) -> egui::FontId {
    egui::FontId::proportional(theme.font_size_body.value())
}

/// 한 줄로 그리고 넘치면 끝을 말줄임한다.
fn one_line(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_w: f32,
) -> egui::Response {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(1.0));
    let galley = ui.fonts(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter().galley(rect.min, galley, color);
    resp
}

/// 경로를 한 줄로 그린다. 넘치면 앞을 줄여 이름이 남게 한다.
fn path_line(ui: &mut egui::Ui, text: &str, font: egui::FontId, color: egui::Color32, max_w: f32) {
    let ellipsis = "…";
    let fits = |s: &str| {
        ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font.clone(), color).size().x) <= max_w
    };
    let shown = if fits(text) {
        text.to_owned()
    } else {
        let chars: Vec<char> = text.chars().collect();
        let mut lo = 0usize;
        let mut hi = chars.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let candidate: String = std::iter::once(ellipsis.to_owned())
                .chain(std::iter::once(chars[mid..].iter().collect::<String>()))
                .collect();
            if fits(&candidate) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        format!(
            "{ellipsis}{}",
            chars[lo.min(chars.len())..].iter().collect::<String>()
        )
    };
    one_line(ui, &shown, font, color, max_w);
}

// ── 진행 상태줄 ──────────────────────────────────────────

/// 시작한 탐색기의 상태줄에 그리는 진행 표시.
pub struct OpStatusProps<'a> {
    /// 바이트 기준 진행률. None 이면 진행 막대는 트랙만 그린다(크기를 모를 때).
    pub fraction: Option<f32>,
    /// 충돌 답을 기다린다. 막대와 왼쪽 글자를 경고색으로 그리고 Cancel 대신 Show 를 둔다.
    pub waiting: bool,
    /// "Copying 12 of 40 · {file}" 또는 대기 문구.
    pub text: &'a str,
    /// "1.2 / 3.4 GB". 없으면 그리지 않는다.
    pub bytes: Option<&'a str>,
    /// "+n queued" Tag. 없으면 그리지 않는다.
    pub queued: Option<&'a str>,
    pub show_label: &'a str,
    pub cancel_tip: &'a str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpStatusResponse {
    pub cancel: bool,
    pub show: bool,
    /// 왼쪽 글자나 queued Tag 를 눌렀다.
    pub open_queue: bool,
}

/// `rect`(상태줄 전체) 위에 진행 표시를 그린다. 배경과 위쪽 경계선은 호출자가 먼저 그린다.
pub fn op_status_line(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    props: &OpStatusProps<'_>,
    paint_close: IconPainter<'_>,
) -> OpStatusResponse {
    let mut out = OpStatusResponse::default();
    let bar_h = theme.explorer_progress_height().value();
    let track = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), bar_h));
    ui.painter()
        .rect_filled(track, 0.0, theme.explorer_progress_track().to_egui());
    if let Some(fraction) = props.fraction {
        let fill = if props.waiting {
            theme.accent_warning()
        } else {
            theme.explorer_progress_fill()
        };
        let mut done = track;
        done.set_width(track.width() * fraction.clamp(0.0, 1.0));
        ui.painter().rect_filled(done, 0.0, fill.to_egui());
    }

    let inner = rect.shrink2(egui::vec2(status_pad(theme), 0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing = egui::vec2(theme.spacing_sm.value(), 0.0);
    if props.waiting {
        out.show = Button::new(props.show_label)
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .show(&mut child, theme)
            .clicked();
    } else {
        out.cancel = IconButton::new()
            .size(ControlSize::Sm)
            .show(&mut child, theme, paint_close)
            .on_hover_text(props.cancel_tip)
            .clicked();
    }
    if let Some(queued) = props.queued {
        let resp = tag(&mut child, theme, queued, TagVariant::Default, false)
            .interact(egui::Sense::click());
        out.open_queue |= resp.clicked();
    }
    if let Some(bytes) = props.bytes {
        child.label(
            egui::RichText::new(bytes)
                .font(caption_mono(theme))
                .color(theme.text_muted().to_egui()),
        );
    }
    let left_w = child.available_width();
    let color = if props.waiting {
        theme.accent_warning()
    } else {
        theme.text_secondary()
    };
    child.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        let resp = one_line(ui, props.text, caption(theme), color.to_egui(), left_w)
            .interact(egui::Sense::click());
        out.open_queue |= resp.clicked();
    });
    out
}

// ── 대기열 팝오버 ────────────────────────────────────────

pub struct QueueRowProps<'a> {
    pub title: &'a str,
    pub sub: &'a str,
    /// 실행 중인 작업. Spinner 를 두고 ×는 취소다. 아니면 layers 글리프와 대기열에서 빼기.
    pub running: bool,
    pub remove_tip: &'a str,
}

/// 메뉴 표면 안에 작업 행을 그린다. 크기·위치·그림자는 호출자가 정한다. 누른 × 의 행 번호를 돌려준다.
pub fn op_queue_rows(
    ui: &mut egui::Ui,
    theme: &Theme,
    rows: &[QueueRowProps<'_>],
    paint_close: IconPainter<'_>,
    paint_queued: IconPainter<'_>,
) -> Option<usize> {
    let mut removed = None;
    let glyph = theme.icon_glyph_size_sm.value();
    for (i, row) in rows.iter().enumerate() {
        let frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(
            theme.spacing_sm.value() as i8,
            theme.spacing_xs.value() as i8,
        ));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(
                    theme.menu_item_height().value() - theme.spacing_xs.value() * 2.0,
                );
                ui.spacing_mut().item_spacing = egui::vec2(theme.spacing_sm.value(), 0.0);
                let muted = theme.text_muted().to_egui();
                if row.running {
                    Spinner::new().size(glyph).color(muted).show(ui, theme);
                } else {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(glyph, glyph), egui::Sense::hover());
                    paint_queued(ui, rect, muted);
                }
                let close_w = ControlSize::Sm.height(theme);
                let text_w = (ui.available_width() - close_w - theme.spacing_sm.value()).max(1.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    one_line(
                        ui,
                        row.title,
                        body(theme),
                        theme.text_primary().to_egui(),
                        text_w,
                    );
                    one_line(ui, row.sub, caption(theme), muted, text_w);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if IconButton::new()
                        .size(ControlSize::Sm)
                        .show(ui, theme, paint_close)
                        .on_hover_text(row.remove_tip)
                        .clicked()
                    {
                        removed = Some(i);
                    }
                });
            });
        });
    }
    removed
}

/// 대기열 팝오버를 `bottom_right` 모서리에 붙여 그린다. 높이는 직전 프레임에 잰 값으로 위치를 정한다.
/// 같은 프레임의 다른 위젯보다 뒤에 불러야 위에 그려지고 클릭을 받는다. 누른 × 의 행 번호를 돌려준다.
pub fn op_queue_popover(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: egui::Id,
    bottom_right: egui::Pos2,
    rows: &[QueueRowProps<'_>],
    paint_close: IconPainter<'_>,
    paint_queued: IconPainter<'_>,
) -> (egui::Rect, Option<usize>) {
    let w = op_queue_width(theme).value();
    let h = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let min = egui::pos2(bottom_right.x - w, bottom_right.y - h);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(egui::Rect::from_min_size(min, egui::vec2(w, h.max(1.0))))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let mut removed = None;
    let rect = op_queue_frame(theme)
        .show(&mut child, |ui| {
            ui.set_width(w - theme.spacing_xs.value() * 2.0 - theme.border_width.value() * 2.0);
            removed = op_queue_rows(ui, theme, rows, paint_close, paint_queued);
        })
        .response
        .rect;
    if (rect.height() - h).abs() > f32::EPSILON {
        ui.ctx().data_mut(|d| d.insert_temp(id, rect.height()));
        ui.ctx().request_repaint();
    }
    (rect, removed)
}

/// 대기열 팝오버의 메뉴 표면. 배경·테두리·반경·안쪽 여백.
pub fn op_queue_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.menu_bg().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.menu_border().to_egui(),
        ))
        .corner_radius(theme.menu_radius().value())
        .shadow(theme.shadow_popover().to_egui())
        .inner_margin(egui::Margin::same(theme.spacing_xs.value() as i8))
}

// ── 이름 충돌 ────────────────────────────────────────────

pub struct ConflictProps<'a> {
    pub title: &'a str,
    pub in_folder: &'a str,
    pub existing_label: &'a str,
    pub existing: &'a str,
    pub incoming_label: &'a str,
    pub incoming: &'a str,
    /// 폴더 충돌 안내("Folders are not merged."). 폴더 충돌일 때만.
    pub no_merge: Option<&'a str>,
    /// 남은 충돌에도 적용하는 체크박스 라벨. 남은 충돌이 있을 때만.
    pub apply_all: Option<&'a str>,
    pub cancel_rest: &'a str,
    pub skip: &'a str,
    /// 파일끼리 충돌할 때만.
    pub replace: Option<&'a str>,
    pub keep_both: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictPick {
    KeepBoth,
    Skip,
    Replace,
    CancelRest,
}

/// 충돌 카드의 내용. 카드 표면(배경·테두리·그림자)은 popup 셸이 그린다. 고른 답을 돌려준다.
/// Enter 는 Keep both, Esc 는 Cancel the rest 다.
pub fn conflict_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    props: &ConflictProps<'_>,
    apply_all: &mut bool,
) -> Option<ConflictPick> {
    let mut pick = None;
    ui.vertical(|ui| {
        let pad_x = conflict_pad_x(theme);
        let md = theme.spacing_md.value();
        let sm = theme.spacing_sm.value();
        let inner_w = (ui.available_width() - pad_x * 2.0).max(1.0);
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: pad_x as i8,
                right: pad_x as i8,
                top: md as i8,
                bottom: sm as i8,
            })
            .show(ui, |ui| {
                ui.set_width(inner_w);
                ui.spacing_mut().item_spacing = egui::vec2(sm, sm);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(props.title)
                            .size(theme.font_size_max.value())
                            .color(theme.text_primary().to_egui()),
                    )
                    .wrap(),
                );
                ui.label(
                    egui::RichText::new(props.in_folder)
                        .font(caption(theme))
                        .color(theme.text_muted().to_egui()),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(sm, STRUCT_GAP_2.value());
                    for (k, v) in [
                        (props.existing_label, props.existing),
                        (props.incoming_label, props.incoming),
                    ] {
                        ui.horizontal(|ui| {
                            let label_w = conflict_label_width(theme);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(label_w, theme.font_size_caption.value()),
                                egui::Sense::hover(),
                            );
                            ui.painter().text(
                                rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                k,
                                caption(theme),
                                theme.text_muted().to_egui(),
                            );
                            let w = ui.available_width();
                            one_line(
                                ui,
                                v,
                                caption_mono(theme),
                                theme.text_secondary().to_egui(),
                                w,
                            );
                        });
                    }
                });
                if let Some(note) = props.no_merge {
                    ui.label(
                        egui::RichText::new(note)
                            .font(caption(theme))
                            .color(theme.text_muted().to_egui()),
                    );
                }
                if let Some(label) = props.apply_all {
                    crate::toggle::checkbox(ui, theme, apply_all, label, true);
                }
            });
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: pad_x as i8,
                right: pad_x as i8,
                top: 0,
                bottom: md as i8,
            })
            .show(ui, |ui| {
                ui.set_width(inner_w);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(sm, 0.0);
                    if Button::new(props.cancel_rest)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, theme)
                        .clicked()
                    {
                        pick = Some(ConflictPick::CancelRest);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Button::new(props.keep_both)
                            .variant(ButtonVariant::Primary)
                            .size(ControlSize::Sm)
                            .show(ui, theme)
                            .clicked()
                        {
                            pick = Some(ConflictPick::KeepBoth);
                        }
                        if let Some(replace) = props.replace
                            && Button::new(replace)
                                .variant(ButtonVariant::Secondary)
                                .size(ControlSize::Sm)
                                .show(ui, theme)
                                .clicked()
                        {
                            pick = Some(ConflictPick::Replace);
                        }
                        if Button::new(props.skip)
                            .variant(ButtonVariant::Secondary)
                            .size(ControlSize::Sm)
                            .show(ui, theme)
                            .clicked()
                        {
                            pick = Some(ConflictPick::Skip);
                        }
                    });
                });
            });
    });
    if pick.is_none() {
        ui.input(|i| {
            if i.key_pressed(egui::Key::Enter) {
                pick = Some(ConflictPick::KeepBoth);
            } else if i.key_pressed(egui::Key::Escape) {
                pick = Some(ConflictPick::CancelRest);
            }
        });
    }
    pick
}

// ── 결과 카드 ────────────────────────────────────────────

pub struct ResultLine<'a> {
    pub path: &'a str,
    pub reason: &'a str,
}

pub struct ResultAction<'a> {
    pub label: &'a str,
    pub variant: ButtonVariant,
}

pub struct ResultCardProps<'a> {
    pub kind: ToastKind,
    pub title: &'a str,
    pub lines: &'a [ResultLine<'a>],
    /// "and n more". 없으면 그리지 않는다.
    pub more: Option<&'a str>,
    pub actions: &'a [ResultAction<'a>],
    pub dismiss_tip: &'a str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResultCardResponse {
    pub dismiss: bool,
    /// 누른 동작 버튼의 번호.
    pub action: Option<usize>,
}

/// 토스트 카드 모양의 작업 결과. 폭은 `width` 로 고정한다. 그린 영역과 응답을 돌려준다.
pub fn result_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    props: &ResultCardProps<'_>,
    paint_close: IconPainter<'_>,
) -> (egui::Rect, ResultCardResponse) {
    let mut out = ResultCardResponse::default();
    let colors = crate::toast::card_colors(theme, props.kind, 1.0);
    let accent_w = theme.toast_accent_width.value();
    let xs = theme.spacing_xs.value();
    let sm = theme.spacing_sm.value();
    let frame = egui::Frame::new()
        .fill(colors.bg)
        .stroke(egui::Stroke::new(theme.border_width.value(), colors.border))
        .corner_radius(theme.corner_radius.value())
        .shadow(theme.shadow_popover().to_egui())
        .inner_margin(egui::Margin {
            left: (TOAST_PADDING_X + accent_w) as i8,
            right: TOAST_PADDING_X as i8,
            top: TOAST_PADDING_Y as i8,
            bottom: TOAST_PADDING_Y as i8,
        });
    let inner_w = (width - TOAST_PADDING_X * 2.0 - accent_w).max(1.0);
    let resp = frame.show(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_width(inner_w);
            ui.spacing_mut().item_spacing = egui::vec2(xs, xs);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(sm, 0.0);
                let close_w = ControlSize::Sm.height(theme);
                let title_w = (inner_w - close_w - sm).max(1.0);
                ui.allocate_ui(egui::vec2(title_w, 0.0), |ui| {
                    ui.set_width(title_w);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(props.title)
                                .font(body(theme))
                                .color(colors.text),
                        )
                        .wrap(),
                    );
                });
                out.dismiss = IconButton::new()
                    .size(ControlSize::Sm)
                    .show(ui, theme, paint_close)
                    .on_hover_text(props.dismiss_tip)
                    .clicked();
            });
            if !props.lines.is_empty() || props.more.is_some() {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, STRUCT_GAP_2.value());
                    for line in props.lines {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                            path_line(
                                ui,
                                line.path,
                                caption_mono(theme),
                                theme.text_secondary().to_egui(),
                                inner_w,
                            );
                            one_line(
                                ui,
                                line.reason,
                                caption(theme),
                                theme.text_muted().to_egui(),
                                inner_w,
                            );
                        });
                    }
                    if let Some(more) = props.more {
                        one_line(
                            ui,
                            more,
                            caption(theme),
                            theme.text_muted().to_egui(),
                            inner_w,
                        );
                    }
                });
            }
            if !props.actions.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(xs, 0.0);
                    for (i, action) in props.actions.iter().enumerate() {
                        if Button::new(action.label)
                            .variant(action.variant)
                            .size(ControlSize::Sm)
                            .show(ui, theme)
                            .clicked()
                        {
                            out.action = Some(i);
                        }
                    }
                });
            }
        })
    });
    let rect = resp.response.rect;
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(accent_w, rect.height()));
    ui.painter().rect_filled(
        bar,
        egui::CornerRadius {
            nw: theme.corner_radius.value() as u8,
            sw: theme.corner_radius.value() as u8,
            ne: 0,
            se: 0,
        },
        colors.accent,
    );
    (rect, out)
}

// ── 드래그 ───────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOp {
    Move,
    Copy,
    Refused,
}

impl DragOp {
    pub fn color(self, theme: &Theme) -> egui::Color32 {
        match self {
            Self::Move => theme.explorer_drag_move_fg().to_egui(),
            Self::Copy => theme.explorer_drag_copy_fg().to_egui(),
            Self::Refused => theme.explorer_drag_refused_fg().to_egui(),
        }
    }
}

pub struct DragChipProps<'a> {
    /// 1줄: 항목 이름 또는 "3 items".
    pub label: &'a str,
    /// 2줄: "Move to Archive" 처럼 번역을 마친 문구. 대상 폴더도 동작 색으로 그린다.
    pub line: &'a str,
    /// 거절 이유. 2줄 뒤에 text-secondary 로 잇는다.
    pub reason: Option<&'a str>,
    pub op: DragOp,
}

/// 포인터 옆 칩. 1줄 글리프는 `paint_item`, 2줄 동작 글리프는 `paint_op` 가 그린다.
pub fn drag_chip(
    ui: &mut egui::Ui,
    theme: &Theme,
    props: &DragChipProps<'_>,
    paint_item: IconPainter<'_>,
    paint_op: IconPainter<'_>,
) -> egui::Rect {
    let xs = theme.spacing_xs.value();
    let sm = theme.spacing_sm.value();
    let max_w = theme.explorer_drag_chip_max_width().value();
    let frame = egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .shadow(theme.shadow_popover().to_egui())
        .inner_margin(egui::Margin::symmetric(sm as i8, xs as i8));
    let text_max = (max_w - sm * 2.0).max(1.0);
    frame
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_max_width(text_max);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, STRUCT_GAP_2.value());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(sm, 0.0);
                    let g = theme.icon_glyph_size_sm.value();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(g, g), egui::Sense::hover());
                    paint_item(ui, rect, theme.text_muted().to_egui());
                    let w = (text_max - g - sm).max(1.0);
                    one_line(
                        ui,
                        props.label,
                        body(theme),
                        theme.text_primary().to_egui(),
                        w,
                    );
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(xs, 0.0);
                    let color = props.op.color(theme);
                    let g = theme.icon_glyph_size_xs.value();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(g, g), egui::Sense::hover());
                    paint_op(ui, rect, color);
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        props.line,
                        0.0,
                        egui::TextFormat::simple(caption(theme), color),
                    );
                    if let Some(reason) = props.reason {
                        job.append(
                            reason,
                            0.0,
                            egui::TextFormat::simple(
                                caption(theme),
                                theme.text_secondary().to_egui(),
                            ),
                        );
                    }
                    job.wrap =
                        egui::text::TextWrapping::truncate_at_width((text_max - g - xs).max(1.0));
                    let galley = ui.fonts(|f| f.layout_job(job));
                    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                    ui.painter().galley(rect.min, galley, color);
                });
            })
        })
        .response
        .rect
}

/// 놓을 대상 표시: 안쪽 1px 강조 링과 강조 틴트. 배치는 바꾸지 않는다.
pub fn paint_drop_target(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, radius: f32) {
    painter.rect(
        rect,
        radius,
        theme.explorer_drop_target_bg().to_egui(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.explorer_drop_target_border().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
}
