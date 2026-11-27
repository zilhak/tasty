//! plugin 연결의 송신 스레드가 받는 복제는 상속 플래그가 꺼져 있다(Windows). 복제 이음새
//! (`try_clone_uninherited` 안)가 끄기 전에 상속을 켜고, 끈 뒤의 값을 본다. 끄는 호출이 빠지거나 평범한
//! `try_clone()` 으로 돌아가면 std 가 돌려준 값과 상관없이 실패한다.

use super::{SEAM_IO_WRITER, start_io};
use crate::process::channel_bytes::{ChannelLedger, ChannelLimits, Direction, metered_channel};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tasty_utils::socket_inherit::seam::{self, CLONE, Scope};

#[test]
fn the_plugin_writer_clone_is_not_inheritable() {
    let ledger = ChannelLedger::new(ChannelLimits::default());
    let (req_tx, req_rx) =
        metered_channel::<String>(16, ledger.open_queue("p", Direction::Request));
    let (resp_tx, _resp_rx) = metered_channel(16, ledger.open_queue("p", Direction::Response));
    let (event_tx, _event_rx) = metered_channel(16, ledger.open_queue("p", Direction::Event));
    let waker: tasty_terminal::waker_factory::SharedWakerFactory =
        Arc::new(tasty_terminal::waker_factory::NoopWakerFactory);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let plugin = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let (host, _) = listener.accept().expect("accept");
    let clone_seam = seam::arm(CLONE, Scope::Port(port));
    let writer_seam = seam::arm(SEAM_IO_WRITER, Scope::Port(port));
    start_io(
        "com.test.inherit",
        host,
        req_rx,
        resp_tx,
        event_tx,
        Arc::new(Mutex::new(Instant::now())),
        &waker,
    )
    .expect("the I/O threads started");
    let (created, cleared) = (clone_seam.created(), writer_seam.cleared());
    drop(req_tx);
    drop(plugin);
    assert!(
        !created.is_empty()
            && created.iter().all(|seen| seen.inheritable)
            && !cleared.is_empty()
            && cleared.iter().all(|seen| !seen.inheritable),
        "clone seam {created:?}, writer after clearing {cleared:?}"
    );
}
