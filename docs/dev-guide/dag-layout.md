# DAG 레이아웃 (`tasty-dag-layout`)

Task DAG 를 화면에 그리기 전에 필요한 **"어느 노드를 어디에 놓을지"** 계산. 좌표만
만들고 아무것도 그리지 않는다.

- crate: [`crates/tasty-dag-layout`](../../crates/tasty-dag-layout)
- 공개 API: `layout_dag(node_ids, edges, cfg) -> GraphLayout`
- 소비처: DAG surface / popup / 미니맵 / 갤러리 specimen (렌더는 이 crate 밖)

## 공개 API

```rust
pub fn layout_dag(
    node_ids: &[String],
    edges: &[(usize, usize)],   // node_ids 인덱스 쌍, (의존 대상 → 의존하는 쪽)
    cfg: &LayoutConfig,
) -> GraphLayout;

pub struct GraphLayout {
    pub nodes: Vec<NodePosition>,  // 입력과 같은 길이·같은 순서
    pub edges: Vec<EdgeRoute>,
    pub width: LogicalPx,          // 노드 + 폴리라인 전체 경계 상자
    pub height: LogicalPx,
    pub has_cycle: bool,           // 사이클 배너용
}

pub struct NodePosition { pub id: String, pub x: LogicalPx, pub y: LogicalPx, pub layer: u32 }
pub struct EdgeRoute {
    pub from: usize, pub to: usize,
    pub points: Vec<(LogicalPx, LogicalPx)>,  // 시작점 → 꺾임점들 → 끝점
    pub back: bool,                           // 레이어 진행을 거스르는 엣지
}
```

`LayoutConfig` 는 `orientation` + `node_size` + `layer_gap` + `sibling_gap` +
`component_gap` 이며 **전부 `LogicalPx`** 다 (raw `f32` 미노출 —
[typed-length](../concepts/typed-length.md)).

### 계약

- `nodes[i].id == node_ids[i]` — 길이·순서가 입력과 같다. 내부 dummy vertex 는
  절대 새어 나오지 않는다.
- `x`/`y` 는 노드 **사각형 좌상단**, 원점은 `(0, 0)` 이고 y 는 아래로 증가한다.
- `points` 는 항상 2 개 이상이고 첫/끝 점은 카드 변 위에 있다. 레이어를 건너뛰는
  엣지는 건너뛴 레이어마다 꺾임점이 하나씩 붙어 3 개 이상이 된다.
- 같은 `(node_ids, edges, cfg)` 는 언제나 같은 결과다(결정적).

### 위치 안정성 불변식

입력은 **id + 의존 엣지 + config** 뿐이다. `TaskState`·duration·진행률 같은 값은
인자로 받지 않는다 — 0.5 초 폴링에서 상태가 바뀔 때마다 노드가 움직이면 그래프를
읽을 수 없다. 상태 전이는 카드 한 장을 다시 칠할 뿐 레이아웃을 바꾸지 않는다.
따라서 **레이아웃 캐시 키에도 상태를 넣지 않는다.**

## Theme 를 의존하지 않는 이유

`LayoutConfig` 기본값이 디자인 확정치(`component.dag-node-width/height` = 168×48,
`dag-layer-gap` = 32, `dag-sibling-gap` = 24)와 같지만, crate 는 `Theme` 를 의존하지
않고 **호출자가 토큰 값을 주입**한다. 순수 계산 crate 가 appearance 레이어를 끌어오면
의존 방향이 뒤집히고, UI 없는 유닛테스트도 불가능해진다.

`component_gap`(조각 사이 간격) 만 대응 토큰이 없다 — 시안이 연결된 DAG 한 개만
그렸기 때문이다. 토큰이 생기면 호출자가 주입하면 된다.

## 왜 별도 crate 인가

- 좌표 계산은 시각화 전용 관심사라 "IPC/GUI 와 독립" 을 원칙으로 삼는
  `tasty-agent` 에 넣을 수 없다 — 넣으면 헤드리스 빌드에 `petgraph` 가 딸려 온다.
