//! 이름·안내·확정 버튼을 표시하는 푸터. 목록보다 먼저 높이를 확보한다.

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, Input};

use super::{
    CRUMB_GLYPH, FilePickerAction, FilePickerMode, FilePickerProps, FpViewState, selected_folder,
};
use crate::adapters::ui::icons;
use crate::theme::Theme;

/// footer "File name" 라벨의 고정폭(디자인 `width:64; flex:none`). 라벨이 이보다 길면
/// 라벨 폭을 쓴다 — 줄지 않는 쪽은 라벨이다. 갤러리 specimen `FOOTER_LABEL_W` 와 같은 값.
const FOOTER_LABEL_W: LogicalPx = LogicalPx(64.0);

/// 덮어쓰기 경고 줄 한 줄의 높이 — caption 글꼴의 행 높이와 경고 글리프 중 큰 쪽.
fn warning_line_height(ui: &egui::Ui, th: &Theme) -> LogicalPx {
    let row = ui.fonts(|f| f.row_height(&egui::FontId::proportional(th.font_size_caption.value())));
    LogicalPx(row).max(CRUMB_GLYPH)
}

/// 푸터 높이. 저장 모드에서 기존 파일명을 둔 채 폴더를 고르면
/// 폴더 안내와 덮어쓰기 경고가 함께 필요하므로 두 줄을 모두 포함한다.
pub(super) fn footer_height(ui: &egui::Ui, props: &FilePickerProps<'_>) -> LogicalPx {
    let th = props.theme;
    let mut h = th.spacing_sm + th.input_height() + th.spacing_sm + th.button_height();
    let extra = usize::from(selected_folder(props).is_some())
        + usize::from(matches!(
            props.mode,
            FilePickerMode::Save {
                overwrite: true,
                ..
            }
        ));
    for _ in 0..extra {
        h = h + warning_line_height(ui, th) + th.spacing_sm;
    }
    h
}

