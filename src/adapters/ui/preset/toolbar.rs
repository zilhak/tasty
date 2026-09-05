//! Preset 상세 패널의 **툴바** — 보기 상태 툴바(Edit/Rename/Duplicate/Delete)와
//! 편집 상태 툴바(name/subtitle 인라인 입력 + Done), 그리고 그 클릭 결과를 store 변경으로
//! 옮기는 적용 단계.
//!
//! `preset.rs` 에서 갈라 나왔다. 툴바는 리스트·프리뷰와 공유하는 상태가 클릭 결과
//! 구조체뿐이라 경계가 얇고, 갈라 두면 `preset.rs` 가 셸 조립에만 집중한다.

use tasty_presets::{PresetKind, PresetStore};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant};

use crate::adapters::ui::icons;
use crate::adapters::ui::toast::{ToastKind, ToastManager, ToastScope};
use crate::i18n::t;

use super::{
    EditMetaState, PresetToolbarClicks, RENAME_W, RenameState, TOOLBAR_SEP_H, duplicate_preset,
    workspace_subtitle_field,
};

/// [`draw_toolbar_editing`] 이 만들어낸 편집 메타 버퍼 + Done 클릭 여부.
pub(super) struct ToolbarEditingOutcome {
    pub(super) edit_meta: Option<EditMetaState>,
    pub(super) done_clicked: bool,
}

/// 편집 상태 툴바: name/subtitle 인라인 입력 + Done 버튼.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_toolbar_editing(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    store: &mut PresetStore,
    theme: &Theme,
    kind: PresetKind,
    name: &str,
    selected: &mut Option<String>,
    toasts: &mut ToastManager,
    edit_meta_id: egui::Id,
) -> ToolbarEditingOutcome {
    let key = format!("{}:{}", kind.as_str(), name);
    let mut meta = ctx
        .data_mut(|d| d.get_temp::<EditMetaState>(edit_meta_id))
        .filter(|m| m.key == key)
        .unwrap_or_else(|| EditMetaState {
            key: key.clone(),
            name: name.to_string(),
            subtitle: workspace_subtitle_field(store, kind, name),
        });

    // name input — lost_focus 시 rename 커밋.
    let name_resp = ui.add(
        egui::TextEdit::singleline(&mut meta.name)
            .desired_width(RENAME_W.value())
            .id(egui::Id::new(("preset_edit_name", kind.as_str()))),
    );
    if name_resp.lost_focus() {
        commit_editing_name(store, kind, name, &mut meta, selected, toasts);
    }

    // subtitle input — Workspace 만(실제 필드). changed 시 즉시 저장.
    if kind == PresetKind::Workspace {
        let sub_resp = ui.add(
            egui::TextEdit::singleline(&mut meta.subtitle)
                .desired_width(RENAME_W.value())
                .hint_text(t("preset.edit.subtitle_hint"))
                .id(egui::Id::new("preset_edit_subtitle")),
        );
        if sub_resp.changed() {
            commit_editing_subtitle(store, name, &meta, toasts);
        }
    }

    let done_clicked = draw_toolbar_done_button(ui, theme);

    ToolbarEditingOutcome {
        edit_meta: Some(meta),
        done_clicked,
    }
}

/// 편집 name 필드가 focus 를 잃었을 때 rename 을 커밋한다. 빈 이름은 거부하고
/// 되돌리며, rename 실패는 toast 로 알리고 원래 이름으로 되돌린다.
fn commit_editing_name(
    store: &mut PresetStore,
    kind: PresetKind,
    name: &str,
    meta: &mut EditMetaState,
    selected: &mut Option<String>,
    toasts: &mut ToastManager,
) {
    let buf = meta.name.trim().to_string();
    if buf.is_empty() {
        meta.name = name.to_string(); // 빈 이름 거부 — 되돌림.
        return;
    }
    if buf == name {
        return;
    }
    match store.rename(kind, name, &buf) {
        Ok(()) => {
            *selected = Some(buf.clone());
            meta.key = format!("{}:{}", kind.as_str(), buf);
            meta.name = buf;
        }
        Err(e) => {
            tracing::warn!("preset rename failed: {e}");
            toasts.push(
                t("preset.toast.rename_failed"),
                ToastKind::Error,
                ToastScope::Window,
            );
            meta.name = name.to_string();
        }
    }
}

/// 편집 subtitle 필드가 바뀌면 즉시 store/disk 에 write-through(auto-save).
fn commit_editing_subtitle(
    store: &mut PresetStore,
    name: &str,
    meta: &EditMetaState,
    toasts: &mut ToastManager,
) {
    let Some(mut p) = store.get_workspace(name).cloned() else {
        return;
    };
    p.subtitle = meta.subtitle.clone();
    // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
    if let Err(e) = store.save_workspace_overwrite(p) {
        tracing::warn!("preset subtitle save failed: {e}");
        toasts.push(
            t("preset.toast.save_failed"),
            ToastKind::Error,
            ToastScope::Window,
        );
    }
}

