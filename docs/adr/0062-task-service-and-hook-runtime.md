# ADR-0062: 작업 실행은 TaskService가, 훅 감시와 실행은 HookRuntime이 소유한다

- **Status**: Accepted — 구현 상태: 단계적 이행 중. TaskService는 root `src/core/task_service.rs`에 있고 `Core.tasks`로 조립되며 러너 등록부(RunnerRegistry)와 훅-작업 대기(HookTaskWaits)를 소유한다. engine별 작업 순번·완료 대기 허브·사건 큐는 TaskScope로 묶였고, task IPC 핸들러와 App 호출은 TaskService API(`runner_*`·`dispatch_handle`·`awaiter` 등)만 거친다. `task-await`는 요청 workspace를 가진 engine의 TaskScope에서 기다린다. 훅 실행은 root `hook_runtime` 모듈과 HookRuntimeState로 모였다. TaskScope와 HookRuntimeState는 EngineSession이 직접 소유하며, 실행 소비자는 CoreState와 별도로 대여한다. 이 소유 이전만으로 구조 명령의 effect 분리나 CoreState 전체의 무자원 replay가 완성되는 것은 아니다. `task-await`의 소유 engine은 App이 창·parked engine을 차례로 확인해 찾는다. TaskService의 `tasty-task-runtime` 추출과 App이 port로 두 서비스를 연결하는 구조도 아직 없다
- **Date**: 2026-09-30
- **Tags**: agents, tasks, hooks, ownership, architecture
- **Group**: agents

## Context

작업 조율의 알고리즘은 이미 `tasty-agent`에 있다. 작업 상태 전이·의존성·협업 primitive·RunnerLoop를 제공하며, DAG는
`crates/tasty-agent/src/task/dag.rs`가 작업 집합에서 계산하는 조회 결과다. 그러나 실행 소유는 흩어져 있다.
`src/core/mod.rs`의 `Core`가 RunnerRegistry와 훅 작업 대기를, `src/core/state.rs`의 `CoreState`가 TaskWakerHub·AgentEventQueue를 소유하고,
IPC 핸들러가 registry를 직접 시작·정지하며, App이 대기 허브 선택과 훅 완료 전달을 맡는다.
`src/app/ipc/app_methods.rs`의 task 대기는 요청 workspace가 아니라 첫 main View나 첫 parked 엔진의 허브를 고르는 경로가 있다.

훅도 비슷하다. `CoreState`가 HookManager·GlobalHookManager를 갖고, `src/core/state/idle_hooks.rs`와 `src/app/idle_hooks.rs`가 조건 검사와 실행 전달을 나누며,
`src/core/state/global_hooks.rs`는 조건 판정 뒤 셸 실행까지 요청한다. 실행 worker는 `src/hook_handler/exec.rs`의 전역 worker다.

[ADR-0054](0054-app-core-view-layers-and-state-ownership.md)에서 `CoreState`는 재생 가능한 구조 모델이어야 하므로 러너·대기자·훅 감시 같은 실행 상태를 가질 수 없다.
작업의 신원과 대기자 소유가 View 탐색 방식에 의존하는 현재 배치도 이 구조와 맞지 않는다.

## Decision

### TaskService

- App이 조립하는 독립 서비스로 둔다. 작업 식별자로 등록·조회·취소·재시도·실행·대기·재시작 정리를 제공한다.
  IPC 핸들러는 인자와 권한 문맥을 넘기고 응답만 변환한다. Core 전체나 View 전체를 서비스 입력으로 받지 않는다.
- 작업 정의·의존성·개별 작업 상태·결과의 원본은 기존 작업 저장소와 모델이다. 서비스 상태(러너 실행·완료 대기·훅-작업 연결·실행 관측)는
  작업 원본의 두 번째 사본을 만들지 않는다. 기존 RunnerRegistry 등은 내부 구현으로 흡수하고 같은 러너를 새 registry에 중복 등록하지 않는다.
- 대기자는 작업이 속한 엔진·workspace 기준으로 찾는다. View 목록을 뒤져 소속을 찾지 않는다.
  조회·구독 등록·재확인의 순서를 하나의 대기 계약으로 제공해 등록 전에 지나간 완료를 놓치지 않게 한다.
- 러너 정지·작업 취소·OS 자식 종료는 서로 다른 계약으로 유지한다. `Run`은 bare subprocess이며 Terminal·Pty를 거치도록 바꾸지 않는다.
- 실행 수명은 DAG 화면의 열림과 무관하다. 엔진·workspace 종료와 서비스 정리를 명시적으로 연결한다.
  View 복원만으로 다른 작업을 다시 실행하지 않는다.

### HookRuntime

- 등록된 훅·대상 범위·idle/interval/once 감시·발화 여부·실행 대기를 HookRuntime과 그 상태로 묶는다.
  조건 판정은 `tasty-hooks`를 재사용하고, shell·IPC 실행 worker와 OS 핸들은 모듈의 private 자원이다.
- surface hook과 global hook의 엔진별 범위를 유지한다. 공유되는 handler 정의 registry와 엔진별 등록·감시 상태를 구분하며,
  이름에 global이 있다는 이유로 모든 훅을 프로세스 전역 원본으로 합치지 않는다.
- 기존 실행 규칙은 유지한다([ADR-0027](0027-lua-and-hook-execution.md)). `HookFired`는 입력 사건으로 훅이 발화했다는 뜻이며 handler의 셸 작업이 끝났다는 뜻이 아니다.

### 두 서비스의 관계와 배치

