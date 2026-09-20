# ADR-0315: plugin 채널의 두 방향은 포화에 다르게 답한다 — 호스트→plugin 은 거절, plugin→호스트 는 대기

- **Status**: Accepted
- **Date**: 2026-09-20
- **Tags**: plugin, host-plugin, resource-bounds, backpressure, reliability, adr-0304, adr-0311

## Context

호스트와 plugin 프로세스 하나 사이에는 채널이 셋 있고(`PluginRequest` ·
`PluginResponse` · `PluginEvent`) **셋 다 무제한** `mpsc::channel` 이었다. 무제한 큐는
소비자가 멈추면 생산자의 속도만큼 메모리를 먹는다. 여기서는 그 자리가 셋이라 한
plugin 이 밀리기 시작하면 **세 방향으로 동시에** 자란다.

멈추는 시나리오가 가설이 아니다. 호스트→plugin 쪽 소비자는 writer 스레드이고 그것은
소켓 `write_all` 에서 막힌다 — plugin 이 자기 소켓을 안 읽으면 그 자리에서 선다.
plugin→호스트 쪽 소비자는 호스트 pump 이고, 프레임이 길어지면 그만큼 안 비운다.

같은 성질의 상한을 소켓 수신 쪽에는 이미 뒀다([ADR-0304](0304-ipc-admission-carries-two-bounds-a-line-and-a-connection-count.md)
— 요청 한 줄의 바이트와 동시 연결 수). 이 결정은 그것을 **plugin 프로세스 경계로**
넓히는 것이고, 그래서 새 방어를 발명하는 것이 아니다.

제약이 하나 있다. **wire 를 바꾸지 않는다.** 포화를 plugin 에게 알리는 제어 응답은 새
프로토콜이라 호환 협상과 함께 정해야 하고, 그것을 여기서 앞당기면 번들 plugin 전부가
버전 bump + 매니페스트 lockstep 을 같은 커밋에 끌고 온다.

## Decision

세 채널을 `sync_channel` 로 유한하게 만들고, **포화의 답을 방향마다 다르게** 정한다.
두 방향이 같은 답을 쓸 수 없기 때문이다.

**호스트 → plugin 은 거절한다.** 송신 12 자리 중 거의 전부가 호스트 main thread 의
pump 안이다(`PluginManager::pump` → `apply_collected_events` · `drain_host_cmds`).
거기서 기다리면 **큐가 찼다는 이유로 프레임이 통째로 선다** — UI 가 멈추는 것은
메모리가 자라는 것보다 나쁘다. 그래서 `try_send` 로 넣고 실패를 호출부가 이미 들고
있던 "보내기 실패" 갈래로 흘린다. 실패 사유는 `RequestSendError` 로 **포화와 소멸을
가른다**: 무제한일 때는 실패 이유가 수신단 소멸 하나뿐이라 가를 필요가 없었는데,
유한해지면서 *일시적* 실패가 생겼고 둘이 뭉개지면 로그에서 "밀리는 중" 과 "죽었다" 를
구분할 수 없다.

**plugin → 호스트 는 기다린다.** 이쪽 송신자는 reader 스레드 하나뿐이라, 막혀도 서는
것은 프레임이 아니라 **소켓 읽기**다. 그것이 정확히 plugin 에 걸고 싶은 backpressure 다.
버리는 선택지는 여기서 성립하지 않는다 — 응답을 버리면 그 요청이 답을 못 받은 채
deadline 으로만 회수되고([ADR-0311](0311-a-namespace-call-expires-into-an-error-not-a-fail-open.md)),
이벤트를 버리면 `Hello` 가 빠져 plugin 이 등록조차 못 할 수 있다. 이벤트는 "있으면
좋은 알림" 이 아니라 등록·수명 전이를 나르는 축이다.

**송신 채널 필드는 비공개다.** 정책을 강제하는 것은 doc 이 아니라 가시성이다 — `pub`
이면 형제 모듈이 `.send()` 로 블로킹 송신을 되살릴 수 있고 그 자리는 아무도 안 본다.