/// footer — 이름 행(라벨 flex:none · 이름 칸 flex:1) · 덮어쓰기 경고 · 버튼 행(버튼 flex:none).
/// 행마다 폭을 이 영역 안의 사각형으로 잡으므로 버튼은 오른쪽 끝에서 먼저 자리 잡는다.
pub(super) fn draw_footer(
    ui: &mut egui::Ui,
    props: &FilePickerProps<'_>,
    action: &mut FilePickerAction,
) {
    let th = props.theme;
    let area = ui.max_rect();
    let w = area.width();
    ui.spacing_mut().item_spacing = egui::vec2(th.spacing_sm.value(), 0.0);
    ui.add_space(th.spacing_sm.value());

    let (name_row, _) = ui.allocate_exact_size(
        egui::vec2(w, th.input_height().value()),
        egui::Sense::hover(),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("file_picker_name_row")
            .max_rect(name_row)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = th.spacing_sm.value();
    let label = row.painter().layout_no_wrap(
        props.name_field_label.to_owned(),
        egui::FontId::proportional(th.font_size_caption.value()),
        th.text_muted().into(),
    );
    let label_w = FOOTER_LABEL_W.value().max(label.size().x);
    let (label_rect, _) =
        row.allocate_exact_size(egui::vec2(label_w, name_row.height()), egui::Sense::hover());
    row.painter().galley(
        egui::pos2(
            label_rect.left(),
            label_rect.center().y - label.size().y * 0.5,
        ),
        label,
        th.text_muted().into(),
    );
    let field_w = row.available_width().max(0.0);
    match props.mode {
        FilePickerMode::Open { selection_text } => {
            read_only_field(
                &mut row,
                th,
                field_w,
                selection_text,
                props.name_placeholder,
            );
        }
        FilePickerMode::Save { name, .. } => {
            let mut buf = name.to_owned();
            if Input::new()
                .placeholder(props.name_placeholder)
                .width(field_w)
                .show(&mut row, th, &mut buf)
                .changed()
            {
                *action = FilePickerAction::EditName(buf);
            }
        }
    }

    if let Some(folder) = selected_folder(props) {
        ui.add_space(th.spacing_sm.value());
        let (line, _) = ui.allocate_exact_size(
            egui::vec2(w, warning_line_height(ui, th).value()),
            egui::Sense::hover(),
        );
        folder_line(ui, th, line, props, folder);
    }

    if let FilePickerMode::Save {
        name,
        overwrite: true,
        ..
    } = props.mode
    {
        ui.add_space(th.spacing_sm.value());
        let (line, _) = ui.allocate_exact_size(
            egui::vec2(w, warning_line_height(ui, th).value()),
            egui::Sense::hover(),
        );
        overwrite_line(ui, th, line, props.overwrite_warning, name);
    }

    ui.add_space(th.spacing_sm.value());
    let (buttons_row, _) = ui.allocate_exact_size(
        egui::vec2(w, th.button_height().value()),
        egui::Sense::hover(),
    );
    let mut buttons = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("file_picker_footer_buttons")
            .max_rect(buttons_row)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    buttons.spacing_mut().item_spacing.x = th.spacing_sm.value();
    let can_confirm = match props.mode {
        // 폴더 하나면 그 안으로 이동하고, 그 외에는 선택한 항목이 모두 파일이어야 한다.
        FilePickerMode::Open { .. } => {
            matches!(props.state, FpViewState::Loaded)
                && !props.selected.is_empty()
                && (selected_folder(props).is_some()
                    || props.selected.iter().all(|name| {
                        props
                            .entries
                            .iter()
                            .find(|e| &e.name == name)
                            .is_some_and(|e| !e.is_dir)
                    }))
        }
        FilePickerMode::Save { can_confirm, .. } => can_confirm,
    };
    if Button::new(props.confirm_label)
        .variant(ButtonVariant::Primary)
        .enabled(can_confirm)
        .show(&mut buttons, th)
        .clicked()
        && can_confirm
    {
        *action = FilePickerAction::Confirm;
    }
    if Button::new(props.cancel_label)
        .variant(ButtonVariant::Ghost)
        .show(&mut buttons, th)
        .clicked()
    {
        *action = FilePickerAction::Cancel;
    }
}

/// 열기 모드의 이름 칸 — 입력 토큰으로 그린 읽기 전용 칸. 넘치는 텍스트는 칸 안에서 잘린다.
fn read_only_field(ui: &mut egui::Ui, th: &Theme, width: f32, text: &str, placeholder: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, th.input_height().value()),
        egui::Sense::hover(),
    );
    let radius = th.input_radius().value();
    ui.painter()
        .rect_filled(rect, radius, th.input_bg().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(th.border_width.value(), th.input_border().to_egui()),
        egui::StrokeKind::Inside,
    );
    let (shown, color) = if text.is_empty() {
        (placeholder, th.text_placeholder())
    } else {
        (text, th.input_fg())
    };
    let inner = rect.shrink2(egui::vec2(th.input_padding_x().value(), 0.0));
    ui.painter()
        .with_clip_rect(inner.intersect(ui.clip_rect()))
        .text(
            egui::pos2(inner.left(), inner.center().y),
            egui::Align2::LEFT_CENTER,
            shown,
            egui::FontId::proportional(th.input_font_size().value()),
            color.into(),
        );
}

