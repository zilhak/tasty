//! 호스트 편집 요청(복사·잘라내기·붙여넣기)의 결과를 정해 회신한다. 계약은
//! `docs/dev-guide/plugin-development.md`의 "편집 결과 회신" 절에 있다.
//!
//! 요청이 든 입력은 요청 앞, 요청 하나, 요청 뒤로 나눠 각각 egui 패스로 돌린다. 요청 패스에는 그 편집만
//! 들어가므로 앞뒤 입력(글자 입력, 클릭)이 바꾼 내용을 편집의 결과로 세지 않는다.
//!
//! 결과는 이렇게 정한다.
//! - 플러그인이 [`report_edit`]이나 `report_edit_outcome`으로 알린 값.
//! - 포커스를 가진 [`EditField`]가 정한 값. 잘라내기·붙여넣기는 그 입력칸 버퍼와 커서로 정한다.
//! - 복사는 요청 패스에 복사 출력이 났는지로 정한다.
//! - 그 밖(어댑터를 쓰지 않은 입력칸의 잘라내기·붙여넣기)은 모름이다.

use std::collections::HashMap;
use std::hash::Hash;

use egui::{Context, Event, Id, RawInput, TextEdit, Ui, text_edit::TextEditOutput};
use tasty_plugin_protocol::{EditRequestWire, EditResultWire, RawInputEventWire, RawInputWire};

/// 요청 패스 동안 egui 임시 데이터에 두는 진행 중 요청.
#[derive(Debug, Clone)]
struct ActiveEdit {
    edit_id: u64,
    edit: EditRequestWire,
    /// 어댑터나 플러그인이 정한 결과.
    result: Option<EditResultWire>,
}

fn active_key() -> Id {
    Id::new("tasty_plugin_sdk.edit_request")
}

/// egui 패스 하나. `edit`이 있으면 그 요청만 담은 요청 패스다.
pub(super) struct Pass {
    pub(super) raw: RawInput,
    pub(super) edit: Option<(u64, EditRequestWire)>,
}

/// 요청 하나의 회신.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditOutcome {
    pub(super) edit_id: u64,
    pub(super) result: EditResultWire,
    pub(super) copied: Option<String>,
}

/// 입력을 패스로 나눈다. 요청이 없으면 패스 하나다. `base`는 이벤트를 뺀 같은 프레임 입력이다.
pub(super) fn split_passes(base: RawInput, input: &RawInputWire) -> Vec<Pass> {
    let mut passes = Vec::new();
    let mut pending: Vec<RawInputEventWire> = Vec::new();
    let with_events = |events: &[RawInputEventWire]| {
        let mut raw = base.clone();
        raw.events = super::input::expand_events(events);
        raw
    };
    for event in &input.events {
        let RawInputEventWire::EditRequest { edit_id, edit } = event else {
            pending.push(event.clone());
            continue;
        };
        if !pending.is_empty() {
            passes.push(Pass {
                raw: with_events(&pending),
                edit: None,
            });
            pending.clear();
        }
        let mut raw = base.clone();
        raw.events = edit_event(edit).into_iter().collect();
        passes.push(Pass {
            raw,
            edit: Some((*edit_id, edit.clone())),
        });
    }
    if passes.is_empty() || !pending.is_empty() {
        passes.push(Pass {
            raw: with_events(&pending),
            edit: None,
        });
    }
    passes
}

/// 요청이 egui에 넣는 이벤트. 모르는 동작은 넣지 않는다.
fn edit_event(edit: &EditRequestWire) -> Option<Event> {
    Some(match edit {
        EditRequestWire::Copy => Event::Copy,
        EditRequestWire::Cut => Event::Cut,
        EditRequestWire::PasteText { text } => Event::Paste(text.clone()),
        // 형식 붙여넣기는 내용이 없다. 입력칸에는 효과가 없고 플러그인이 직접 처리해 알린다.
        EditRequestWire::Paste => Event::Paste(String::new()),
        EditRequestWire::Unknown => return None,
    })
}

