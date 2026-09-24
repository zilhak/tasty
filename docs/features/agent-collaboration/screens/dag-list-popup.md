# DAG 목록 popup

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `ui_kits/terminal/overlays/dag_surfaces.jsx` 의 `DagWindow` · `dagRowItems` (claude design)
- **popup id**: `dag_list` · **스코프**: `PopupScope::Workspace`

DAG 를 **잠깐 확인하고 닫는** 관측 창이다. 탭 하나를 통째로 점유하는
[DAG 그래프 surface](dag-graph-surface.md) 와 같은 데이터를 보지만, 목록에서 시작해
하나를 고르면 같은 영역이 그 그래프로 교체된다(`DrillDown`). back bar 로 목록에 돌아온다.

`PopupScope::Workspace`에 속하므로 다른 workspace로 전환하면 숨겨지고, 돌아오면
보던 상태 그대로 다시 나타난다.

## 트리거

- 도구 메뉴 → `Task DAGs`
- 단축키 — `KeybindingSettings.toggle_dag_list`(기본 `Ctrl+Shift+G`, mac 프리셋 `Alt+Shift+G`).
  설정 → 단축키 → General 에서 바꾼다([../../keybindings/index.md](../../keybindings/index.md)).

여는 경로는 둘 다 `UiIntent::OpenPopup` / `TogglePopup` +
`OpenPopupMode::WithScope(PopupScope::Workspace(활성 인덱스))` 로 연다. 스코프는 정의가
아니라 **여는 시점**에 정해지므로 `PopupDef.default_scope` 는 안전한 기본(`Window`)만 들고 있다.

release에는 에이전트가 이 popup을 강제로 여는 IPC가 없다. 사용자 조작을 재현하는
기능은 [debug IPC](../../../dev-guide/debug-ipc.md)로 제한한다. 에이전트는
`agent.dag_list` / `agent.dag_get`으로 같은 데이터를 조회한다.

## UI 요소 인벤토리

| 영역 | 요소 |
|------|------|
| 타이틀바 | gitTree 아이콘 · "Task DAGs" · 닫기. 드래그 핸들 |
| 검색·필터 줄 | 검색 입력(이름 / workspace 이름) · 상태 **다중선택** 드롭다운(rollup 6 종) |
| 토글 줄 | "이 워크스페이스만" 체크박스 |
| 목록 | DAG 행(스크롤). 비면 빈 상태 2 종 |
| 푸터 | `보이는 수 of 전체 수 DAGs`(mono) · 닫기 버튼 |
| 디테일 | back bar(← + DAG 이름 + actions 슬롯에 줌 클러스터 · 러너 배지) + [그래프 화면 한 벌](dag-graph-surface.md)의 캔버스·시트 |

### 목록 행

`ListCtrl` 행 하나 = gitTree 아이콘 + DAG 이름 + `workspace · 마지막 갱신` 설명 +
trailing 클러스터. trailing 은 세 조각이다:

- **출처 태그** — `source == "derived"` 일 때만. 사용자가 `metadata.dag` 로 선언한 그룹이
  아니라 의존 연결성에서 도출됐다는 표시다(도출 규칙은 [부모 기획](../index.md)).
- **rollup 상태** — 노드와 같은 글리프·상태 이름을 쓴다. DAG의 대표 상태는 아래의
  6종이다. 작은 글자의 대비를 4.5:1로 유지하도록 라벨 색은 상태 accent 대신
  `-label` 색상에서 읽는다.
- **`완료/전체`** — 고정폭 숫자로 정확한 수를 보여준다. 진행 막대는 쓰지 않는다.
  완료 수에는 성공뿐 아니라 실패·취소·건너뜀처럼 더 이상 진행되지 않는 task도 포함한다.

### 상태 필터

체크박스 **다중선택** 드롭다운이다. 한 상태만 고를 수 있으면 "아직 안 끝난 DAG"(대기 +
준비 + 실행중)나 "끝난 DAG"(성공 + 건너뜀)를 한 번에 볼 수 없다 — 사람이 이 목록을 여는
용건이 대개 그 둘이다.

