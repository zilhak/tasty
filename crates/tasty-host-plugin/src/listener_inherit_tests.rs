//! plugin 호스트 listener, accept 한 plugin 연결, 인증 줄을 읽는 복제는 상속 플래그가 꺼져 있다(Windows).
//! 이음새(`tasty_utils::socket_inherit::seam`)가 만든 직후(복제는 `try_clone_uninherited` 안에서) 상속을
//! 일부러 켜고, 끈 뒤의 값을 본다. 끄는 호출이 빠지거나 복제가 평범한 `try_clone()` 으로 돌아가면 std 가
//! 돌려준 값과 상관없이 실패한다.

use super::{HostListener, SEAM_ACCEPT, SEAM_AUTH_READER, SEAM_LISTENER};
use crate::protocol::AuthMessage;
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;
use tasty_utils::socket_inherit::is_inheritable;
use tasty_utils::socket_inherit::seam::{self, CLONE, Scope, Seen};

fn set_then_cleared(created: &[Seen], cleared: &[Seen]) -> bool {
    !created.is_empty()
        && created.iter().all(|seen| seen.inheritable)
        && !cleared.is_empty()
        && cleared.iter().all(|seen| !seen.inheritable)
}

#[test]
fn the_plugin_channel_sockets_are_not_inheritable() {
    let listener_seam = seam::arm(SEAM_LISTENER, Scope::Thread);
    let listener = HostListener::bind().expect("bind");
    let port = listener.port();
    let accept_seam = seam::arm(SEAM_ACCEPT, Scope::Port(port));
    let clone_seam = seam::arm(CLONE, Scope::Port(port));
    let auth_seam = seam::arm(SEAM_AUTH_READER, Scope::Port(port));
    let token = "inherit-token";
    let pending = listener.register(token);
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let auth = AuthMessage {
        plugin_id: "com.test.inherit".into(),
        token: token.into(),
    };
    let line = serde_json::to_string(&auth).expect("auth json") + "\n";
    client.write_all(line.as_bytes()).expect("auth");
    let accepted = pending
        .wait(Duration::from_secs(10))
        .expect("the listener handed the authenticated connection over");
    let handed = is_inheritable(&accepted).expect("handed connection");
    let listener_seen = (listener_seam.created(), listener_seam.cleared());
    let accept_seen = (accept_seam.created(), accept_seam.cleared());
    let auth_seen = (clone_seam.created(), auth_seam.cleared());
    assert!(
        !handed
            && set_then_cleared(&listener_seen.0, &listener_seen.1)
            && set_then_cleared(&accept_seen.0, &accept_seen.1)
            && set_then_cleared(&auth_seen.0, &auth_seen.1),
        "handed {handed}, listener {listener_seen:?}, accepted {accept_seen:?}, \
         auth reader (clone seam, cleared) {auth_seen:?}"
    );
}