/// 요청 패스를 시작한다.
pub(super) fn begin(ctx: &Context, edit_id: u64, edit: &EditRequestWire) {
    let active = ActiveEdit {
        edit_id,
        edit: edit.clone(),
        result: None,
    };
    ctx.data_mut(|d| d.insert_temp(active_key(), active));
}

/// 요청 패스가 끝난 뒤 결과를 정한다. `copied`는 그 패스의 복사 출력, `reported`는 패스 밖에서 알린 값이다.
pub(super) fn finish(
    ctx: &Context,
    edit_id: u64,
    edit: &EditRequestWire,
    copied: Option<String>,
    reported: Option<bool>,
) -> EditOutcome {
    let active = ctx.data_mut(|d| {
        let active = d.get_temp::<ActiveEdit>(active_key());
        d.remove::<ActiveEdit>(active_key());
        active
    });
    let copied = copied.filter(|text| !text.is_empty());
    let claimed = reported
        .map(acted_result)
        .or_else(|| active.and_then(|a| a.result));
    let result = claimed.unwrap_or(match edit {
        EditRequestWire::Copy => acted_result(copied.is_some()),
        EditRequestWire::Unknown => EditResultWire::NotActed,
        EditRequestWire::Cut | EditRequestWire::Paste | EditRequestWire::PasteText { .. } => {
            EditResultWire::Unknown
        }
    });
    // 일어나지 않은 편집의 복사는 버린다. 호스트가 원래 키를 넘기면 다음 처리자가 복사한다.
    let copied = copied.filter(|_| result != EditResultWire::NotActed);
    EditOutcome {
        edit_id,
        result,
        copied,
    }
}

fn acted_result(acted: bool) -> EditResultWire {
    if acted {
        EditResultWire::Acted
    } else {
        EditResultWire::NotActed
    }
}

/// 이 패스가 처리 중인 편집 요청. 패스 안에서 편집을 직접 처리하는 플러그인 코드가 읽는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    pub edit_id: u64,
    pub edit: EditRequestWire,
}

/// 이 패스가 처리 중인 편집 요청. 요청 패스가 아니면 `None`이다.
pub fn edit_request(ctx: &Context) -> Option<EditRequest> {
    ctx.data(|d| d.get_temp::<ActiveEdit>(active_key()))
        .map(|a| EditRequest {
            edit_id: a.edit_id,
            edit: a.edit,
        })
}

/// [`EditField`] 밖에서 이 패스의 편집 요청을 처리한 결과를 알린다. 먼저 정해진 결과가 있으면 바꾸지 않는다.
pub fn report_edit(ctx: &Context, acted: bool) {
    set_result(ctx, acted_result(acted));
}

fn set_result(ctx: &Context, result: EditResultWire) {
    ctx.data_mut(|d| {
        let key = active_key();
        if let Some(mut active) = d.get_temp::<ActiveEdit>(key)
            && active.result.is_none()
        {
            active.result = Some(result);
            d.insert_temp(key, active);
        }
    });
}

/// 입력 이벤트에 든 편집 요청. egui 패스 밖에서 편집을 처리하는 플러그인(이미지 붙여넣기 등)이 읽고,
/// 결과를 `EguiMeshSurface::report_edit_outcome`·`EguiMeshPopup::report_edit_outcome`으로 알린다.
pub fn edit_requests(input: &RawInputWire) -> impl Iterator<Item = (u64, &EditRequestWire)> {
    input.events.iter().filter_map(|event| match event {
        RawInputEventWire::EditRequest { edit_id, edit } => Some((*edit_id, edit)),
        _ => None,
    })
}

/// 패스 밖에서 알린 결과. 요청 id 마다 하나다.
#[derive(Debug, Default)]
pub(super) struct Reported(HashMap<u64, bool>);

impl Reported {
    pub(super) fn insert(&mut self, edit_id: u64, acted: bool) {
        self.0.entry(edit_id).or_insert(acted);
    }

