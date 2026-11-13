//! 편집 요청과 회신 짝짓기 시험. 수신 스레드가 모은 것처럼 회신을 직접 넣는다.

use std::sync::Arc;

use tasty_plugin_protocol::{CopyTargetWire, EditResultWire};
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::*;
use crate::process::PluginProcess;

const MD: &str = "com.tasty.markdown";
const IMG: &str = "com.tasty.image";

fn popup(instance_id: u64) -> CopyTargetWire {
    CopyTargetWire::Popup { instance_id }
}

/// `plugins` 가 실행 중이고 회신을 약속한 manager.
fn mgr(plugins: &[&str]) -> PluginManager {
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    for id in plugins {
        start(&mut mgr, id);
    }
    mgr
}

fn start(mgr: &mut PluginManager, id: &str) {
    let (process, _tap) = PluginProcess::stub_with_request_rx(id);
    mgr.processes.insert(id.to_string(), process);
    mgr.apply_edit_events(vec![id.to_string()], Vec::new(), &[]);
}

fn reply(
    channel: &str,
    target: CopyTargetWire,
    edit_id: u64,
    result: EditResultWire,
    copied: Option<&str>,
) -> ReceivedEditOutcome {
    ReceivedEditOutcome {
        channel: channel.to_string(),
        target,
        edit_id,
        result,
        copied: copied.map(str::to_string),
    }
}

fn receive(mgr: &mut PluginManager, items: Vec<ReceivedEditOutcome>) {
    mgr.apply_edit_events(Vec::new(), items, &[]);
}

/// (id, 결과, 복사 글자)
fn taken(mgr: &mut PluginManager) -> Vec<(u64, PluginEditResult, Option<String>)> {
    mgr.take_edit_outcomes()
        .into_iter()
        .map(|o| (o.request.edit_id, o.result, o.copied))
        .collect()
}

/// 회신을 약속하지 않았거나 실행 중이 아닌 플러그인에는 요청을 만들지 않는다. 호출자는 키를 넘긴다.
#[test]
fn only_a_running_plugin_that_opted_in_gets_requests() {
    let mut mgr = mgr(&[MD]);
    let (process, _tap) = PluginProcess::stub_with_request_rx(IMG);
    mgr.processes.insert(IMG.to_string(), process);
    assert!(mgr.supports_edit_outcome(MD));
    assert!(!mgr.supports_edit_outcome(IMG), "약속하지 않았다");
    assert!(
        mgr.begin_edit_request(IMG, PluginCopyTarget::Popup(1))
            .is_none()
    );
    assert!(!mgr.supports_edit_outcome("com.tasty.absent"));

    mgr.apply_edit_events(vec!["com.tasty.absent".into()], Vec::new(), &[]);
    assert!(
        mgr.begin_edit_request("com.tasty.absent", PluginCopyTarget::Popup(1))
            .is_none(),
        "실행 중이 아니다"
    );
}

/// 같은 배치에서 연결이 끊긴 채널의 약속은 넣지 않는다.
#[test]
fn a_hello_from_a_disconnected_channel_is_not_recorded() {
    let mut mgr = mgr(&[]);
    let (process, _tap) = PluginProcess::stub_with_request_rx(MD);
    mgr.processes.insert(MD.to_string(), process);
    mgr.apply_edit_events(vec![MD.into()], Vec::new(), &[MD.to_string()]);
    assert!(!mgr.supports_edit_outcome(MD));
}

/// 회신은 그 요청의 결과로 한 번 나오고 복사 글자를 함께 싣는다. 실행 안 함의 복사 글자는 버린다.
#[test]
fn a_reply_resolves_its_request_once_with_the_copied_text() {
    let mut mgr = mgr(&[MD]);
    let copy = mgr
        .begin_edit_request(MD, PluginCopyTarget::Popup(2))
        .unwrap();
    let paste = mgr
        .begin_edit_request(MD, PluginCopyTarget::Popup(2))
        .unwrap();
    assert_ne!(copy.edit_id, paste.edit_id);
    receive(
        &mut mgr,
        vec![
            reply(
                MD,
                popup(2),
                paste.edit_id,
                EditResultWire::NotActed,
                Some("x"),
            ),
            reply(
                MD,
                popup(2),
                copy.edit_id,
                EditResultWire::Acted,
                Some("hi"),
            ),
            reply(MD, popup(2), copy.edit_id, EditResultWire::NotActed, None),
        ],
    );
    assert_eq!(
        taken(&mut mgr),
        vec![
            (paste.edit_id, PluginEditResult::NotActed, None),
            (copy.edit_id, PluginEditResult::Acted, Some("hi".into())),
        ],
        "역순 회신은 받은 순서대로, 중복 회신은 버린다"
    );
    assert!(taken(&mut mgr).is_empty());
}

