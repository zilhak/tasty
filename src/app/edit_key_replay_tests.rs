//! 풀린 보류의 재생 순서 시험.

use super::*;

fn release(queued: Vec<u32>) -> Release<u32> {
    Release {
        replay: Some((0, EditKey::Copy)),
        queued,
    }
}

/// 플러그인 보류 뒤에 줄 선 입력은 플러그인 단축키 단계 앞에서 붙잡혔으므로 그 단계부터 다시 거친다. 원래 키는
/// view 에서 붙잡혔으므로 판정한 편집으로 view 에 한 번 넘긴다.
#[test]
fn input_queued_behind_a_plugin_hold_goes_through_plugin_shortcuts_again() {
    assert_eq!(
        replay_steps(release(vec![1, 2]), HoldSite::BeforePluginShortcuts),
        vec![
            ReplayStep::Original(0, EditKey::Copy),
            ReplayStep::Input(1),
            ReplayStep::Input(2),
        ]
    );
}

/// 호스트 입력칸 보류의 줄 선 입력은 view 안에서 붙잡혔으므로 view 로 넘긴다. 단축키를 두 번 판정하지 않는다.
#[test]
fn input_queued_inside_the_view_goes_back_to_the_view() {
    assert_eq!(
        replay_steps(release(vec![1]), HoldSite::View),
        vec![ReplayStep::Original(0, EditKey::Copy), ReplayStep::View(1)]
    );
}

/// 소비한 편집은 원래 키 없이 줄 선 입력만 재생한다.
#[test]
fn a_consumed_edit_replays_only_the_queued_input() {
    let consumed = Release {
        replay: None,
        queued: vec![7],
    };
    assert_eq!(
        replay_steps(consumed, HoldSite::BeforePluginShortcuts),
        vec![ReplayStep::Input(7)]
    );
}