/// 선택한 폴더의 안내. 저장 대상이 아니라는 설명 또는 열기 버튼으로 들어간다는 설명이다.
/// 버튼 이름은 실제 confirm_label을 사용한다.
fn folder_line(
    ui: &mut egui::Ui,
    th: &Theme,
    line: egui::Rect,
    props: &FilePickerProps<'_>,
    folder: &str,
) {
    let muted: egui::Color32 = th.text_muted().into();
    let size = th.font_size_caption.value();
    let glyph = CRUMB_GLYPH.value();
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(line.left(), line.center().y - glyph * 0.5),
        egui::vec2(glyph, glyph),
    );
    icons::FOLDER.image(glyph, muted).paint_at(ui, icon_rect);
    let text_left = icon_rect.right() + th.spacing_xs.value();

    let prose = egui::TextFormat::simple(egui::FontId::proportional(size), muted);
    let mono = egui::TextFormat::simple(egui::FontId::monospace(size), muted);
    let mut job = egui::text::LayoutJob::default();
    match props.mode {
        FilePickerMode::Save { .. } => {
            let (before, after) = props
                .folder_not_save_target
                .split_once("{name}")
                .unwrap_or((props.folder_not_save_target, ""));
            job.append(before, 0.0, prose.clone());
            job.append(folder, 0.0, mono);
            job.append(after, 0.0, prose);
        }
        FilePickerMode::Open { .. } => {
            // 강조는 굵기가 아니라 색으로 준다 — egui 는 semibold 를 못 고른다.
            let emphasis = egui::TextFormat::simple(
                egui::FontId::proportional(size),
                th.text_secondary().into(),
            );
            for (part, kind) in split_named(props.folder_open_enters) {
                match kind {
                    Slot::Prose => job.append(part, 0.0, prose.clone()),
                    Slot::Name => job.append(folder, 0.0, mono.clone()),
                    Slot::Confirm => job.append(props.confirm_label, 0.0, emphasis.clone()),
                }
            }
        }
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width((line.right() - text_left).max(0.0));
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(text_left, line.center().y - galley.size().y * 0.5),
        galley,
        muted,
    );
}

/// `folder_open_enters` 문구의 조각 종류.
enum Slot {
    Prose,
    Name,
    Confirm,
}

/// 언어마다 순서가 다를 수 있어 {name}과 {confirm}을 나타나는 순서대로 나눈다.
fn split_named(template: &str) -> Vec<(&str, Slot)> {
    let mut out = Vec::new();
    let mut rest = template;
    while !rest.is_empty() {
        let name = rest.find("{name}");
        let confirm = rest.find("{confirm}");
        let (at, len, slot) = match (name, confirm) {
            (Some(n), Some(c)) if n < c => (n, "{name}".len(), Slot::Name),
            (Some(_), Some(c)) => (c, "{confirm}".len(), Slot::Confirm),
            (Some(n), None) => (n, "{name}".len(), Slot::Name),
            (None, Some(c)) => (c, "{confirm}".len(), Slot::Confirm),
            (None, None) => {
                out.push((rest, Slot::Prose));
                break;
            }
        };
        if at > 0 {
            out.push((&rest[..at], Slot::Prose));
        }
        out.push(("", slot));
        rest = &rest[at + len..];
    }
    out
}

/// 덮어쓰기 경고 줄 — `alertTriangle` + 문구(이름만 mono). caption · `accent-warning`.
fn overwrite_line(ui: &mut egui::Ui, th: &Theme, line: egui::Rect, template: &str, name: &str) {
    let warn: egui::Color32 = th.accent_warning().into();
    let size = th.font_size_caption.value();
    let glyph = CRUMB_GLYPH.value();
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(line.left(), line.center().y - glyph * 0.5),
        egui::vec2(glyph, glyph),
    );
    icons::ALERT_TRIANGLE
        .image(glyph, warn)
        .paint_at(ui, icon_rect);
    let text_left = icon_rect.right() + th.spacing_xs.value();
    let mut job = egui::text::LayoutJob::default();
    let (before, after) = template.split_once("{name}").unwrap_or((template, ""));
    let prose = egui::TextFormat::simple(egui::FontId::proportional(size), warn);
    job.append(before, 0.0, prose.clone());
    job.append(
        name,
        0.0,
        egui::TextFormat::simple(egui::FontId::monospace(size), warn),
    );
    job.append(after, 0.0, prose);
    job.wrap = egui::text::TextWrapping::truncate_at_width((line.right() - text_left).max(0.0));
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(text_left, line.center().y - galley.size().y * 0.5),
        galley,
        warn,
    );
}