- `rust-sugiyama` / `petgraph` 의존이 이 crate 하나에 갇힌다.
- UI 없이 유닛테스트가 되고, surface·popup·갤러리 specimen 이 같은 함수를 공유한다.

`crates/tasty-agent/src/task/graph.rs` 의 `TaskGraph`(사이클 검출·downstream·readiness)
는 건드리지 않는다. 이 crate 는 그 위에 좌표만 얹는 어댑터다.

## 어댑터 경계

외부 레이아웃 라이브러리(`rust-sugiyama` 0.4, MIT) 호출은 `engine.rs` **한 모듈
안에서만** 일어난다. 자체 구현으로 갈아끼워도 공개 API 는 그대로다 — 성숙도가 낮은
크레이트를 쓰는 대가를 이 경계 안에 가둔 것이다.

### 라이브러리에 대해 실측으로 확인한 사실 (0.4.0)

문서화되어 있지 않거나 문서와 다른 항목들이라, 갈아끼울 때 다시 확인해야 한다.

| 항목 | 실측 결과 | 어댑터의 대응 |
|------|-----------|----------------|
| 사이클 | phase 0 이 greedy feedback arc set 으로 엣지를 뒤집어 스스로 없앤다 (모듈 주석의 "미구현" 서술은 낡았다) | 격자 폴백 없이 **정상 레이어 배치**를 그대로 쓴다 |
| self-loop | 뒤집어도 사라지지 않아 내부 `assert!` 가 깨진다 | 호출 **전에** 걷어낸다. 사이클 판정에는 반영 |
| dummy vertex | 최종 출력에서 필터된다 | 유령 노드 누출은 없다. 대신 **꺾임점을 직접 합성**한다 |
| 반환 id | 우리가 준 id 가 아니라 vertex 를 넣은 순서(petgraph 인덱스) | `0..n` 을 순서대로 넣어 둘을 일치시킨다 |
| 반환 `width`/`height` | 픽셀이 아니라 레이어 개수·최대 레이어 폭 | 쓰지 않고 경계 상자를 다시 잰다 |
| 레이어 번호 | 돌려주지 않는다. y 는 rank 별 offset 이라 같은 레이어면 같은 값 | 서로 다른 y 를 정렬한 순서로 복원한다 |
| `dummy_size` | 문서는 "vertex_spacing 의 배수" 라 하지만 코드는 절대 크기로 쓴다 | `sibling_gap` 을 그대로 준다 |

### 설정

- `ranking_type: Up` — 소스를 전부 레이어 0 에 모으는 longest-path layering. 시안
  mock 과 같은 배치다(기본값 `MinimizeEdgeLength` 는 소스를 아래로 끌어내린다).
  "실노드가 하나도 없는 레이어" 가 생기지 않아 레이어 번호 복원이 엣지 span 과
  어긋나지 않는다는 부수 효과도 있다.
- `vertex_spacing: sibling_gap` — 라이브러리가 이 값을 vertex 크기에 패딩으로
  더하므로, 같은 레이어 이웃 카드의 중심 간 거리가 `cross_extent + sibling_gap` 이
  된다.
- `transpose` — 교차를 한 번 더 줄이는 선택적 패스. **노드 128 개 이하에서만 켠다**
  (아래 성능 참조).

## 방향 토글

내부 계산은 화면 축이 아니라 **레이어 축(along) / 형제 축(cross)** 이라는 추상 축으로
하고, 화면 좌표로 옮기는 마지막 단계에서만 두 축을 x/y 에 붙인다. 그래서 방향 토글은
알고리즘을 두 벌 만들지 않고 축 교환 한 번으로 끝난다.

기본값은 `LeftRight` — `agent.task_graph --format dot` 이 `rankdir=LR` 을 내보내 CLI
출력과 멘탈 모델이 일치하고, 카드가 가로로 긴 형태(168×48)라 화면 폭을 아낀다.
카드는 회전하지 않으므로 방향이 바뀌면 **어느 변이 형제 축을 향하는지**만 바뀐다
(`LeftRight` → cross_extent = 카드 높이, `TopDown` → cross_extent = 카드 너비).