    pub(super) fn take(&mut self, edit_id: u64) -> Option<bool> {
        self.0.remove(&edit_id)
    }

    /// 이번 입력에 없는 요청의 값은 회신할 곳이 없다.
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

/// 편집 결과를 정할 수 있는 입력칸. 플러그인은 편집 입력칸을 `egui::TextEdit` 대신 이것으로 그린다.
/// 포커스를 가진 이 입력칸이 요청을 받으면 적용 전후를 비교해 결과를 정한다.
///
/// - 잘라내기: 편집 가능한 칸에서 선택이 지워졌으면 실행. 읽기 전용 칸은 지우지 못하므로 요청을 위젯에 주지
///   않고 실행 안 함이다. 선택이 남고 복사도 나지 않는다. 비밀번호 칸은 지우기만 하고 실행이다.
/// - 붙여넣기: 버퍼가 바뀌었거나, 비지 않은 글자를 붙여넣어 커서·선택이 요청 전과 달라졌으면 실행. 같은 글자로
///   덮어써도 선택이 붙여넣은 글자 뒤의 커서로 접힌다. 내용 없는 붙여넣기(형식 붙여넣기, 빈 글자)는 egui 가
///   무시하므로 버퍼가 바뀌지 않는 한 실행 안 함이다.
/// - 복사: 복사 출력으로 정한다(비밀번호 칸은 egui가 복사하지 않아 실행 안 함).
pub struct EditField<'t> {
    text: &'t mut String,
    multiline: bool,
    id_salt: Option<Id>,
    password: bool,
    read_only: bool,
}

impl<'t> EditField<'t> {
    /// 한 줄 입력칸.
    pub fn singleline(text: &'t mut String) -> Self {
        Self {
            text,
            multiline: false,
            id_salt: None,
            password: false,
            read_only: false,
        }
    }

    /// 여러 줄 입력칸.
    pub fn multiline(text: &'t mut String) -> Self {
        Self {
            multiline: true,
            ..Self::singleline(text)
        }
    }

    /// `egui::TextEdit::id_salt`와 같다. 없으면 자동 id를 쓴다.
    pub fn id_salt(mut self, salt: impl Hash) -> Self {
        self.id_salt = Some(Id::new(salt));
        self
    }

    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    /// 읽기 전용. 선택과 복사는 되고 편집은 되지 않는다.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// 입력칸을 그린다. `customize`로 힌트·너비 같은 나머지 `TextEdit` 설정을 더한다.
    pub fn show(
        self,
        ui: &mut Ui,
        customize: impl FnOnce(TextEdit<'_>) -> TextEdit<'_>,
    ) -> TextEditOutput {
        let id = match self.id_salt {
            Some(salt) => ui.make_persistent_id(salt),
            None => ui.next_auto_id(),
        };
        let ctx = ui.ctx().clone();
        let claim = ctx
            .data(|d| d.get_temp::<ActiveEdit>(active_key()))
            .filter(|a| a.result.is_none() && ctx.memory(|m| m.has_focus(id)))
            .map(|a| a.edit);
        let before = claim
            .as_ref()
            .map(|_| (self.text.clone(), cursor_range(&ctx, id)));
        // 읽기 전용 칸의 잘라내기는 위젯이 선택을 접지 않도록 이 패스 입력에서 걷어낸다. 요청 패스에는 그 요청만
        // 들어 있으므로 다른 입력은 걷지 않는다.
        if self.read_only && matches!(claim, Some(EditRequestWire::Cut)) {
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Cut)));
        }
        let output = if self.read_only {
            let mut view: &str = self.text.as_str();
            build(&mut view, self.multiline, id, self.password, customize).show(ui)
        } else {
            build(
                &mut *self.text,
                self.multiline,
                id,
                self.password,
                customize,
            )
            .show(ui)
        };
        let (Some(edit), Some((text_before, range))) = (claim, before) else {
            return output;
        };
        let changed = *self.text != text_before;
        let acted = match edit {
            EditRequestWire::Cut => !self.read_only && changed,
            // 형식 붙여넣기는 입력칸에 넣을 내용이 없다.
            EditRequestWire::Paste => changed,
            // 글자를 넣은 egui 는 선택을 붙여넣은 글자 뒤의 커서로 접고, 빈 글자는 무시해 커서도 그대로 둔다.
            // 요청 패스에는 이 요청만 들어 있으므로 커서 범위가 달라졌다면 붙여넣기가 처리된 것이다.
            EditRequestWire::PasteText { .. } => {
                changed || !self.read_only && cursor_range(&ctx, id) != range
            }
            // 복사는 복사 출력으로 정한다.
            EditRequestWire::Copy | EditRequestWire::Unknown => return output,
        };
        set_result(&ctx, acted_result(acted));
        output
    }
}

