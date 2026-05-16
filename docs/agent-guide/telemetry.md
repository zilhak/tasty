# 텔레메트리 (관측 / 비용)

Tasty 는 AI 에이전트의 활동을 도메인 메트릭으로 기록·집계·차단할 수 있는 관측
계층을 제공한다. 이 문서는 에이전트 입장에서 어떻게 측정·조회·통제하는지 다룬다.

## 모델

| 개념 | 설명 |
|------|------|
| Metric | 메트릭 이름 (`input_tokens`, `wall_time_ms`, `ipc_calls`, …) — `^[a-zA-Z][a-zA-Z0-9_]{0,63}$` |
| Agent | 측정 대상 식별자 (`tasty.<plugin_id>` / `cli.<exe>` / `_host`). 미명시 시 CallerContext 로부터 자동 결정 |
| Workspace | 측정이 속한 워크스페이스. 없으면 `global` scope 영속 |
| Op | `Set` (절대값 덮어쓰기), `Inc` (누적 합산) — 시계열 합산 시 둘 다 sum 으로 동작 |
| Window | 시계열 윈도우 (`1m`, `5m`, `1h`, `1d`) |
| Tags | 임의 k:v map. 일부 메서드 (`ipc_calls`) 는 `method` 태그를 자동 부여 |

이벤트는 `tasty.telemetry.event.{ts:013}.{seq:04}` 키로 영속되며, 조회 핸들러는
prefix scan + 순수 집계로 응답한다. 호스트 재시작 후에도 누적값은 보존된다.

## 기록

```bash
# 단일 record
tasty telemetry record --agent my-plugin --metric input_tokens --value 1234
# 누적 (Inc) — files_read 를 1 씩 증가
tasty telemetry record --agent my-plugin --metric files_read --value 1 --op inc
# workspace 한정
tasty telemetry record --agent my-plugin --metric wall_time_ms --value 1500 --workspace-id 3
```

플러그인은 `telemetry.record` IPC 로 동일하게 호출한다 (`telemetry` 권한 필요).
배치 기록은 `telemetry.record_batch { events: [...] }` 로 N 건을 1 트랜잭션에
적재한다 (개별 검증 실패는 batch 전체 실패).

`ipc_calls` 메트릭은 dispatcher 가 plugin IPC 호출마다 자동으로 1 회씩 기록한다
(`tags.method` 에 정규 메서드명). agent 가 호스트(`_host`) 이거나 메서드가
`telemetry.*` 면 측정 자체를 건너뛴다 (자기측정 / 재귀 방지).

## 조회

```bash
# 누적 합 / count
tasty telemetry summary --agent my-plugin --metric input_tokens
# 1시간 버킷 시계열
tasty telemetry timeseries --metric input_tokens --window 1h --agent my-plugin
# top-N (agent 또는 workspace 기준)
tasty telemetry top --metric input_tokens --by agent --limit 10
```

JSON 응답은 `--json` 플래그로 가져온다. `since` / `until` (unix ms) 로 윈도우
필터링.

## Cost Cap

임계값을 정해 액션을 발화한다. 같은 (agent, metric, window) 의 raw event sum 이
`threshold` 이상이면 `triggered` 마크되고 action 이 실행된다.

```bash
# 1시간 윈도우에 input_tokens 100k 넘으면 plugin IPC 전부 거부
tasty telemetry cap set --agent my-plugin --metric input_tokens \
    --threshold 100000 --window 1h --action stop
tasty telemetry cap list
tasty telemetry cap status --agent my-plugin   # current_value / ratio
tasty telemetry cap reset --agent my-plugin    # triggered 비우기
```

