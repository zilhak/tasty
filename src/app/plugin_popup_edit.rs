//! 플러그인 팝업 입력칸에 보낼 편집 키를 플러그인 요청과 보류로 바꾼다. 창 view가 판정해 남긴 편집을 view
//! 처리 직후(플러그인 단축키 단계 뒤) 꺼낸다. 회신을 약속한 플러그인이면 요청 id를 붙여 팝업 입력으로 보내고
//! 원래 키를 그 창에 붙잡는다. 약속하지 않았으면 원래 키를 넘긴 편집으로 한 번 재생한다(egui-winit 기본 처리).
//! 계약: `docs/design/policies/key-mapping.md`의 "플러그인 입력칸" 절.

use std::time::Instant;

use tasty_plugin_protocol::RawInputEventWire;
use winit::event::WindowEvent;
use winit::window::WindowId;

use super::App;
use super::edit_key_replay::ReplayStep;
use crate::adapters::ui::input::edit_key::EditKey;
use crate::plugin::PluginManager;
use crate::plugin::manager::{PluginCopyTarget, PluginEditRequest};
use crate::view::main::plugin_popup_keys::PluginEditAsk;
use crate::view::ui::View as _;

/// 창 view가 남긴 플러그인 팝업 편집을 어떻게 처리할지.
#[derive(Debug)]
enum PopupEditPlan {
    /// 원래 키를 넘긴 편집으로 재생한다(egui-winit 기본 처리).
    Replay(Vec<ReplayStep<WindowEvent>>),
    /// 요청을 팝업에 붙이고 원래 키를 회신까지 붙잡는다.
    Hold(Box<PopupEditHold>),
}

/// `wire`를 팝업 `instance`의 다음 입력에 붙이고 원래 키를 `request`의 회신까지 붙잡는다.
#[derive(Debug)]
struct PopupEditHold {
    instance: u64,
    wire: RawInputEventWire,
    request: PluginEditRequest,
    original: WindowEvent,
    edit: EditKey,
}

impl App {
    /// 창 view가 남긴 플러그인 팝업 편집을 요청과 보류로 바꾼다.
    pub(super) fn begin_plugin_popup_edit(&mut self, id: WindowId) {
        let Some(ask) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|v| v.as_main_mut())
            .and_then(|main| main.take_plugin_edit_ask())
        else {
            return;
        };
        match popup_edit_plan(self.plugin_manager.as_mut(), ask) {
            PopupEditPlan::Replay(steps) => self.replay_edit_steps(id, steps),
            PopupEditPlan::Hold(hold) => {
                let PopupEditHold {
                    instance,
                    wire,
                    request,
                    original,
                    edit,
                } = *hold;
                if let Some(main) = self.view.views.get_mut(&id).and_then(|v| v.as_main_mut()) {
                    main.state.plugin_popup_edit_requests.push((instance, wire));
                    main.mark_dirty();
                }
                self.plugin_edit_holds
                    .hold(id, request, original, edit, Instant::now());
            }
        }
    }
}

/// 편집 `ask`의 처리. 팝업의 플러그인이 회신을 약속했으면 요청을 만들어 보류한다. 회신을 약속하지 않은 플러그인
/// (번들 플러그인 포함)이나 그 사이 닫힌 팝업이면 원래 키를 넘긴 편집으로 한 번 재생한다.
fn popup_edit_plan(mgr: Option<&mut PluginManager>, ask: PluginEditAsk) -> PopupEditPlan {
    let Some(request) = mgr.and_then(|mgr| popup_edit_request(mgr, ask.instance)) else {
        return PopupEditPlan::Replay(vec![ReplayStep::Original(ask.original, ask.edit)]);
    };
    PopupEditPlan::Hold(Box::new(PopupEditHold {
        instance: ask.instance,
        wire: RawInputEventWire::EditRequest {
            edit_id: request.edit_id,
            edit: ask.wire(),
        },
        request,
        original: ask.original,
        edit: ask.edit,
    }))
}

/// 열린 팝업 `instance`의 플러그인에 보낼 편집 요청. 팝업이 닫혔거나 그 플러그인이 회신을 약속하지 않았으면
/// `None`이고 원래 키를 넘긴다.
fn popup_edit_request(mgr: &mut PluginManager, instance: u64) -> Option<PluginEditRequest> {
    let plugin_id = mgr
        .popup_instances()
        .find(|(id, _)| *id == instance)
        .map(|(_, inst)| inst.plugin_id.clone())?;
    mgr.begin_edit_request(&plugin_id, PluginCopyTarget::Popup(instance))
}

#[cfg(test)]
#[path = "plugin_popup_edit_tests.rs"]
mod tests;
