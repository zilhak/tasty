//! `Tick::AttachView` 처리 — attach 뷰의 3초 cadence 갱신(작업 J).
//!
//! 사용자 확정 UX: 원격 워크스페이스/surface 의 readonly·mirror 뷰는 **실시간 stream 이
//! 아니라 3초 polling** 으로 갱신한다(plan §4). 전송(tap forwarder)은 그대로 두고,
//! 이 핸들러가 *렌더 갱신 cadence* 만 3초로 게이트한다:
//!
//! ① 서버측 readonly display mirror 를 live grid 스냅샷으로 갱신(J-1).
//! ② client mirror 의 누적 출력 버퍼를 mirror Terminal 에 적용(J-6).

use crate::app::App;
use crate::view::RepaintSource;

impl App {
    pub(crate) fn poll_attach_views(&mut self) {
        // ① 서버측 readonly 뷰(피점유측): 점유 surface 의 live grid 를 snapshot→display
        //    mirror 에 feed. 점유 mirror 가 있는 window 만 dirty 표시.
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.refresh_readonly_views()
            {
                w.mark_dirty_from(RepaintSource::AttachMirror);
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            // parked 는 window 가 없어 렌더 의미 없음. mirror 만 정리/갱신(no repaint).
            let _ = engine.refresh_readonly_views(); // dirty 여부 반환값 무시 — parked 는 repaint 안 함.
        }

        // ② client mirror(점유측): 평소엔 reader thread 의 `AttachClientData` wake 로
        //    실시간 적용된다. 여기서는 backstop 으로만 호출 — 혹시 누락된 출력 적용 +
        //    끊긴 세션 정리(`apply_attach_client_output`, attach_client.rs, J-6).
        self.apply_attach_client_output();
    }
}