- **아무것도 안 켜면 전체 통과.** 기본값이 이것이라 "모든 상태" 를 따로 고르는 항목이 없다.
  트리거는 켜진 개수에 따라 세 갈래로 읽힌다(0 개 / N 개 / 전부). 위젯이 제공하는 일괄
  토글 행("전부 선택 / 전부 해제")도 이 화면에서는 **끈다** — 켜야 할 어휘가 6 종뿐이라
  아껴 주는 클릭이 거의 없고, 전체를 보는 방법이 이미 "아무것도 안 켜기" 로 있다.
- **하나 이상 켜면 OR 매칭** — 켜진 것 중 하나와만 같아도 보인다.
- popup 을 닫으면 다른 상태와 함께 기본값으로 되돌아간다.
- **키보드만으로 조작된다** — 트리거에서 `↓`/`Enter`/`Space` 로 열고, `↑`/`↓`/`Home`/`End`
  로 행을 옮기고, `Space`/`Enter` 로 토글하고(메뉴는 안 닫힌다), `Esc` 로 닫는다. 위젯 쪽
  규약이라 상세는 `crates/tasty-ui-widgets/src/multi_select.rs`.

필터에는 rollup의 6종(대기·준비·실행중·성공·실패·건너뜀)만 표시한다. 개별 task의
취소·알 수 없음은 DAG의 대표 상태로 나오지 않는다. 취소가 섞이면 건너뜀으로,
알 수 없음이 남으면 대기로 집계한다. 필터 값과 실제 rollup 값이 일치하는지는 시험으로 확인한다.

### 목록의 범위 — 전 workspace

창이 workspace 스코프인 것과 **목록의 범위**는 별개다. 목록은 기본적으로 전 workspace 의
DAG 를 나열하고 행마다 소속 workspace 를 보여준다:

- `agent.dag_list` 가 `workspace_id` 생략 시 살아있는 전 workspace 를 훑도록 설계돼 있고
  (불가침 원칙 3 — 포커스 독립성), 화면이 그 설계를 좁힐 이유가 없다.
- 사람이 DAG 를 찾을 때의 실제 질문이 "어느 워크스페이스에 뒀더라" 인 경우가 흔하다.

좁히고 싶으면 "이 워크스페이스만" 토글을 쓴다.

### 표시 순서 — 최근 갱신순

목록은 **가장 최근에 움직인 DAG 가 맨 위**다. 정렬 키는 `updated_at` 내림차순 — 소속 task 의
(`finished_at` ∪ `started_at` ∪ `created_at`) 최대값이라 "방금 만든 것" 과 "방금 움직인 것" 을
둘 다 위로 올린다. 최근 작업을 찾기 위한 순서다. 정렬 방향을 바꾸는 UI 는 없다.

동률은 `id` 내림차순, 그래도 같으면 소속 workspace id 오름차순으로 끊는다. explicit id
(`d:<metadata.dag>`)는 사용자가 정한 키라 workspace 가 다르면 같은 값이 나올 수 있어 세 키를
모두 써야 전순서가 된다 — 아무것도 안 움직이는 동안 폴링이 여러 번 돌아도 행이 자리를
바꾸지 않는다.

**`agent.dag_list` 응답 순서는 이와 다르고, 바뀌지 않는다.** 응답은 (workspace 순회 순서,
DAG id 오름차순)이며 이 고정된 순서는 화면이 선택 상태를 id 로 들고 폴링마다 재계산하기 위한
계약이자 CLI/IPC 소비자가 함께 보는 값이다. 표시 순서는 화면의 관심사이므로 popup 이 받은
목록을 자기 쪽에서 다시 정렬한다.

## 상호작용

| 조작 | 결과 |
|------|------|
| 행 클릭 | 디테일로 전환(0ms). 줌/선택/레이아웃은 새 그래프 기준으로 새로 시작 |
| back bar `←` | 목록 복귀. 디테일이 들고 있던 그래프 상태는 버린다 |
| `Esc`(상태 필터 열림) | 드롭다운만 닫는다. 창도 디테일도 그대로고, 포커스는 트리거에 남아 `↓` 로 바로 다시 열린다 |
| `Esc`(디테일) | 목록으로 한 걸음. 창이 바로 닫히지 않는다 |
| `Esc`(목록) | 창을 닫는다 |
| 단축키 재입력 | popup 이 **포커스를 쥐고 있는 동안에는 닫히지 않는다**(아래) |
| 디테일 안 | pan / zoom / 선택 / 방향 전환. **대상 DAG 전환은 없다** — 목록에서 이미 골랐다 |

