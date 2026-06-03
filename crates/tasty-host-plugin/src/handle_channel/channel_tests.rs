//! `HandleStream` / `HandleListener` 단위 테스트 (unix 전용).

#![cfg(all(test, unix))]

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

#[test]
fn handle_listener_bind_produces_endpoint() {
    let l = HandleListener::bind().expect("bind");
    assert!(!l.endpoint().is_empty());
    assert!(std::path::Path::new(l.endpoint()).exists());
}

#[test]
fn handle_listener_drop_removes_socket_file() {
    let path: std::path::PathBuf;
    {
        let l = HandleListener::bind().expect("bind");
        path = std::path::PathBuf::from(l.endpoint());
        assert!(path.exists());
    }
    assert!(!path.exists(), "socket file should be removed on Drop");
}

#[test]
fn auth_flow_matches_token() {
    let listener = HandleListener::bind().expect("bind");
    let endpoint = listener.endpoint().to_string();
    let token = "test-handle-token".to_string();

    std::thread::scope(|s| {
        let token_clone = token.clone();
        let endpoint_clone = endpoint.clone();
        s.spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let mut stream = UnixStream::connect(&endpoint_clone).unwrap();
            let auth = AuthMessage {
                plugin_id: "com.test.plugin".into(),
                token: token_clone,
            };
            let line = serde_json::to_string(&auth).unwrap() + "\n";
            stream.write_all(line.as_bytes()).unwrap();
            stream.flush().unwrap();
            // ack 한 줄 read해서 채널이 살아 있음을 확인.
            let cloned = stream.try_clone().unwrap();
            let mut reader = BufReader::new(cloned);
            let mut ack = String::new();
            reader.read_line(&mut ack).unwrap();
            assert!(ack.contains("\"ok\":true"));
            std::thread::sleep(Duration::from_millis(50));
        });

        let stream = listener.expect_connection(&token, Duration::from_secs(2));
        assert!(stream.is_some(), "expected handle stream to be received");
    });
}

#[test]
fn send_handle_delivers_fd_via_scm_rights() {
    use std::os::fd::AsRawFd;
    use tasty_plugin_protocol::SharedBufferId;

    // socketpair로 host/plugin 양쪽 simulate.
    let (host_raw, plugin_raw) = UnixStream::pair().expect("socketpair");
    let mut host_stream = HandleStream::from_unix(host_raw);

    // /dev/null fd 하나를 cmsg에 실어 보낸다.
    let f = std::fs::File::open("/dev/null").expect("open /dev/null");
    let send_fd = f.as_raw_fd();
    let msg = HandleChannelMessage::HandleAttach {
        request_id: 1,
        id: SharedBufferId(42),
        size: 4096,
    };
    host_stream.send_handle(&msg, send_fd).expect("send_handle");

    // plugin 측에서 recvmsg로 받기 — unix_wire::recv_with_fd 직접 호출.
    let mut buf = [0u8; 4096];
    let (n, fds) = unix_wire::recv_with_fd(&plugin_raw, &mut buf).expect("recv");
    assert!(n > 0);
    assert_eq!(fds.len(), 1, "정확히 fd 1개가 와야 함");
    let recv_fd = fds[0];
    assert_ne!(recv_fd, send_fd, "kernel이 dup해서 다른 번호의 fd 전달");

    // bytes에는 JSON 한 줄.
    let line = std::str::from_utf8(&buf[..n]).expect("utf8").trim();
    let got: HandleChannelMessage = serde_json::from_str(line).expect("json");
    assert_eq!(got, msg);

    // SAFETY: 받은 fd close — leak 방지. recv_fd는 방금 dup된 valid한 file descriptor.
    unsafe {
        libc::close(recv_fd);
    }
}

