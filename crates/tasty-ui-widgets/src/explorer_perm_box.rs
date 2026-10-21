//! 탐색기 Properties 의 권한 편집 상자. 본체와 갤러리가 같은 함수를 부른다.
//! 시안 `PermEditor`: Permissions 행 아래 내용 폭(라벨 열 포함) 전체를 쓰는 상자로, Unix 는
//! 소유자·그룹·기타 × 읽기·쓰기·실행 3×3 체크박스와 바뀔 모드, Windows 는 "읽기 전용" 하나다.
//! 열 머리와 역할명은 `explorer-props-perm-cell-width` 칸 안에서 줄바꿈하고 말줄임하지 않는다.
//! 문구는 호출자가 번역해 넘기고, 상태(적용 중·실패)와 값의 소유도 호출자에게 있다.

use tasty_type_appearance::theme::Theme;

use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::spinner::Spinner;
use crate::toggle::checkbox;

/// 상자 문구. 모두 번역된 값이다.
#[derive(Clone, Copy)]
pub struct PermBoxText<'a> {
    /// 읽기 · 쓰기 · 실행 열 머리.
    pub heads: [&'a str; 3],
    /// 소유자 · 그룹 · 기타 역할명.
    pub roles: [&'a str; 3],
    pub read_only: &'a str,
    pub windows_hint: &'a str,
    pub failed: &'a str,
    pub apply: &'a str,
    pub applying: &'a str,
    pub cancel: &'a str,
}

/// 편집 중인 값. Unix 는 rwx 9비트, Windows 는 읽기 전용 특성이다.
pub enum PermBoxValue<'a> {
    Mode(&'a mut u32),
    ReadOnly(&'a mut bool),
}

pub struct PermBoxProps<'a> {
    pub text: PermBoxText<'a>,
    pub value: PermBoxValue<'a>,
    /// Unix 상자 아래 안내(폴더만 바꿈 · 특수 비트 유지). 없으면 줄을 두지 않는다.
    pub note: Option<&'a str>,
    /// 적용 실패 이유(OS 오류). 있으면 실패 줄을 보인다.
    pub failure: Option<&'a str>,
    /// 적용 중. 모든 컨트롤을 잠그고 Apply 에 Spinner 를 단다.
    pub busy: bool,
    /// 값이 읽은 값과 다르다. 아니면 Apply 를 끈다.
    pub changed: bool,
}

/// 상자 안에서 누른 버튼.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermBoxClick {
    None,
    Cancel,
    Apply,
}

/// `rw-r--r-- · 644` 형태의 읽기 표시.
pub fn perm_readout(bits: u32) -> String {
    let mut text = String::with_capacity(9);
    for shift in [6u32, 3, 0] {
        let triad = (bits >> shift) & 0o7;
        text.push(if triad & 4 != 0 { 'r' } else { '-' });
        text.push(if triad & 2 != 0 { 'w' } else { '-' });
        text.push(if triad & 1 != 0 { 'x' } else { '-' });
    }
    format!("{text} · {:03o}", bits & 0o777)
}

/// 상자를 그린다. 호출자가 준 폭(Permissions 행의 내용 폭)을 모두 쓴다.
pub fn perm_box(ui: &mut egui::Ui, theme: &Theme, props: PermBoxProps<'_>) -> PermBoxClick {
    let PermBoxProps {
        text,
        value,
        note,
        failure,
        busy,
        changed,
    } = props;
    let gap = theme.spacing_sm.value();
    let mut click = PermBoxClick::None;
    ui.add_space(theme.spacing_xs.value());
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.explorer_props_perm_border().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(gap as i8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            match value {
                PermBoxValue::Mode(bits) => {
                    grid(ui, theme, &text, bits, busy);
                    ui.label(
                        egui::RichText::new(perm_readout(*bits))
                            .monospace()
                            .size(theme.font_size_caption.value())
                            .color(theme.text_secondary().to_egui()),
                    );
                    if let Some(note) = note {
                        muted_line(ui, theme, note);
                    }
                }
                PermBoxValue::ReadOnly(read_only) => {
                    checkbox(ui, theme, read_only, text.read_only, !busy);
                    muted_line(ui, theme, text.windows_hint);
                }
            }
            if let Some(reason) = failure {
                failure_line(ui, theme, text.failed, reason);
            }
            click = buttons(ui, theme, &text, busy, changed);
        });
    ui.add_space(theme.spacing_xs.value());
    click
}

/// 칸 폭에서 줄바꿈한 caption 글자. `align` 은 칸 안의 가로 정렬이다.
fn cell_text(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    color: egui::Color32,
    width: f32,
    align: egui::Align,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontId::proportional(theme.font_size_caption.value()),
        color,
        width,
    );
    job.halign = align;
    ui.fonts(|f| f.layout_job(job))
}

