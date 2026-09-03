//! 서버 attention 의 attach mirror push(`StreamControl::Attention`, server→client)를
//! loopback `TcpStream` 으로 실제 실행 중인 서버 인스턴스에 대해 검증한다.
//!
//! frame/handshake 헬퍼는 `tests/attach_common/mod.rs` 를 공유한다 — attach client 는
//! 실제 `tasty` GUI 앱이 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다. 서버
//! 인스턴스는 `common::shared()` 하나를 이 test binary 전체가 함께 쓰고, 점유가 필요한
//! 테스트는 `create_workspace()` 로 자기 workspace 를 만든다.
//!
//! **GUI 두 인스턴스를 실제로 attach 해 미러 사이드바의 배지를 눈으로 확인하는 e2e**
//! 는 이 headless 작업 환경(GPU 디스플레이 없음)에서 실행할 수 없다 — 이 test 는 그
//! 대체로, (1) attach 점유 획득 → (2) `surface.completion` IPC 로 서버에서 attention
//! raise → (3) 1Hz forward tick 이 `attention` Control 프레임을 원격 surface id 와
//! 함께 push 하는 전체 경로를 프로토콜 레벨에서 실행한다.
//! `docs/features/surface-highlight/index.md` "검증 한계" 절 참고.

mod attach_common;
mod common;

use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

use attach_common::{TAG_CONTROL, open_workspace_attach};
use serde_json::{Value, json};

/// dedup(스팸 없음) 확인용 정적 대기 — 1Hz forward tick 을 여러 번 지나칠 만큼만
/// 기다린다. `attach_common::read_frame` 은 타임아웃에 panic 하므로 여기서는
/// 타임아웃을 정상 종료로 다루는 자체 reader 를 쓴다.
const QUIET_WINDOW: Duration = Duration::from_millis(3_000);

/// 프레임 하나를 읽되, read 타임아웃이면 `None`. `attach_common::read_frame` 의
/// 비-panic 판(negative assertion 전용).
fn try_read_frame(stream: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let mut hdr = [0u8; 5];
    if stream.read_exact(&mut hdr).is_err() {
        return None;
    }
    let tag = hdr[0];
    let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 && stream.read_exact(&mut payload).is_err() {
        return None;
    }
    Some((tag, payload))
}

/// `attention` Control 프레임이 올 때까지 읽되, `kind` 가 non-null 인 첫 프레임을
/// 돌려준다. 세션 시작 직후 서버는 attention 이 없는 상태를 알리는 `kind: null`
/// baseline 을 1회 push 할 수 있으므로(점유 시작 시점과 raise 시점의 경합) 그건
/// 건너뛴다. 그 외 무관한 control 프레임(터미널 스냅샷·`activity` 등)도 건너뛴다.
fn wait_for_raised_attention(stream: &mut TcpStream) -> Value {
    loop {
        let (tag, payload) = attach_common::read_frame(stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("event").and_then(|e| e.as_str()) != Some("attention") {
            continue;
        }
        if v.get("kind").map(Value::is_null).unwrap_or(true) {
            continue; // baseline(해제 상태) push — 아직 raise 전.
        }
        return v;
    }
}

/// 서버에서 `needs_input` attention 이 raise 되면 점유 client 에게 `attention`
/// Control 프레임이 원격 surface id 와 함께 push 된다. `NeedsInput` 은 서버(PTY 를
/// 가진 인스턴스)에서만 나오는 kind 라, 이 경로가 없으면 미러 사용자에게 "응답 필요"
/// 가 도달할 방법이 원천적으로 없다.
#[test]
fn attention_is_pushed_to_the_attach_holder() {
    let server = common::shared();
    let ws = server.create_workspace("attention-push");

    let mut stream = open_workspace_attach(server.port(), ws.id);

    server.call(
        "surface.completion",
        json!({ "surface_id": ws.surface_id, "kind": "needs_input" }),
    );

    let frame = wait_for_raised_attention(&mut stream);
    assert_eq!(
        frame["surface_id"], ws.surface_id,
        "attention 은 **원격(서버) surface id** 로 앵커된다 — client 가 자기 매핑으로 로컬 mirror id 를 찾는다: {frame:?}"
    );
    assert_eq!(
        frame["kind"], "needs_input",
        "서버가 raise 한 kind 가 그대로 실려야 한다: {frame:?}"
    );
}

/// `completion` kind 도 같은 채널로 전달된다 — attention 은 kind 별 별도 채널이
/// 아니라 하나의 프레임에 kind 를 실어 보낸다.
#[test]
fn completion_kind_is_pushed_over_the_same_channel() {
    let server = common::shared();
    let ws = server.create_workspace("attention-push-completion");

    let mut stream = open_workspace_attach(server.port(), ws.id);

    // kind 생략 = Completion(하위 호환 — CLI/OSC 133 producer 경로).
    server.call("surface.completion", json!({ "surface_id": ws.surface_id }));

    let frame = wait_for_raised_attention(&mut stream);
    assert_eq!(frame["surface_id"], ws.surface_id);
    assert_eq!(frame["kind"], "completion", "{frame:?}");
}

/// 값이 바뀌지 않는 tick 에는 프레임이 나가지 않는다(스팸 없음) — 서버측
/// `last_forwarded_attention` dedup 이 실제 스트림에서도 동작하는지 확인한다.
/// 같은 kind 로 IPC 를 다시 호출해도 값이 그대로라 새 프레임이 없어야 한다.
#[test]
fn unchanged_attention_does_not_respam_the_stream() {
    let server = common::shared();
    let ws = server.create_workspace("attention-push-dedup");

    let mut stream = open_workspace_attach(server.port(), ws.id);
    server.call(
        "surface.completion",
        json!({ "surface_id": ws.surface_id, "kind": "needs_input" }),
    );
    let first = wait_for_raised_attention(&mut stream);
    assert_eq!(first["kind"], "needs_input");

    // 같은 kind 로 재발동 — 서버 store 값이 바뀌지 않으므로 forward 대상이 아니다.
    server.call(
        "surface.completion",
        json!({ "surface_id": ws.surface_id, "kind": "needs_input" }),
    );

    stream
        .set_read_timeout(Some(QUIET_WINDOW))
        .expect("set quiet-window read timeout");
    let mut extra: Vec<Value> = Vec::new();
    while let Some((tag, payload)) = try_read_frame(&mut stream) {
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("event").and_then(|e| e.as_str()) == Some("attention") {
            extra.push(v);
        }
    }
    assert!(
        extra.is_empty(),
        "값이 그대로면 1Hz tick 이 여러 번 지나도 attention 프레임이 나가면 안 된다: {extra:?}"
    );
}
