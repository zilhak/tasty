# 작업 DAG

에이전트가 할 일을 **작업(task)** 으로 만들고 의존 관계로 묶으면 그래프가 됩니다. tasty 의 러너가 순서를 지켜 실행하고, 실패하면 정해둔 대로 처리하고, 결과를 다음 작업에 넘깁니다. 사람은 윈도우에서 진행 상황을 그래프로 봅니다.

명령은 전부 `tasty` CLI 라 에이전트가 셸에서 그대로 호출합니다. 사람이 주로 보는 곳은 [진행 보기](#진행-보기) 입니다.

모든 명령에 `--workspace-id` 가 필요합니다. 작업은 워크스페이스에 속하고, 어느 워크스페이스가 활성이든 결과가 달라지지 않습니다.

## 러너 켜기

작업을 만들어도 러너가 꺼져 있으면 아무것도 실행되지 않습니다. 워크스페이스마다 한 번 켭니다.

```sh
tasty agent task-run --workspace-id 2 --action start
tasty agent task-run --workspace-id 2 --action status
tasty agent task-run --workspace-id 2 --action stop
```

tasty 를 재시작하면 러너는 자동으로 켜지지 않습니다. 작업 자체는 남아 있으니 다시 `start` 하면 이어서 진행합니다. DAG 윈도우의 헤더에도 러너가 멈춰 있으면 다시 켜는 명령이 그대로 표시됩니다.

## 작업 만들기

```sh
tasty agent task-create --workspace-id 2 --name build \
  --command '{"kind":"run","command":["cargo","build"]}'
```

만들면 작업 ID 가 돌아옵니다. 이 ID 로 의존 관계를 걸고 상태를 조회합니다.

| 명령 종류 | 하는 일 |
|---|---|
| `run` | 명령을 그냥 실행합니다. 터미널을 차지하지 않는 백그라운드 프로세스이고, 표준 출력과 표준 에러를 각각 끝에서 64KiB 까지 담아 결과에 싣습니다. 대화형 프로그램은 여기에 맞지 않습니다 |
| `custom` | tasty 자신의 동작을 작업으로 만듭니다. 자식 에이전트 띄우기처럼 터미널을 만드는 일이 여기 해당합니다 |
| `reduce` | 여러 작업의 결과를 하나로 합칩니다 |
| `wait_barrier` | 배리어에 신호가 다 모일 때까지 기다립니다 |

명령 JSON 은 파일로 빼서 `--command @build.json` 처럼 넘겨도 됩니다.

## 순서와 실패 처리

```sh
tasty agent task-create --workspace-id 2 --name test \
  --command '{"kind":"run","command":["cargo","test"]}' \
  --depends-on t-build --on-failure abort
```

`--depends-on` 에 적은 작업이 모두 끝나야 이 작업이 준비 상태가 됩니다. 사이클이 생기는 그래프는 만들 때 거부됩니다.

| 실패 정책 | 뜻 | 어디에 붙이나 |
|---|---|---|
| `abort` (기본) | 의존하던 작업이 실패하면 이 작업은 건너뜁니다. 그 아래로도 이어서 건너뜁니다 | 의존하는 쪽 |
| `continue_downstream` | 의존하던 작업이 실패해도 이 작업은 진행합니다 | 의존하는 쪽 |
| `fallback:<작업 ID>` | 이 작업이 실패하면 대신 그 작업을 깨웁니다 | 실패할 수 있는 쪽 |

붙이는 위치를 헷갈리기 쉽습니다. `abort` 와 `continue_downstream` 은 **뒤따르는** 작업에 붙여야 그 작업의 준비 판정에 반영되고, `fallback` 은 반대로 **실패할 수 있는** 작업 자신에 붙여야 합니다. 위치를 바꿔 붙이면 조용히 아무 일도 일어나지 않습니다.

폴백으로 쓸 작업은 본 작업보다 먼저 만들어야 합니다. 그 사이에 러너가 먼저 실행해버리는 것을 막으려면 폴백 쪽을 `--reserved-for-fallback` 으로 만듭니다. 그러면 자기를 참조하는 본 작업이 생길 때까지 실행되지 않습니다.

## 앞 작업의 결과 넘기기

`--depends-on` 은 순서만 묶습니다. 앞 작업의 결과를 뒤 작업의 **입력**으로 넘기려면 자리표시자를 씁니다.

```
${task.<작업 ID>.output}          결과 전체
${task.<작업 ID>.output/stdout/text}   결과 안의 한 값
```

작업을 보낼 때 그 자리에 실제 값이 들어갑니다. 자식 에이전트를 띄우고 그 자식에게 말을 거는 흐름처럼, 만들 때는 알 수 없고 실행해봐야 정해지는 값을 넘길 때 씁니다. 참조하는 작업은 반드시 `--depends-on` 에 적혀 있어야 하고, 아니면 만들 때 거부됩니다. 값의 형태는 유지됩니다. 자리표시자 하나만 있는 문자열은 숫자면 숫자로 바뀝니다.

## 진행 보기

작업이 어떻게 흘러가는지 보는 화면이 둘입니다. 둘 다 같은 데이터를 봅니다.

- **작업 DAG** <!-- en: Task DAGs --> 창 — `Ctrl+Shift+G`, 또는 사이드바 **도구** <!-- en: Tools --> 메뉴. 목록에서 하나를 골라 잠깐 보고 닫는 용도입니다. 검색과 상태 필터가 있습니다.
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
tasty agent task-get --workspace-id 2 --id t-build
tasty agent task-graph --workspace-id 2 --format dot   # Graphviz 로 그리기
```

## 기다리기와 손보기

```sh
tasty agent task-await --workspace-id 2 --id t-test              # 끝날 때까지 대기
tasty agent task-retry --workspace-id 2 --id t-test              # 실패·취소·건너뛴 작업 재시도
tasty agent task-cancel --workspace-id 2 --id t-test
tasty agent task-set-result --workspace-id 2 --id t-manual --state succeeded
tasty agent task-purge --workspace-id 2 --states succeeded
```

- `task-await` 는 기본 10분까지 기다리고 그 안에 안 끝나면 시간 초과로 돌아옵니다. `--timeout-ms 0` 이면 무한정 기다립니다.
- `task-set-result` 는 러너가 실행하지 않은 일, 예컨대 사람이 손으로 하는 확인 절차를 끝났다고 알릴 때 씁니다.
- `task-delete` 는 다른 작업이 참조하고 있으면 거부하고 참조하는 쪽 ID 를 알려줍니다. 실행 중인 작업은 먼저 취소해야 합니다.

## 동시 실행 제한과 신호

작업을 여럿 돌릴 때 쓰는 조율 장치가 함께 있습니다.

| 장치 | 쓰임 |
|---|---|
| 세마포어 | 같은 이름을 단 작업이 동시에 몇 개까지 돌지 정합니다. 작업을 만들 때 `--concurrency-limit <이름>` 이 짧은 표기입니다 |
| 배리어 | 정해진 수의 신호가 모일 때까지 막습니다. `wait_barrier` 작업으로 그래프에 끼워 넣습니다 |
| 리스 | 파일 같은 자원을 한 번에 하나만 잡게 합니다. 만료 시간이 있고, 충돌하면 실패하거나 기다립니다 |
| 리듀서 | 여러 작업의 결과를 하나로 합칩니다. 첫 성공만, 전부, JSON 병합, 텍스트 이어붙이기 중에 고릅니다 |
| 요청량 제한 | 에이전트별 · 지표별로 정해진 시간에 몇 번까지 허용할지 정합니다 |

```sh
tasty agent semaphore-create --workspace-id 2 --name build --permits 2
tasty agent barrier-create --workspace-id 2 --name ready --count-required 3
tasty agent lease-acquire --workspace-id 2 --resource file:/tmp/db --holder agent-a --ttl-ms 60000
tasty agent task-reduce --workspace-id 2 --inputs t-a,t-b --strategy all --extract-path /stdout/text
```

전체 목록은 `tasty agent --help` 에 있습니다.

## 다음 읽을 것

- [CLI 로 tasty 다루기](cli.md) — 에이전트가 쓰는 명령 전반
- [Claude · Codex](claude-codex.md) — 자식 에이전트를 띄우고 통지받기
- [훅 · 알림 · 웹훅](hooks-notifications.md) — 명령이 끝났을 때 알림 받기