/// 다른 플러그인 채널, 다른 대상, 모르는 id, 취소한 요청의 회신은 결과가 아니다.
#[test]
fn a_reply_that_does_not_match_its_request_is_dropped() {
    let mut mgr = mgr(&[MD, IMG]);
    let request = mgr
        .begin_edit_request(MD, PluginCopyTarget::Popup(2))
        .unwrap();
    let cancelled = mgr
        .begin_edit_request(MD, PluginCopyTarget::Popup(2))
        .unwrap();
    mgr.cancel_edit_request(cancelled.edit_id);
    receive(
        &mut mgr,
        vec![
            reply(IMG, popup(2), request.edit_id, EditResultWire::Acted, None),
            reply(MD, popup(3), request.edit_id, EditResultWire::Acted, None),
            reply(
                MD,
                CopyTargetWire::Surface { surface_id: 2 },
                request.edit_id,
                EditResultWire::Acted,
                None,
            ),
            reply(
                MD,
                CopyTargetWire::Unknown,
                request.edit_id,
                EditResultWire::Acted,
                None,
            ),
            reply(MD, popup(2), 999, EditResultWire::Acted, None),
            reply(MD, popup(2), cancelled.edit_id, EditResultWire::Acted, None),
        ],
    );
    assert!(taken(&mut mgr).is_empty());
    receive(
        &mut mgr,
        vec![reply(
            MD,
            popup(2),
            request.edit_id,
            EditResultWire::Unknown,
            Some("c"),
        )],
    );
    assert_eq!(
        taken(&mut mgr),
        vec![(request.edit_id, PluginEditResult::Unknown, Some("c".into()))],
        "맞지 않는 회신을 버린 뒤에도 요청은 기다린다"
    );
}

/// 다시 시작한 실행이 이전 실행의 id 로 회신해도 결과가 아니다. 이전 실행의 요청은 정리 때 `Gone`으로 끝났다.
#[test]
fn a_restarted_plugin_cannot_answer_the_previous_run() {
    let mut mgr = mgr(&[MD]);
    let request = mgr
        .begin_edit_request(MD, PluginCopyTarget::Popup(2))
        .unwrap();
    mgr.processes.remove(MD);
    start(&mut mgr, MD);
    receive(
        &mut mgr,
        vec![reply(
            MD,
            popup(2),
            request.edit_id,
            EditResultWire::Acted,
            None,
        )],
    );
    assert!(taken(&mut mgr).is_empty(), "실행이 다르다");

    mgr.forget_plugin_runtime(MD, "plugin restart");
    assert_eq!(
        taken(&mut mgr),
        vec![(request.edit_id, PluginEditResult::Gone, None)]
    );
    assert!(!mgr.supports_edit_outcome(MD), "새 hello 까지 약속이 없다");
}

/// 연결이 끊긴 배치의 회신은 넣지 않고, 기다리던 요청은 `Gone`이다. 다른 플러그인의 회신은 남는다.
#[test]
fn a_disconnect_ends_waiting_requests_as_gone() {
    let mut mgr = mgr(&[MD, IMG]);
    let gone = mgr
        .begin_edit_request(MD, PluginCopyTarget::Popup(2))
        .unwrap();
    let kept = mgr
        .begin_edit_request(IMG, PluginCopyTarget::Surface(5))
        .unwrap();
    mgr.forget_edit_requests(MD);
    mgr.apply_edit_events(
        Vec::new(),
        vec![
            reply(MD, popup(2), gone.edit_id, EditResultWire::Acted, None),
            reply(
                IMG,
                CopyTargetWire::Surface { surface_id: 5 },
                kept.edit_id,
                EditResultWire::Acted,
                None,
            ),
        ],
        &[MD.to_string()],
    );
    assert_eq!(
        taken(&mut mgr),
        vec![
            (gone.edit_id, PluginEditResult::Gone, None),
            (kept.edit_id, PluginEditResult::Acted, None),
        ]
    );
}

/// 기다리는 요청이 상한을 넘으면 가장 오래된 요청이 모름으로 끝나고, 늦은 회신은 버린다.
#[test]
fn the_oldest_waiting_request_ends_unknown_beyond_the_limit() {
    let mut mgr = mgr(&[MD]);
    let ids: Vec<u64> = (0..=EDIT_REQUEST_LIMIT)
        .map(|_| {
            mgr.begin_edit_request(MD, PluginCopyTarget::Popup(1))
                .unwrap()
                .edit_id
        })
        .collect();
    assert_eq!(
        taken(&mut mgr),
        vec![(ids[0], PluginEditResult::Unknown, None)]
    );
    receive(
        &mut mgr,
        vec![reply(MD, popup(1), ids[0], EditResultWire::Acted, None)],
    );
    assert!(taken(&mut mgr).is_empty());
}

/// 꺼내지 않은 결과가 상한을 넘으면 가장 오래된 결과부터 버린다.
#[test]
fn the_outcome_queue_drops_the_oldest_beyond_the_limit() {
    let mut mgr = mgr(&[MD]);
    let mut replies = Vec::new();
    for _ in 0..EDIT_OUTCOME_QUEUE_LIMIT + 1 {
        let request = mgr
            .begin_edit_request(MD, PluginCopyTarget::Popup(1))
            .unwrap();
        replies.push(reply(
            MD,
            popup(1),
            request.edit_id,
            EditResultWire::Acted,
            None,
        ));
        receive(&mut mgr, std::mem::take(&mut replies));
    }
    let got = taken(&mut mgr);
    assert_eq!(got.len(), EDIT_OUTCOME_QUEUE_LIMIT);
    assert_eq!(got.first().map(|o| o.0), Some(2), "첫 결과를 버렸다");
}
