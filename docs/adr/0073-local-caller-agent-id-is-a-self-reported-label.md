# ADR-0073: 토큰 없는 로컬 호출자의 agent ID 는 요청 봉투로 받는 자기 신고 표시값이다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: ipc, telemetry, audit, compatibility, security
- **Group**: foundation

## Context

텔레메트리·감사·헤드리스 PTY 소유자는 호출자의 agent ID 를 기록한다. 세션 토큰을 쓰는 에이전트는 호스트가 `session.issue` 때 기록한 ID 로, 플러그인은 매니페스트 ID 로 구분된다. 토큰 없이 부른 로컬 호출(`CallerContext::Local`)은 요청에 호출자 정보가 없어서 모두 `_host` 로 기록됐다. 그래서 토큰 없이 띄운 에이전트나 사용자가 `TASTY_AGENT_ID` 를 둔 셸에서 부른 `tasty` 는 사용자와 구분되지 않았다.

호스트 프로세스 자신의 `TASTY_AGENT_ID` 는 쓸 수 없다. 그 값은 이 Tasty 를 띄운 바깥 인스턴스에서 상속한 것이라 실제 호출자와 관계가 없다. 호출자의 값은 호출자가 보내야 한다.

로컬 IPC 는 TCP 포트에 닿는 누구나 모든 메서드를 부를 수 있는 신뢰 경계다([ADR-0006](0006-bounded-ipc-transport.md), [ADR-0011](0011-secrets-and-local-trust.md)). 호출자가 보낸 ID 는 검증할 수 없다.

## Decision

요청 봉투(`tasty_ipc::protocol::JsonRpcRequest`)에 선택 필드 `caller_agent_id` 를 둔다. CLI 는 자기 환경의 `TASTY_AGENT_ID` 가 비어 있지 않으면 그 값을 싣는다(`tasty_ipc::protocol::caller_agent_id_from_env`).

- 이 값은 **자기 신고 표시값**이다. 세션 토큰이 없을 때만 `CallerContext::Local { claimed_agent_id }` 에 담기고, `CallerContext::agent_id()` 를 거쳐 텔레메트리 기록(`telemetry record` 의 기본 agent, `ipc_calls` 집계), 감사 레코드의 `caller_id`(caller_kind 는 `local` 그대로), 헤드리스 PTY 의 `owner_agent_id` 에 쓰인다.
- 권한 판단에는 쓰지 않는다. Local 의 권한(`ensure_allowed`·`permissions`), memory owner(`owner()` = `_host`), rate limit 제외(`should_rate_limit` 의 Local 분기)는 값이 있어도 바뀌지 않는다.
- 세션 토큰이 있으면 세션이 정한 ID 가 우선하고 이 값은 무시한다. 잘못된 토큰은 여전히 거절한다.
- 형식은 텔레메트리 agent ID 규칙(`tasty_telemetry::validate_agent_id`: `[a-zA-Z0-9_-]`, 64자 이하)이다. 맞지 않으면 버리고 `_host` 로 기록하며 요청은 거절하지 않는다. 표시용 값 때문에 사용자의 명령이 실패하면 안 된다.
- 호환: 봉투 필드 추가는 [API 규약](../dev-guide/api-conventions.md)의 minor 변경이다. 구 서버는 모르는 필드를 무시하고 `_host` 로 기록할 뿐 요청은 그대로 처리하므로, CLI 는 전송 전에 capability 를 묻지 않는다. 서버는 `system.info` 에 `ipc.caller-agent-id` 버전 1 을 선언해 지원 여부를 조회할 수 있게 한다.

## Consequences

- `TASTY_AGENT_ID=a` 인 셸에서 부른 `tasty` 의 텔레메트리·감사·PTY 소유자 표시가 `a` 로 나뉜다. 토큰 없는 Local 호출에 ID 가 있으면 `ipc_calls` 도 그 ID 로 집계된다(이전에는 `_host` 라 집계에서 빠졌다).
- 위조할 수 있다. 로컬 호출자는 다른 에이전트·플러그인의 ID 를 적어 그 이름의 텔레메트리를 늘릴 수 있고, 그 이름에 걸린 cap 이 발동할 수 있다. 이것은 `telemetry record --agent` 로 이미 가능한 일이라 새 능력이 아니다. 텔레메트리 모델은 정직한 에이전트의 폭주를 막는 안전망이고 적대적 호출자 방어가 아니다.
- `CallerContext::Local` 이 필드를 가진 variant 가 되어, Local 을 판별하는 곳은 `CallerContext::Local { .. }`, 만드는 곳은 `CallerContext::local()` 을 쓴다.

## Alternatives Considered

- **호스트 프로세스 env 의 `TASTY_AGENT_ID`**: 바깥 인스턴스의 상속값이라 호출자와 무관하다. 기각.
- **형식이 틀리면 요청을 거절**: 표시값 하나 때문에 사용자 셸의 모든 `tasty` 명령이 실패한다. 기각.
- **값을 Local 에서 Agent 로 승격**: 검증할 수 없는 값으로 권한 집합을 고르게 된다. 신원이 필요하면 `session.issue` 토큰을 쓴다. 기각.
- **CallerContext 와 별도로 ID 를 핸들러에 전달**: 소비처(감사·텔레메트리 게이트·PTY·record)마다 인자를 늘려야 하고, 호출자가 누구인가라는 한 사실이 두 곳으로 나뉜다. 기각.

## Reconsideration Triggers

코드와 설정에서 확인:

- 로컬 IPC 에 인증(연결별 토큰 등)이 생겨 Local 호출자를 검증할 수 있게 되면, 자기 신고 대신 검증된 ID 를 쓰는지 다시 정한다.
- `CallerContext::agent_id()` 의 결과가 권한·소유권 판단(memory owner, 플러그인 cap 차단, rate limit 대상 선정)에 쓰이게 되면 Local 의 자기 신고 값이 그 판단에 들어가지 않는지 다시 확인한다. 자동 검사는 `a_claimed_id_changes_neither_permissions_nor_the_memory_owner`(권한·owner)뿐이다.

## References

- `crates/tasty-ipc/src/protocol.rs` — `JsonRpcRequest::caller_agent_id`, `caller_agent_id_from_env`
- `crates/tasty-ipc/src/caller.rs` — `CallerContext::Local`, `local_claiming`, `resolve_caller_from_envelope`
- `crates/tasty-ipc/src/capability.rs` — `ipc.caller-agent-id`
- [텔레메트리 도출 규칙](../features/telemetry/index.md#도출-규칙)
