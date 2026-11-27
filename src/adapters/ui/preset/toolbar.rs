//! 보기·편집 모드의 프리셋 툴바와 선택한 동작 처리.

use crate::view::preset::draft::PresetDrafts;
use tasty_presets::PresetKind;
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

/// 프리셋 이름·이름 바꾸기 칸. 공용 Input이라 input-bg 채움, input-border 테두리, Input 포커스 모양이다.
fn name_field(ui: &mut egui::Ui, theme: &Theme, buf: &mut String, id: egui::Id) -> egui::Response {
    tasty_ui_widgets::Input::new()
        .id(id)
        .width(RENAME_W.value())
        .show(ui, theme, buf)
}

/// 편집 상태 툴바: name/subtitle 인라인 입력 + Done 버튼.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_toolbar_editing(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    store: &mut PresetDrafts,
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

    let name_resp = name_field(
        ui,
        theme,
        &mut meta.name,
        egui::Id::new(("preset_edit_name", kind.as_str())),
    );
    if name_resp.lost_focus() {
        commit_editing_name(store, kind, name, &mut meta, selected, toasts);
    }

    if kind == PresetKind::Workspace {
        let sub_resp = ui.add(
            egui::TextEdit::singleline(&mut meta.subtitle)
                .desired_width(RENAME_W.value())
                .hint_text(tasty_egui_theme::hint_text(
                    theme,
                    t("preset.edit.subtitle_hint"),
                ))
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
    store: &mut PresetDrafts,
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
    match store.queue_rename(kind, name, &buf) {
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

/// 편집 subtitle이 바뀌면 App이 저장할 값 요청을 초안에 넣는다.
fn commit_editing_subtitle(
    store: &mut PresetDrafts,
    name: &str,
    meta: &EditMetaState,
    toasts: &mut ToastManager,
) {
    let Some(mut p) = store.get_workspace(name).cloned() else {
        return;
    };
    p.subtitle = meta.subtitle.clone();
    if let Err(e) = store.queue_workspace_overwrite(p) {
        tracing::warn!("preset subtitle save failed: {e}");
        toasts.push(
            t("preset.toast.save_failed"),
            ToastKind::Error,
            ToastScope::Window,
        );
    }
}

/// 완료 버튼과 자동 저장 안내.
fn draw_toolbar_done_button(ui: &mut egui::Ui, theme: &Theme) -> bool {
    let mut done_clicked = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if Button::new(t("preset.toolbar.done"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Sm)
            .show(ui, theme)
            .clicked()
        {
            done_clicked = true;
        }
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
#[allow(clippy::too_many_arguments)] // reason: 형제 draw_toolbar_editing 과 같은 툴바 상태 묶음을 받는 egui 그리기 함수이고, 세 &mut 의 소유자가 각각 다르다
pub(super) fn draw_toolbar_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    store: &mut PresetDrafts,
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
        let resp = name_field(
            ui,
            theme,
            &mut r.buffer,
            egui::Id::new(("preset_rename_input", kind.as_str())),
        );
        if r.request_focus {
            resp.request_focus();
            r.request_focus = false;
        }
        let esc = ui.is_enabled() && ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            *rename = None; // 취소
        } else if ui.is_enabled() && resp.lost_focus() {
            let buf = r.buffer.trim().to_string();
            if !buf.is_empty() && buf != r.original {
                match store.queue_rename(kind, &r.original, &buf) {
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
                .family(tasty_ui_widgets::label_font_family())
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    }

    let mut rename_clicked = false;
    let mut duplicate_clicked = false;
    let mut delete_clicked = false;
    let mut edit_clicked = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if Button::new(t("preset.toolbar.edit"))
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .leading_icon(&|ui, rect, c| icons::EDIT.image(rect.width(), c).paint_at(ui, rect))
            .show(ui, theme)
            .clicked()
        {
            edit_clicked = true;
        }
        ui.add_space(theme.spacing_xs.value());
        let bw = theme.border_width.value();
        let (sep_rect, _) = ui.allocate_exact_size(
            egui::vec2(bw.max(1.0), TOOLBAR_SEP_H.value()),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(sep_rect, 0.0, theme.separator.to_egui_premultiplied());
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
    store: &mut PresetDrafts,
    kind: PresetKind,
    current: &Option<String>,
    editing: &mut bool,
    selected_node: &mut Option<usize>,
    selected: &mut Option<String>,
    rename: &mut Option<RenameState>,
    clicks: PresetToolbarClicks,
) {
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
        match store.queue_delete(kind, &name) {
            Ok(()) => {
                *selected = None;
                *rename = None;
                ctx.request_repaint();
            }
            Err(e) => tracing::warn!("preset delete failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::painted_test_support::{paint_ctx, themes};

    /// 이름 칸 상자의 채움·테두리 색. 칸 폭이 RENAME_W인 테두리 사각형을 읽는다.
    fn field_look(theme: &Theme, focus: bool) -> (egui::Color32, egui::Color32) {
        let ctx = egui::Context::default();
        let id = egui::Id::new("preset_name_field_test");
        if focus {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        let p = paint_ctx(&ctx, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut buf = "Dev".to_owned();
                name_field(ui, theme, &mut buf, id);
            });
        });
        let w = RENAME_W.value();
        let edge = p
            .stroked_rects()
            .into_iter()
            .find(|(r, _, _)| r.width() == w)
            .expect("칸 테두리");
        let fill = p
            .shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r)
                    if r.rect == edge.0 && r.fill != egui::Color32::TRANSPARENT =>
                {
                    Some(r.fill)
                }
                _ => None,
            })
            .unwrap_or(edge.1);
        (fill, edge.2)
    }

    /// 이름 바꾸기 중 칸 안쪽 가로 여백을 누르고 떼도 이름을 확정하지 않고 편집을 이어 간다.
    /// 확정은 이름 칸이 포커스를 잃을 때 돈다(`rename`이 `None`이 된다).
    #[test]
    fn pressing_the_name_field_padding_keeps_the_rename_open() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        let kind = PresetKind::Tab;
        let id = egui::Id::new(("preset_rename_input", kind.as_str()));
        let mut store = PresetDrafts::default();
        let mut selected = None;
        let mut rename = Some(RenameState {
            kind,
            original: "Dev".to_owned(),
            buffer: "Dev 2".to_owned(),
            request_focus: true,
        });
        let mut frame = |events: Vec<egui::Event>, rename: &mut Option<RenameState>| {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 400.0),
                )),
                events,
                ..Default::default()
            };
            // 포커스와 응답만 본다. 패스 출력(도형·텍스처)은 쓰지 않는다.
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        draw_toolbar_view(
                            ui,
                            &theme,
                            &mut store,
                            kind,
                            "Dev",
                            "",
                            rename,
                            &mut selected,
                        );
                    });
                });
            });
        };
        frame(Vec::new(), &mut rename);
        frame(Vec::new(), &mut rename);
        assert!(ctx.memory(|m| m.has_focus(id)), "이름 칸 포커스");
        // 글자 칸(TextEdit) 왼쪽 끝에서 가로 padding의 절반만큼 바깥, 곧 상자 안쪽 여백이다.
        let text = ctx.read_response(id).expect("이름 칸 TextEdit").rect;
        let pad = egui::pos2(
            text.min.x - theme.input_padding_x().value() / 2.0,
            text.center().y,
        );
        let button = |pressed| egui::Event::PointerButton {
            pos: pad,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        frame(vec![egui::Event::PointerMoved(pad)], &mut rename);
        frame(vec![button(true)], &mut rename);
        assert!(rename.is_some(), "여백을 누른 패스에 이름을 확정했다");
        frame(vec![button(false)], &mut rename);
        frame(Vec::new(), &mut rename);
        assert!(rename.is_some(), "여백을 뗀 뒤 이름을 확정했다");
        assert!(ctx.memory(|m| m.has_focus(id)), "여백 누름 뒤 포커스");
    }

    /// 프리셋 이름 칸은 Input 칸 모양이다: 쉴 때 input-bg 채움과 input-border 테두리, 포커스면 input-border-focus.
    #[test]
    fn the_name_field_takes_the_input_field_look() {
        for theme in themes() {
            assert_ne!(theme.input_border(), theme.input_border_focus());
            let (fill, edge) = field_look(&theme, false);
            assert_eq!(fill, theme.input_bg().to_egui(), "채움");
            assert_eq!(edge, theme.input_border().to_egui(), "테두리");
            let (_, focused) = field_look(&theme, true);
            assert_eq!(
                focused,
                theme.input_border_focus().to_egui(),
                "포커스 테두리"
            );
        }
    }
}
