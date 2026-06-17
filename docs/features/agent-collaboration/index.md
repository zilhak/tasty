# 다중 에이전트 협업 (Agent collaboration)

- **Status**: Implemented
- **주체**: AI Agent (여럿이 한 인스턴스 공유)
- **ADR**: 없음
- **코드**: `agent.*` 핸들러(`src/adapters/ipc/handler/agent.rs`), 영속 `tasty-memory`
- **화면**: 없음 (IPC/CLI 전용)
- **메서드 목록**: [reference/api](../../reference/api.md#에이전트-협업-agent)

## 목적

여러 AI 에이전트가 같은 tasty 인스턴스를 공유할 때 쓰는 협업 primitive 6종. 모두 `agent`(AgentManage) 권한 하나로 grant 된다 — `agent.*` 단일 네임스페이스 + `<verb>_<modifier>` 패턴. 영속은 `tasty.agent.*` memory 키(task/barrier/semaphore/lease 는 `workspace:<id>` scope, rate-limit 은 `global`).

## 내부 동작

### Poll-based 모델

`*_await` 는 blocking 이 아니라 **현재 상태 즉시 응답**(poll). 호출자가 terminal 상태가 아니면 반복 호출한다 — blocking await 의 lock-up 위험을 피하기 위해. (scheduler 도입 시 long-poll 로 분기 예정.)

### 6 primitive

- **Task DAG** — 의존성 그래프 + state 머신. 사이클은 create 시 거부, 상태 변화 시 transitive downstream 자동 재평가. 상태 8종(`waiting/ready/running/succeeded/failed/cancelled/skipped/unknown`). command 4종(`claude_spawn/run/custom/reduce`). OnFailure 3종(`abort`(기본, downstream skip cascade)/`continue_downstream`/`fallback{task}`).
- **Barrier** — N회 signal 모이면 닫히는 게이트. `timeout_ms` 경과 시 `timed_out` 으로 **lazy 전이**(별도 스레드 없음 — signal/state/list 호출 시 도장).
- **Semaphore** — N permit 동시 점유. 같은 holder 재acquire 는 idempotent(retry-safe), permit 회복은 그 holder 의 release 만.
- **Lease** — 협조적(advisory) 자원 점유 마커 + TTL. OS 락 아님(위반 감지 수준). mode `fail`(충돌 시 `-32009`) / `block`(`acquired:false`). 만료는 list/acquire 시 lazy evict.
- **Reducer** — N task 결과를 단일 값으로 합성. 5전략: `first_success`/`all`/`merge_json`/`concat_text`/`custom`(호스트 shell, stdin 에 결과 배열 JSON). 단발 `agent.task_reduce` 또는 DAG 노드(`TaskCommand::Reduce`).
- **Rate-limit** — (agent, metric) token bucket(보충률 `limit/per_ms`, 상한 `burst`). `global` scope. 누적 임계인 [telemetry cap](../telemetry/index.md) 과 구분(이쪽은 *시간당 비율*). 현재 CRUD + `try_consume`(자동 미들웨어 결합은 후속).

## 인터페이스

- **AI Agent / CLI**: `tasty agent {task-create,task-list,...,barrier-*,semaphore-*,lease-*,task-reduce,rate-limit-*}`. `--command`/`--metadata` 는 인라인 JSON 또는 `@path`. 전체 표 → [reference/api](../../reference/api.md#에이전트-협업-agent).

## 에러 코드

`-32004`(not found) · `-32008`(already terminal) · `-32009`(lease conflict) · `-32602`(사이클/미존재 dep/잘못된 strategy 등) · `-32603`(internal).

## 관련

- [telemetry](../telemetry/index.md) — rate-limit vs cap 구분 · [human-handoff](../human-handoff/index.md) — approval
- [design/systems/memory](../../design/systems/memory.md) — 영속 backing store
