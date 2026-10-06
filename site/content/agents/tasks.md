<a id="작업-dag"></a>

# 작업 순서 관리 (DAG)

빌드가 끝나면 테스트를 실행하고, 결과를 다음 작업에 넘겨보세요. 할 일을 **작업(task)**으로 등록하고 실행 순서를 연결하면 Tasty가 순서와 실패 처리를 관리합니다. 이렇게 연결한 작업 그래프를 **DAG**라고 부릅니다.

작업은 `tasty` CLI로 만들고 관리합니다. 실행 상황을 화면에서 확인하려면 [진행 보기](#진행-보기)를 참고하세요.

각 명령에는 작업이 속한 워크스페이스의 `--workspace-id`를 지정합니다. 다른 워크스페이스를 보고 있어도 지정한 곳에서 작업합니다.

## 러너 켜기

작업을 만들어도 러너가 꺼져 있으면 아무것도 실행되지 않습니다. 워크스페이스마다 한 번 켭니다.

```sh
tasty agent task-run --workspace-id 2 --action start
tasty agent task-run --workspace-id 2 --action status
tasty agent task-run --workspace-id 2 --action stop
```

tasty 를 재시작하면 러너는 자동으로 켜지지 않습니다. 작업 자체는 남아 있으니 다시 `start` 하면 이어서 진행합니다. 러너가 멈춰 있으면 DAG 화면의 러너 표시에도 그 사실이 나오고, 표시 위에 마우스를 올리면 다시 켜는 명령이 그대로 보입니다. DAG 탭에서는 그 명령이 머리말에 한 줄로도 적힙니다.

## 작업 만들기

```sh
BUILD=$(tasty agent task-create --workspace-id 2 --name build \
  --command '{"kind":"run","command":["cargo","build"]}' | jq -r .id)
```

만들면 작업 ID 가 돌아옵니다. 이 ID 로 의존 관계를 걸고 상태를 조회합니다.

| 명령 종류 | 하는 일 |
|---|---|
| `run` | 지정한 명령을 실행합니다. 터미널을 차지하지 않는 백그라운드 프로세스이고, 표준 출력과 표준 에러를 각각 끝에서 64KiB 까지 담아 결과에 싣습니다. 대화형 프로그램은 여기에 맞지 않습니다. 환경변수는 Tasty 를 실행한 환경을 받되, 아래 표 다음의 예외가 있습니다 |
| `custom` | tasty 자신의 동작을 작업으로 만듭니다. 자식 에이전트 띄우기처럼 터미널을 만드는 일이 여기 해당합니다 |
| `reduce` | 여러 작업의 결과를 하나로 합칩니다 |
| `wait_barrier` | 배리어에 신호가 다 모일 때까지 기다립니다 |

명령 JSON 은 파일로 빼서 `--command @build.json` 처럼 넘겨도 됩니다.

`run` 명령, 아래 후처리 명령, 셸 명령으로 결과를 합치는 리듀서는 Tasty 를 실행한 환경을 그대로 받습니다. 다만 Tasty 를 Claude Code 안에서 띄웠다면 그 세션을 가리키는 변수는 넘기지 않고(터미널과 같습니다), 다른 Tasty 의 터미널에서 띄웠다면 그 Tasty 를 가리키는 `TASTY_SESSION_TOKEN`·`TASTY_SURFACE_ID`·`TASTY_PARENT_HOME`·`TASTY_AGENT_ID` 도 넘기지 않습니다. 그래서 명령 안에서 `tasty` 를 부르면 이 Tasty 에 닿습니다.

## 순서와 실패 처리

```sh
TEST=$(tasty agent task-create --workspace-id 2 --name test \
  --command '{"kind":"run","command":["cargo","test"]}' \
  --depends-on "$BUILD" --on-failure abort | jq -r .id)
```

`--depends-on` 에 적은 작업이 모두 끝나야 이 작업이 준비 상태가 됩니다. 사이클이 생기는 그래프는 만들 때 거부됩니다.

| 실패 정책 | 뜻 | 어디에 붙이나 |
|---|---|---|
| `abort` (기본) | 의존하던 작업이 실패하면 이 작업은 건너뜁니다. 그 아래로도 이어서 건너뜁니다 | 의존하는 쪽 |
| `continue_downstream` | 의존하던 작업이 실패해도 이 작업은 진행합니다 | 의존하는 쪽 |
| `fallback:<작업 ID>` | 이 작업이 실패하면 대신 그 작업을 깨웁니다 | 실패할 수 있는 쪽 |

붙이는 위치를 헷갈리기 쉽습니다. `abort` 와 `continue_downstream` 은 **뒤따르는** 작업에 붙여야 그 작업의 준비 판정에 반영되고, `fallback` 은 반대로 **실패할 수 있는** 작업 자신에 붙여야 합니다. 다른 위치에 지정하면 원하는 실패 처리가 적용되지 않습니다.

폴백으로 쓸 작업은 본 작업보다 먼저 만들어야 합니다. 그 사이에 러너가 먼저 실행해버리는 것을 막으려면 폴백 쪽을 `--reserved-for-fallback` 으로 만듭니다. 예약하면 본 작업을 만들기 전에도 실행되지 않습니다. 본 작업을 연결한 뒤에는 그 작업이 실패해 폴백을 활성화할 때까지 기다립니다.

## 앞 작업의 결과 넘기기

`--depends-on` 은 순서만 묶습니다. 앞 작업의 결과를 뒤 작업의 **입력**으로 넘기려면 자리표시자를 씁니다.

```
${task.<작업 ID>.output}          결과 전체
${task.<작업 ID>.output/stdout/text}   결과 안의 한 값
```

작업을 보낼 때 그 자리에 실제 값이 들어갑니다. 자식 에이전트를 띄우고 그 자식에게 말을 거는 흐름처럼, 만들 때는 알 수 없고 실행해봐야 정해지는 값을 넘길 때 씁니다. 참조하는 작업은 반드시 `--depends-on` 에 적혀 있어야 하고, 아니면 만들 때 거부됩니다. 값의 형태는 유지됩니다. 자리표시자 하나만 있는 문자열은 숫자면 숫자로 바뀝니다.

## 타입을 정한 작업 묶음 한 번에 보내기

결과의 타입까지 정해 두고 싶으면 작업들을 그래프 하나로 묶어 보냅니다. 그래프 전체를 먼저 검사하므로, 하나라도 잘못되면 아무 작업도 만들어지지 않고 오류에 어느 작업의 어느 자리가 틀렸는지 나옵니다. 모든 작업이 만들어지기 전에는 어떤 작업도 실행되지 않습니다. 그래프 하나에는 작업을 1000 개까지 담을 수 있고, 더 많으면 여러 그래프로 나눠 보냅니다.

```sh
tasty agent task-graph-submit --workspace-id 2 --graph @graph.json --dry-run   # 검사만
tasty agent task-graph-submit --workspace-id 2 --graph @graph.json
```

```json
{"contract_version": 2,
 "tasks": [
   {"id": "build", "command": {"kind": "run", "command": ["cargo", "build"]}},
   {"id": "report", "command": {"kind": "run", "command": ["notify"]},
    "input_schema": {"type": "object", "fields": {"code": {"type": "int64"}}},
    "bindings": {"code": {"from_task": "build"}},
    "input_mapping": {"args": ["/code"]}}]}
```

각 작업의 `id` 는 직접 정합니다. `bindings` 는 입력을 어디서 받을지 정합니다. 고정 값(`literal`), 앞 작업 결과의 한 값(`from_task` 와 `pointer`), 본 작업과 폴백 중 실행된 쪽(`one_of`)을 쓸 수 있고, 타입이 맞지 않으면 보낼 때 거부됩니다. 받은 값은 `input_mapping` 에 적은 자리에만 들어갑니다. `args` 는 명령 뒤에 인자로 하나씩 붙이고, `stdin: true` 는 입력 전체를 JSON 으로 표준 입력에 씁니다. 값 안의 `$(...)` 나 공백은 다시 해석되지 않습니다. 실제로 넘긴 값은 작업의 `input_snapshot` 에서 볼 수 있습니다. 이 그래프의 작업에는 위 자리표시자를 쓰지 않습니다.

작업 결과를 다른 명령(판정·요약 도구 등)으로 한 번 더 처리해 결과로 삼으려면 `postprocess` 를 붙입니다. 본 작업이 성공하면 그 명령을 실행하고, 표준 출력을 작업의 결과로 받습니다.

```json
{"id": "judge",
 "command": {"kind": "run", "command": ["make-draft"]},
 "output_schema": {"type": "enum", "values": ["pass", "revise"]},
 "postprocess": {"command": ["judge-cli", "--json"],
                 "stdin": {"draft": {"from": "raw", "pointer": "/execution/stdout/text"}},
                 "stdout": {"pointer": "/verdict"},
                 "timeout_ms": 120000,
                 "retry": {"max_retries": 1}}}
```

- 명령은 셸 없이 그대로 실행됩니다. 셸이 필요하면 `["sh", "-c", "..."]` 처럼 직접 적습니다. 로그인 정보는 Tasty 를 실행한 환경을 그대로 씁니다. 다만 Tasty 를 Claude Code 안에서 띄웠다면 그 세션을 가리키는 변수는 넘기지 않습니다(터미널과 같습니다). Tasty 를 다른 Tasty 의 터미널에서 띄웠다면 그 Tasty 의 터미널을 가리키는 `TASTY_SURFACE_ID` 같은 변수도 넘기지 않습니다.
- `stdin` 은 표준 입력에 쓸 JSON 의 필드를 정합니다. `from` 은 `input`(작업 입력), `raw`(본 작업 결과), `artifacts` 이고 `pointer` 로 그 안의 값을 고릅니다.
- 표준 출력은 기본적으로 JSON 값 하나여야 합니다. `"stdout": {"format": "text"}` 를 쓰면 글자 그대로 받습니다. 진행 로그는 표준 오류로 내보내면 결과에 섞이지 않습니다.
- `timeout_ms` 는 반드시 적습니다. 시간이 지나거나 작업을 취소하면 그 명령과 그 명령이 띄운 프로세스를 끝냅니다.
- `retry` 를 적으면 실패했을 때 본 작업은 다시 하지 않고 후처리만 그 횟수만큼 다시 실행합니다.
- 후처리가 끝날 때까지 작업은 실행 중으로 보이고 다음 작업은 기다립니다. `task-get` 은 `state: running` 다음 줄에 `phase: postprocessing (run 1)`(재시도를 기다리는 중이면 `phase: retry_wait (run 2)`)을 보여 줍니다.
- 후처리가 실패하면 `task-get` 의 `state` 줄에 이유가 나오고, 그 아래에 `postprocess: run 2 failed (nonzero_exit), exit_code 3` 처럼 마지막 실행의 원인과 종료 코드, 재시도로 넘어간 실행의 원인(`postprocess retried after: ...`)이 나옵니다.
- 후처리가 도는 중에 Tasty 를 끄거나 다시 시작하면 그 후처리는 다시 실행되지 않고 실패로 끝납니다. 다시 하려면 `task-retry` 로 작업을 다시 실행합니다.
- Tasty 가 강제로 끝나면(`kill` 등) 후처리 명령이 띄운 다른 프로세스는 남을 수 있습니다. Linux 에서는 후처리 명령 자체에 종료 신호가 가고, Windows 에서는 함께 끝나며, macOS 에서는 모두 남습니다.

Tasty 가 메모리 파일을 열지 못해 [임시 메모리](cli.md#에이전트가-함께-쓰는-메모리)로 동작하는 동안에는 그래프가 재시작 뒤에 남지 않으므로 그냥 보내면 거부됩니다. 재시작하면 사라져도 괜찮은 그래프는 `"durability": "best_effort"` 를 넣어 보냅니다.

### 결과에 따라 다음 작업 고르기

작업에 `transitions` 를 적으면 그 작업의 결과를 보고 다음에 실행할 작업을 고릅니다. 아래 예에서 `review` 의 판정이 `pass` 면 `ship`, `revise` 면 `fix`, 그 밖이면 `human` 만 실행됩니다.

```json
{"id": "review", "command": {"kind": "run", "command": ["review-tool"]},
 "output_schema": {"type": "object", "fields": {
   "verdict": {"type": "enum", "values": ["pass", "revise", "review"]}}},
 "transitions": {
   "cases": [
     {"when": {"compare": {"path": "/verdict", "op": "eq", "value": "pass"}}, "to": ["ship"]},
     {"when": {"in": {"path": "/verdict", "values": ["revise"]}}, "to": ["fix"]}],
   "otherwise": ["human"]}}
```

- 조건은 결과의 한 자리(`path`)를 값과 비교하거나(`compare`: `eq`·`ne`·`lt`·`le`·`gt`·`ge`), 값 목록에 드는지 봅니다(`in`). `all`·`any`·`not` 으로 묶을 수 있습니다. 결과의 타입에 맞지 않는 조건은 보낼 때 거부됩니다. 소수는 범위로만 비교합니다.
- 기본으로는 맞는 조건이 하나여야 합니다. 둘 이상 맞으면 그 작업이 실패로 끝납니다. 맞는 것을 모두 실행하려면 `"mode": "all_matches"` 를 넣습니다.
- 아무 조건도 맞지 않을 때 실행할 작업(`otherwise`)이나 아무것도 실행하지 않음(`"no_match": "finish"`)을 반드시 적습니다.
- 고르지 않은 작업과 그 뒤로만 이어진 작업은 실패가 아니라 "선택되지 않음"으로 끝납니다. 갈래가 다시 만나는 작업은 실제로 실행된 갈래만 기다립니다. 실행된 갈래가 실패하면 그 실패는 그대로 전해집니다.
- 실행되지 않을 수 있는 갈래의 결과를 꼭 필요한 입력으로 받으면 보낼 때 거부됩니다. 갈래마다 다른 작업이 값을 내면 `one_of` 로 받고, 없어도 되는 값이면 입력 필드를 optional 로 하거나 기본값을 줍니다.
- 작업의 `route` 에 고른 작업이, 선택되지 않은 작업의 `skip` 에 그 이유가 남습니다. 모든 작업이 성공했거나 선택되지 않았으면 DAG 는 성공으로 표시됩니다. 실패한 작업도 폴백이 대신 성공했으면 DAG 를 실패로 만들지 않습니다.

### 에이전트에게 일을 시키고 답을 결과로 받기

`agent` 작업은 Claude 나 Codex 세션에 지시 하나를 보내고, 그 답을 작업의 결과로 받습니다. 결과는 다음 작업의 입력이나 위의 갈래 조건에 그대로 쓸 수 있습니다.

```json
{"id": "review",
 "command": {"kind": "agent", "provider": "claude", "workspace_id": 2,
             "session": {"kind": "existing", "surface_id": 12},
             "instruction": "이 브랜치의 변경을 검토해 주세요",
             "timeout_ms": 1800000},
 "output_schema": {"type": "enum", "values": ["approve", "revise"]}}
```

- `command` 의 `workspace_id` 는 새 세션을 띄울 워크스페이스입니다. 값을 내는 `task-submit` 은 작업이 있는 워크스페이스로 보내며, 지시문 끝에 붙는 안내에 그 번호가 들어 있습니다.
- `session` 이 `{"kind": "new", "parent_surface": <서피스>}` 면 그 서피스 아래에 새 세션을 띄워 지시합니다. `{"kind": "existing", "surface_id": <서피스>}` 면 이미 열린 세션에 보냅니다.
- 이미 열린 세션은 비어 있을 때만 지시를 받습니다. 직접 대화하는 중이면 끝날 때까지 기다리므로 대화가 섞이지 않습니다. 같은 세션을 쓰는 작업은 차례로 실행됩니다.
- 결과 타입을 적지 않으면 그 턴의 마지막 답변이 문자열 결과가 됩니다.
- 위 예처럼 결과 타입을 정하면 에이전트가 `tasty agent task-submit` 으로 값을 내야 합니다. 지시문 끝에 내는 방법과 타입이 자동으로 붙습니다. 낸 값은 바로 검사하고, 턴이 끝날 때 결과가 됩니다. `revise` 같은 판정도 성공한 결과입니다.
- 입력을 받는 작업은 `"input_mapping": {"input_block": true}` 를 적습니다. 입력이 지시문 뒤에 JSON 으로 붙습니다.
- 답을 내지 않고 턴이 끝나거나, 턴이 오류로 끝나거나, 세션이 끝나거나, `timeout_ms` 가 지나면 작업은 실패합니다. 실패 이유는 `task-get` 의 결과에 구분돼 나옵니다. 값을 내지 않고 턴이 끝났으면 그 턴의 마지막 답이 앞 2000자까지 실패 기록에 남습니다.
- 에이전트가 권한 승인 같은 입력을 기다리면 작업은 실행 중으로 남고, `task-get` 이 `phase: awaiting_input` 줄과 `agent session: claude surface 12, awaiting input since …` 줄을 보여 줍니다.
- 작업이 끝나거나 취소돼도 세션은 닫히지 않습니다. Tasty 를 다시 시작하면 실행 중이던 agent 작업은 실패로 끝납니다.

## 진행 보기

작업이 어떻게 흘러가는지 보는 화면이 둘입니다. 둘 다 같은 데이터를 봅니다.

- **작업 DAG** <!-- en: Task DAGs --> 창 — `Ctrl+Shift+G`, 또는 사이드바 **도구** <!-- en: Tools --> 메뉴. 목록에서 하나를 골라 잠깐 보고 닫는 용도입니다. 검색과 상태 필터가 있습니다. 하나를 고르면 같은 자리가 그래프로 바뀌고, 맨 윗줄의 돌아가기 화살표 옆에 확대·축소·전체 맞춤·방향 전환과 러너 표시가 놓입니다.
- **DAG 탭** — 탭 하나를 차지하고 계속 띄워 두는 그래프입니다. `tasty new tab --pane <ID> --type dag_graph` 로 열거나, 이미 있는 서피스에서 `Alt+'` 를 눌러 **DAG** 로 바꿉니다. 확대와 축소, 전체 맞춤, 방향 전환이 있고, 노드를 누르면 명령 · 의존성 · 소요 시간 · 종료 코드 · 출력이 보입니다.

한 워크스페이스에서 서로 무관한 그래프를 여럿 돌려도 됩니다. 목록은 의존 관계로 이어진 덩어리를 하나의 DAG 로 묶어 보여줍니다. 작업에 `--metadata '{"dag":"이름"}'` 을 붙이면 연결 여부와 무관하게 같은 이름끼리 묶입니다.

| 상태 | 뜻 |
|---|---|
| **대기** <!-- en: Waiting --> | 의존하는 작업이 아직 안 끝났습니다 |
| **준비** <!-- en: Ready --> | 실행 조건을 갖췄고 러너를 기다립니다 |
| **실행** <!-- en: Running --> | 실행 중 |
| **성공** <!-- en: Succeeded --> · **실패** <!-- en: Failed --> | 끝났습니다 |
| **취소** <!-- en: Cancelled --> · **건너뜀** <!-- en: Skipped --> | 사람이 취소했거나, 앞이 실패해 건너뛰었습니다 |
| **알수없음** <!-- en: Unknown --> | 판정할 수 없습니다 |

터미널에서 보려면:

```sh
tasty agent dag-list                                   # 모든 워크스페이스의 DAG
tasty agent task-list --workspace-id 2 --state waiting,ready,running
tasty agent task-get --workspace-id 2 --id "$BUILD"
tasty agent task-graph --workspace-id 2 --format dot   # Graphviz 로 그리기
```

<a id="기다리기와-손보기"></a>

## 작업 기다리기와 관리

```sh
tasty agent task-await --workspace-id 2 --id "$TEST"             # 끝날 때까지 대기
tasty agent task-retry --workspace-id 2 --id "$TEST"             # 실패·취소·건너뛴 작업 재시도
tasty agent task-cancel --workspace-id 2 --id "$TEST"
tasty agent task-set-result --workspace-id 2 --id "$MANUAL" --state succeeded
tasty agent task-purge --workspace-id 2 --states succeeded
```

- `task-await` 는 기본 10분까지 기다리고 그 안에 안 끝나면 시간 초과로 돌아옵니다. `--timeout-ms 0` 이면 무한정 기다립니다.
- 타입을 정한 작업은 폴백이 이미 실행됐으면 `task-retry` 가 거부됩니다. 본 작업이 다시 성공하면 둘 중 하나를 받는 다음 작업이 값을 둘 보게 되기 때문입니다. 다시 돌리려면 새 작업으로 보냅니다.
- 타입을 정한 작업에는 `--reset-downstream` 을 쓸 수 없습니다. 다음 작업들은 이미 이전 실행의 실패나 고른 경로대로 끝났기 때문입니다. `task-retry` 는 그 작업만 다시 실행합니다. 뒤따르는 작업까지 다시 돌리려면 새 그래프로 보냅니다.
- `task-set-result` 는 러너가 실행하지 않은 일, 예컨대 사람이 손으로 하는 확인 절차를 끝났다고 알릴 때 씁니다.
  - 타입을 정한 작업은 실행할 때마다 회차 ID(`<작업 ID>#<번호>`)를 받습니다. `--attempt-id` 로 어느 회차의 결과인지 밝히면, 그 사이 작업이 다시 실행됐을 때 옛 보고가 새 실행을 끝내지 않고 거부됩니다.
  - 같은 보고를 다시 보내면 처음과 같은 답이 돌아옵니다. 이미 끝난 회차에 다른 결과를 보내면 거부됩니다.
- `task-delete` 는 다른 작업이 참조하고 있으면 거부하고 참조하는 쪽 ID 를 알려줍니다. 실행 중인 작업은 먼저 취소해야 합니다.

## 끝난 것을 사건으로 받기

작업이 끝났는지 되풀이해 물어보는 대신, 끝나는 대로 받아볼 수 있습니다.

```sh
tasty events follow --filter 'agent.*'
```

한 줄에 사건 하나가 JSON 으로 나오므로 셸에서 그대로 받아 씁니다.

```sh
tasty events follow --filter 'agent.*' | while read -r line; do
  echo "사건: $line"
done
```

- **읽던 위치는 수신자가 저장합니다.** 답에 함께 오는 `next_offset` 을 다음 번 `--offset` 으로 주면 끊겼던 자리부터 이어집니다. 한 번만 읽고 말 때는 `tasty events fetch --offset <번호>` 를 씁니다.
- 사건은 메모리에만 남고 최근 것만 보관합니다. 너무 오래 끊겨 있어 그 사이가 밀려났으면 **조용히 처음부터 주지 않고** 몇 건을 못 줬는지 알려 줍니다. 그 알림은 사건 목록과 섞이지 않게 따로 나오므로 위의 `while read` 가 방해받지 않습니다.
- Tasty 를 다시 켜면 사건은 사라지고 위치도 처음부터 다시 매겨집니다. 답에 함께 오는 `epoch` 이 지난번과 다르면 저장한 위치는 재시작 전 실행의 값입니다.
- 연결이 끊기면 `follow` 는 다시 붙을 때 줄 `--offset` 과 `--epoch` 을 알려 주고 끝납니다. 그대로 붙여 다시 실행하면, 그 사이 Tasty 가 다시 켜졌을 때 그렇다고 알리고 새로 시작된 사건의 처음부터 받습니다. `--reconnect` 를 주면 끝나지 않고 1초마다 다시 붙어 이어 갑니다.
- 지금 나오는 것은 **작업이 끝났을 때**와 **배리어가 닫혔을 때** 둘입니다. 왜 실패했는지는 사건에 실리지 않으니 그때는 `tasty agent task-get` 으로 봅니다.
- 읽기가 느린 수신자를 위해 별도 대기열을 계속 늘리지는 않습니다. 최근 이벤트의 공용 보관 범위를 벗어나면 누락을 확인해야 합니다.

## 동시 실행 제한과 신호

작업을 여럿 돌릴 때 쓰는 조율 장치가 함께 있습니다.

| 장치 | 쓰임 |
|---|---|
| 세마포어 | 같은 이름을 단 작업이 동시에 몇 개까지 돌지 정합니다. 작업을 만들 때 `--concurrency-limit <이름>` 이 짧은 표기입니다 |
| 배리어 | 정해진 수의 신호가 모일 때까지 막습니다. `wait_barrier` 작업으로 그래프에 끼워 넣습니다 |
| 리스 | 파일 같은 자원을 한 번에 하나만 잡게 합니다. 만료 시간이 있고, 충돌하면 실패하거나 못 잡았다는 응답을 바로 돌려줍니다 — 기다리지 않습니다 |
| 리듀서 | 여러 작업의 결과를 하나로 합칩니다. 첫 성공만, 전부, JSON 병합, 텍스트 이어붙이기 중에 고릅니다 |
| 요청량 제한 | 에이전트별 · 지표별로 정해진 시간에 몇 번까지 허용할지 정합니다 |

```sh
tasty agent semaphore-create --workspace-id 2 --name build --permits 2
tasty agent barrier-create --workspace-id 2 --name ready --count-required 3
tasty agent lease-acquire --workspace-id 2 --resource file:/tmp/db --holder agent-a --ttl-ms 60000
tasty agent task-reduce --workspace-id 2 --inputs "$A,$B" --strategy all --extract-path /stdout/text
```

세마포어 · 배리어 이름에는 영문 소문자 · 숫자 · `.` · `_` · `-` 만 씁니다. 다른 문자가 섞이면 명령이 어느 글자가 문제인지 알려 주며 거절합니다. 리스의 자원 이름에는 이 제한이 없습니다.

전체 목록은 `tasty agent --help` 에 있습니다.

<a id="다음-읽을-것"></a>

## 함께 살펴보기

- [tasty CLI 사용하기](cli.md) — 에이전트가 쓰는 명령 전반
- [Claude · Codex](claude-codex.md) — 자식 에이전트를 띄우고 통지받기
- [훅 · 알림 · 웹훅](hooks-notifications.md) — 명령이 끝났을 때 알림 받기