⚠️ `layer_gap` 32 / `sibling_gap` 24 는 시안이 **TD 기준**으로 잡은 값이다. LR 에서
밀집하거나 교차가 급증하면 값을 임의로 조정하지 말고 방향별 간격 토큰을 디자인에
요청한다.

## 엣지 라우팅

`EdgeRoute.points` 는 **꺾임점 좌표까지만** 책임진다. 직교(orthogonal) 세그먼트와
elbow 반경(`component.dag-edge-corner-radius`), 화살촉은 렌더가 이 폴리라인을 펴서
만든다.

꺾임점이 이 crate 의 책임인 이유: 레이어를 건너뛰는 엣지가 어디를 지나야 하는지는
**레이어 배치 결과를 알아야만** 정해진다. 렌더 쪽에서는 재현할 수 없다.

합성 방식 — 엣지가 건너뛰는 각 중간 레이어마다 점을 하나씩 놓되, 출발/도착 형제
좌표를 선형 보간한 위치를 후보로 삼고 그 자리가 카드에 막혀 있으면 가장 가까운 **빈
통로**(이웃 카드 중점 또는 양 끝 바깥)로 밀어낸다. 그래서 꺾임점은 절대 카드 위를
지나지 않는다. 라이브러리가 dummy 폭만큼 통로를 이미 비워두므로 대개 보간 위치가
그대로 통과한다.

## 방어 동작

| 입력 | 동작 |
|------|------|
| 노드 0 개 | 빈 `GraphLayout` |
| 노드 1 개 · 엣지 0 개 | 원점에 카드 하나 |
| self-loop · 범위 밖 인덱스 · 중복 엣지 | 그 엣지만 조용히 버린다 |
| 고립 노드 혼재 | 조각별로 배치해 형제 축 방향으로 나란히 붙인다(모든 조각의 레이어 0 이 같은 줄) |
| 사이클 | 정상 레이어 배치 + `has_cycle = true` + 되돌아가는 엣지 `back = true` |
| 라이브러리 패닉 / 신뢰 불가 결과 | `catch_unwind` 로 가두고 격자 폴백 (패닉이 GUI 프레임으로 새지 않는다) |

같은 레이어 안에서 카드가 겹치지 않도록 최소 간격을 보장하는 패스가 마지막에 한 번
돈다. 정상 동작에서는 아무것도 바꾸지 않고, 라이브러리가 규칙을 어겼을 때만 순서를
유지한 채 밀어낸다.

## 성능

실측(release, x86_64, `layout_dag` 단일 호출):

| 그래프 | `transpose` 켬 | `transpose` 끔 |
|--------|----------------|----------------|
| 200 노드 / 443 엣지 | 22ms | 7ms |
| 500 노드 / 1118 엣지 | 455ms | 48ms |

`transpose` 비용이 초선형이라 **노드 128 개 이하에서만 켠다**. task DAG 는 대부분
수십 노드라 그 구간에서는 품질 이득을 그대로 가져가고, 큰 그래프에서만 교차 일부를
포기하는 대신 최악 시간이 묶인다.

> **소비처 주의**: 200 노드 구간이 한 프레임(16ms)에 아슬아슬하다. DAG 화면은
> **매 프레임 재계산하지 말고 캐시**한다. 위치 안정성 불변식대로 캐시 키는
> `(node_ids, edges, LayoutConfig)` 뿐이고 task 상태는 넣지 않는다 — 그래야 0.5 초
> 폴링에서 상태만 바뀔 때 재계산이 아예 일어나지 않는다.

## 폴백 전략

`rust-sugiyama` 는 0.4.0(2025-09) 이후 릴리스가 없고 사용자가 적다. 좌표가 이상하게
나오거나 패닉하면 자체 구현으로 대체한다 — 필요한 것은 (1) 위상 정렬 레이어 할당
(2) 레이어 내 barycenter 정렬 (3) 균등 간격 배치 3 단계뿐이고, 교체 범위는 `engine.rs`
안에 갇혀 있다. `routing.rs` 와 공개 API 는 그대로 재사용된다.
