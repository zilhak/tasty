# 다중 에이전트 협업 (Agent collaboration)

- **Status**: Implemented
- **주체**: AI Agent (여럿이 한 인스턴스 공유)
- **ADR**: [에이전트 작업 조율과 DAG 화면](../../adr/0042-agent-coordination-and-task-views.md)
- **코드**: `agent.*` 핸들러(`src/adapters/ipc/handler/agent.rs`), 영속 `tasty-memory`
- **화면**: 둘 다 호스트의 작업 조회 데이터를 사용한다 — [DAG 그래프 surface](screens/dag-graph-surface.md)(`tasty new tab --type dag_graph`)는 탭 하나를 점유하는 상주 관찰용, [DAG 목록 popup](screens/dag-list-popup.md)(도구 메뉴 · `KeybindingSettings.toggle_dag_list`)은 목록에서 하나를 골라 잠깐 확인하고 닫는 용도의 workspace 스코프 창이다. IPC/CLI 관측 수단(`agent.task_list`/`task_graph`/`task_get`/`dag_list`/`dag_get`)은 그대로 유효하다.
- **메서드 목록**: [reference/api](../../reference/api.md#에이전트-협업-agent)

## 목적

여러 AI 에이전트가 같은 인스턴스에서 작업을 조율하는 6가지 기능이다. 모두 `agent`(AgentManage) 권한으로 사용한다 — `agent.*` 단일 네임스페이스 + `<verb>_<modifier>` 패턴. 영속은 `tasty.agent.*` memory 키(task/barrier/semaphore/lease 는 `workspace:<id>` scope, rate-limit 은 `global`).

## 내부 동작

### `*_await` — 메서드마다 다르다

`barrier_await` 는 blocking 이 아니라 **현재 상태 즉시 응답**(poll, `barrier_state` 의 alias) — 호출자가 terminal 상태가 아니면 반복 호출한다.

`task_await` 는 응답을 기다린다(`TaskWakerHub` 기반, 워커 스레드에서 처리). `timeout_ms` 생략 시 기본 10분(600,000ms, 잠정값 — 실사용 경험이 쌓이면 재조정)까지 대기하고, 그 안에 terminal 에 도달하지 못하면 `{"outcome":"timed_out"}` 으로 반환한다. `timeout_ms: 0` 을 명시하면 이 기본값을 우회해 무한 대기한다. **local caller 전용**(`local_only`) — `approval.await` 와 대칭으로, plugin SDK 는 단일 워커 스레드가 요청을 직렬 처리하므로 plugin 이 `task_await` 로 블록되면 자기 자신의 다른 host→plugin 요청을 전혀 처리하지 못한다(자기 자신이 호출한 응답 수신은 별도 경로라 자기-교착까지는 아니지만, 그 task 가 자신을 다시 호출하는 구성이면 dispatch 타임아웃(5s)으로 정상 작업이 실패한다). plugin 은 대신 완료 판정 전략(`[[contributes.completion_strategy]]`)을 선언해 러너가 대신 기다리게 하거나, `task_get` 을 폴링한다.

<a id="6-primitive"></a>

### 6가지 협업 기능

#### Task DAG

Task DAG는 작업의 의존 관계와 상태를 관리한다. 생성 시 의존성 사이클을 거절하며 상태가 바뀌면 후속 작업을 다시 평가한다.

- 상태 8종: `waiting`, `ready`, `running`, `succeeded`, `failed`, `cancelled`, `skipped`, `unknown`
- command 4종: `run`, `custom`(선택적 `poll`), `reduce`, `wait_barrier`
- `run`은 Surface 없는 자식 프로세스다. stdout·stderr 각각 마지막 64KiB를 보관한다. 성공하면 `TaskResult.output`의 `{"pid","stdout":{"text","truncated","dropped_bytes"},"stderr":{...}}`로, 실패하면 오류 메시지로 반환한다. TTY는 지원하지 않으며 필요하면 `pty.*`를 사용한다.
- `custom`은 폴링이나 외부 훅으로 완료를 판단한다. 예를 들어 `host/command-completed`는 OSC 133 셸 통합의 완료 훅과 종료 코드를 사용한다.

실패 정책은 종류에 따라 설정할 작업이 다르다.

| OnFailure | 설정 위치와 동작 |
|---|---|
| `abort`(기본) | 후속 작업에 설정. 의존 작업이 실패하면 후속 작업을 skipped로 처리 |
| `continue_downstream` | 후속 작업에 설정. 의존 작업이 실패해도 진행 가능 여부를 평가 |
| `fallback{task}` 또는 `fallback{inline}` | 실패할 수 있는 선행 작업에 설정. 그 작업이 Failed가 되면 대체 작업 실행 |

fallback을 후속 작업에 설정해도 의존성 실패로 인한 Skipped에는 적용되지 않는다. 이 경우 해당 작업이 경고 없이 waiting에 남을 수 있다.

기존 작업을 지정한 `fallback{task}`는 원래 작업이 Failed가 되기 전까지 실행하지 않는다. 자신의 `depends_on`이 비어도 바로 ready가 되지 않는다. 원래 작업이 Succeeded·Cancelled·Skipped로 끝나면 대체 작업도 skipped로 마감한다. `fallback{inline}`은 실패 시 작업 자체를 생성하므로 이 대기 상태를 거치지 않는다.

기존 fallback 작업은 원래 작업보다 먼저 만들어야 한다. 두 생성 호출 사이에 러너가 대체 작업을 먼저 실행하지 않도록 `reserved_for_fallback: true`(CLI `--reserved-for-fallback`)로 예약한다. 그러면 의존 작업이 끝나도 waiting을 유지하고, 이를 참조하는 원래 작업의 생성 호출이 예약을 해제한다. 참조 작업을 끝내 만들지 않으면 계속 waiting에 남으므로 `task_delete`로 정리하고 다시 만든다. 예약 없이 생성하면 이 경합을 막지 못한다.

자세한 완료 전략과 출력 캡처는 [작업 러너](../../dev-guide/agent-runner.md#완료-판정-전략-레지스트리-srccompletion_strategy)와 [run 출력](../../dev-guide/agent-runner.md#run-출력-캡처)을 따른다.

#### 참조 검증과 데이터 전달

`task_create`는 `depends_on`, `OnFailure::Fallback{task}`, `TaskCommand::Reduce.inputs`가 가리키는 ID의 존재를 검사하고 없으면 `-32602`로 거절한다(`crates/tasty-agent/src/task/store.rs`). 실패할 때 생성하는 `Fallback{inline}`은 제외한다. 이미 저장된 끊어진 참조를 자동 복구하지는 않는다. 해당 작업은 waiting에 남을 수 있고, 존재하지 않는 fallback은 실패 전이 때 경고도 남긴다.

선행 작업 결과를 입력으로 사용할 때는 `${task.<id>.output<pointer>}`를 쓴다. 실행 직전에 `result.output`에서 RFC 6901 JSON Pointer로 값을 읽어 `Custom.params`의 JSON 트리, `Run.command` 인자, `Run.cwd`에 넣는다. 포인터를 생략한 `${task.t-a.output}`은 출력 전체를 뜻한다.

문자열이 placeholder 하나뿐이면 JSON 타입을 유지해 교체한다. 숫자 결과는 숫자로 남아 `as_u64` 검사도 통과한다. 다른 텍스트와 섞였을 때만 문자열로 보간한다. 대상은 `depends_on` 또는 `Reduce.inputs`에 선언돼 있어야 하며 미선언·잘못된 문법은 생성 시 `-32602`로 거절한다. 작업·결과·포인터를 찾지 못하면 null로 대신하지 않고 실패를 알린다([결과를 파라미터로 전달](../../dev-guide/agent-runner.md#선행-task-출력을-파라미터로-넘기기--taskidoutputpointer)).

`Reduce.inputs`도 그래프 엣지와 사이클 검사에 포함한다. 다만 reducer는 실패 결과도 모으므로 입력 하나가 실패했다고 skipped로 처리하지 않는다. 입력이 모두 종결될 때까지 waiting을 유지한 뒤 ready가 된다.

#### 작업·DAG 조회

`task_get`의 CLI 출력에는 command 종류, `depends_on`, `on_failure`, `metadata`가 포함된다. `task_graph`의 노드는 `command_kind`·`on_failure_kind`를, 엣지는 `depends_on`·`fallback`·`reduce` 종류를 제공한다. dot에서는 각각 실선·주황 점선·파랑 점선으로 표시한다.

**fallback 참조는 사이클 검사 대상이 아니다.** `detect_cycles()`·`TaskGraph::dfs_cycle`은 `depends_on`과 `Reduce.inputs`만 순회한다. A와 F가 서로를 fallback으로 참조해도 존재 검사만 통과하면 저장된다. 그래프에는 보이지만 `-32602`로 차단되지 않는다.

DAG는 별도 영속 레코드가 아니라 workspace의 task에서 도출한다.

1. `task.metadata.dag`가 문자열이면 같은 값끼리 explicit 그룹으로 묶는다. 연결 여부와 무관하다.
2. 나머지는 `depends_on`·`Fallback.task`·`Reduce.inputs`·`metadata.fallback_of` 역참조를 무방향으로 본 약연결 컴포넌트로 묶는다(derived).

`metadata.dag_name`을 표시 이름으로 쓰고 없으면 explicit 키, root task 이름 순서로 선택한다. metadata와 관계로 도출하므로 새 DAG 필드를 추가하는 마이그레이션 없이 기존 작업도 포함한다.

ID는 explicit의 `d:<metadata.dag 값>` 또는 derived의 `c:<root task id>`다. root는 그룹에서 `(created_at, id)`가 가장 작은 작업이다. 같은 작업 집합은 같은 ID를 가지며 전체 식별에는 `(workspace_id, id)`를 쓴다.

`dag_list`의 각 항목에는 `id`, `workspace_id`, `name`, `source`(`explicit` 또는 `derived`), `task_count`, 8종 `state_counts`, `rollup_state`, `created_at`, `updated_at`, `root_task_ids`, `has_cycle`이 있다. `include_tasks:true`일 때만 `task_ids`를 포함한다. rollup 판단 순서는 running → failed → 전부 종결(succeeded/skipped) → ready → waiting이다.

workspace_id를 생략하면 살아 있는 모든 workspace를 조회하고 `scope: "live_workspaces"`로 범위를 알린다. 삭제된 workspace의 고아 작업은 제외하며 부팅 GC가 정리한다. `dag_get`은 선택한 DAG의 부분집합으로 `task_graph`와 같은 nodes·edges 또는 dot을 만든다.

#### 작업 삭제

`task_delete`는 다른 작업이 `depends_on`·`Fallback.task`·`Reduce.inputs`로 참조하면 `-32010`과 `error.data.referenced_by` 목록을 반환한다. `cascade:true`는 이를 참조하는 작업들을 재귀적으로 함께 지우고, `force:true`는 참조 검사만 생략한다. force 사용 뒤 끊어진 참조는 호출자가 처리해야 한다.

running 작업은 옵션과 관계없이 삭제하지 않는다(`-32011`). 먼저 취소해야 한다. waiting·ready·종결 상태는 참조 조건을 충족하면 삭제할 수 있다. 종결 작업만 허용하면 방치된 waiting reducer와 그 입력을 정리할 수 없기 때문이다.

`task_purge`는 `states`와 `older_than_ms`로 후보를 고른다. 후보 밖의 작업이 참조하는 대상은 `retained`로 남기며 `dry_run:true`는 삭제 계획만 반환한다. 실제 삭제 시 `tasty.agent.handle.<id>`·`tasty.agent.run_result.<id>`도 정리한다.

#### Barrier

N회 signal을 받으면 닫힌다. `timeout_ms`가 지났는지는 signal·state·list 호출 때 확인해 `timed_out`으로 바꾸며 별도 스레드를 만들지 않는다.

#### Semaphore

N개 permit으로 동시 점유를 제한한다. 같은 holder의 재획득은 중복 점유를 만들지 않고 `acquired_at`·`expires_at`을 갱신한다. permit은 holder가 반납하거나 명시한 `--ttl-ms`가 만료될 때 회수한다. 기본은 만료 없음이다. 정상 작업 도중 permit을 회수하면 다른 작업이 같은 자원에 들어갈 수 있기 때문이다.

`semaphore-set-permits`로 한도를 줄여도 기존 holder는 강제로 회수하지 않는다. 반납될 때까지 새 획득을 거절한다([ADR-0042](../../adr/0042-agent-coordination-and-task-views.md)).

`task.metadata.semaphore = { name }`인 작업은 permit 수만큼 Running이 되고 나머지는 Ready로 기다린다. `task-create --concurrency-limit <name>`으로 지정할 수 있다([동시성 제한](../../dev-guide/agent-runner.md#동시성-제한-concurrency-limit)).

#### Lease

TTL이 있는 협조적 자원 점유 표시이며 OS 잠금은 아니다. 충돌 시 mode `fail`은 `-32009`, `block`은 `acquired:false`를 반환한다. 만료된 항목은 list·acquire 때 정리한다.

풀 모드는 `task.metadata.lease.candidates`에서 하나를 배정한다. 실제 자원은 `${lease.resource}`를 치환해 Run.cwd·Custom.params에 전달한다. 기본 fixed 모드는 후보 안에서만 선택한다. 명시적으로 elastic을 켜면 소진 시 `overflow_prefix+N` 후보를 원자적으로 만들며, `fail` 모드에서 풀을 사용할 수 없으면 `-32012`를 반환한다([자원 풀](../../dev-guide/agent-runner.md#자원-풀-배정-lease-pool--candidateselastic)).

#### Reducer

여러 작업의 결과를 `first_success`·`all`·`merge_json`·`concat_text`·`custom`의 5가지 방식으로 합친다. custom은 호스트 셸에 결과 배열 JSON을 stdin으로 보낸다. 단발 `agent.task_reduce` 또는 DAG의 `TaskCommand::Reduce`로 사용하며 후자의 inputs는 의존성에도 포함한다.

#### Rate-limit

`(agent, metric)`별 token bucket이며 보충률은 `limit/per_ms`, 상한은 `burst`다. global scope에 저장한다. 누적 비용을 제한하는 [telemetry cap](../telemetry/index.md)과 달리 단위 시간당 사용량을 제한한다. CRUD·`try_consume`을 제공하며 IPC의 `should_rate_limit`이 호출마다 확인한다.

## 자식 에이전트를 DAG 노드로

Claude / Codex 자식을 **띄우고 그 완료를 기다리는 일**을 Task DAG 노드 하나로 표현할 수 있다. 전용 command kind 는 없다 — `custom` 노드로 plugin 의 spawn 메서드를 호출하면 된다. 코어는 어떤 에이전트인지 모른 채 IPC dispatch → 폴링만 한다.

```sh
tasty agent task-create --workspace-id 1 --name spawn-worker \
  --command '{"kind":"custom","ipc_method":"claude.spawn",
              "params":{"surface_id":560,"workspace":"wt-11",
                        "cwd":"/home/me/work/wt-11","prompt":"이 디렉토리의 테스트를 고쳐라"}}'
```

**`poll` 을 주지 않는 것이 요점이다.** 두 plugin 이 자기 매니페스트에 `[[contributes.completion_strategy]]` 를 선언하면서 `default_for_methods` 로 자기 `spawn`/`tell` 메서드를 지목해 두었으므로, 러너가 dispatch 시점에 그 전략을 자동으로 집어 폴링 모드로 전이한다. 전략이 없으면(= 해당 plugin 이 비활성이면) `custom` 노드는 dispatch 성공 즉시 `Succeeded` 가 되므로, 자식이 도는 동안 기다려주지 않는다.

`params.surface_id` 는 자식을 매달 **부모** surface 다. spawn 응답의 `child_surface_id` 가 `map_from_response` 를 타고 poll 호출의 파라미터로 옮겨간다.

두 plugin 의 전략 값이 다르므로 그대로 옮겨 쓰면 안 된다:

| | claude | codex |
|---|---|---|
| dispatch 메서드 | `claude.spawn` · `claude.tell` | `codex.spawn` · `codex.tell` |
| poll 메서드 | `claude.state` | `codex.state` |
| `map_from_response` (spawn) | `child_surface_id` → `surface_id` | `child_surface_id` → `surface` |
| `map_from_response` (tell) | `surface_id` → `surface_id` | `surface_id` → `surface` |
| `terminal_states`(성공) | `idle`, `needs_input` | `idle` |
| `failure_states`(실패) | `exited` | `exited` |

poll 파라미터 키가 다른 것은 각 plugin 의 state 핸들러가 요구하는 이름이 다르기 때문이고(`surface_id` vs `surface`), codex 의 목록에 `needs_input` 이 없는 것은 관측이 안 돼서가 아니다 — codex `PermissionRequest` hook 이 그 상태를 실제로 세운다([plugins/codex](../../plugins/codex/index.md)). 두 plugin 이 갈리는 것은 **그 상태를 노드 종결로 볼지**다: claude 는 되묻기를 종결로 보고(사람이 답할 때까지 기다리는 것이 그 노드가 할 수 있는 전부), codex 는 도구 실행 승인이라 답이 오면 같은 턴이 이어지므로 종결로 보지 않는다. 지금은 그 판단을 바꾸지 않는다 — 바꾸면 `codex.spawn` 노드의 의미가 달라진다. spawn 과 tell 은 소스 키도 다르다: spawn 응답은 `child_surface_id`(새로 만든 자식), tell 응답은 `surface_id`(이미 존재하는 대상)를 싣는다.

### 이어서 지시 주기 (`tell`)

`tell` 에도 기본 전략(`tell-wait`)이 있어 **자식이 그 지시를 마칠 때까지** 노드가 `running` 에 머문다. spawn 노드와 달리 `surface_id` 는 **자식** surface 다:

```sh
tasty agent task-create --workspace-id 1 --name tell-worker \
  --command '{"kind":"custom","ipc_method":"claude.tell",
              "params":{"surface_id":561,"message":"방금 고친 테스트를 다시 돌려라"}}'
```

호스트 `terminal.tell` 은 본문 주입에 성공한 직후 대상 자식의 `idle`(과 `needs_input`) 플래그를 내린다 — 그러지 않으면 첫 폴링 tick 이 직전 턴의 상태를 읽고 자식이 답을 시작하기도 전에 노드를 완료 처리한다. `terminal.broadcast` 도 같다.

### 실패 판정

자식이 종료되면(`exited` — 프로세스 종료 또는 surface 소멸) 그 노드는 **`failed`** 다. 두 plugin 모두 `exited` 를 `failure_states` 로 분류하므로, 자식 에이전트 노드에서도 `on_failure`(`abort` / `continue_downstream` / `fallback`)가 그대로 동작한다 — 기본값 `abort` 면 downstream 이 `skipped` 로 cascade 된다. 실패 메시지에는 상태값과 poll 응답 요약이 함께 실린다.

claude 의 `needs_input`(사람 승인 대기)은 **성공** 쪽에 남는다. 사람이 승인해주면 이어서 끝나는 상태라 영구 실패가 아니고, 두 plugin 의 spawn/tell 완료 알림이 이미 `idle` 과 동일 취급하는 계약을 깨지 않기 위함이다. 승인 대기를 실패로 보고 싶으면 그 노드에 인라인 `poll` 을 주어 `failure_states` 를 직접 지정한다.

앞 노드의 산출물을 뒤 노드 파라미터로 넘기는 것(예: spawn 이 만든 자식의 surface id 를 뒤따르는 tell 노드에 넘기기)은 위 "노드 간 데이터 흐름(`${task.<id>.output<pointer>}`)" 이 다룬다.

전략 레지스트리·dispatch 배선 상세는 [dev-guide/agent-runner](../../dev-guide/agent-runner.md#hostexecutor-매핑).

### 사건으로 받기 — 폴링하지 않고

종결 사실은 Event Bus 로도 나간다. plugin 이 `agent.task_finished` · `agent.barrier_closed` 를 구독하면 `task_get` 을 되풀이해 묻지 않아도 된다. 두 키의 payload·등급·구독 조건은 [reference/event-catalog](../../reference/event-catalog.md#agent-scopesystem-experimental) 이 정본이고, 무엇을 싣고 무엇을 안 싣는지의 근거는 [ADR-0033](../../adr/0033-event-feed-delivery.md) 이다.

경계 셋만 여기 적는다.

- **종결만 나간다.** `waiting`/`ready`/`running` 으로 들어가는 전이는 이벤트를 보내지 않는다 — 종결에는 모든 진입 경로가 지나는 공통 처리 지점(`task_await` 를 깨우는 그 자리)가 있고 비종결에는 없다.
- **lease 만료는 사건이 아니다.** 만료는 전이가 아니라 조회 시점에 확인하는 조건라 "언제 일어났다" 가 없다.
- **왜 실패했는지는 payload 에 없다.** 실패 원인은 `task_id`로 `task_get` 을 부른다 — 구독 권한과 호출 권한이 다른 것이 그 이유다.

발화는 `task_await` 의 blocking 동작과 간섭하지 않는다. 같은 처리가 대기자에게 알리고 피드에도 기록하며, **대기자가 없어도 피드에는 적힌다.**

plugin 이 아닌 쪽은 `events.fetch` 로 같은 사건을 **위치로** 읽는다. 이 메서드는 로컬 호출자만 사용할 수 있다 — 세션 토큰을 들고 붙은 외부 에이전트는 이 메서드를 직접 못 부르고, 로컬 소켓의 `tasty` CLI 를 경유한다. `tasty events follow --filter 'agent.*'` 가 그 루프이고, 한 줄에 한 사건씩 JSON 으로 찍으므로 셸에서 `while read` 로 받는다. 끊겼다 다시 붙을 때는 마지막 위치를 그대로 주면 그 사이 사건부터 이어 받고, 기다린 사이 위치가 링 밖으로 밀렸으면 설명 없이 처음부터 반환하지 않고 건너뛴 수를 알린다. 재시작을 넘는 재부착은 세대를 함께 준다 — 연결이 끊기면 `follow` 가 다시 붙을 `--offset` · `--epoch` 을 stderr 에 찍고 끝나고(`--reconnect` 면 1 초마다 다시 붙는다), 그 세대가 새 세대와 다르거나 위치가 새 피드의 끝보다 뒤면 stderr 로 알리고 새 세대의 처음부터 잇는다([ADR-0033](../../adr/0033-event-feed-delivery.md)). 커서를 소비자가 드는 이유와 재시작이 위치를 리셋하는 이유는 [ADR-0033](../../adr/0033-event-feed-delivery.md).

## 인터페이스

- **AI Agent / CLI**: `tasty agent {task-create,task-list,...,dag-list,dag-get,barrier-*,semaphore-*,lease-*,task-reduce,rate-limit-*}`. `--command`/`--metadata` 는 인라인 JSON 또는 `@path`. 전체 표 → [reference/api](../../reference/api.md#에이전트-협업-agent).
- **state 필터는 콤마 다중값** — `task-list --state`(단수) 와 `task-purge --states`(복수)는 플래그 이름만 다를 뿐 같은 파싱을 쓴다: `--state waiting,ready,running` 처럼 여러 state 를 OR 로 매칭하고, 단일값도 그대로 동작한다. "아직 안 끝난 task 가 남았는가" 를 한 번의 조회로 판정하는 완료 감지의 기본 패턴. 예시 → [dev-guide/agent-runner §CLI 예](../../dev-guide/agent-runner.md#cli-예).

## 재시작 동작

task 는 영속되지만(`Scope::Workspace`) runner thread 는 in-memory 다 — 호스트 재시작 후 **runner 는 자동으로 켜지지 않는다.** 대신:

- **부팅 시 상태 정리 1회** — 라이브 workspace 전부에 대해 stale semaphore/lease holder 회수 + 직전 `Running` task 를 `Failed("host restart")` 로 마감 + persisted `DispatchHandle` reload(살아있는 `ShellProcess`/`PolledDispatch`/`BarrierPoll`/미만료 `AwaitExternal` 는 복원, 죽은 건 마감). runner thread 는 여전히 안 켜져 있으므로 복원된 handle 을 실제로 poll 하려면 수동(또는 plugin) `agent.task_run --action start` 가 필요하다.
- **같은 부팅 정리 경로에서 자동 GC 도 함께 돈다** — 상태 무관 + 잠정 임계값(7일) 이상 방치된 task 를 `task_purge` 와 동일한 참조 안전 로직(`plan_sweep`/`apply_sweep_plan`)으로 쓸어낸다. memory 자체 TTL(`PutOpts.expires_at`)은 쓰지 않는다 — TTL 만료는 참조 무결성·상태 검사를 우회해 dangling 참조를 재도입하기 때문. 상세: [dev-guide/agent-runner](../../dev-guide/agent-runner.md#자동-gc).
- **정지 상태는 조회로 드러난다** — `task_list`/`task_graph` 응답에 `runner: { running, crashed, ready_count, running_count, store_error, list_failures }` 가 동반된다. runner 가 꺼져 있어도 `ready_count`/`running_count` 는 store 의 실제 값이라, "할 일은 있는데 아무도 안 돌리고 있다"가 이 응답만으로 드러난다. store 를 못 읽으면 두 카운트는 `null` + `store_error` 이고(0 으로 흡수하지 않는다), 러너가 살아 있는데 계속 못 읽는 상태는 `list_failures` 로 드러난다. `task_get` 은 `AwaitExternal` 로 외부 신호를 기다리는 task 에 `awaiting_external: { wait_key, deadline_ms }` 를 실어 "그냥 running" 과 구분한다.
- **`hook_task_waits`(push 완료 전략의 hook_id → task_id 매핑)는 비영속** — 재시작하면 그 task 는 훅으로는 깨어날 수 없다. 대신 `AwaitExternal` handle 자체가 `deadline_ms` 를 들고 다니므로(핸들은 영속), 다음 재시작의 reload 가 만료 여부를 독자적으로 판정해 마감한다.
- `agent.task_run` 은 plugin 도 호출 가능(`AgentManage`) — 자동 시작이 없으므로 plugin 이 자기 workspace 의 runner를 직접 시작할 수 있어야 하기 때문이다.

상세: [dev-guide/agent-runner](../../dev-guide/agent-runner.md#재시작-계약).

## 에러 코드

`-32004`(not found) · `-32008`(already terminal) · `-32009`(lease conflict) · `-32010`(task 참조 중 — `task_delete` 기본 거부, `error.data.referenced_by` 에 참조자 목록) · `-32011`(task 가 `running` — 삭제 불가, `cancel` 선행 필요) · `-32012`(lease pool 소진 — fixed 전부 점유 중이거나 elastic `max_candidates` 상한 도달, mode `fail`) · `-32602`(사이클/미존재 dep(`depends_on`/`Fallback.task`/`Reduce.inputs`)/잘못된 strategy/`depends_on` 밖을 가리키거나 문법이 깨진 `${task.<id>.output…}` 참조 등) · `-32603`(internal).

**이름·id 의 문자 규칙.** semaphore·barrier 의 `name`, task 의 `id`, rate-limit 의 `id` 는 그대로 memory 키의 한 조각이 되므로 memory 키 규칙([design/systems/memory](../../design/systems/memory.md) — 소문자 `a-z`·`0-9`·`.`·`_`·`-`, 접두사 포함 256 바이트)을 따른다. 어기면 `-32602` 이고, 메시지는 **호출자가 준 값 기준** 문자 좌표(0 부터)와 그 문자로 무엇이 틀렸는지 말한다(`semaphore name "v6S": invalid char at 2: 'S' (allowed: …; at most 234 bytes)` — 다바이트 문자도 입력한 그대로, `aé` 면 `invalid char at 1: 'é'`) — 상한 바이트 수는 종류마다 접두사 길이만큼 다르다. 판정은 생성뿐 아니라 그 값으로 키를 만드는 모든 호출(acquire·release·delete·get 등)에서 같다. lease 의 `resource` 는 키에 넣기 전에 인코딩하므로 이 규칙이 없다. 인코딩 대신 판정을 고른 근거는 [ADR-0042](../../adr/0042-agent-coordination-and-task-views.md).

## 관련

- [telemetry](../telemetry/index.md) — rate-limit vs cap 구분 · [human-handoff](../human-handoff/index.md) — approval
- [design/systems/memory](../../design/systems/memory.md) — 영속 backing store
- [dev-guide/agent-runner](../../dev-guide/agent-runner.md) — task runner 내부 동작(dispatch/poll, 완료 판정 전략 레지스트리)
- [ADR-0042](../../adr/0042-agent-coordination-and-task-views.md) — task graph를 화면 두 곳에서 제공하고 host 내장 기능으로 구현한 이유
