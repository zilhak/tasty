//! 대기열에 남은 native 누름 알림이 키 쥘 곳을 어떻게 바꿀지의 예측(N1). native 콜백은 host 상태를 읽지 못하므로,
//! host 가 게시 자리마다(`key_release::publish_key_holder`) 지금 상태에서 알림 한 건의 결과를 surface 별로 적은 표를
//! 다리에 넘긴다(`MainViewState::press_forecast`). 키를 판정할 때 다리는 게시된 키 쥘 곳에서 출발해 대기열의 알림을
//! 도착 순서대로 이 표로 적용한다(`holder_after_notices`). 알림 소비(`webview_keys::press_and_publish`)가 실제 전이를
//! 적용하면 그 자리에서 새 표가 게시된다.
//!
//! TODO: S6c 입력 순서 경계 설계가 대체할 임시 장치.

use std::collections::HashSet;

use super::keys::{NativeInput, PressKind};

/// 게시 시점 상태에서 알림 한 건의 결과. 표에 없는 surface 의 알림은 무효다(표시되지 않은 탭, 창 전체를 막는 노드).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PressForecast {
    /// 창 전체를 막는 노드(무대, 설정 열기 요청, 키를 가진 복원 안내)가 있다. 어느 알림도 surface 에 키를 주지 않는다.
    pub window_blocked: bool,
    /// 엔진 선택 surface.
    pub selected: Option<u32>,
    /// 지금 포커스 노드가 surface 본문이다. 포커스 획득만으로 키 쥘 곳이 옮는다.
    pub free: bool,
    /// 누름이 키 쥘 곳을 그 surface 로 만든다.
    pub click_holds: HashSet<u32>,
    /// 누름이 그 surface 경로를 덮는 덮개에 간다. 키 쥘 곳이 없어지고 선택은 그대로다.
    pub click_covered: HashSet<u32>,
    /// 포커스 노드가 surface 이거나 경로 밖 팝업일 때, 포커스 획득이 키 쥘 곳을 그 surface 로 만든다(덮이지 않음).
    #[cfg(any(windows, target_os = "macos", test))]
    pub focus_valid: HashSet<u32>,
    /// 지금 포커스 노드(팝업)가 그 surface 의 포커스 획득으로 풀린다.
    #[cfg(any(windows, target_os = "macos", test))]
    pub focus_releases_now: HashSet<u32>,
}

/// 앞 알림이 남긴 포커스 노드의 꼴.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Node {
    /// 게시 시점 노드 그대로다.
    Published,
    /// 앞 누름이 팝업·주소 띠 포커스를 놓았다. 노드는 surface 본문이다.
    Body,
    /// 앞 누름이 덮개에 갔다. 노드는 그 덮개이고 선택은 그대로다.
    Cover,
}

/// 게시된 키 쥘 곳 `holder` 에서 출발해 `inputs` 의 누름 알림을 도착 순서대로 `forecast` 로 적용한 키 쥘 곳. 무효
/// 알림은 건너뛴다(앞 값을 되살리지 않는다). 전달 키는 건너뛴다(그 효과는 소비가 게시한다, S6c 질문 2).
pub(crate) fn holder_after_notices<'a>(
    forecast: &PressForecast,
    holder: Option<u32>,
    inputs: impl IntoIterator<Item = &'a NativeInput>,
) -> Option<u32> {
    let mut holder = holder;
    let mut selected = forecast.selected;
    let mut node = if forecast.free {
        Node::Body
    } else {
        Node::Published
    };
    for input in inputs {
        let NativeInput::Press { surface_id, kind } = input else {
            continue;
        };
        let sid = *surface_id;
        match kind {
            PressKind::Click => {
                if forecast.click_holds.contains(&sid) {
                    holder = Some(sid);
                    selected = Some(sid);
                    node = Node::Body;
                } else if forecast.click_covered.contains(&sid) {
                    holder = None;
                    node = Node::Cover;
                }
            }
            #[cfg(any(windows, target_os = "macos", test))]
            PressKind::Focus => {
                let moves = forecast.focus_valid.contains(&sid)
                    && match node {
                        Node::Body => true,
                        Node::Published => forecast.focus_releases_now.contains(&sid),
                        // 덮개는 선택이 바뀔 때만 경로 밖이 되어 풀린다(`normalize_focus`).
                        Node::Cover => selected != Some(sid),
                    };
                if moves {
                    holder = Some(sid);
                    selected = Some(sid);
                    node = Node::Body;
                }
            }
        }
    }
    // Linux 는 포커스 획득 알림이 없어 선택·노드를 읽지 않는다.
    #[cfg(not(any(windows, target_os = "macos", test)))]
    let _ = (selected, node);
    holder
}