`toggle_dag_list` 는 토글 바인딩이지만 실사용에서는 **여는 쪽으로만** 동작한다: 포커스된
popup 이 있으면 전역 단축키가 전부 막히기 때문이다(기존 설계 — 검색 입력이 키보드를 가져야
한다. 명령 팔레트도 같다). 포커스를 다른 곳에 준 뒤 다시 누르면 정상적으로 닫힌다. 닫는
경로는 `Esc`(디테일에서는 목록으로 한 걸음) · 타이틀바 X · 푸터 Close 버튼이다.

상태 필터가 열려 있으면 `Esc`는 필터만 닫는다. 창의 `Esc` 처리에서 본문 렌더링까지
건너뛰면 드롭다운이 키를 처리할 수 없으므로 본문은 계속 그린다.

디테일은 surface와 같은 `draw_dag_graph`를 사용하고, 대상 `dag_id`와 `direction`은
popup 상태에서 받는다. popup 폭 560은 상세 도킹 기준 640보다 작아 노드 상세가 항상
하단 시트로 나온다.

`DagChrome::BackBar`에서는 그래프 헤더와 캔버스의 줌 버튼을 숨기고 back bar에 줌 버튼과
러너 배지를 배치한다. back bar는 본문보다 먼저 그리므로 조작 결과를 같은 프레임에
본문에 전달한다. 대상 DAG는 목록에서 이미 골랐으므로 디테일에는 DAG 선택기가 없다.

## 상태별 시각

| 상태 | 화면 |
|------|------|
| DAG 가 하나도 없음 | "No DAGs yet" + 의존 관계로 묶인 task 가 여기 나타난다는 안내 |
| 필터가 전부 걸러냄 | "No matching DAGs" + 조건을 바꾸라는 안내 |
| 목록 폴링 실패 | 마지막으로 성공한 목록을 그대로 둔다(그래프 폴링과 같은 계약) |
| 다른 workspace 활성 | popup 자체가 그려지지 않는다. 복귀 시 상태 그대로 다시 뜬다 |
| 디테일 안의 상태들 | [그래프 surface 문서](dag-graph-surface.md#상태별-시각) 와 동일 |

## 갱신과 비용

목록과 그래프 모두 **0.5 초** 주기로 다시 읽는다(runner tick 과 같은 주기). 열려 있는 동안만
`request_repaint_after` 를 걸고, 스코프 밖 workspace 에서는 draw 자체가 돌지 않으므로 예약도
남지 않는다. 그래프 쪽 폴링 게이트는 surface 와 **같은 함수**(`DagGraphView::poll_if_stale`)를
쓴다 — 주기와 실패 처리가 두 경로에서 갈라질 수 없게 한 지점으로 모았다.

## 영속

**없다.** popup 은 surface 가 아니라 레이아웃 snapshot/restore 대상이 아니고, 상태 전부가
`AppState.dialogs.dag_list` 에 있다가 `PopupDef.on_close` 에서 기본값으로 되돌아간다. 닫는
경로 6 가지가 전부 그 훅을 지나므로([popup-implementation](../../../dev-guide/popup-implementation.md)),
어떻게 닫든 다음 open 은 **목록 뷰**에서 시작한다.

## 시각 소스

`ui_kits/terminal/overlays/dag_surfaces.jsx` 의 `DagWindow` / `dagRowItems` — 픽셀·토큰·
레이아웃 수치의 단일 출처. 크기 560 × 460 은 `component.dag-popup-width` / `-height` 토큰이다.

디테일 뷰의 배치는 시안과 같다 — 헤더 없음, back bar actions 슬롯이 줌 클러스터와 러너
배지를 든다. 렌더는 여전히 그래프 화면 한 벌이고, **헤더와 조작 버튼의 위치**만 `DagChrome` 두
갈래로 갈린다(탭 surface = `Own`, popup 디테일 = `BackBar`).

## 갤러리 specimen

`cargo run -p tasty-gallery` → **Layouts** 페이지의 `Task DAG · list rows & workspace popup`
섹션. 목록 행 4 종(`dag-rows`)과 560 × 460 popup 두 뷰(`dag-window`)를 전시한다.
