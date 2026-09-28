//! 설정 › 일반 › 권한(macOS) 화면의 콘텐츠 컬럼. 본체와 갤러리가 함께 호출한다.
//!
//! 위에서부터 상태 표, 감지 설명 줄, [모든 권한 요청] 버튼, 요청 설명 줄을 `space-md`
//! 간격으로 쌓는다. 표는 행마다 라벨 열(1fr)과 상태·행 액션 열로 나뉘고, 표 맨 위와
//! 각 행 아래에 1px `border-default` 선을 긋는다. 상태는 글리프와 문구로 구분하며
//! 색은 글리프에만 준다. 폭이 좁으면 상태와 액션이 다음 줄로 내려가 오른쪽에 붙고,
//! 라벨은 말줄임하지 않는다.
//!
//! 이 view는 TCC 상태를 읽거나 요청하지 않는다. 문구와 상태는 호출부가 넘기고, 눌린
//! 버튼만 돌려준다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::button::{Button, ButtonVariant};
use crate::chip::{TagVariant, tag, tag_width};
use crate::control::ControlSize;
use crate::help_hint::HelpHint;
use crate::spinner::Spinner;
use crate::tooltip::TooltipPlacement;

/// 라벨 줄과 보조 줄 사이 간격. 디자인은 이 자리에 역할 토큰 없이 primitive `size-2`를
/// 직접 쓰고, 대응하는 Theme 치수가 없어 배율만 적용해 쓴다.
const LABEL_DETAIL_GAP: LogicalPx = LogicalPx(2.0);

/// 권한 상태 네 가지. "확인 불가"와 "자동 확인 불가"는 색이 같고 글리프로 구분한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermState {
    /// 허용됨.
    Granted,
    /// 허용 안 됨.
    Missing,
    /// 추정할 근거가 없어 확인하지 못함.
    Unknown,
    /// 확인하려는 동작 자체가 권한 요청을 띄워 자동으로 확인하지 않음.
    NotObservable,
}

impl PermState {
    fn glyph(self) -> tasty_icons::Icon {
        match self {
            PermState::Granted => tasty_icons::CHECK,
            PermState::Missing => tasty_icons::ALERT_CIRCLE,
            PermState::Unknown => tasty_icons::HELP_CIRCLE,
            PermState::NotObservable => tasty_icons::EYE_OFF,
        }
    }

    fn glyph_color(self, theme: &Theme) -> egui::Color32 {
        match self {
            PermState::Granted => theme.perm_granted_fg(),
            PermState::Missing => theme.perm_missing_fg(),
            PermState::Unknown => theme.perm_unknown_fg(),
            PermState::NotObservable => theme.perm_unobservable_fg(),
        }
        .to_egui()
    }

    fn word_color(self, theme: &Theme) -> egui::Color32 {
        match self {
            PermState::Granted | PermState::Missing => theme.text_primary(),
            PermState::Unknown | PermState::NotObservable => theme.text_muted(),
        }
        .to_egui()
    }
}

/// 표의 한 행.
pub struct PermRow<'a> {
    pub label: &'a str,
    /// 라벨 옆 HelpHint 문구.
    pub hint: Option<&'a str>,
    /// 라벨 아래 보조 줄.
    pub detail: Option<&'a str>,
    /// 라벨 옆 Tag 문구.
    pub tag: Option<&'a str>,
    pub state: PermState,
    /// 상태 문구(번역된 값).
    pub state_label: &'a str,
    /// 행 끝 Secondary/Sm 버튼 라벨.
    pub action: Option<&'a str>,
}

/// 화면 입력.
pub struct MacPermissionsView<'a> {
    /// HelpHint 버블 id를 구분한다. 한 화면에 여러 벌을 그리는 갤러리는 각각 달라야 한다.
    pub id_salt: egui::Id,
    pub rows: &'a [PermRow<'a>],
    pub detection_note: &'a str,
    pub request_label: &'a str,
    pub request_note: &'a str,
    pub requesting_note: &'a str,
    /// 요청 진행 중이면 버튼을 비활성으로 두고 설명 줄을 진행 표시로 바꾼다.
    pub requesting: bool,
}

/// 눌린 버튼.
#[derive(Default)]
pub struct MacPermissionsOutput {
    /// 행 액션을 누른 행의 인덱스.
    pub row_action: Option<usize>,
    pub request_clicked: bool,
}

