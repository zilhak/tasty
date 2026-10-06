//! OSC 133 D가 명령 완료인지 가린다. 셸 통합의 precmd는 첫 프롬프트를 그리기 전에도 D를 보내므로
//! (`D;0` → `A`), 같은 PTY 연결에서 프롬프트(A)를 그린 적이 없을 때의 D는 셸 시작 보고로 보고 명령
//! 완료로 세지 않는다. respawn은 새 연결이므로 새 셸의 시작 D도 같은 규칙으로 걸러진다.

use crate::runtime::engine_access::EngineMut;

impl EngineMut<'_> {
    /// 프롬프트(A)를 그렸다. 이 연결의 다음 D가 명령 완료가 된다.
    pub(crate) fn note_prompt_shown(&mut self, surface_id: u32, generation: u64) {
        self.live.prompt_shown.insert(surface_id, generation);
    }

    /// D를 받았을 때 명령 완료로 셀지 정하고 프롬프트 기록을 비운다.
    pub(crate) fn take_command_completed(&mut self, surface_id: u32, generation: u64) -> bool {
        self.live.prompt_shown.remove(&surface_id) == Some(generation)
    }

    pub(crate) fn forget_prompt_shown(&mut self, surface_id: u32) {
        self.live.prompt_shown.remove(&surface_id);
    }
}

#[cfg(test)]
mod tests {
    fn engine() -> crate::runtime::engine_session::EngineSession {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn the_startup_report_before_the_first_prompt_is_not_a_completion() {
        let mut session = engine();
        let mut e = session.borrow_mut();
        assert!(!e.take_command_completed(1, 7), "첫 프롬프트 전 D");
        e.note_prompt_shown(1, 7);
        assert!(e.take_command_completed(1, 7), "프롬프트 뒤 D는 명령 완료");
        assert!(!e.take_command_completed(1, 7), "프롬프트 없이 이어진 D");
    }

    #[test]
    fn a_new_connection_starts_without_a_prompt() {
        let mut session = engine();
        let mut e = session.borrow_mut();
        e.note_prompt_shown(1, 7);
        assert!(!e.take_command_completed(1, 8), "respawn 뒤 새 셸의 시작 D");
        e.note_prompt_shown(2, 7);
        e.forget_prompt_shown(2);
        assert!(!e.take_command_completed(2, 7), "닫은 surface");
    }
}