fn build<'b>(
    text: &'b mut dyn egui::TextBuffer,
    multiline: bool,
    id: Id,
    password: bool,
    customize: impl FnOnce(TextEdit<'_>) -> TextEdit<'_>,
) -> TextEdit<'b> {
    let edit = if multiline {
        TextEdit::multiline(text)
    } else {
        TextEdit::singleline(text)
    };
    customize(edit.id(id).password(password))
}

/// 입력칸 `id`의 커서 범위 (secondary, primary).
fn cursor_range(ctx: &Context, id: Id) -> Option<(usize, usize)> {
    let range = TextEdit::load_state(ctx, id)?.cursor.char_range()?;
    Some((range.secondary.index, range.primary.index))
}

impl super::EguiMeshCore {
    /// 보내지 않은 편집 요청 회신을 모두 꺼낸다.
    fn take_edit_outcomes(&mut self) -> Vec<EditOutcome> {
        std::mem::take(&mut self.edit_outcomes)
    }

    /// 보내지 않은 편집 요청 회신을 호스트에 `PluginEvent::EditOutcome` 으로 보낸다. 요청마다 한 번
    /// 나가도록, 화면이 바뀌지 않아 프레임 송신을 생략하는 경우에도 생략 판정 전에 부른다.
    #[cfg(any(unix, windows))]
    pub(super) fn notify_edit_outcomes(
        &mut self,
        host: &crate::host::HostHandle,
        target: tasty_plugin_protocol::CopyTargetWire,
    ) -> Result<(), crate::error::PluginError> {
        for outcome in self.take_edit_outcomes() {
            host.notify(&tasty_plugin_protocol::PluginEvent::EditOutcome {
                target,
                edit_id: outcome.edit_id,
                result: outcome.result,
                copied: outcome.copied,
            })?;
        }
        Ok(())
    }
}

/// surface·popup 의 편집 회신 API.
macro_rules! edit_outcome_api {
    ($ty:ty) => {
        impl $ty {
            /// egui 패스 밖에서 처리한 편집 요청의 결과를 알린다([`edit_requests`]로 읽은 요청). 그 요청을 담은
            /// 다음 렌더가 이 값으로 회신한다. 같은 id 를 다시 알리면 처음 값을 쓴다.
            pub fn report_edit_outcome(&mut self, edit_id: u64, acted: bool) {
                self.core.reported_edits.insert(edit_id, acted);
            }

            /// 보내지 않은 편집 요청 회신을 (요청 id, 결과, 복사 글자)로 꺼낸다. `paint`·`repaint_last` 는
            /// 회신을 호스트에 보내며 소비하므로, `run_frame` 만 쓰는 호출자가 꺼낼 때 쓴다.
            pub fn take_edit_outcomes(&mut self) -> Vec<(u64, EditResultWire, Option<String>)> {
                self.core
                    .take_edit_outcomes()
                    .into_iter()
                    .map(|o| (o.edit_id, o.result, o.copied))
                    .collect()
            }
        }
    };
}
edit_outcome_api!(super::EguiMeshSurface);
edit_outcome_api!(super::EguiMeshPopup);

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
