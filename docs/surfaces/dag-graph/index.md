# DAG 그래프 (`dag_graph`)

- **Status**: Implemented
- **kind**: `dag_graph` — host 내장
- **렌더 경로**: host-egui
- **주체**: 로컬 사용자(관찰) · AI Agent(생성)
- **ADR**: [ADR-0042](../../adr/0042-agent-coordination-and-task-views.md)
- **코드**: 등록 `register_dag_graph`(`src/runtime/surface_registry/builtins.rs`), 모델 `DagGraphSurface`(`crates/tasty-model/src/dag_graph_surface.rs`), 화면 `src/adapters/ui/surface/dag_graph/`
- **화면**: [screens/graph.md](screens/graph.md)

## 목적·지원 범위

workspace 의 task DAG 를 노드/엣지 그래프로 관찰하는 surface 다. 노드를 편집하거나 task 를 실행하지 않는다. DAG 의 도메인·실행·협업 API(`agent.*`)와 DAG 목록 popup 은 [다중 에이전트 협업](../../features/agent-collaboration/index.md)에 있고, 이 문서는 그 데이터를 보여 주는 surface 의 계약만 다룬다.

## 상태 소유자

- 구조 트리의 leaf 는 다른 kind 와 같은 `SurfaceDescriptor` 다. 관찰 대상(`dag_id`·`workspace_id`)과 방향은 `DagGraphSurface` 가 가진다.
- DAG 데이터는 host 의 task store 가 가진다. surface 는 아래 조회 서비스로 읽기만 한다.
- 줌·팬·선택은 화면 상태이며 저장하지 않는다(아래 저장·복원).

### 조회 소유와 취소

View는 `EngineRead::dag_source()`로 읽기 요청을 등록하고 완료된 스냅샷만 받는다.
공유 task store의 잠금 획득·조회·DAG 집계는 App 소유 워커가 수행한다.
동시에 실행하는 워커는 App 전체 4개, engine별 1개이며 대기 큐는 engine별 16개다.
큐가 가득 차면 View의 수신 핸들이 요청을 보존해 다음 폴링에 다시 등록한다.

명시한 `workspace_id`가 다른 창의 engine에 속해도 조회할 수 있다. App은 완료 전달 전에
대상 workspace의 실제 소유 engine과 journal incarnation/runtime epoch를 다시 대조하고,
러너 표시도 그 소유자의 상태를 사용한다. View는 요청을 낸 engine과 표시 대상 DAG를
대조한다. surface activation 변경·숨김·닫힘은 진행 중인 수신 핸들을 버린다.

같은 대상의 일시적인 조회 실패는 마지막 정상 화면을 유지한다. 소유 세대가 바뀌거나
workspace가 사라진 응답은 기존 데이터를 지운다. 저장소 잠금을 기다리는 워커도
수신 핸들 취소나 App 종료를 관측하면 잠금 해제를 기다리지 않고 반환한다.
이미 실행 중인 저장소 호출은 강제로 중단하지 않으며 App의 종료 대기 기한을 따른다.

## 생성·갱신·종료

- `tasty new tab --pane <PANE> --type dag_graph`, `tasty split --level surface --target-surface <SID> --type dag_graph` 로 만든다. [surface 변환](../../features/convert-surface/index.md) popup 에도 다른 kind 와 같이 나온다.
- params: `dag_id`(별칭 `dag`) · `workspace_id` · `direction`. `dag_id`·`workspace_id` 를 모두 생략하면 이 surface 가 속한 workspace 에서 진행 중인 DAG 를 고른다(없으면 가장 최근 갱신). 활성 workspace 가 아니라 소속 workspace 다.
- 화면 갱신 주기와 캐시는 [화면 문서의 갱신과 비용](screens/graph.md#갱신과-비용)에 있다.
- 숨김·닫힘은 진행 중인 조회 수신 핸들을 버린다(위 조회 소유와 취소).

## 저장·복원

`direction` 과 (지정했다면) `dag_id` / `workspace_id` 만 레이아웃에 저장된다. 줌/팬/선택은
저장하지 않는다 — 재시작 후 그래프 모양이 달라져 있을 수 있어 예전 뷰포트를 복원하면 엉뚱한
빈 곳을 보게 된다. 복원 직후에는 auto-fit 이 돈다.

## IPC·CLI

이 kind 전용 IPC 는 없다. 생성·분할·닫기는 공통 명령([작업 영역](../../features/work-area/index.md#인터페이스))을 쓰고, DAG 데이터의 조회와 조작은 `agent.*`([다중 에이전트 협업](../../features/agent-collaboration/index.md))을 쓴다.

## headless·원격 제약

host 내장 kind 라 헤드리스에도 등록된다. 헤드리스에서 이 surface 를 만들었을 때와 attach mirror 에서의 동작은 아직 이 문서에 정리되지 않았다.

## 검증 기준

- Given 진행 중인 DAG 가 있는 workspace When `tasty new tab --pane <PANE> --type dag_graph` 로 params 없이 만든다 Then 이 surface 가 속한 workspace 의 진행 중인 DAG 를 관찰한다.
- Given `direction` 을 바꾼 DAG surface When 앱을 다시 시작한다 Then 방향과 지정한 대상은 복원되고 줌·팬·선택은 복원되지 않는다.

## 화면

- [screens/graph.md](screens/graph.md) — 그래프 캔버스, 상호작용, 상태별 시각, 갱신 주기
