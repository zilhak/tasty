//! GUI와 헤드리스가 공유하는 PTY 종료 처리. 창이 없다는 이유로 호출하지 않는다.
use crate::core::{Core, CoreState};
use crate::state::RequestContext;

pub(crate) fn handle(
    core: &mut Core,
    state: &mut RequestContext,
    engine: &mut CoreState,
    surface: u32,
) {
    // 두 호스트 모두 HookFired로 작업 대기자를 깨우며 GUI는 이벤트도 방송한다.
    let exec = core.hook_executor();
    for fired in engine
        .hooks
        .fire(&exec, surface, tasty_hooks::HookEvent::ProcessExit)
    {
        state.enqueue_host_event(fired);
    }
    #[cfg(feature = "gui")]
    state.enqueue_host_event(crate::state::PendingHostEvent::ProcessExited {
        surface_id: surface,
    });
    // 종료한 프로세스는 복원 스냅샷에 남기지 않는다.
    // intent-exempt: explicit PTY exit cascade, not a new user or agent command
    state.close_surface_by_id_no_snapshot(engine, surface, true);
    // 닫기 처리가 표시해 둔 구조 변경을 mirror에도 전달한다.
    state.reconcile_presentation(engine);
    engine.refresh_attach_presentation(&state.navigation);
    engine.push_structure_changes();
}