용량은 셋 다 1024 다. **이 수는 파생이 아니다.** 프로토콜에 프레임당 메시지 수의
상한이 없고 `pending_requests` 도 `HashMap` 이라 상한이 없으며, 관측된 분포도 없다.
고른 근거는 둘뿐이다 — (1) pump 가 매 프레임 세 큐를 **끝까지** 비우므로 한 프레임
분량의 버스트를 여러 번 담을 수 있으면 정상 사용은 상한에 안 닿는다, (2) 곱이 작다:
채널 3 × 1024 × 번들 plugin 9 = 27,648 슬롯이고 메시지가 수 KB 라도 수십 MB 다.

## Consequences

- **얻은 것**: 세 자리가 **유한해졌다.** 그전에는 어느 쪽도 상한이 없었다.
- **얻은 것**: 실패가 두 값으로 갈렸다. 포화는 일시적이고 소멸은 영구인데 전에는 같은
  사건으로 보였다.
- **★ 잃은 것 — 포화에서 호스트가 요청을 버린다.** 전에는 밀려도 큐에 쌓였다. 이제
  `RequestSendError::Full` 인 요청은 **사라진다.** 버려지는 것이 무엇인지에 따라
  결과가 다르다: `surface.set_context` 가 빠지면 그 프레임의 입력이 안 가고,
  `ipc.result` 가 빠지면 호출자가 결과를 영영 못 받고, `shutdown` 이 빠지면 plugin 이
  graceful 기회를 잃고 kill 로 회수된다(`docs/architecture/shutdown-sequence.md`).
  이것이 (b) 가 필요한 이유다 — plugin 에게 포화를 알리고 재시도를 협상하는 것은
  wire 변경이라 여기서 못 한다.
- **잃은 것**: reader 스레드가 **두 큐를 공유**한다. 이벤트 큐가 차서 막히면 그 뒤에
  줄 선 응답도 같이 늦는다. 전에는 둘 다 무제한이라 이 결합이 없었다. 늦어진 응답은
  ADR-0311 의 deadline 을 건드릴 수 있다.
- **운영 비용**: 용량 셋이 파생이 아니라 정책값이다. 정상 사용이 상한에 닿는지는
  로그로만 알 수 있고, 닿으면 그 수가 틀린 것이다.

## Alternatives Considered

- **양방향 모두 블로킹** — 호스트 main thread 가 plugin 의 소비 속도에 묶인다. plugin
  하나가 안 읽으면 창 전체가 선다. 메모리를 지키려고 UI 를 내주는 교환이라 안 골랐다.
- **양방향 모두 버림** — 응답을 버리면 요청이 고아가 되고(deadline 으로만 회수),
  `Hello` 를 버리면 등록이 빠진다. plugin→호스트 쪽은 잃을 것이 일시적 지연이 아니라
  **상태**라 성립하지 않는다.
- **포화를 plugin 에게 제어 응답으로 알린다** — 올바른 종착지이지만 **wire 가 걸린다.**
  새 메시지의 의미를 구/신 plugin 이 같게 읽는지는 호환 협상에서 정할 일이고, 그
  시점에 번들 plugin 전부의 버전 bump 가 따라온다. 그래서 분리했다.
- **무제한 유지 + 워치독으로 길이만 관측** — 관측은 상한이 아니다. 길이를 알아도 막을
  자리가 없으면 같은 속도로 자란다. 순서가 반대다.
- **메시지 종류별 우선순위 큐** — 버릴 것을 고를 수 있게 되지만, 무엇이 버려도 되는지는
  plugin 마다 다르고 호스트가 알 방법이 없다. 상한 없이 정책부터 만드는 형태라 뺐다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- 호스트→plugin 이 기다리는 쪽으로 되돌아가면 `process::tests` 의
  `a_full_queue_is_refused_instead_of_awaited` 가 **멈추지 않고 실패한다**(판정을 다른
  스레드에서 timeout 으로 받는다 — 멈춘 시험은 실패보다 나쁘다).
- 포화와 소멸이 한 값으로 뭉개지면 `saturation_is_told_apart_from_a_dead_writer` 가 잡는다.
- 용량 상수 셋 중 하나라도 **안 쓰이면 컴파일이 실패한다** — 워크스페이스가 `dead_code`
  를 `deny` 로 두므로(루트 `Cargo.toml`) 프로덕션 자리에서 상수를 빼고 리터럴을 쓰면
  `constant ... is never used` 로 죽는다. 세 상수 전부에 걸린다.