| 액션 | 평가 시점 동작 | 후속 IPC 처리 |
|------|---------------|--------------|
| `notify` | 활성 워크스페이스에 알림 추가 | 변화 없음 |
| `stop` | 알림 + `triggered` 기록 | plugin agent 모든 IPC `-32007 cap_blocked` 거부 |
| `pause` | 알림 + `triggered` 기록 | plugin agent 모든 IPC `-32007 cap_blocked` 거부 |
| `require_approval` | `approval.request` 자동 발행 (severity=warn) + `triggered` 기록 | plugin agent 모든 IPC `-32007 cap_blocked` 거부 — 사용자 응답 후 `cap.reset` 으로 재개 |

차단된 plugin 본인은 `cap.reset` 도 막힌다. Local (CLI) 호출만 reset 가능.

## 이상 탐지

`AnomalyDetector` 가 dispatcher 후크에서 (agent, method) sliding window 를
갱신한다. 활성 휴리스틱은 `CallBurst` — 1분 내 1000 회 이상이면 발화. 같은
(agent, method) 의 재발화는 1분 쿨다운으로 dedup. `SlowLoop` / `RssSurge` 는
타입만 정의돼 있고 후속 sub-phase 에서 켜진다.

```bash
tasty telemetry anomaly list --agent my-plugin --kind call_burst
```

발화 레코드는 `tasty.telemetry.anomaly.{ts}.{id}` 키로 Global scope 영속 +
활성 워크스페이스 알림.

## 세션 요약

결정론적 순수 집계 (LLM 호출 없음).

```bash
tasty telemetry session-summary --format markdown
tasty telemetry session-summary --workspace-id 1 --format json --top-n 20
```

집계 항목:

- **tokens**: `ipc_calls` 제외 모든 metric 의 sum
- **ipc_calls**: total + method 별 top-N (기본 10, sort desc by count)
- **approvals**: total / pending / responded / timed_out / cancelled + responded choice 분포
- **anomalies**: Global scope prefix scan + since/until 윈도우

`workspace_id` 미지정 시 전 워크스페이스 합산 (포커스 독립 원칙).

## Claude Code 통합

`tasty claude install` 로 hook 을 등록하면 다음이 자동으로 텔레메트리에 적재된다:

| 이벤트 | 적재 메트릭 |
|--------|------------|
| `session-start` | (시작 시각 기억 only — record 없음) |
| `stop` / `subagent-stop` / `session-end` | `wall_time_ms` = `now - session_start` (start 가 기록돼 있을 때만) |
| `notification` (`--message` 동반 시) | `\btokens?:\s*(\d+)\b` 패턴 매칭 시 `input_tokens` |

기록 주체는 `tasty.com.tasty.claude` agent. tags 에 `surface_id` 포함.
호스트 재시작 시 wall_time_starts 가 휘발되므로 진행 중 세션의 wall_time 은
누락될 수 있다.

## 검증 시나리오 매핑

| ID | 시나리오 | 통과 조건 |
|----|---------|---------|
| T1 | record/summary | `tasty telemetry record --agent x --metric input_tokens --value 1234 && tasty telemetry summary --agent x` → sum=1234 |
| T2 | inc 누적 | `--op inc --value 1` 10 회 → sum=10 |
| T4 | top | 두 agent record 후 `tasty telemetry top --metric input_tokens --by agent --limit 2 --json` |
| T5 | cap notify | notify cap 설정 후 임계 초과 record → notification.list 에 cap 발화 |
| T9 | 이상 탐지 | 1분 내 동일 method 1000 회 → `tasty telemetry anomaly list --json` 에 등장 |
| T10 | 자동 요약 | `tasty telemetry session-summary --format markdown` 출력 검증 |
| T11 | dispatcher 카운트 | TASTY_AGENT_ID 환경에서 IPC 100 회 → `summary` ipc_calls=100 |
| T12 | claude.hook | session-start/stop 호출 시 `wall_time_ms` 자동 기록 |
| T14 | Plugin 권한 | `Telemetry` 권한 없는 plugin 의 `telemetry.record` → `-32004 missing_permission` |

전체 IPC/CLI 표면은 [api-reference.md](api-reference.md) 의 "Telemetry" 섹션을
참조.
