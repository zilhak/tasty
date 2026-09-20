# ADR-0327: 전송 계층은 침묵 대신 답하고, 그 답의 쓰기에는 시간 상한이 있다

- **Status**: Accepted
- **Date**: 2026-09-20
- **Tags**: ipc, transport, resource-bounds, reliability

## Context

일반 request-response 경로의 **쓰기 쪽에 상한이 하나도 없었다.** `write_json_line` 의
`writeln!` + `flush` 는 무한정 블록한다. 응답을 안 읽는 peer 가 소켓 송신 버퍼를 채우면
그 연결 스레드가 거기서 멈추고, 멈춘 스레드는 `ConnectionSlot` 을 계속 쥔다 — 자리는
`Drop` 에서만 돌아온다. 그런 peer 가 `MAX_CONCURRENT_CONNECTIONS` 만큼이면 정상 client 가
못 붙는다.

즉 [ADR-0304](0304-ipc-admission-carries-two-bounds-a-line-and-a-connection-count.md)
가 센 자리는 **쓰기에 상한이 없어서 영구 점유될 수 있었다.** 수신에 상한을 둔 결정이
쓰기 쪽 무한 대기로 무력화되는 형태다.

같은 소켓의 **스트림 업그레이드** 경로는 사정이 다르다. 거기 쓰기는 프레임을 나르는
전용 write 스레드가 하고(`spawn_stream_write_thread`), 그 스레드는 쓰기 오류 하나에
루프를 끊는다. 시간 상한을 얹으면 출력이 몰린 attach 가 타임아웃 한 번에 끊기고, 프레임이
반만 나간 뒤 끊기면 길이 접두사 기준이 어긋나 그 뒤가 전부 오정렬된다.

## Decision

