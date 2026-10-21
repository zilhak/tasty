//! Properties 의 권한 편집 상자. 시안 `PermEditor`(갤러리 "Properties actions")를 따른다.
//! Permissions 행의 Edit 가 행 아래에 연다. Unix 는 소유자·그룹·기타 × 읽기·쓰기·실행 3×3 체크박스와
//! 바뀔 모드(글자·8진), Windows 는 "읽기 전용" 특성 하나다. Apply 전에는 아무것도 쓰지 않는다.
//! Apply 는 explorer 파일 작업 대기열로 가고, 끝나면 성공은 상자를 닫고 실패는 이유를 보인다.
//! 둘 다 실제 값을 다시 읽는다. 폴더 안으로 내려가지 않고 권한 상승을 요청하지 않는다.

use std::path::PathBuf;

use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Spinner};

use crate::adapters::ui::icons;
use crate::app::file_jobs::permissions::PermChange;
use crate::app::local_reads::{ItemFacts, ItemKind};
use crate::i18n::t;
use crate::theme::Theme;

/// 편집 상자 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    Editing,
    /// 대기열에 넣은 요청을 기다린다. 컨트롤을 잠근다.
    Applying(u64),
    /// 적용하지 못했다. OS 이유를 보이고 다시 읽은 값으로 칸을 맞춘다.
    Failed(String),
}

/// 열린 편집 상자.
#[derive(Debug, Clone)]
pub(crate) struct PermEdit {
    pub(crate) path: PathBuf,
    /// 마지막으로 읽은 값. Apply 는 `draft` 가 이것과 다를 때만 켠다.
    pub(crate) current: PermChange,
    pub(crate) draft: PermChange,
    pub(crate) folder: bool,
    /// setuid·setgid·sticky 가 있으면 남긴다는 줄을 보인다.
    pub(crate) special: bool,
    pub(crate) phase: Phase,
}

/// 상자 안에서 누른 것.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermClick {
    None,
    Cancel,
    Apply,
}

/// 권한 행이 보일 것: 편집 가능, 소유자 아님 안내, 아무것도 없음.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermAccess {
    Editable,
    Locked,
    None,
}

/// 한 로컬 항목의 권한을 바꿀 수 있는가. 링크는 바꾸지 않는다. Unix 는 소유자이거나 root 여야 한다.
pub(crate) fn access(item: &ItemFacts) -> PermAccess {
    if matches!(item.kind, ItemKind::Link { .. }) {
        return PermAccess::None;
    }
    if cfg!(windows) {
        return PermAccess::Editable;
    }
    match (item.mode_bits, item.can_change_mode) {
        (Some(_), true) => PermAccess::Editable,
        (Some(_), false) => PermAccess::Locked,
        (None, _) => PermAccess::None,
    }
}

/// 읽은 값으로 바꿀 값을 만든다.
pub(crate) fn current_of(item: &ItemFacts) -> Option<PermChange> {
    if cfg!(windows) {
        Some(PermChange::ReadOnly(item.read_only))
    } else {
        item.mode_bits.map(|bits| PermChange::Mode(bits & 0o777))
    }
}

impl PermEdit {
    pub(crate) fn open(item: &ItemFacts) -> Option<Self> {
        let current = current_of(item)?;
        Some(Self {
            path: item.path.clone(),
            current,
            draft: current,
            folder: matches!(item.kind, ItemKind::Folder),
            special: item.mode_bits.is_some_and(|bits| bits & 0o7000 != 0),
            phase: Phase::Editing,
        })
    }

    /// 다시 읽은 값으로 맞춘다. 실패 뒤라면 칸도 실제 값으로 돌린다.
    pub(crate) fn reread(&mut self, item: &ItemFacts) {
        if item.path != self.path {
            return;
        }
        let Some(current) = current_of(item) else {
            return;
        };
        self.current = current;
        self.special = item.mode_bits.is_some_and(|bits| bits & 0o7000 != 0);
        if !matches!(self.phase, Phase::Applying(_)) {
            self.draft = current;
        }
    }

    pub(crate) fn changed(&self) -> bool {
        self.draft != self.current
    }

    pub(crate) fn busy(&self) -> bool {
        matches!(self.phase, Phase::Applying(_))
    }
}