- HookRuntime은 완료 관측을 TaskService에 전달할 뿐 작업 저장소를 직접 수정하지 않고 DAG를 실행하지 않는다.
  둘은 서로 의존하지 않으며 App이 port로 연결한다. 작업과 훅을 모두 포괄하는 AutomationManager는 만들지 않는다.
- Webhook의 HTTP 수신·인증은 기존 webhook 모듈에 남는다. handler 실행 기능을 내부 port로 공유하는 정도로 연결한다.
- 순수 구조 replay에서 러너와 훅을 발화시키지 않는다. 작업·훅 상태는 구조 이벤트 저널의 범위가 아니며 원자성이나 완전한 재생을 주장하지 않는다
  ([ADR-0055](0055-structural-domain-event-sourcing.md)). 구조 변경을 요청하는 작업은 CommandExecutor 경계로 들어간다.
- TaskService는 `tasty-task-runtime`으로, HookRuntime은 우선 root 내부 모듈로 둔다([ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)).
- 추가하지 않는 것: DagManager·DagState(작업 모델과 중복), 범용 Scheduler(기존 TimerHub와 workspace별 RunnerLoop 재사용), 설정·승인·memory를 모으는 범용 StateManager.

## Consequences

작업 대기와 완료 전달이 View 탐색 방식과 무관해지고, 다른 엔진의 허브에서 대기하는 종류의 결함이 구조적으로 생기기 어렵다.
작업 실행 테스트를 GUI 없이 package 단위로 돌릴 수 있다. 훅 감시와 실행 수명이 한 모듈에 모여 등록·해제·발화를 한 곳에서 확인할 수 있다.

엔진 간 이동과 저널 import에서 작업 소속 식별자와 기존 작업 기록의 매핑을 정해야 한다. 훅 이벤트의 엔진별 범위를 유지하면서
전역 handler 정의를 공유해야 하므로 두 수준을 섞지 않도록 API를 나눠야 한다.
이 결정은 작업 실행을 이벤트 소싱으로 복구한다는 보장을 주지 않는다. 그런 보장이 필요하면 실행 attempt·결과 관측·재시도 계약을 따로 설계한다.

## Alternatives Considered

- 러너·대기자를 `Core`와 `CoreState`에 계속 두는 안: 재생 모델이 실행 자원을 갖고 작업 소속이 View 탐색에 의존한다.
- 작업과 훅을 하나의 자동화 관리자로 합치는 안: 수명·실패 의미·입력이 달라 한쪽 변경이 다른 쪽 계약을 흔든다.
- DAG를 별도 영속 엔티티로 두는 안: 작업 저장소와 원본이 둘이 된다.
- 기존 `tasty-agent`에 호스트 실행을 넣는 안: 협업 API의 작은 소비자에 plugin·IPC 실행 의존이 얹힌다.
- Webhook 수신을 HookRuntime에 합치는 안: HTTP listener·인증·수명은 이미 별도 모듈로 나뉘어 있고 합칠 이유가 없다.
- 파일 열기용 FileOpenService를 함께 만드는 안: 요청 수명·취소를 실제로 맡길 때까지 보류한다. 현재는 file 모듈이 결과를 반환하고 picker를 View로 옮기는 것으로 충분하다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 작업 실행을 재시작 후 이어서 복구해야 하는 요구가 생기면 실행 기록 계약을 설계하고 이벤트 소싱 범위를 다시 정한다.
- HookRuntime의 공개 API가 안정되고 root 밖 소비자가 생기면 별도 crate 추출을 검토한다.
- 파일 열기 요청의 비동기 수명과 취소를 서비스가 맡아야 하면 FileOpenService 신설을 검토한다.

### 실행 결과로 확인

- 여러 엔진에서 `task-await` 대기가 다른 엔진의 완료를 놓치거나 오인하는 사례가 재현되면 대기 계약과 소속 식별을 다시 본다.
- 훅 실행 대기열이 차서 실행하지 못하는 사례가 반복되면 worker 수와 큐 예산을 측정해 조정한다.

## References

- [ADR-0027](0027-lua-and-hook-execution.md) · [ADR-0041](0041-agent-state-and-completion.md) · [ADR-0042](0042-agent-coordination-and-task-views.md) · [ADR-0032](0032-webhook-admission.md)
- [ADR-0054](0054-app-core-view-layers-and-state-ownership.md) · [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)
- [작업 러너](../dev-guide/agent-runner.md), [훅](../features/hooks/index.md)
- 현재 구현: 작업 실행은 `src/core/task_service.rs`(TaskService·TaskScope·TaskAwaiter), `src/core/agent/task.rs`(TaskService 작업 API), `src/core/agent/runner_host.rs`, `src/core/agent/runner_thread.rs`, `src/core/agent/task_waker.rs`, `src/core/agent/hook_wait.rs`, `src/adapters/ipc/handler/agent/task.rs`(task IPC), `src/app/ipc/app_methods/task_await.rs`(대기 engine 선택). 훅은 `src/hook_runtime/mod.rs`(HookRuntimeState), `src/hook_runtime/worker.rs`, `src/hook_runtime/trigger.rs`, `src/hook_runtime/global.rs`, `src/core/state/global_hooks.rs`, `src/app/idle_hooks.rs`, `src/hook_handler/exec.rs`. engine별 상태를 필드로 가진 곳은 `src/runtime/engine_session.rs`이며 process TaskService는 `src/core/mod.rs`(`Core`)에서 조립한다. 실행 대여 타입은 `src/core/engine_access.rs`다.
