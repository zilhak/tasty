# 에이전트 텔레메트리 (Telemetry)

- **Status**: Implemented

비용·관측·이상 탐지의 기반이 되는 메트릭 수집 계층. `tasty-telemetry` 크레이트가 도메인 로직(이벤트 모델 / 키 컨벤션 / 순수 집계 / Cost Cap 타입)을, 호스트가 `tasty-memory` 영속화와 IPC/CLI 어댑터를 담당한다. 단계 4.1-4.2 는 raw event 기록 / 즉시 집계 / dispatcher 자동 카운트, 4.3a 는 Cost Cap CRUD, 4.3b 는 record 후 inline cap 평가 + `Notify` 액션 발화. 잔여 액션(Stop/Pause/RequireApproval)·이상 탐지·자동 요약은 후속 sub-phase 에서 켜진다.

### 핵심 컴포넌트
- `tasty-telemetry`: `TelemetryEvent` (agent / workspace_id / metric / value / op / ts / tags) + `MetricBucket` (1m/1h/1d 윈도우) + `Op::{Set,Inc,Dec}` + `summarize_events`/`aggregate_into_buckets`/`top_n` 같은 pure aggregation. `validate_metric` (`[a-z][a-z0-9_]*`, 1..=64) / `validate_agent_id` (`[a-zA-Z0-9_-]+`, 1..=64) 으로 키 안전성을 보장
- `AgentId`: 단계 4.0의 잠정 식별 모델. Plugin caller → manifest `plugin_id`, Local caller → env `TASTY_AGENT_ID` (없으면 `_host`). Phase 6의 session token 인증 도입 시 verifiable 로 승격됨 (자세한 한계: `docs/dev-guide/agent-identification.md`)
- 키 컨벤션: `tasty.telemetry.event.{ts:013}.{seq:04}` (이벤트), `tasty.telemetry.bucket.{w}.{m}.{a}.{ws:013}` (롤업 버킷, 4.2+에서 사용). 같은 ms 안의 충돌을 막기 위해 `TelemetrySeq` AtomicU64 가 host singleton 으로 단조 시퀀스 발급

### Dispatcher 자동 카운트 (Phase 4.2)
- `handle_with_caller` 가 권한 검사 직후 `record_ipc_call(state, caller, method)` 호출 — 비-host caller 의 모든 IPC 가 자동으로 `ipc_calls` metric 으로 적재된다 (`method` 태그로 식별자 구분)
- `_host` agent 와 `telemetry.*` 메서드 자체는 카운트 제외 (자기-측정 / 재귀 폭주 방지)
- 실패는 best-effort warn 로그 — IPC dispatch 를 막지 않음
- cap_eval 통합 (Phase 4.3) 은 같은 진입점에 후행 도입 예정

### IPC 측정 5종 (Permission `Telemetry`)
- `telemetry.record` — 단일 메트릭 이벤트 기록. `metric` (필수), `value`, `op` ∈ {set, inc, dec}, `agent` (선택, default caller agent), `workspace_id` (선택, default 활성 워크스페이스), `tags` (선택, string→string). 응답: `{ key, ts, agent, metric }`
- `telemetry.record_batch` — `events: []` 배열을 한 번에. 모든 이벤트는 동일한 `ts` 와 단조 증가 `seq` 로 저장됨
- `telemetry.summary` — (metric, agent) 별 집계 (`sum/count/min/max/last`). 필터: `metric`/`agent`/`workspace_id`/`since`/`until` (unix ms)
- `telemetry.timeseries` — 윈도우 단위 버킷 시계열. `metric` 필수, `window` ∈ {1m, 1h, 1d, default 1m}. 단계 4.1은 raw event 에서 즉시 집계 (사전 롤업 캐시는 4.2+ 도입)
- `telemetry.top` — `by` ∈ {agent, workspace} 기준 sum 내림차순 top-N (default limit 10)

### Cost Cap CRUD (Phase 4.3a, Permission `Telemetry`)
- 도메인 타입: `CapWindow ∈ {Total, Hour, Day}`, `CapAction ∈ {Stop, Pause, RequireApproval, Notify}`, `CostCap { id, agent, metric, threshold, window, action, created_at, triggered? }`, `CapTriggered { at, value }`. 모두 `tasty-telemetry` 가 순수 도메인으로 보유
- 영속: `Scope::Global` 의 `tasty.telemetry.cap.{id}` (workspace 비종속, agent 단위). cap id 는 `cap_{ts:013}{seq:04}` (`TelemetrySeq` 재사용)
- `telemetry.cap.set` — 새 cap 정의. `{ agent, metric, threshold>0, window?=total∈{total,1h,1d}, action?=notify∈{stop,pause,require_approval,notify} }` → `CostCap` (생성된 `id` 포함)
- `telemetry.cap.list` — `{ agent? }` 필터로 cap 목록 조회 → `{ entries[], count }` (created_at 오름차순)
- `telemetry.cap.remove` — `{ id }` → `{ removed: true, id }` (없으면 `-32004 not_found`)
- `telemetry.cap.status` — `{ agent? }` → `{ entries[], count }`. 각 entry 는 cap 본체 + `current_value` (윈도우 내 raw event sum) + `ratio` (current/threshold)
- `telemetry.cap.reset` — `{ id? }` 또는 `{ agent? }` (둘 중 최소 하나) → `{ reset_ids[], count }`. 매칭된 cap 들의 `triggered` 필드를 비워 액션 재발화 가능 상태로 되돌림
- **평가 (Phase 4.3b)**: `record` / `record_batch` / dispatcher 자동 카운트 직후 inline 으로 cap 평가 — agent+metric 가 일치하는 미발화 cap 들의 `current_value` 를 즉시 계산해 `threshold` 이상이면 `triggered: { at, value }` 마크 후 액션 발화. evaluate 자체는 best-effort warn 로그 — 실패해도 record 응답은 영향 없음
- **Notify 액션 (Phase 4.3b)**: 활성 워크스페이스에 알림 추가 (`title="Cap '<metric>' 임계 도달"`, body 에 agent/metric/value/threshold/window/cap_id 포함). 차단 없음
- **Stop / Pause 액션 (Phase 4.3c)**: cap 이 `triggered` 인 plugin agent 의 모든 IPC 는 dispatcher pre-check 에서 `-32007 cap_blocked` 로 거부된다. trigger 시점에 동시에 알림을 발행해 차단 사실이 사용자에게 보인다. CLI/Local caller 는 검사 대상이 아니므로 `tasty telemetry cap reset --id <ID>` 로 해제 가능. `Stop` 의 OS 프로세스 종료(claude.kill) 트리거는 별도 `claude.kill` IPC 가 도입될 때 결합 (현재 Stop 과 Pause 의 실효 동작은 동일 — 후속 IPC 거부)
- **RequireApproval 액션 (Phase 4.3d)**: cap 이 처음 triggered 되면 host 가 자동으로 `approval.request` 발행 (severity=warn, body 에 reset 명령 포함). 이후 plugin IPC 는 `Stop`/`Pause` 와 동일하게 `-32007 cap_blocked` 로 거부 — 사용자가 popup 에서 결정한 뒤 `cap.reset` 으로 재개