/// `rw-r--r-- · 644` 형태의 읽기 표시.
pub(crate) fn readout(bits: u32) -> String {
    let mut text = String::with_capacity(9);
    for shift in [6u32, 3, 0] {
        let triad = (bits >> shift) & 0o7;
        text.push(if triad & 4 != 0 { 'r' } else { '-' });
        text.push(if triad & 2 != 0 { 'w' } else { '-' });
        text.push(if triad & 1 != 0 { 'x' } else { '-' });
    }
    format!("{text} · {:03o}", bits & 0o777)
}

/// 상자를 그린다. 내용 폭을 모두 쓴다.
pub(crate) fn draw(ui: &mut egui::Ui, th: &Theme, edit: &mut PermEdit) -> PermClick {
    let busy = edit.busy();
    let gap = th.spacing_sm.value();
    let mut click = PermClick::None;
    ui.add_space(th.spacing_xs.value());
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.explorer_props_perm_border().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(gap as i8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            match &mut edit.draft {
                PermChange::Mode(bits) => {
                    grid(ui, th, bits, busy);
                    ui.label(
                        egui::RichText::new(readout(*bits))
                            .monospace()
                            .size(th.font_size_caption.value())
                            .color(th.text_secondary().to_egui()),
                    );
                }
                PermChange::ReadOnly(read_only) => {
                    tasty_ui_widgets::checkbox(
                        ui,
                        th,
                        read_only,
                        t("explorer.props.perm.read_only"),
                        !busy,
                    );
                    muted_line(ui, th, t("explorer.props.perm.windows_hint"));
                }
            }
            if matches!(edit.draft, PermChange::Mode(_)) && (edit.folder || edit.special) {
                let mut lines = Vec::new();
                if edit.folder {
                    lines.push(t("explorer.props.perm.folder_only"));
                }
                if edit.special {
                    lines.push(t("explorer.props.perm.special_kept"));
                }
                muted_line(ui, th, &lines.join(" "));
            }
            if let Phase::Failed(reason) = &edit.phase {
                failure(ui, th, reason);
            }
            click = buttons(ui, th, busy, edit.changed());
        });
    ui.add_space(th.spacing_xs.value());
    click
}

/// 역할 열 + 읽기·쓰기·실행 열. 칸 폭은 `explorer-props-perm-cell-width`.
fn grid(ui: &mut egui::Ui, th: &Theme, bits: &mut u32, busy: bool) {
    let cell = th.explorer_props_perm_cell_width().value();
    let row_h = th.explorer_props_row_min_height().value();
    let caption = |text: &str, color: egui::Color32| {
        egui::RichText::new(text)
            .size(th.font_size_caption.value())
            .color(color)
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, th.spacing_xs.value());
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(cell);
            for key in [
                "explorer.props.perm.read",
                "explorer.props.perm.write",
                "explorer.props.perm.execute",
            ] {
                centered(ui, (cell, 0.0), 0.0, |ui| {
                    ui.label(caption(t(key), th.text_muted().to_egui()));
                });
            }
        });
        for (role, shift) in [
            ("explorer.props.perm.owner", 6u32),
            ("explorer.props.perm.group", 3),
            ("explorer.props.perm.others", 0),
        ] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(cell, row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(egui::vec2(cell, row_h));
                        ui.add(
                            egui::Label::new(caption(
                                t(role),
                                th.explorer_props_value_fg().to_egui(),
                            ))
                            .truncate(),
                        );
                    },
                );
                for bit in [4u32, 2, 1] {
                    let mask = bit << shift;
                    centered(ui, (cell, row_h), th.checkbox_size().value(), |ui| {
                        let mut on = *bits & mask != 0;
                        if tasty_ui_widgets::checkbox(ui, th, &mut on, "", !busy).changed() {
                            *bits = if on { *bits | mask } else { *bits & !mask };
                        }
                    });
                }
            });
        }
    });
}

/// 폭 `w` · 높이 `h` 칸의 가운데에 높이 `content_h` 인 것 하나를 둔다. 머리 글자는 `h` 0 으로 글자 높이만 쓴다.
fn centered(
    ui: &mut egui::Ui,
    (w, h): (f32, f32),
    content_h: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(w, h));
            ui.add_space(((h - content_h) / 2.0).max(0.0));
            add(ui);
        },
    );
}

fn muted_line(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(th.font_size_caption.value())
                .color(th.text_muted().to_egui()),
        )
        .wrap(),
    );
}

