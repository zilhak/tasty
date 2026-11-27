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
/// 글자 칸이 가로 여백까지 덮어, 편집 중 여백 누름이 포커스를 풀어 이름을 확정하지 않는다.
fn name_field(ui: &mut egui::Ui, theme: &Theme, buf: &mut String, id: egui::Id) -> egui::Response {
    tasty_ui_widgets::Input::new()
        .id(id)
        .text_covers_padding(true)
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

    /// 이름 칸 상자의 채움 색과 그 상자에 그린 획(획 쪽, 색). 칸 폭이 RENAME_W인 Inside 테두리 사각형을 읽는다.
    fn field_look(
        theme: &Theme,
        focus: bool,
    ) -> (egui::Color32, Vec<(egui::StrokeKind, egui::Color32)>) {
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
        let rect = p
            .shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r)
                    if r.rect.width() == w
                        && r.stroke.width > 0.0
                        && r.stroke_kind == egui::StrokeKind::Inside =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .expect("칸 테두리");
        let fill = p
            .shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r) if r.rect == rect && r.fill != egui::Color32::TRANSPARENT => {
                    Some(r.fill)
                }
                _ => None,
            })
            .expect("칸 채움");
        (fill, p.strokes_on(rect))
    }

    /// 이름 칸을 그리는 두 경로: 편집 툴바 이름 칸, 보기 툴바 인라인 이름 바꾸기 칸.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum NamePath {
        Edit,
        View,
    }

    const KIND: PresetKind = PresetKind::Tab;
    const META_ID: &str = "preset_name_press_meta";

    /// 이름 칸 하나를 띄운 툴바. 초안에 프리셋이 없어 확정하면 이름 바꾸기가 실패한다.
    /// 편집 툴바는 실패 toast로, 보기 툴바는 `rename`이 `None`이 되는 것으로 확정을 읽는다.
    struct NameHarness {
        path: NamePath,
        ctx: egui::Context,
        theme: Theme,
        store: PresetDrafts,
        selected: Option<String>,
        toasts: ToastManager,
        rename: Option<RenameState>,
        /// 마지막 패스에 그린 이름 칸 상자(폭 RENAME_W인 Inside 테두리).
        field: egui::Rect,
    }

    impl NameHarness {
        fn new(path: NamePath) -> Self {
            let ctx = egui::Context::default();
            let mut rename = None;
            match path {
                NamePath::Edit => {
                    ctx.memory_mut(|m| m.request_focus(Self::id_of(path)));
                    ctx.data_mut(|d| {
                        d.insert_temp(
                            egui::Id::new(META_ID),
                            EditMetaState {
                                key: format!("{}:Dev", KIND.as_str()),
                                name: "Dev 2".to_owned(),
                                subtitle: String::new(),
                            },
                        )
                    });
                }
                NamePath::View => {
                    rename = Some(RenameState {
                        kind: KIND,
                        original: "Dev".to_owned(),
                        buffer: "Dev 2".to_owned(),
                        request_focus: true,
                    });
                }
            }
            let mut h = Self {
                path,
                ctx,
                theme: tasty_themes::mocha_fallback(),
                store: PresetDrafts::default(),
                selected: None,
                toasts: ToastManager::new(),
                rename,
                field: egui::Rect::NOTHING,
            };
            h.frame(Vec::new());
            h.frame(Vec::new());
            assert!(
                h.ctx.memory(|m| m.has_focus(h.id())),
                "{path:?} 이름 칸 포커스"
            );
            h
        }

        fn id_of(path: NamePath) -> egui::Id {
            match path {
                NamePath::Edit => egui::Id::new(("preset_edit_name", KIND.as_str())),
                NamePath::View => egui::Id::new(("preset_rename_input", KIND.as_str())),
            }
        }

        fn id(&self) -> egui::Id {
            Self::id_of(self.path)
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 400.0),
                )),
                events,
                ..Default::default()
            };
            let Self {
                path,
                ctx,
                theme,
                store,
                selected,
                toasts,
                rename,
                ..
            } = self;
            let out = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| match path {
                        NamePath::Edit => {
                            let meta_id = egui::Id::new(META_ID);
                            let outcome = draw_toolbar_editing(
                                ui, ctx, store, theme, KIND, "Dev", selected, toasts, meta_id,
                            );
                            if let Some(meta) = outcome.edit_meta {
                                ctx.data_mut(|d| d.insert_temp(meta_id, meta));
                            }
                        }
                        NamePath::View => {
                            draw_toolbar_view(ui, theme, store, KIND, "Dev", "", rename, selected);
                        }
                    });
                });
            });
            let w = RENAME_W.value();
            if let Some(r) = out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Rect(r)
                    if r.rect.width() == w
                        && r.stroke.width > 0.0
                        && r.stroke_kind == egui::StrokeKind::Inside =>
                {
                    Some(r.rect)
                }
                _ => None,
            }) {
                self.field = r;
            }
        }

        fn committed(&self) -> bool {
            match self.path {
                NamePath::Edit => self.toasts.len() > 0,
                NamePath::View => self.rename.is_none(),
            }
        }

        fn buffer(&self) -> String {
            match self.path {
                NamePath::Edit => {
                    self.ctx
                        .data(|d| d.get_temp::<EditMetaState>(egui::Id::new(META_ID)))
                        .expect("편집 메타")
                        .name
                }
                NamePath::View => self.rename.as_ref().expect("이름 바꾸기 중").buffer.clone(),
            }
        }

        fn range(&self) -> Option<(usize, usize)> {
            egui::text_edit::TextEditState::load(&self.ctx, self.id())
                .and_then(|s| s.cursor.char_range())
                .map(|r| (r.primary.index, r.secondary.index))
        }

        fn caret_at(&mut self, at: usize) {
            let id = self.id();
            let mut st = egui::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
            st.cursor
                .set_char_range(Some(egui::text_selection::CCursorRange::one(
                    egui::text::CCursor::new(at),
                )));
            st.store(&self.ctx, id);
            self.frame(Vec::new());
            assert_eq!(self.range(), Some((at, at)), "커서 준비");
        }

        /// 글자 칸 왼쪽 끝(상자 왼쪽 + 가로 padding)에서 dx 바깥.
        fn left_pad(&self, dx: f32) -> egui::Pos2 {
            let pad = self.theme.input_padding_x().value();
            egui::pos2(self.field.min.x + pad - dx, self.field.center().y)
        }

        /// 글자 칸 오른쪽 끝(상자 오른쪽 - 가로 padding)에서 dx 바깥.
        fn right_pad(&self, dx: f32) -> egui::Pos2 {
            let pad = self.theme.input_padding_x().value();
            egui::pos2(self.field.max.x - pad + dx, self.field.center().y)
        }
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    const PATHS: [NamePath; 2] = [NamePath::Edit, NamePath::View];
    /// Claude 탐침과 같은 글자 칸 바깥 거리. 8은 egui 근접 판정(5) 밖이다.
    const DXS: [f32; 3] = [2.0, 4.0, 8.0];

    /// 이름 칸 여백 누름은 누름·뗌이 다른 패스든 한 패스든 이름을 확정하지 않고 편집을 이어 간다.
    #[test]
    fn pressing_the_name_field_padding_keeps_editing() {
        for path in PATHS {
            for dx in DXS {
                let mut h = NameHarness::new(path);
                let at = h.left_pad(dx);
                h.frame(vec![egui::Event::PointerMoved(at)]);
                h.frame(vec![button(at, true)]);
                assert!(!h.committed(), "{path:?} dx={dx} 누른 패스에 확정했다");
                h.frame(vec![button(at, false)]);
                h.frame(Vec::new());
                assert!(!h.committed(), "{path:?} dx={dx} 뗀 뒤 확정했다");
                assert!(
                    h.ctx.memory(|m| m.has_focus(h.id())),
                    "{path:?} dx={dx} 포커스"
                );

                let mut h = NameHarness::new(path);
                let at = h.left_pad(dx);
                h.frame(vec![egui::Event::PointerMoved(at)]);
                h.frame(vec![button(at, true), button(at, false)]);
                h.frame(Vec::new());
                assert!(
                    !h.committed(),
                    "{path:?} dx={dx} 한 패스 누름·뗌에 확정했다"
                );
                assert!(
                    h.ctx.memory(|m| m.has_focus(h.id())),
                    "{path:?} dx={dx} 한 패스 포커스"
                );
            }
        }
    }

    /// 왼쪽 여백 누름은 캐럿을 처음으로, 오른쪽 여백 누름은 끝으로 보낸다. 왼쪽 여백에서 시작해
    /// 끝까지 끈 드래그는 선택을 만든다. "Dev 2"는 다섯 글자이고 커서는 3에서 시작한다.
    #[test]
    fn the_name_field_padding_places_the_caret_and_starts_a_drag() {
        for path in PATHS {
            for dx in DXS {
                let mut h = NameHarness::new(path);
                h.caret_at(3);
                let at = h.left_pad(dx);
                h.frame(vec![egui::Event::PointerMoved(at)]);
                h.frame(vec![button(at, true)]);
                h.frame(vec![button(at, false)]);
                assert_eq!(h.range(), Some((0, 0)), "{path:?} dx={dx} 왼쪽 누름");

                let mut h = NameHarness::new(path);
                h.caret_at(3);
                let at = h.right_pad(dx);
                h.frame(vec![egui::Event::PointerMoved(at)]);
                h.frame(vec![button(at, true)]);
                h.frame(vec![button(at, false)]);
                assert_eq!(h.range(), Some((5, 5)), "{path:?} dx={dx} 오른쪽 누름");

                let mut h = NameHarness::new(path);
                h.caret_at(3);
                let at = h.left_pad(dx);
                let end = h.right_pad(0.0);
                h.frame(vec![egui::Event::PointerMoved(at)]);
                h.frame(vec![button(at, true)]);
                h.frame(vec![egui::Event::PointerMoved(end)]);
                h.frame(vec![button(end, false)]);
                assert_eq!(h.range(), Some((5, 0)), "{path:?} dx={dx} 드래그 선택");
                assert!(!h.committed(), "{path:?} dx={dx} 드래그에 확정했다");
            }
        }
    }

    /// 여백 누름과 같은 패스의 글자 키는 TextEdit에 들어가고(캐럿이 처음으로 간 뒤라 앞에 붙는다),
    /// 다음 패스 Enter가 확정한다.
    #[test]
    fn a_key_typed_with_a_padding_press_goes_in_and_enter_then_commits() {
        for path in PATHS {
            let mut h = NameHarness::new(path);
            let at = h.left_pad(8.0);
            h.frame(vec![egui::Event::PointerMoved(at)]);
            h.frame(vec![button(at, true), egui::Event::Text("x".to_owned())]);
            h.frame(vec![button(at, false)]);
            assert!(!h.committed(), "{path:?} 여백 누름에 확정했다");
            assert_eq!(h.buffer(), "xDev 2", "{path:?} 같은 패스 글자");
            h.frame(vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            h.frame(Vec::new());
            assert!(h.committed(), "{path:?} Enter가 확정하지 않았다");
        }
    }

    /// 프리셋 이름 칸은 Input 칸 모양이다: 쉴 때 input-bg 채움과 input-border 테두리 하나, 포커스면
    /// input-border-focus 테두리와 그 바깥 링.
    #[test]
    fn the_name_field_takes_the_input_field_look() {
        let inside = egui::StrokeKind::Inside;
        for theme in themes() {
            assert_ne!(theme.input_border(), theme.input_border_focus());
            let (fill, rest) = field_look(&theme, false);
            assert_eq!(fill, theme.input_bg().to_egui(), "채움");
            assert_eq!(
                rest,
                vec![(inside, theme.input_border().to_egui())],
                "테두리"
            );
            let (_, focused) = field_look(&theme, true);
            let f = theme.input_border_focus().to_egui();
            assert_eq!(
                focused,
                vec![(inside, f), (egui::StrokeKind::Outside, f)],
                "포커스 테두리와 링"
            );
        }
    }
}