### 영속화 정책
- workspace_id 있는 이벤트 → `scope=workspace:<id>`, 없으면 `global`
- 조회 시 workspace_id 명시되면 단일 scope 만, 아니면 store 의 모든 scope 순회 후 필터링
- TTL 없음 (단계 4.1) — 추후 retention 정책은 cap/롤업과 함께 도입

### 이상 탐지 (Phase 4.4)
- `tasty-telemetry::AnomalyDetector` — in-memory sliding window 기반 휴리스틱. 호스트 재시작 시 윈도우 비워짐 (영속은 anomaly 레코드만)
- **CallBurst (활성)**: 동일 (agent, method) 가 1분 내 1000 회 이상 호출되면 발화. dedup 쿨다운 1분 — 같은 burst 가 매 호출마다 spam 되지 않음
- **SlowLoop / RssSurge (미활성)**: 타입만 정의됨. 추가 신호(메서드 시퀀스 패턴, agent RSS 보고)가 필요해 후속 sub-phase 에서 켜진다
- 발화 시: notification 발행 + `tasty.telemetry.anomaly.{ts:013}.{id}` 키로 Global scope 영속
- `telemetry.anomaly.list` IPC — `agent` / `kind` / `since` / `until` 필터, `detected_at` 오름차순. anomaly_rule.set/remove 는 후속 phase

### 세션 요약 (Phase 4.5)
- `telemetry.session_summary` IPC — 결정론적 순수 집계 (LLM 호출 없음)
- 파라미터: `workspace_id?` (없으면 전 workspace 합산), `since?`, `until?`, `format?∈{markdown,json}` (기본 `markdown`), `top_n?` (기본 10)
- 집계 항목:
  - **tokens**: `ipc_calls` 를 제외한 모든 metric 의 sum (k:v map)
  - **ipc_calls**: `ipc_calls_total` 과 method 별 top-N (sort desc by count, tiebreak by method name)
  - **approvals**: total / pending / responded / timed_out / cancelled + responded choice 별 count (`tasty.approval.*` 영속 레코드를 모든 scope 에서 prefix scan)
  - **anomalies**: Global scope 에서 prefix scan, since/until 윈도우 적용
- markdown 출력은 헤더+표 구조. json 은 동일한 SessionSummary 구조체를 그대로 직렬화
- 영속(`tasty.telemetry.session_summary.*`) 은 옵션 — 본 sub-phase 에선 생략

### Claude Code hook 통합 (Phase 4.6)
- `tasty-plugin-claude` 가 `claude.hook` 이벤트를 텔레메트리에 자동 적재 (manifest 에 `telemetry` 권한 추가)
- `session-start`: state 에 시작 시각 기록 (HostCall 없음)
- `stop` / `subagent-stop` / `session-end`: 시작 시각이 있으면 `wall_time_ms = now - start` 를 `telemetry.record` 로 발행 (`tags.surface_id` 포함)
- `notification --message <text>`: 텍스트에 `\btokens?:\s*(\d+)\b` (정규식 없이 수동 스캔, 워드 경계 검증) 매칭 시 매칭값으로 `input_tokens` 발행
- 측정 주체 agent 는 `tasty.com.tasty.claude`. 호스트 재시작 시 wall_time_starts 휘발 — 진행 중 세션은 누락만 발생하고 잘못된 값은 나오지 않는다
- CLI: `tasty claude hook <event> [--surface] [--session] [--message]`

### CLI
- `tasty telemetry {record,summary,timeseries,top}` — 단일 record 기록 / 집계 조회
- `tasty telemetry cap {set,list,remove,status,reset}` — Cost Cap CRUD
- `tasty telemetry anomaly list` — 검출된 이상 신호 조회 (`--agent`, `--kind`, `--since`, `--until`)
- `tasty telemetry session-summary` — 세션 요약 (`--workspace-id`, `--since`, `--until`, `--format`, `--top-n`)
