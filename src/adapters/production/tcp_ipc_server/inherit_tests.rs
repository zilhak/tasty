//! 서버가 accept한 연결의 소켓과 reader 복제는 상속 플래그가 꺼져 있다(Windows).
//! 실제 accept 루프를 거친다. accept 직후 상속을 일부러 켜서(`inherit_seam`), 루프의 끄는 호출이 빠지거나
//! 슬롯 분기 뒤로 가거나 그 결과를 버리면 std가 돌려준 값과 상관없이 실패한다.

use super::inherit_seam::{self, Seen};
use super::{MAX_CONCURRENT_CONNECTIONS, TcpIpcServer};
use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;
use tasty_ipc::stream_hub::{StreamContext, StreamHub};
use tasty_telemetry::ConnectionStats;

const WAIT: Duration = Duration::from_secs(10);

struct Running {
    server: TcpIpcServer,
    stats: Arc<ConnectionStats>,
    _dir: tempfile::TempDir,
    _home: crate::test_support::TastyHomeGuard,
}

fn start() -> Running {
    let home = crate::test_support::TastyHomeGuard::new();
    let dir = tempfile::tempdir().expect("tempdir");
    let port_file = dir.path().join("tasty.port");
    let (inbound_tx, _inbound_rx) = std::sync::mpsc::channel();
    let ctx = StreamContext {
        hub: StreamHub::new(),
        inbound_tx,
        waker: Arc::new(|| {}),
        heartbeat_timeout: Default::default(),
    };
    let stats = Arc::new(ConnectionStats::default());
    let server = TcpIpcServer::start_with_port_file(
        Some(port_file.to_string_lossy().into_owned()),
        None,
        ctx,
        stats.clone(),
    )
    .expect("server");
    Running {
        server,
        stats,
        _dir: dir,
        _home: home,
    }
}

fn connect(running: &Running) -> TcpStream {
    let client = TcpStream::connect(("127.0.0.1", running.server.port)).expect("connect");
    client.set_read_timeout(Some(WAIT)).expect("read timeout");
    client
}

#[test]
fn a_served_connection_keeps_its_sockets_out_of_child_processes() {
    let running = start();
    let (_disarm, seen) = inherit_seam::arm(running.server.port, false);
    let _client = connect(&running);
    assert_eq!(
        seen.recv_timeout(WAIT).expect("the connection was served"),
        Seen::Served {
            reader: false,
            writer: false
        }
    );
}

#[test]
fn a_refused_connection_is_kept_out_of_child_processes_before_the_refusal() {
    let running = start();
    while running
        .stats
        .try_open(MAX_CONCURRENT_CONNECTIONS as u64)
        .is_some()
    {}
    let (_disarm, seen) = inherit_seam::arm(running.server.port, false);
    let client = connect(&running);
    assert_eq!(
        seen.recv_timeout(WAIT).expect("the connection was refused"),
        Seen::Refused { socket: false }
    );
    let mut line = String::new();
    BufReader::new(client)
        .read_line(&mut line)
        .expect("the refusal line");
    assert!(!line.is_empty(), "the refusal was not written");
}

#[test]
fn a_connection_that_cannot_be_kept_out_is_closed_unserved() {
    let running = start();
    let (_disarm, seen) = inherit_seam::arm(running.server.port, true);
    let mut client = connect(&running);
    let mut byte = [0u8; 1];
    let read = client.read(&mut byte);
    assert!(
        matches!(read, Ok(0))
            || read.as_ref().is_err_and(|error| {
                error.kind() == std::io::ErrorKind::ConnectionReset
                    || error.kind() == std::io::ErrorKind::ConnectionAborted
            }),
        "the server kept the connection open: {read:?}"
    );
    assert!(
        seen.try_recv().is_err(),
        "the connection was served or refused"
    );
    let snapshot = running.stats.snapshot();
    assert_eq!(
        snapshot.accept_waits,
        snapshot.accepted + snapshot.refused_saturated
    );
}