/// alertCircle + "Couldn't change permissions"(explorer-props-perm-error-fg) + OS 이유(mono muted).
fn failure(ui: &mut egui::Ui, th: &Theme, reason: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
        let error = th.explorer_props_perm_error_fg().to_egui();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            let g = th.icon_glyph_size_sm.value();
            let (r, _) = ui.allocate_exact_size(egui::vec2(g, g), egui::Sense::hover());
            icons::ALERT_CIRCLE.image(g, error).paint_at(ui, r);
            ui.label(
                egui::RichText::new(t("explorer.props.perm.failed"))
                    .size(th.font_size_caption.value())
                    .color(error),
            );
        });
        ui.add(
            egui::Label::new(
                egui::RichText::new(reason)
                    .monospace()
                    .size(th.font_size_caption.value())
                    .color(th.text_muted().to_egui()),
            )
            .wrap(),
        );
    });
}

/// 오른쪽 정렬 Cancel(ghost sm) · Apply(primary sm). 적용 중이면 둘 다 잠그고 Apply 에 Spinner 를 단다.
fn buttons(ui: &mut egui::Ui, th: &Theme, busy: bool, changed: bool) -> PermClick {
    let mut click = PermClick::None;
    // 남은 높이를 차지하지 않도록 버튼 한 줄 높이로 묶는다. 차지하면 팝업 높이 측정이 계속 커진다.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ControlSize::Sm.height(th)),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            let spinner_size = th.icon_glyph_size_sm;
            let spinner = |ui: &mut egui::Ui, rect: egui::Rect, _: egui::Color32| {
                Spinner::new()
                    .size(spinner_size.value())
                    .color(th.spinner_indicator().to_egui())
                    .paint_in(ui, th, rect);
            };
            let label = if busy {
                t("explorer.props.perm.applying")
            } else {
                t("explorer.props.perm.apply")
            };
            let mut apply = Button::new(label)
                .variant(ButtonVariant::Primary)
                .size(ControlSize::Sm)
                .enabled(!busy && changed);
            if busy {
                apply = apply.leading_icon(&spinner).leading_icon_size(spinner_size);
            }
            if apply.show(ui, th).clicked() {
                click = PermClick::Apply;
            }
            if Button::new(t("button.cancel"))
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .enabled(!busy)
                .show(ui, th)
                .clicked()
            {
                click = PermClick::Cancel;
            }
        },
    );
    click
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn item(kind: ItemKind, bits: Option<u32>, owner: bool) -> ItemFacts {
        ItemFacts {
            path: PathBuf::from("/w/deploy.env"),
            kind,
            size: 0,
            modified: None,
            created: None,
            mode: None,
            read_only: false,
            mode_bits: bits,
            can_change_mode: owner,
            hardlink: None,
        }
    }

    #[test]
    fn the_readout_names_the_mode_and_its_octal() {
        assert_eq!(readout(0o644), "rw-r--r-- · 644");
        assert_eq!(readout(0o750), "rwxr-x--- · 750");
        assert_eq!(readout(0o4755), "rwxr-xr-x · 755");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_or_root_edits_and_links_never_do() {
        assert_eq!(
            access(&item(ItemKind::File, Some(0o644), true)),
            PermAccess::Editable
        );
        assert_eq!(
            access(&item(ItemKind::File, Some(0o644), false)),
            PermAccess::Locked
        );
        let link = ItemKind::Link {
            target: PathBuf::from("t"),
            state: crate::app::local_reads::LinkTarget::Missing,
        };
        assert_eq!(access(&item(link, None, true)), PermAccess::None);
    }

    #[cfg(unix)]
    #[test]
    fn apply_turns_on_only_after_a_change_and_a_failure_rereads_the_real_mode() {
        let mut edit = PermEdit::open(&item(ItemKind::Folder, Some(0o1775), true)).unwrap();
        assert!(edit.folder && edit.special);
        assert!(!edit.changed());
        edit.draft = PermChange::Mode(0o770);
        assert!(edit.changed());
        edit.phase = Phase::Applying(7);
        // 적용 중에 읽은 값은 칸을 덮지 않는다.
        edit.reread(&item(ItemKind::Folder, Some(0o1775), true));
        assert_eq!(edit.draft, PermChange::Mode(0o770));
        edit.phase = Phase::Failed("Read-only file system (os error 30)".into());
        edit.reread(&item(ItemKind::Folder, Some(0o1775), true));
        assert_eq!(
            edit.draft,
            PermChange::Mode(0o775),
            "the grid shows the real mode"
        );
        assert!(!edit.changed());
    }
}