/// Done(primary) 버튼 + "saved automatically" affordance. 클릭 여부를 반환.
fn draw_toolbar_done_button(ui: &mut egui::Ui, theme: &Theme) -> bool {
    let mut done_clicked = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // Done (primary) — 우측 끝.
        if Button::new(t("preset.toolbar.done"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Sm)
            .show(ui, theme)
            .clicked()
        {
            done_clicked = true;
        }
        // "saved automatically" affordance — Save 버튼 없음을 명시.
        ui.label(
            egui::RichText::new(t("preset.toolbar.saved"))
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
    done_clicked
}

/// [`draw_toolbar_view`] 의 버튼 클릭 결과.
pub(super) struct ToolbarViewClicks {
    pub(super) rename_clicked: bool,
    pub(super) duplicate_clicked: bool,
    pub(super) delete_clicked: bool,
    pub(super) edit_clicked: bool,
}

/// 일반(비-편집) 상태 툴바: rename 인라인 입력 또는 name/subtitle 라벨 + Edit·
/// delete·duplicate·rename 아이콘 버튼.
pub(super) fn draw_toolbar_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    store: &mut PresetStore,
    kind: PresetKind,
    name: &str,
    detail_sub: &str,
    rename: &mut Option<RenameState>,
    selected: &mut Option<String>,
) -> ToolbarViewClicks {
    let renaming = rename
        .as_ref()
        .is_some_and(|r| r.kind == kind && r.original == name);
    if renaming {
        let r = rename.as_mut().unwrap();
        let resp = ui.add(
            egui::TextEdit::singleline(&mut r.buffer)
                .desired_width(RENAME_W.value())
                .id(egui::Id::new(("preset_rename_input", kind.as_str()))),
        );
        if r.request_focus {
            resp.request_focus();
            r.request_focus = false;
        }
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            *rename = None; // 취소
        } else if resp.lost_focus() {
            // 커밋: 이름이 바뀌었으면 rename, 아니면 그냥 닫기.
            let buf = r.buffer.trim().to_string();
            if !buf.is_empty() && buf != r.original {
                match store.rename(kind, &r.original, &buf) {
                    Ok(()) => *selected = Some(buf),
                    Err(e) => tracing::warn!("preset rename failed: {e}"),
                }
            }
            *rename = None;
        }
    } else {
        ui.label(egui::RichText::new(name).strong());
        ui.label(
            egui::RichText::new(detail_sub)
                .monospace()
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    }

    let mut rename_clicked = false;
    let mut duplicate_clicked = false;
    let mut delete_clicked = false;
    let mut edit_clicked = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // 우측 끝부터: Edit · | · delete · duplicate · rename.
        if Button::new(t("preset.toolbar.edit"))
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .leading_icon(&|ui, rect, c| icons::EDIT.image(rect.width(), c).paint_at(ui, rect))
            .show(ui, theme)
            .clicked()
        {
            edit_clicked = true;
        }
        // separator.
        ui.add_space(theme.spacing_xs.value());
        let bw = theme.border_width.value();
        let (sep_rect, _) = ui.allocate_exact_size(
            egui::vec2(bw.max(1.0), TOOLBAR_SEP_H.value()),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(sep_rect, 0.0, theme.separator.to_egui());
        ui.add_space(theme.spacing_xs.value());

        if IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .show(ui, theme, &|ui, rect, c| {
                icons::TRASH.image(rect.width(), c).paint_at(ui, rect)
            })
            .on_hover_text(t("preset.toolbar.delete"))
            .clicked()
        {
            delete_clicked = true;
        }
        if IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .show(ui, theme, &|ui, rect, c| {
                icons::CLIPBOARD.image(rect.width(), c).paint_at(ui, rect)
            })
            .on_hover_text(t("preset.toolbar.duplicate"))
            .clicked()
        {
            duplicate_clicked = true;
        }
        if IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .show(ui, theme, &|ui, rect, c| {
                icons::EDIT.image(rect.width(), c).paint_at(ui, rect)
            })
            .on_hover_text(t("preset.toolbar.rename"))
            .clicked()
        {
            rename_clicked = true;
        }
    });

    ToolbarViewClicks {
        rename_clicked,
        duplicate_clicked,
        delete_clicked,
        edit_clicked,
    }
}

/// Edit↔Done 토글 + rename/duplicate/delete 클릭을 store/editing/selected 에 반영.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_toolbar_actions(
    ctx: &egui::Context,
    store: &mut PresetStore,
    kind: PresetKind,
    current: &Option<String>,
    editing: &mut bool,
    selected_node: &mut Option<usize>,
    selected: &mut Option<String>,
    rename: &mut Option<RenameState>,
    clicks: PresetToolbarClicks,
) {
    // Edit↔Done 토글 — 진입/이탈 시 선택 노드 초기화.
    if clicks.edit_clicked {
        *editing = true;
        *selected_node = None;
        *rename = None;
        ctx.request_repaint();
    }
    if clicks.done_clicked {
        *editing = false;
        *selected_node = None;
        ctx.request_repaint();
    }

    let Some(name) = current.clone() else {
        return;
    };
    if clicks.rename_clicked {
        *rename = Some(RenameState {
            kind,
            original: name.clone(),
            buffer: name.clone(),
            request_focus: true,
        });
        ctx.request_repaint();
    }
    if clicks.duplicate_clicked
        && let Some(n) = duplicate_preset(store, kind, &name)
    {
        *selected = Some(n);
        ctx.request_repaint();
    }
    if clicks.delete_clicked {
        match store.delete(kind, &name) {
            Ok(()) => {
                *selected = None;
                *rename = None;
                ctx.request_repaint();
            }
            Err(e) => tracing::warn!("preset delete failed: {e}"),
        }
    }
}
