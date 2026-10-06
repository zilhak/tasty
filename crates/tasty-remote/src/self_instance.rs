//! SSH 터널 너머의 tasty가 이 프로세스 자신인지 판정한다.
//!
//! 호스트명이나 LAN IP로 자기 머신을 지정하면 요청 포트는 터널의 로컬 포트라서 자기 IPC 포트와
//! 비교해도 같지 않다. 그래서 터널을 연 뒤 상대의 `system.info`가 보고한 `instance_id`를 이
//! 프로세스의 값과 비교한다. 신뢰 경계는 SSH이며 별도 권한 검사를 더하지 않는다.

use crate::outbound::AttemptToken;

/// 상대가 이 프로세스 자신이라서 attach를 거절했다. 호출자는 downcast해 자기 포트 거절과
/// 같은 오류로 응답하거나 거절 기록에 남긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThisInstance {
    /// 터널의 로컬 포트.
    pub port: u16,
}

impl std::fmt::Display for ThisInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "attach target reached over SSH is this instance itself; attaching to itself is refused"
        )
    }
}

impl std::error::Error for ThisInstance {}

/// SSH 터널로 연결한 엔드포인트면 상대의 `instance_id`를 확인한다. loopback 직결(터널 없음)은
/// 포트 비교가 맡으므로 묻지 않는다. 상대가 `instance_id`를 보고하지 않으면(이전 버전) 판정하지
/// 않고 통과시킨다. `system.info` 요청 자체가 실패하면 그 오류를 돌려준다.
pub fn refuse_this_instance(
    tunneled: bool,
    port: u16,
    attempt: Option<&AttemptToken>,
) -> anyhow::Result<()> {
    if !tunneled {
        return Ok(());
    }
    let info =
        crate::browse::probe_method_bound(port, "system.info", serde_json::json!({}), attempt)?;
    if reports_this_instance(&info) {
        return Err(ThisInstance { port }.into());
    }
    Ok(())
}

fn reports_this_instance(info: &serde_json::Value) -> bool {
    info.get("instance_id").and_then(serde_json::Value::as_str)
        == Some(tasty_ipc::instance::instance_id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    /// 요청 한 줄을 읽고 `system.info` 결과 한 줄을 돌려주는 가짜 서버.
    fn serve_once(result: serde_json::Value) -> (u16, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).expect("read");
            let reply = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result });
            writeln!(&stream, "{reply}").expect("write");
            line
        });
        (port, handle)
    }

    #[test]
    fn a_peer_reporting_this_process_id_is_refused() {
        let (port, server) =
            serve_once(serde_json::json!({ "instance_id": tasty_ipc::instance::instance_id() }));
        let error = refuse_this_instance(true, port, None).expect_err("same instance");
        assert_eq!(
            error.downcast_ref::<ThisInstance>(),
            Some(&ThisInstance { port })
        );
        assert!(server.join().expect("server").contains("\"system.info\""));
    }

    #[test]
    fn another_instance_or_an_older_peer_passes() {
        for result in [
            serde_json::json!({ "instance_id": "another-instance" }),
            serde_json::json!({ "version": "0.1.0" }),
        ] {
            let (port, server) = serve_once(result);
            refuse_this_instance(true, port, None).expect("not this instance");
            server.join().expect("server");
        }
    }

    #[test]
    fn a_direct_loopback_endpoint_is_not_asked() {
        // 아무도 듣지 않는 포트라도 터널이 아니면 연결하지 않는다.
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .expect("free port")
            .port();
        refuse_this_instance(false, port, None).expect("not asked");
    }
}
