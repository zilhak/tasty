use crate::app::command::CoreEvent;

/// 셸 통합이 보내는 바이트를 surface에 넣고 drain해 명령 완료 이벤트 수를 센다.
fn completions(
    core: &mut crate::app::services::AppServices,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    sid: u32,
    bytes: &[u8],
) -> usize {
    engine
        .find_terminal_by_id_mut(sid)
        .expect("terminal surface")
        .feed_bytes(bytes);
    let outcome = core.process_all_pty_output(engine);
    outcome
        .events
        .iter()
        .filter(|e| matches!(e, CoreEvent::TerminalCommandCompleted { surface_id, .. } if *surface_id == sid))
        .count()
}

/// zsh·bash 통합은 첫 프롬프트를 그리기 전에도 precmd가 `D;0`을 보낸다. 그 D는 명령 완료가 아니다.
#[test]
fn the_shell_startup_report_is_not_a_command_completion() {
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core_builder()
        .build()
        .expect("core");
    let (state, mut session) = crate::state::tests::test_state();
    let mut engine = session.borrow_mut();
    let sid = *engine
        .workspace_at(state.active_workspace_index(&engine.read()))
        .expect("workspace")
        .all_surface_ids()
        .first()
        .expect("surface");

    let startup = completions(
        &mut core,
        &mut engine,
        sid,
        b"\x1b]133;D;0\x07\x1b]133;A\x07",
    );
    assert_eq!(startup, 0, "첫 프롬프트 전 D가 명령 완료로 나갔다");

    let command = completions(
        &mut core,
        &mut engine,
        sid,
        b"\x1b]133;C;cmd=true\x07\x1b]133;D;0\x07\x1b]133;A\x07",
    );
    assert_eq!(command, 1, "프롬프트 뒤 명령의 D는 명령 완료다");
}
