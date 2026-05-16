# Agent 식별 — 잠정 모델 (Phase 4)

## 개요

`tasty-core::AgentId` 는 Phase 4 (관측/비용) 의 메트릭/cap/anomaly 가 agent 차원으로 집계되기 위한 식별자다. **현재 위조 가능한 잠정 모델** — Phase 6 의 session token 인증 도입 시 verifiable 로 승격한다.

## 도출 규칙

| Caller | agent_id |
|---|---|
| Plugin process | manifest 의 `plugin_id` (이미 manifest 등록을 통해 인증됨) |
| Local CLI/사용자 | env `TASTY_AGENT_ID` (없으면 sentinel `_host`) |

호스트 본 바이너리:

```rust
caller.agent_id()           // tasty_core::AgentId
```

라이브러리 크레이트 (예: `tasty-telemetry`) 에서 자기 caller 를 식별할 때:

```rust
tasty_core::AgentId::from_env()
```

## Child agent 의 env 주입

`claude.launch` / `claude.spawn` 으로 띄운 child Claude 는 PTY shell 위에서 다음 형태로 실행된다:

```
$ TASTY_AGENT_ID=claude_s<surface_id> claude
```

- `claude_s<surface_id>` 패턴은 surface 와 1:1 — 호스트가 surface_id 로 거꾸로 추적 가능
- shell history 에 한 줄 echo 되지만 정상 동선과 동일 (cd echo 와 같은 정책)
- inline env prefix 라 export 와 달리 history/profile 오염 없음
- prompt 와 결합 시: `TASTY_AGENT_ID=claude_s42 claude "$(cat /tmp/prompt.txt)"`

## 호환 표

| 위치 | sentinel / 형식 |
|---|---|
| `AgentId::HOST` | `"_host"` |
| `tasty_memory::HOST_OWNER` | `"_host"` (memory.db `owner` 컬럼과 호환) |
| `CallerContext::owner()` | Local → `"_host"`, Plugin → `plugin_id` |
| `CallerContext::agent_id()` | Local → env 또는 `"_host"`, Plugin → `plugin_id` |

`agent_id()` 는 Local 분기에서 env 를 본다는 점에서 `owner()` 와 다르다. memory 의 `owner` 는 plugin 사이 데이터 격리용이라 `_host` 로 일괄 묶고, telemetry 의 `agent_id` 는 child agent 까지 분리해야 하므로 env 를 추가로 본다.

## 보안 한계 (R2)

env `TASTY_AGENT_ID` 는 **위조 가능**하다:

- 적대적 agent 가 다른 agent 의 id 를 사칭 → cap/budget 우회
- 사용자가 직접 env 를 설정 → 자기를 child 로 위장 (의미 없음)

이 phase 는 **악의보단 버그** 영역으로 처리한다 — *정직한 agent 의 폭주를 막기 위한 안전망*. 적대적 agent 방어는 OS 권한·plugin 매니페스트가 우선.

Phase 6 의 session token 인증 도입 시:
- 호스트가 child spawn 시 비밀 token 발급
- agent 가 IPC 호출 시 token 을 같이 보내야 함
- 호스트가 token → agent_id 매핑을 메모리에서만 보관 (env 노출 X)
- `AgentId::from_caller(caller)` 가 token 검증 결과로 도출

승격 시점에 `docs/agent-guide/telemetry.md` 의 보안 경고 문구를 같이 갱신한다.

## 테스트

```bash
# 1) host sentinel
unset TASTY_AGENT_ID
tasty list info > /dev/null
# (Phase 4.1 도입 후) tasty telemetry summary --agent _host --json

# 2) env override
TASTY_AGENT_ID=foo tasty list info > /dev/null
# tasty telemetry summary --agent foo --json

# 3) child Claude
tasty claude spawn --workspace 1 --cwd /tmp
# child surface 의 첫 줄: "TASTY_AGENT_ID=claude_s<N> claude"
# tasty telemetry summary --agent claude_s<N> --json
```

`tasty_core::agent_id::tests` 에 단위 테스트 (`from_env_host_when_unset` / `from_env_picks_up_value` / `from_env_empty_treated_as_host`).
