//! plugin `surface.unfocused` 감지. 화면에 낸 프레임의 키 포커스 값(그 프레임 첫머리에 정한
//! `MainViewState::frame_key_focus`)을 따로 적어 두고, 키 포커스 진입 큐(`take_key_focus_entries`)와 함께
//! 키 포커스가 어느 surface에서 떠나고 들어갔는지를 일어난 순서대로 다시 만든다. 키 포커스 판정은 읽기만 한다.
//! 키 하나나 입력 패스 하나의 수신자(팝업 키 주인 결속)는 프레임 값이 아니므로 이 감지에 들지 않는다.
//! 떠난 surface가 plugin surface인지는 그 surface를 마지막으로 화면에 낸 프레임에서 적은 값으로 정한다. 그 뒤 같은
//! ID가 다른 kind로 바뀌어도 판정은 바뀌지 않는다.

use crate::runtime::engine_read::EngineRead;

/// 화면에 낸 프레임의 키 포커스 값과 마지막으로 알린 값.
#[derive(Debug, Default)]
pub(crate) struct KeyFocusExits {
    /// 마지막으로 제출에 성공한 프레임의 키 포커스 값.
    shown: Option<u32>,
    /// 지금까지 만든 이동이 끝난 값. 다음 감지의 시작 값이다.
    told: Option<u32>,
    /// 낸 프레임의 키 포커스 surface가 그 프레임에서 plugin surface였는지. 감지할 때 `told` 밖의 항목은 지운다.
    plugin_when_shown: std::collections::HashMap<u32, bool>,
}

/// 키 포커스의 이동 하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyFocusMove {
    /// 키 포커스가 `surface_id`를 떠나 `next`(surface가 없으면 None)로 갔다. `plugin`은 `surface_id`를 마지막으로
    /// 낸 프레임에서 그 surface가 plugin surface였는지다.
    Left {
        surface_id: u32,
        next: Option<u32>,
        plugin: bool,
    },
    /// 키 포커스가 `prev`에서 `surface_id`로 들어왔다.
    Entered { surface_id: u32, prev: Option<u32> },
}

impl super::MainViewState {
    /// 그리기를 마친 직후(`finish_frame_key_focus` 바로 뒤) 부른다. 화면에 낸 프레임이면 그 프레임 값과, 그 값의
    /// surface가 지금 plugin surface인지를 적는다.
    pub(crate) fn note_shown_key_focus(&mut self, engine: &EngineRead<'_>, shown: bool) {
        if shown {
            let frame = self.frame_key_focus();
            let exits = &mut self.key_focus_exits;
            exits.shown = frame;
            if let Some(sid) = frame {
                exits
                    .plugin_when_shown
                    .insert(sid, is_plugin_surface(engine, sid));
            }
        }
    }

    /// 지난 감지 뒤 화면에 낸 프레임들의 키 포커스 이동을 순서대로 꺼낸다. 한 전이에서는 떠남이 들어옴보다 먼저다.
    pub(crate) fn take_key_focus_moves(&mut self) -> Vec<KeyFocusMove> {
        let entries = self.take_key_focus_entries();
        let exits = &mut self.key_focus_exits;
        let plugin = &exits.plugin_when_shown;
        let (moves, told) = key_focus_moves(exits.told, &entries, exits.shown, |sid| {
            plugin.get(&sid).copied().unwrap_or(false)
        });
        exits.told = told;
        exits.plugin_when_shown.retain(|sid, _| Some(*sid) == told);
        moves
    }
}

/// `told`에서 시작해 진입들(들어간 surface, 그 앞 값)을 지나 `shown`에 이르는 이동. 진입 큐에는 surface로 들어간
/// 전이만 있으므로, 진입의 앞 값이 지금 값과 다르면 그 사이에 surface 없는 값(키를 가진 팝업 등)으로 떠난 전이가
/// 있었다. 끝 값이 `shown`과 다르면 마지막 진입 뒤에 떠난 전이가 있었다. `plugin`은 떠난 surface가 낸 프레임에서
/// plugin surface였는지다.
pub(crate) fn key_focus_moves(
    told: Option<u32>,
    entries: &[(u32, Option<u32>)],
    shown: Option<u32>,
    plugin: impl Fn(u32) -> bool,
) -> (Vec<KeyFocusMove>, Option<u32>) {
    let go = |moves: &mut Vec<KeyFocusMove>, at: &mut Option<u32>, to: Option<u32>| {
        if let Some(surface_id) = *at
            && *at != to
        {
            moves.push(KeyFocusMove::Left {
                surface_id,
                next: to,
                plugin: plugin(surface_id),
            });
        }
        *at = to;
    };
    let mut moves = Vec::new();
    let mut at = told;
    for &(surface_id, prev) in entries {
        go(&mut moves, &mut at, prev);
        go(&mut moves, &mut at, Some(surface_id));
        moves.push(KeyFocusMove::Entered { surface_id, prev });
    }
    go(&mut moves, &mut at, shown);
    (moves, at)
}

/// plugin이 그리는 surface인지. host가 egui-mesh(`EguiMeshSurface`)나 webview(`RemoteSurface`)로 보관하는
/// surface다. attach mesh mirror는 로컬에 plugin 프로세스가 없어 들지 않는다. 없는 surface는 아니다.
pub(crate) fn is_plugin_surface(engine: &EngineRead<'_>, surface_id: u32) -> bool {
    engine
        .find_surface_by_id(surface_id)
        .is_some_and(|s| s.mesh().is_some() || s.remote_webview().is_some())
}

#[cfg(test)]
#[path = "key_focus_exits_tests.rs"]
mod tests;