/// 02c-6 happy path: 호스트에서 shm을 만들어 fd를 보내고, plugin 측에서 받아
/// 매핑한 뒤 동일 영역에 쓴 바이트가 호스트에서 보이는지 확인.
/// 또한 plugin 측에서 Dirty를 보내고 호스트의 HandleStreamReader가 디코드하는지도
/// 검증.
#[cfg(unix)]
#[test]
fn shared_buffer_roundtrip_via_handle_channel() {
    use tasty_plugin_protocol::{PixelRect, SharedBufferId};

    let (host_raw, plugin_raw) = UnixStream::pair().expect("socketpair");
    let mut host_stream = HandleStream::from_unix(host_raw);
    let host_reader_stream = host_stream.reader().expect("reader split");

    // host: shm 영역 생성 + sendable 핸들 준비.
    let (host_mem, sendable) = tasty_shm::create(4096).expect("shm create");
    let payload =
        tasty_shm::prepare_send(sendable, tasty_shm::PeerPid::Same).expect("prepare_send");

    // host → plugin: HandleAttach + SCM_RIGHTS(fd).
    let id = SharedBufferId(7);
    let attach = HandleChannelMessage::HandleAttach {
        request_id: 1,
        id,
        size: 4096,
    };
    host_stream
        .send_handle(&attach, payload.raw_fd())
        .expect("send_handle");

    // plugin 측: raw socket에서 recvmsg로 fd를 받는다.
    let mut buf = [0u8; 4096];
    let (n, fds) = unix_wire::recv_with_fd(&plugin_raw, &mut buf).expect("plugin recv");
    assert!(n > 0);
    assert_eq!(fds.len(), 1, "정확히 fd 1개가 도착");
    let line = std::str::from_utf8(&buf[..n]).expect("utf8").trim();
    let got_msg: HandleChannelMessage = serde_json::from_str(line).expect("decode HandleAttach");
    assert_eq!(got_msg, attach);

    // plugin 측: 받은 fd를 tasty_shm::receive로 매핑.
    let plugin_mem = tasty_shm::receive(tasty_shm::ReceivedPayload::Fd {
        fd: fds[0],
        size: 4096,
    })
    .expect("receive map");
    assert_eq!(plugin_mem.len(), 4096);
    assert_eq!(host_mem.len(), 4096);

    // plugin 측에서 영역 전체를 0xAB로 채운다.
    // SAFETY: 단일 스레드 테스트 — host는 아직 동시 mutate하지 않음.
    unsafe {
        plugin_mem.as_mut_slice().fill(0xAB);
    }

    // host 측에서도 같은 영역이 보여야 한다.
    // SAFETY: plugin 쪽이 write를 마쳤고 후속 동시 mutate 없음.
    let host_view = unsafe { host_mem.as_slice() };
    assert!(
        host_view.iter().all(|&b| b == 0xAB),
        "공유 매핑이 동일해야 함"
    );

    // plugin → host: Dirty 메시지 송신 (raw write, fd 없이).
    let dirty = HandleChannelMessage::Dirty {
        id,
        rect: Some(PixelRect {
            x: 0,
            y: 0,
            w: 32,
            h: 32,
        }),
    };
    let mut dirty_line = serde_json::to_string(&dirty).expect("encode dirty");
    dirty_line.push('\n');
    unix_wire::send_with_fd(&plugin_raw, dirty_line.as_bytes(), None).expect("plugin send dirty");

    // host: HandleStreamReader로 Dirty 디코드.
    let mut reader = host_reader_stream;
    let (got_dirty, aux) = reader.recv_message().expect("host recv dirty");
    assert!(aux.is_none(), "Dirty에는 ancillary fd 없음");
    assert_eq!(got_dirty, dirty);

    // SAFETY: payload는 method scope 끝까지 살아 있어야 send 측 fd가 유효.
    // 명시적 drop으로 의도 표시.
    drop(payload);
    // plugin이 받은 매핑은 plugin_mem이 자체 소유 (Drop에서 munmap).
    // recv_with_fd가 반환한 fds[0]은 tasty_shm::receive로 소유권 이전됨.
    // 명시적 drop으로 scope 끝 정리 시점을 코드에 박아둔다 (host_mem도 동일).
    drop(host_mem);
    drop(plugin_mem);
}

#[test]
fn auth_flow_rejects_unknown_token() {
    let listener = HandleListener::bind().expect("bind");
    let endpoint = listener.endpoint().to_string();

    std::thread::scope(|s| {
        let endpoint_clone = endpoint.clone();
        s.spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let mut stream = UnixStream::connect(&endpoint_clone).unwrap();
            let auth = AuthMessage {
                plugin_id: "com.test.plugin".into(),
                token: "unknown-token".into(),
            };
            let line = serde_json::to_string(&auth).unwrap() + "\n";
            stream.write_all(line.as_bytes()).expect("test auth write");
            stream.flush().expect("test auth flush");
            let cloned = stream.try_clone().unwrap();
            let mut reader = BufReader::new(cloned);
            let mut ack = String::new();
            // 호스트가 ack(false)를 보낸 뒤 stream을 닫으므로 read_line이 EOF로 끝날 수
            // 있다 — 아래 assert가 본 검증이라 read 결과 자체는 무시.
            let _ = reader.read_line(&mut ack);
            assert!(ack.contains("\"ok\":false"));
        });

        let stream = listener.expect_connection("expected-token", Duration::from_millis(800));
        assert!(stream.is_none(), "expected no stream (token mismatch)");
    });
}
