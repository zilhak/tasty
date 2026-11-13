//! 플러그인 편집 보류 시험. manager 의 회신 stub 으로 요청을 만들고 회신을 넣는다.

use std::sync::Arc;

use tasty_plugin_protocol::{CopyTargetWire, EditResultWire};
use tasty_terminal::waker_factory::NoopWakerFactory;
use winit::event::Ime;

use super::*;
use crate::plugin::manager::{PluginCopyTarget, PluginManager};

const MD: &str = "com.tasty.markdown";

fn window(n: u64) -> WindowId {
    WindowId::from(n)
}

/// 원래 키를 대신하는 창 이벤트. 보류는 이벤트 종류를 보지 않는다.
fn key(n: usize) -> WindowEvent {
    WindowEvent::Ime(Ime::Commit(format!("key{n}")))
}

fn mgr() -> PluginManager {
    let mut mgr = PluginManager::with_registries(
        Arc::new(NoopWakerFactory),
        Arc::new(crate::file::format::FileFormatRegistry::new()),
        Arc::new(crate::file::handler::FileHandlerRegistry::new()),
    );
    mgr.attach_edit_outcome_stub_for_test(MD);
    mgr
}

/// `window` 에 원래 키 `key(0)` 을 붙잡은 Copy 요청을 만든다.
fn hold(
    holds: &mut PluginEditHolds,
    mgr: &mut PluginManager,
    window: WindowId,
    edit: EditKey,
    now: Instant,
) -> u64 {
    let request = mgr
        .begin_edit_request(MD, PluginCopyTarget::Popup(1))
        .expect("stub 은 회신을 약속했다");
    let id = request.edit_id;
    holds.hold(window, request, key(0), edit, now);
    id
}

fn reply(mgr: &mut PluginManager, id: u64, result: EditResultWire, copied: Option<&str>) {
    mgr.receive_edit_outcome_for_test(
        MD,
        CopyTargetWire::Popup { instance_id: 1 },
        id,
        result,
        copied,
    );
}

use super::super::edit_key_replay::ReplayStep;

fn replayed(item: &ResolvedPluginEdit) -> Option<(WindowEvent, EditKey)> {
    item.steps.iter().find_map(|step| match step {
        ReplayStep::Original(event, edit) => Some((event.clone(), *edit)),
        _ => None,
    })
}

/// 줄 선 입력. 플러그인 보류의 줄은 플러그인 단축키 단계 앞에서 붙잡혔으므로 그 단계부터 다시 거친다.
fn queued(item: &ResolvedPluginEdit) -> Vec<WindowEvent> {
    item.steps
        .iter()
        .filter_map(|step| match step {
            ReplayStep::Original(..) => None,
            ReplayStep::Input(event) => Some(event.clone()),
            ReplayStep::View(_) => panic!("플러그인 보류 뒤 입력이 단축키 단계를 건너뛴다"),
        })
        .collect()
}

/// 실행 안 함 회신은 원래 키를 판정한 편집으로 한 번 재생하고, 보류 중 줄 선 입력이 그 뒤에 온다.
#[test]
fn a_not_acted_reply_replays_the_key_before_the_queued_input() {
    let (mut holds, mut mgr, now) = (PluginEditHolds::default(), mgr(), Instant::now());
    let id = hold(&mut holds, &mut mgr, window(1), EditKey::Paste, now);
    assert!(holds.queue(window(1), &key(1)));
    assert!(holds.queue(window(1), &WindowEvent::HoveredFile("a".into())));
    assert!(
        !holds.queue(window(1), &WindowEvent::Focused(true)),
        "창 상태는 줄 서지 않는다"
    );
    assert!(!holds.queue(window(2), &key(2)), "다른 창은 보류가 없다");
    reply(&mut mgr, id, EditResultWire::NotActed, Some("x"));
    let resolved = holds.resolve(mgr.take_edit_outcomes());
    assert_eq!(resolved.len(), 1);
    let item = &resolved[0];
    assert_eq!(item.window, window(1));
    assert_eq!(replayed(item), Some((key(0), EditKey::Paste)));
    assert_eq!(
        queued(item),
        vec![key(1), WindowEvent::HoveredFile("a".into())]
    );
    assert_eq!(item.copied, None, "일어나지 않은 편집의 복사는 없다");
    assert!(!holds.queue(window(1), &key(3)), "보류가 풀렸다");
}

/// 실행 회신은 키를 소비하고 복사 글자를 넘긴다. 모름과 대상 사라짐도 재생하지 않는다.
#[test]
fn acted_unknown_and_gone_replies_consume_the_key() {
    let (mut holds, mut mgr, now) = (PluginEditHolds::default(), mgr(), Instant::now());
    let acted = hold(&mut holds, &mut mgr, window(1), EditKey::Cut, now);
    let unknown = hold(&mut holds, &mut mgr, window(2), EditKey::Paste, now);
    hold(&mut holds, &mut mgr, window(3), EditKey::Paste, now);
    reply(&mut mgr, acted, EditResultWire::Acted, Some("hi"));
    reply(&mut mgr, unknown, EditResultWire::Unknown, None);
    mgr.disconnect_edit_outcome_stub_for_test(MD);
    let mut resolved = holds.resolve(mgr.take_edit_outcomes());
    resolved.sort_by_key(|r| r.result.map(|r| r as u8));
    let got: Vec<_> = resolved
        .iter()
        .map(|r| (r.window, r.result, replayed(r).is_some(), r.copied.clone()))
        .collect();
    let mut want = vec![
        (
            window(1),
            Some(PluginEditResult::Acted),
            false,
            Some("hi".to_string()),
        ),
        (window(2), Some(PluginEditResult::Unknown), false, None),
        (window(3), Some(PluginEditResult::Gone), false, None),
    ];
    want.sort_by_key(|r| r.1.map(|r| r as u8));
    assert_eq!(got, want);
}

/// 회신이 상한 안에 오지 않으면 키를 소비하고(실행됐을 수 있다) 줄 선 입력을 돌려주며, 그 요청은 더 기다리지
/// 않는다. 늦게 온 회신은 보류가 없어 버린다.
#[test]
fn a_hold_without_a_reply_times_out_without_replaying() {
    let (mut holds, mut mgr, now) = (PluginEditHolds::default(), mgr(), Instant::now());
    let id = hold(&mut holds, &mut mgr, window(1), EditKey::Paste, now);
    assert!(holds.queue(window(1), &key(1)));
    assert_eq!(holds.deadline(), Some(now + PLUGIN_HOLD_LIMIT));
    let (early, cancelled) = holds.expire(now + PLUGIN_HOLD_LIMIT - Duration::from_millis(1));
    assert!(early.is_empty() && cancelled.is_empty());

    let (late, cancelled) = holds.expire(now + PLUGIN_HOLD_LIMIT);
    assert_eq!(cancelled, vec![id]);
    assert_eq!(late.len(), 1);
    assert_eq!(replayed(&late[0]), None);
    assert_eq!(queued(&late[0]), vec![key(1)]);
    assert_eq!(holds.deadline(), None);

    mgr.cancel_edit_request(id);
    reply(&mut mgr, id, EditResultWire::NotActed, None);
    assert!(holds.resolve(mgr.take_edit_outcomes()).is_empty());
}