**request-response 연결의 쓰기에 `RESPONSE_WRITE_TIMEOUT` 을 건다.** 값은 고르지 않고
`stream::HEARTBEAT_TIMEOUT` 에서 **파생한다** — 그 상수가 이미 답하는 물음이 같다("이
loopback 소켓의 상대가 얼마나 진척을 안 내면 죽은 것으로 보는가"). 같은 소켓의 attach
쪽은 그 값을 read timeout 으로 걸고, 그 소켓의 **client 측은 같은 값을 자기 write
timeout 으로 건다**(`src/app/attach_client.rs`). 여기서 새 수를 고르면 같은 물음에 답이
둘이 된다.

**거는 자리는 `configure_socket` 이 아니라 스트림 업그레이드 판별 뒤다.** 소켓 옵션은
`try_clone` 한 reader/writer 가 함께 받으므로, 업그레이드로 갈 수 있는 자리에서 걸면
위 Context 의 attach 파손이 따라온다. 업그레이드 경로의 쓰기 상한은 별개 결정이고 여기서
앞당기지 않는다.

**타임아웃은 연결을 닫는 사건이다.** 플랫폼이 그렇게 정한다 — Windows 는 타임아웃 후
소켓에 얼마나 썼는지가 불명이라(부분 전송) 한 줄이 반만 나갈 수 있다. 그 뒤로 계속 읽으면
client 는 잘린 JSON 뒤에 다음 응답이 이어 붙은 것을 본다. 그래서 **쓰기 실패는 예외 없이
연결 종료**로 처리한다. dispatch 응답 경로는 이미 그랬고, parse error 응답 경로는 아니었다
(`send_parse_error` 가 `trace` 한 줄만 남기고 연결을 유지했다) — **그 갈래를 닫는 쪽으로
바꾼다.** 그 판단은 쓰기가 무한정 블록하던 시절에 맞았다: 그때 쓰기 실패는 소켓이 이미
깨졌다는 뜻이라 계속 읽어도 곧 EOF 였다. 지금은 **살아 있는 소켓에 한 줄이 반만 나간
상태**가 같은 갈래로 떨어진다.

같은 이유로 그 자리의 로그를 `trace` 에서 `warn` 으로 올린다. 타임아웃이 **발화했다**는
것을 말하는 자리가 거기뿐인데 기본 필터는 `trace` 를 버린다.

**거절을 응답으로 알리는 형태는 같은 회차의 뒤 조각에서 확정한다.** ADR-0304 는 수신
상한 초과를 "닫고 응답하지 않는다" 로 정하면서 그 이유를 "응답으로 알리는 것은 wire 가
걸리는 별개 결정" 이라고 적었다. 이 상한이 그 결정의 **선행**이다 — 거절을 응답으로
알리려면 그 응답을 쓰는 자리가 시간에 갇혀 있어야 하고, 특히 연결 상한 거절은 accept
스레드에서 일어나므로 상한 없는 쓰기를 거기 두면 accept 루프 전체가 멈출 수 있다.

## Consequences

- **얻은 것**: 연결 스레드가 **쓰기에서 영구히 멈출 수 없다.** ADR-0304 의 연결 상한이
  세는 자리가 회수되지 않는 경로 하나가 닫혔다.
- **얻은 것**: parse error 응답의 쓰기 실패가 이제 **연결을 끝낸다.** 잘린 줄 뒤에 다음
  응답이 이어 붙는 상태가 원천적으로 안 생긴다.
- **잃은 것**: 상한을 초과하는 **느린 정상 peer 를 끊는다.** 이 소켓은 127.0.0.1 loopback
  전용이라(`TcpListener::bind("127.0.0.1:0")`) 링크 지연으로 상한에 닿는 경우는 없고,
  닿으려면 상대가 응답을 안 읽어야 한다. 그래도 이것은 정책이지 증명이 아니다.
- **잃은 것 (★ 발화를 재는 채널이 없다)**: 상한이 **걸렸다**는 것은 시험이 잡지만
  (`admission_tests::the_response_write_timeout_is_actually_on_the_socket` 이 소켓에게
  값을 되묻는다) **발화한다**는 것은 아무 시험도 안 잡는다. 재현하려면 상대가 안 읽어
  송신 버퍼가 차야 하는데 그 버퍼 크기는 커널이 자동 조정한다 — 느리고 불안정하다.
- **운영 비용**: 두 상수가 한 값을 공유한다. `stream::HEARTBEAT_TIMEOUT` 이 움직이면
  이 상한도 함께 움직인다. 그것이 의도다 — 갈라져야 할 이유가 생기면 그 이유를
  `RESPONSE_WRITE_TIMEOUT` 의 doc 에 적고 그때 가른다.

## Alternatives Considered

- **`configure_socket` 에서 모든 연결에 건다** — 한 줄 더 짧지만 attach 스트림이 같이
  걸린다. 출력이 몰린 attach 가 타임아웃 한 번에 끊기고, 부분 전송된 프레임이 길이 접두사
  정렬을 깨 그 뒤 전부가 오정렬된다. 대칭보다 그쪽 파손이 크다.
- **새 수를 고른다(예: 30 초)** — 같은 물음에 답이 둘이 된다. 파생 원천이 이미 있는데
  새 수를 고르면 둘이 갈릴 때 어느 쪽이 맞는지 판정할 근거가 없다.
- **타임아웃 뒤 이어 쓰기(재시도)** — 얼마나 썼는지 모르는 플랫폼이 있어 이어 쓸 지점을
  못 정한다. 재시도는 그 자체로 잘린 줄을 만든다.
- **쓰기를 별도 스레드로 옮긴다** — 연결 상한이 막으려던 자원(스레드)을 연결마다 하나 더
  쓴다. 상한을 위해 상한의 대상을 늘리는 형태다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- `RESPONSE_WRITE_TIMEOUT` 이 소켓에 실제로 걸리는가는
  `admission_tests::the_response_write_timeout_is_actually_on_the_socket` 이 잡는다.
  값이 안 걸리는 실패와 플랫폼이 값을 반올림하는 실패가 둘 다 그 시험에서 드러난다 —
  좌변을 상수와 비교하지 않고 **소켓에게 되묻기** 때문이다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- **상한이 정상 peer 를 끊는가.** 재는 법: 호스트 로그에서 `IPC write error` ·
  `IPC parse-error response write failed` 를 찾는다. 그 줄이 정상 사용 중에 나오면
  상한이 짧거나 이 소켓에 loopback 아닌 상대가 붙은 것이다.
- **Windows 에서 부분 전송이 실제로 나는가.** 이 레포의 개발 머신은 Linux 라 그 갈래가
  관측되지 않는다. 재는 법: Windows 에서 응답을 안 읽는 client 를 붙이고 타임아웃 뒤
  client 가 받은 바이트가 줄의 접두사인지 본다. 지금은 **미측정**이고, 닫는 처방은 그
  가능성에 대한 방어다.

## References

- 관련 ADR: [ADR-0304](0304-ipc-admission-carries-two-bounds-a-line-and-a-connection-count.md)
  — 수신 쪽 두 상한. 이 결정이 그 상한을 무력화하던 쓰기 경로를 닫는다.
- 관련 ADR: [ADR-0004](0004-ipc-transport-tcp.md) — 이 소켓이 TCP loopback 인 이유.
- 관련 dev-guide: [attach-behavior](../dev-guide/attach-behavior.md) — 스트림 경로가 이
  상한의 대상이 아닌 이유(프레임 쓰기와 전용 write 스레드).
- **코드 근거 (결정이 실현된 현재 위치)**: `RESPONSE_WRITE_TIMEOUT` ·
  `TcpIpcServer::arm_response_write_timeout` · `TcpIpcServer::send_parse_error`
  (`src/adapters/production/tcp_ipc_server.rs`).
