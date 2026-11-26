//! IME 허용 판정. 창의 egui 출력 입구(`GpuState::apply_platform_output`)가 다음 입력의 IME 허용 값을 이 함수로 정한다.
//! Windows 는 대상과 관계없이 IME 를 허용하므로 이 판정을 읽지 않는다.

use crate::adapters::ui::popup::PopupKeyboard;
use crate::adapters::ui::popup::focus::PopupNode;
use crate::state::MainViewState;
use crate::state::focus_node::FocusNode;

/// IME 허용을 정하는 대상. 창의 지금 포커스 노드에서 만든다(`ime_target`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImeTarget {
    /// host 팝업. `drawn` 은 방금 그린 몫의 노드가 같은 인스턴스인지다.
    HostPopup { drawn: bool },
    /// plugin 팝업과 그 선언 `keyboard`.
    PluginPopup(PopupKeyboard),
    /// 그 밖의 노드(무대·설정 열기 요청·주소 띠·말풍선·surface).
    Other,
}

/// host 팝업 대상은 방금 그린 몫이 그 인스턴스이고 egui 텍스트 위젯이 IME 출력을 냈을 때만 켠다(`egui_ime_output`).
/// plugin 팝업은 선언 `keyboard` 가 `Text` 일 때 켠다. 그 밖의 대상은 켠다.
pub(crate) fn ime_allowed(target: ImeTarget, egui_ime_output: bool) -> bool {
    match target {
        ImeTarget::HostPopup { drawn } => drawn && egui_ime_output,
        ImeTarget::PluginPopup(keyboard) => keyboard == PopupKeyboard::Text,
        ImeTarget::Other => true,
    }
}

/// 지금 포커스 노드 `node` 의 IME 대상. `drawn` 은 방금 그린 몫의 노드(`MainViewState::pass_focus_node`)다. plugin 팝업
/// 선언이 없으면 기본값 `Text` 다.
pub(crate) fn ime_target(state: &MainViewState, node: FocusNode, drawn: FocusNode) -> ImeTarget {
    match node {
        FocusNode::Popup(PopupNode::Host(iid)) => ImeTarget::HostPopup {
            drawn: drawn == FocusNode::Popup(PopupNode::Host(iid)),
        },
        FocusNode::Popup(PopupNode::Plugin(id)) => ImeTarget::PluginPopup(
            state
                .plugin_popup_key_decls
                .get(&id)
                .map_or(PopupKeyboard::Text, |(keyboard, _)| *keyboard),
        ),
        FocusNode::Stage
        | FocusNode::SettingsRequest
        | FocusNode::HtmlAddress(_)
        | FocusNode::Tutorial
        | FocusNode::Surface(_) => ImeTarget::Other,
    }
}

/// 다음 입력의 IME 허용. 대상은 지금 포커스 노드다(`set_ime_allowed` 는 다음 입력부터 적용되므로 다음 입력의 출발
/// 노드를 읽는다). 그리기 판정이 아니라 `source_guards::draw_pass_key_reads` 의 몫 노드 원칙 밖이다. `drawn` 은 몫을
/// 닫기 전에 붙잡은 그 몫의 노드(없으면 지금 노드, `input_pass::run_input_pass`)이고 `egui_ime_output` 은 그 몫
/// 위젯의 IME 출력이다.
pub(crate) fn window_ime_allowed(
    state: &MainViewState,
    core: &crate::core::CoreState,
    drawn: Option<FocusNode>,
    egui_ime_output: bool,
) -> bool {
    let node = state.focus_node(core);
    ime_allowed(
        ime_target(state, node, drawn.unwrap_or(node)),
        egui_ime_output,
    )
}

#[cfg(test)]
#[path = "ime_permission_tests.rs"]
mod tests;
