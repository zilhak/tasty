# 텔레메트리 (Telemetry)

- **Status**: Implemented
- **주체**: AI Agent (측정 대상) · 로컬 사용자 (조회/통제)
- **ADR**: 없음
- **코드**: `telemetry.*` 핸들러, `tasty-telemetry`, 영속 `tasty-memory`
- **화면**: 없음 (cap 발화 시 알림)
- **메서드 목록**: [reference/api](../../reference/api.md#텔레메트리-telemetry)

## 목적

AI 에이전트 활동을 도메인 메트릭으로 **기록·집계·차단**하는 관측 계층. 비용(토큰)·호출량을 추적하고 임계 초과 시 액션을 발화한다.

## 내부 동작

### 모델

Metric(`input_tokens`/`ipc_calls`/…) × Agent(`tasty.<plugin_id>`/`cli.<exe>`/`_host`, 미명시 시 CallerContext 자동) × Workspace(없으면 `global`) × Op(`Set`/`Inc`, 시계열은 둘 다 sum) × Window(`1m/5m/1h/1d`) × Tags. 이벤트는 `tasty.telemetry.event.{ts}.{seq}` 로 영속, 조회는 prefix scan + 순수 집계(재시작 후 누적 보존).

`ipc_calls` 는 dispatcher 가 plugin IPC 호출마다 자동 1회 기록(`tags.method`). `_host` 또는 `telemetry.*` 는 자기측정/재귀 방지로 skip.

### Cost Cap

(agent, metric, window) raw sum 이 `threshold` 이상이면 `triggered` + 액션:

| 액션 | 후속 IPC |
|------|----------|
| `notify` | 변화 없음 (알림만) |
| `pause` | 그 plugin agent 의 모든 IPC `-32007 cap_blocked`(과거 `stop` 값으로 저장된 cap 은 `pause` 로 자동 마이그레이션됨) |
| `require_approval` | `approval.request`(severity=warn) 자동 + IPC 차단 → 사용자 응답 후 `cap.reset` 으로 재개 |

차단된 plugin 본인은 `cap.reset` 도 막힌다 — **Local(CLI)만 reset**. 누적 임계인 cap 과 *시간당 비율*인 [agent rate-limit](../agent-collaboration/index.md) 은 다른 시스템.

### 이상 탐지

`AnomalyDetector` 가 세 휴리스틱을 유지한다 — **진짜 정체/누수 탐지가 아니라 값싼 신호 기반 후보 알림**이라는 전제로 소비해야 한다.

| 휴리스틱 | 판정 | 기본값 |
|---|---|---|
| `CallBurst` | (agent, method) sliding window 호출 카운트 | 1분 1000회↑ |
| `SlowLoop` | (agent, method, params-hash) sliding window 반복 카운트(동일 파라미터 반복) | 5분 20회↑ |
| `RssSurge` | agent 당 최근 5개 RSS 샘플의 **엄격한 단조 증가**(스파이크 1회는 발화 안 함, 추세만) | 5 샘플 |

RSS 값 소스는 caller 타입별로 다르다: **Plugin** 은 host(`tasty-host-plugin::PluginManager`)가 `PluginProcess.child` 의 PID 를 sysinfo 로 30초 간격 직접 sampling(agent 자가 보고는 신뢰 불가 — 정확한 자기 RSS 를 보고할 유인이 없음). **Agent** 는 PID 기반이 구조적으로 불가능(원격/별도 프로세스)해 `telemetry.record` 자가 보고(`metric == "rss_bytes"`)로 받는다. 세 휴리스틱 모두 같은 (agent, kind, subject) 1분 쿨다운 dedup 을 공유하고, 발화는 동일하게 `tasty.telemetry.anomaly.*` 영속 + 알림.

### 세션 요약

결정론적 순수 집계(LLM 없음): tokens(ipc_calls 제외 metric sum) / ipc_calls(method 별 top-N) / approvals 분포 / anomalies. `workspace_id` 미지정 시 전 워크스페이스 합산(포커스 독립).

## 인터페이스

- **AI Agent / CLI**: `telemetry.record(_batch)`(`telemetry` 권한) · `summary/timeseries/top` · `cap.{set,list,status,reset}` · `anomaly.list` · `session_summary`. [reference/api](../../reference/api.md#텔레메트리-telemetry).
- **Claude Code 통합**: `tasty claude install` hook 이 `session-start`→`stop` 의 `wall_time_ms`, notification 의 `input_tokens`(`tokens: N` 패턴)를 `tasty.com.tasty.claude` agent 로 자동 적재. [claude plugin](../../plugins/claude/index.md).

## 관련

- [agent-collaboration](../agent-collaboration/index.md) — rate-limit 과의 구분 · [human-handoff](../human-handoff/index.md) — require_approval