/// 넘겨받은 `ui`의 폭에 화면을 그린다.
pub fn mac_permissions(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &MacPermissionsView<'_>,
) -> MacPermissionsOutput {
    let mut out = MacPermissionsOutput::default();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_md.value();

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let top = ui.cursor().min.y;
            rule(ui, theme, top);
            for (i, row) in view.rows.iter().enumerate() {
                if perm_row(ui, theme, view.id_salt.with(i), row) {
                    out.row_action = Some(i);
                }
            }
        });

        note(ui, theme, view.detection_note, theme.text_muted().to_egui());

        if Button::new(view.request_label)
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Md)
            .enabled(!view.requesting)
            .show(ui, theme)
            .clicked()
        {
            out.request_clicked = true;
        }

        if view.requesting {
            requesting_line(ui, theme, view.requesting_note);
        } else {
            note(ui, theme, view.request_note, theme.text_muted().to_egui());
        }
    });
    out
}

fn rule(ui: &egui::Ui, theme: &Theme, y: f32) {
    let x = ui.max_rect().x_range();
    ui.painter().hline(
        x,
        y + theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
    );
}

fn body_galley(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    color: egui::Color32,
    wrap: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        color,
        wrap,
    );
    job.wrap.max_width = wrap;
    ui.fonts(|f| f.layout_job(job))
}

/// 행 하나를 그리고 행 액션이 눌렸는지 돌려준다.
fn perm_row(ui: &mut egui::Ui, theme: &Theme, id: egui::Id, row: &PermRow<'_>) -> bool {
    let width = ui.available_width();
    let pad_y = theme.spacing_xs.value();
    let col_gap = theme.spacing_lg.value();
    let cluster_gap = theme.spacing_md.value();
    let glyph = theme.icon_glyph_size_sm.value();
    let status_gap = theme.perm_status_gap().value();
    let hint_gap = theme.help_hint_gap().value();
    let detail_gap = (LABEL_DETAIL_GAP.value() * theme.ui_zoom).round();

    // 오른쪽 묶음(상태 + 행 액션)의 폭을 먼저 잰다.
    let word = body_galley(
        ui,
        theme,
        row.state_label,
        row.state.word_color(theme),
        f32::INFINITY,
    );
    let status_w = glyph + status_gap + word.size().x;
    let action_w = row.action.map(|label| {
        let g = ui.fonts(|f| {
            f.layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(ControlSize::Sm.font_size(theme)),
                egui::Color32::PLACEHOLDER,
            )
        });
        g.size().x + ControlSize::Sm.pad_x(theme) * 2.0
    });
    let cluster_w = status_w + action_w.map_or(0.0, |w| cluster_gap + w);
    let cluster_h = action_w.map_or(word.size().y.max(glyph), |_| ControlSize::Sm.height(theme));

    // 라벨 줄: 라벨 · HelpHint · Tag. 라벨 열의 폭이 모자라면 오른쪽 묶음을 다음 줄로 내린다.
    let extras_w = row.hint.map_or(0.0, |_| hint_gap + glyph)
        + row.tag.map_or(0.0, |t| hint_gap + tag_width(ui, theme, t));
    let label_natural = body_galley(
        ui,
        theme,
        row.label,
        theme.text_secondary().to_egui(),
        f32::INFINITY,
    );
    let one_line = label_natural.size().x + extras_w + col_gap + cluster_w <= width;
    let label_col_w = if one_line {
        width - col_gap - cluster_w
    } else {
        width
    };
    let label = body_galley(
        ui,
        theme,
        row.label,
        theme.text_secondary().to_egui(),
        (label_col_w - extras_w).max(glyph),
    );
    let detail = row.detail.map(|d| {
        let mut job = egui::text::LayoutJob::simple(
            d.to_owned(),
            egui::FontId::proportional(theme.font_size_caption.value()),
            theme.text_muted().to_egui(),
            label_col_w,
        );
        job.wrap.max_width = label_col_w;
        ui.fonts(|f| f.layout_job(job))
    });
    let label_line_h = label.size().y.max(glyph);
    let label_block_h = label_line_h + detail.as_ref().map_or(0.0, |d| detail_gap + d.size().y);

    let content_h = if one_line {
        label_block_h.max(cluster_h)
    } else {
        label_block_h + theme.spacing_xs.value() + cluster_h
    };
    let row_h = (content_h + pad_y * 2.0).max(theme.perm_row_height().value());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::hover());

    // 라벨 열은 한 줄 배치에서 세로 가운데, 줄바꿈 배치에서는 위쪽에 붙인다.
    let label_top = if one_line {
        rect.center().y - label_block_h * 0.5
    } else {
        rect.min.y + pad_y
    };
    let mut x = rect.min.x;
    ui.painter().galley(
        egui::pos2(x, label_top + (label_line_h - label.size().y) * 0.5),
        label.clone(),
        egui::Color32::PLACEHOLDER,
    );
    x += label.size().x;
    let line_mid = label_top + label_line_h * 0.5;
    if let Some(hint) = row.hint {
        x += hint_gap;
        let r = egui::Rect::from_min_size(
            egui::pos2(x, line_mid - glyph * 0.5),
            egui::vec2(glyph, glyph),
        );
        in_rect(ui, egui::UiBuilder::new().max_rect(r), |ui| {
            HelpHint::new(hint)
                .placement(TooltipPlacement::Bottom)
                .id_source(id.with("hint"))
                .show(ui, theme);
        });
        x += glyph;
    }
    if let Some(t) = row.tag {
        x += hint_gap;
        let w = tag_width(ui, theme, t);
        let r = egui::Rect::from_min_size(
            egui::pos2(x, line_mid - label_line_h * 0.5),
            egui::vec2(w, label_line_h),
        );
        in_rect(
            ui,
            egui::UiBuilder::new()
                .max_rect(r)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                tag(ui, theme, t, TagVariant::Default, false);
            },
        );
    }
    if let Some(d) = detail {
        ui.painter().galley(
            egui::pos2(rect.min.x, label_top + label_line_h + detail_gap),
            d,
            egui::Color32::PLACEHOLDER,
        );
    }

    // 오른쪽 묶음: 상태(글리프 + 문구) · 행 액션.
    let cluster_mid = if one_line {
        rect.center().y
    } else {
        rect.max.y - pad_y - cluster_h * 0.5
    };
    let mut cx = rect.max.x - cluster_w;
    let glyph_rect = egui::Rect::from_min_size(
        egui::pos2(cx, cluster_mid - glyph * 0.5),
        egui::vec2(glyph, glyph),
    );
    row.state
        .glyph()
        .image(glyph, row.state.glyph_color(theme))
        .paint_at(ui, glyph_rect);
    cx += glyph + status_gap;
    ui.painter().galley(
        egui::pos2(cx, cluster_mid - word.size().y * 0.5),
        word.clone(),
        egui::Color32::PLACEHOLDER,
    );
    cx += word.size().x;
    let mut clicked = false;
    if let (Some(label), Some(w)) = (row.action, action_w) {
        cx += cluster_gap;
        let h = ControlSize::Sm.height(theme);
        let r = egui::Rect::from_min_size(egui::pos2(cx, cluster_mid - h * 0.5), egui::vec2(w, h));
        in_rect(ui, egui::UiBuilder::new().max_rect(r), |ui| {
            clicked = Button::new(label)
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, theme)
                .clicked();
        });
    }

    rule(ui, theme, rect.max.y - theme.border_width.value());
    clicked
}