/// 역할 열 + 읽기·쓰기·실행 열. 칸은 왼쪽부터 붙여 놓는다.
fn grid(ui: &mut egui::Ui, theme: &Theme, text: &PermBoxText<'_>, bits: &mut u32, busy: bool) {
    let cell = theme.explorer_props_perm_cell_width().value();
    let row_min = theme.explorer_props_row_min_height().value();
    let check = theme.checkbox_size().value();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, theme.spacing_xs.value());
        // 열 머리: 가운데 정렬, 줄바꿈한 머리는 위로 자라도록 아래를 맞춘다.
        let heads = text.heads.map(|h| {
            cell_text(
                ui,
                theme,
                h,
                theme.text_muted().to_egui(),
                cell,
                egui::Align::Center,
            )
        });
        let head_h = heads.iter().map(|g| g.size().y).fold(0.0, f32::max);
        let (row, _) = ui.allocate_exact_size(egui::vec2(cell * 4.0, head_h), egui::Sense::hover());
        for (i, galley) in heads.into_iter().enumerate() {
            let left = row.left() + cell * (i as f32 + 1.0);
            // 가운데 정렬 galley 는 원점이 칸 가운데다.
            let origin = egui::pos2(left + cell / 2.0, row.bottom() - galley.size().y);
            ui.painter()
                .galley(origin, galley, egui::Color32::PLACEHOLDER);
        }
        for (role, shift) in text.roles.iter().zip([6u32, 3, 0]) {
            let name = cell_text(
                ui,
                theme,
                role,
                theme.explorer_props_value_fg().to_egui(),
                cell,
                egui::Align::LEFT,
            );
            let h = name.size().y.max(row_min);
            let (row, _) = ui.allocate_exact_size(egui::vec2(cell * 4.0, h), egui::Sense::hover());
            let name_y = row.center().y - name.size().y / 2.0;
            ui.painter().galley(
                egui::pos2(row.left(), name_y),
                name,
                egui::Color32::PLACEHOLDER,
            );
            for (i, bit) in [4u32, 2, 1].into_iter().enumerate() {
                let mask = bit << shift;
                let center = egui::pos2(row.left() + cell * (i as f32 + 1.5), row.center().y);
                let rect = egui::Rect::from_center_size(center, egui::vec2(check, check));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let mut on = *bits & mask != 0;
                if checkbox(&mut child, theme, &mut on, "", !busy).changed() {
                    *bits = if on { *bits | mask } else { *bits & !mask };
                }
            }
        }
    });
}

fn muted_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        )
        .wrap(),
    );
}

/// alertCircle + 실패 제목(explorer-props-perm-error-fg) + OS 이유(mono muted).
fn failure_line(ui: &mut egui::Ui, theme: &Theme, title: &str, reason: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        let error = theme.explorer_props_perm_error_fg().to_egui();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
            let g = theme.icon_glyph_size_sm.value();
            let (r, _) = ui.allocate_exact_size(egui::vec2(g, g), egui::Sense::hover());
            tasty_icons::ALERT_CIRCLE.image(g, error).paint_at(ui, r);
            ui.label(
                egui::RichText::new(title)
                    .size(theme.font_size_caption.value())
                    .color(error),
            );
        });
        ui.add(
            egui::Label::new(
                egui::RichText::new(reason)
                    .monospace()
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            )
            .wrap(),
        );
    });
}

/// 오른쪽 정렬 Cancel(ghost sm) · Apply(primary sm). 적용 중이면 둘 다 잠그고 Apply 에 Spinner 를 단다.
fn buttons(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &PermBoxText<'_>,
    busy: bool,
    changed: bool,
) -> PermBoxClick {
    let mut click = PermBoxClick::None;
    // 남은 높이를 차지하지 않도록 버튼 한 줄 높이로 묶는다. 차지하면 팝업 높이 측정이 계속 커진다.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ControlSize::Sm.height(theme)),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            let spinner_size = theme.icon_glyph_size_sm;
            let spinner = |ui: &mut egui::Ui, rect: egui::Rect, _: egui::Color32| {
                Spinner::new()
                    .size(spinner_size.value())
                    .color(theme.spinner_indicator().to_egui())
                    .paint_in(ui, theme, rect);
            };
            let mut apply = Button::new(if busy { text.applying } else { text.apply })
                .variant(ButtonVariant::Primary)
                .size(ControlSize::Sm)
                .enabled(!busy && changed);
            if busy {
                apply = apply.leading_icon(&spinner).leading_icon_size(spinner_size);
            }
            if apply.show(ui, theme).clicked() {
                click = PermBoxClick::Apply;
            }
            if Button::new(text.cancel)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .enabled(!busy)
                .show(ui, theme)
                .clicked()
            {
                click = PermBoxClick::Cancel;
            }
        },
    );
    click
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: PermBoxText<'static> = PermBoxText {
        heads: ["Read", "Write", "Execute"],
        roles: ["Owner", "Group", "Others"],
        read_only: "Read-only",
        windows_hint: "hint",
        failed: "failed",
        apply: "Apply",
        applying: "Applying…",
        cancel: "Cancel",
    };

    /// 고정 폭 영역에 상자를 그리고 그린 높이를 돌려준다.
    fn box_height(text: PermBoxText<'static>) -> f32 {
        let ctx = egui::Context::default();
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let mut height = 0.0;
        let mut bits = 0o644;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(330.0, 800.0),
            )),
            ..Default::default()
        };
        drop(ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let top = ui.cursor().top();
                    perm_box(
                        ui,
                        &theme,
                        PermBoxProps {
                            text,
                            value: PermBoxValue::Mode(&mut bits),
                            note: None,
                            failure: None,
                            busy: false,
                            changed: false,
                        },
                    );
                    height = ui.cursor().top() - top;
                });
        }));
        height
    }

    #[test]
    fn long_heads_and_roles_wrap_instead_of_truncating() {
        let short = box_height(TEXT);
        let long = box_height(PermBoxText {
            heads: ["Read access", "Write access", "Execute access"],
            roles: ["Owning user", "Group", "Everyone else"],
            ..TEXT
        });
        // 줄바꿈한 머리와 역할명만큼 상자가 높아진다. 말줄임이면 높이가 같다.
        assert!(long > short, "long {long} short {short}");
    }

    #[test]
    fn readout_shows_text_and_octal() {
        assert_eq!(perm_readout(0o1750), "rwxr-x--- · 750");
        assert_eq!(perm_readout(0o644), "rw-r--r-- · 644");
    }
}
