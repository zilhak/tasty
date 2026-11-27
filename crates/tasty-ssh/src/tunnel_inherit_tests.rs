//! 터널 준비의 포트 예약 listener 와 준비 확인 소켓은 끈 뒤 상속 플래그가 꺼져 있다(Windows). 이음새
//! (`tasty_utils::socket_inherit::seam`)가 만든 직후 상속을 일부러 켜고, 끈 뒤의 값을 본다.

use super::{SEAM_READY, SEAM_RESERVE, local_end_accepts, reserve_local_port};
use tasty_utils::socket_inherit::seam::{self, Scope, Seen};

fn set_then_cleared(created: &[Seen], cleared: &[Seen]) -> bool {
    !created.is_empty()
        && created.iter().all(|seen| seen.inheritable)
        && !cleared.is_empty()
        && cleared.iter().all(|seen| !seen.inheritable)
}

#[test]
fn the_tunnel_setup_sockets_are_not_inheritable() {
    let reserve_seam = seam::arm(SEAM_RESERVE, Scope::Thread);
    let ready_seam = seam::arm(SEAM_READY, Scope::Thread);
    let (listener, port) = reserve_local_port().expect("reserve");
    assert!(local_end_accepts(port), "the reserved listener accepts");
    drop(listener);
    let reserve_seen = (reserve_seam.created(), reserve_seam.cleared());
    let ready_seen = (ready_seam.created(), ready_seam.cleared());
    assert!(
        set_then_cleared(&reserve_seen.0, &reserve_seen.1)
            && set_then_cleared(&ready_seen.0, &ready_seen.1),
        "reserve {reserve_seen:?}, ready {ready_seen:?}"
    );
}
