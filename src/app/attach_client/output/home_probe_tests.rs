//! 연결 때 보낸 원격 홈 조회의 응답이 탐색기 주소창의 홈이 되는지 확인한다.

use super::*;
use crate::app::attach_client::tests::{supply_ids, test_session};
use crate::app::attach_client::wire::mirror_event_from_control;
use std::collections::HashMap;

fn list_dir_result(request_id: u64, ok: bool, dir: &str) -> MirrorEvent {
    let payload = serde_json::to_vec(&serde_json::json!({
        "event": "list_dir_result",
        "request_id": request_id,
        "ok": ok,
        "dir": dir,
        "entries": [],
        "truncated": false,
        "reason": if ok { None } else { Some("no home directory") },
    }))
    .unwrap();
    mirror_event_from_control(&payload).expect("목록 회신은 이벤트가 된다")
}

#[test]
fn the_home_probe_reply_becomes_the_remote_home_and_other_replies_do_not() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let ws_id = 9_000u32;
    let mut sess = test_session(ws_id, HashMap::new());
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
    // 보낼 때와 같이 소비자 없는 요청으로 기록한다.
    sess.state.pending_list_dir_consumers.insert(11, None);
    sess.state.pending_list_dir_consumers.insert(12, None);
    state.explorer_views.begin_home_probe(ws_id, 11);

    {
        let mut host = MirrorHost::windowed(&mut state, &mut engine);
        let events = vec![
            list_dir_result(12, true, "/not/the/home"),
            list_dir_result(11, true, "/home/remote"),
        ];
        apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, events);
    }
    assert_eq!(
        state.explorer_views.remote_home(ws_id),
        Some(std::path::Path::new("/home/remote"))
    );

    // 재연결의 새 조회가 실패하면 이전 연결의 홈을 쓰지 않는다.
    sess.state.pending_list_dir_consumers.insert(13, None);
    state.explorer_views.begin_home_probe(ws_id, 13);
    {
        let mut host = MirrorHost::windowed(&mut state, &mut engine);
        let events = vec![list_dir_result(13, false, "")];
        apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, events);
    }
    assert_eq!(state.explorer_views.remote_home(ws_id), None);
}