- **0 이 되는 쪽은 `REQUEST_QUEUE_CAPACITY` 하나만 잡힌다** —
  `the_request_queue_holds_exactly_its_capacity` 의 `const { assert!(… > 0) }` 가 컴파일
  시점에 막는다. 나머지 둘(`RESPONSE_QUEUE_CAPACITY` · `EVENT_QUEUE_CAPACITY`)에는
  **그 채널이 없다** — 둘을 0 으로 바꿔도 이 크레이트의 시험 266 건이 전부 통과한다(실측).
  그쪽을 재는 두 시험(`responses_past_the_capacity_…` · `events_past_the_capacity_…`)이
  상수가 아니라 리터럴 `sync_channel(1)` 을 쓰는 것은 일부러다: 그 시험이 재는 것은 값이
  아니라 **버리는가 기다리는가**이고, 버퍼가 작을수록 가득 찬 순간을 확실히 만난다(용량의
  여러 배를 흘리는 것과 같은 근거). 상수를 읽게 바꾸면 그 확실성이 줄고, 그래도 증명되는
  것은 *시험이* 상수를 읽는다는 것뿐이라 프로덕션의 0 은 여전히 안 잡힌다.
- plugin→호스트 가 버리는 쪽으로 바뀌면 `responses_past_the_capacity_are_delayed_not_dropped`
  와 `events_past_the_capacity_are_delayed_not_dropped` 가 잡는다. 두 시험은 생산 함수
  (`handle_incoming_response` · `handle_incoming_event`)를 직접 부르고, 용량의 여러 배를
  연속으로 흘려 **스레드 순서에 기대지 않는다.** 한 건만 흘리는 형태로 처음 짰을 때는
  버리는 변이가 살아남았다 — 수신자가 먼저 비운 순간에 걸리면 통과했다.
- 형제 모듈이 `.send()` 로 블로킹 송신을 되살리면 **컴파일이 실패한다**(비공개 필드).

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- **정상 사용이 1024 에 닿는가.** 세 수 어느 것도 관측에서 나오지 않았다. 재는 법:
  호스트 로그에서 `request queue full` 을 찾는다 — 포화 거절마다 호출부가 한 줄 남긴다.
  정상 사용 중에 나오면 그 수가 틀린 것이다. plugin→호스트 쪽은 거절이 없어 로그가 안
  남으므로, 그쪽은 reader 스레드가 오래 막히는지를 봐야 한다(현재 그 상태를 노출하는
  값이 없다 — 아래 References 의 후속).
- **버려진 요청이 무엇을 깨뜨리는가.** 위 Consequences 가 든 셋은 코드 경로를 읽어
  적은 것이고 재현한 것이 아니다. 재는 법: writer 를 막아 놓은 plugin 에 대고
  `surface.set_context` · `ipc.result` · `shutdown` 을 각각 상한 너머로 보내 무엇이
  관측되는지 본다.
- **reader 스레드의 두 큐 결합이 deadline 을 건드리는가.** 재는 법: 이벤트를 상한까지
  채운 상태에서 namespace 호출의 만료율을 본다.

## References

- 관련 ADR: [ADR-0304](0304-ipc-admission-carries-two-bounds-a-line-and-a-connection-count.md)
  — 같은 형태의 상한을 IPC 소켓 수신 쪽에 둔 결정. 이 ADR 은 그것을 plugin 프로세스
  경계로 넓힌다.
- 관련 ADR: [ADR-0311](0311-a-namespace-call-expires-into-an-error-not-a-fail-open.md)
  — 응답을 버리면 안 되는 이유(만료가 caller 에 대한 오류가 된다).
- 관련 architecture: [shutdown-sequence](../architecture/shutdown-sequence.md)
  — `req_tx` 에 넣는 shutdown 요청이 이제 거절될 수 있다는 것.
- **코드 근거 (결정이 실현된 현재 위치)**: `PluginProcess::try_send_request` ·
  `try_send_request` · `RequestSendError` · `REQUEST_QUEUE_CAPACITY` ·
  `RESPONSE_QUEUE_CAPACITY` · `EVENT_QUEUE_CAPACITY` ·
  `handle_incoming_response` · `handle_incoming_event`
  (`crates/tasty-host-plugin/src/process.rs`).