/// 이미 할당한 행 안의 자리에 위젯을 둔다. `scope_builder`와 달리 부모 커서를 움직이지 않는다.
fn in_rect(ui: &mut egui::Ui, builder: egui::UiBuilder, add: impl FnOnce(&mut egui::Ui)) {
    let mut child = ui.new_child(builder);
    add(&mut child);
}

/// caption · line-height-ui · `measure-xl` 폭 상한의 설명 줄.
fn note(ui: &mut egui::Ui, theme: &Theme, text: &str, color: egui::Color32) {
    let wrap = ui.available_width().min(theme.measure_xl.value());
    let galley = ui.fonts(|f| f.layout_job(note_job(theme, text, color, wrap)));
    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter()
        .galley(rect.min, galley, egui::Color32::PLACEHOLDER);
}

fn note_job(theme: &Theme, text: &str, color: egui::Color32, wrap: f32) -> egui::text::LayoutJob {
    let size = theme.font_size_caption.value();
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            line_height: Some(size * theme.line_height_ui),
            ..Default::default()
        },
    );
    job
}

/// 요청 중 줄: Spinner(icon-size-sm) + text-secondary 문구, 간격 `space-sm`.
/// 스피너는 첫 줄 가운데에 맞춘다.
fn requesting_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let spinner = theme.icon_glyph_size_sm.value();
    let gap = theme.spacing_sm.value();
    let wrap = (ui.available_width().min(theme.measure_xl.value()) - spinner - gap).max(spinner);
    let galley = ui.fonts(|f| {
        f.layout_job(note_job(
            theme,
            text,
            theme.text_secondary().to_egui(),
            wrap,
        ))
    });
    let first_line_h = galley
        .rows
        .first()
        .map_or(galley.size().y, |r| r.rect.height());
    let size = egui::vec2(
        spinner + gap + galley.size().x,
        galley.size().y.max(spinner),
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let spin_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x, rect.min.y + (first_line_h - spinner) * 0.5),
        egui::vec2(spinner, spinner),
    );
    in_rect(ui, egui::UiBuilder::new().max_rect(spin_rect), |ui| {
        Spinner::new().size(spinner).show(ui, theme);
    });
    ui.painter().galley(
        egui::pos2(rect.min.x + spinner + gap, rect.min.y),
        galley,
        egui::Color32::PLACEHOLDER,
    );
}
