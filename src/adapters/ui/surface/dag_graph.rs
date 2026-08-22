//! `DagGraphSurface` 의 egui 렌더 — task DAG 를 노드/엣지 그래프로 그린다.
//!
//! **관찰 전용**이다. 노드 편집기가 아니라서 포트/소켓도, 드래그 이동도, 연결
//! 어피던스도 없다 — 인터랙션은 pan / zoom / 선택 / 대상 DAG 전환뿐이다.
//!
//! # 모듈 분담
//!
//! | 모듈 | 책임 |
//! |------|------|
//! | [`model`] | 상태 8종 어휘 + 화면이 소비하는 데이터 형태(`DagData`), `Task` → 그 형태로의 변환 |
//! | [`view`] | surface 별 뷰 상태(줌/팬/선택) · 폴링 게이트 · 레이아웃 캐시 |
//! | [`render`] | 최상위 조립 — 헤더 + 캔버스 + 상세(패널/시트) |
//! | [`canvas`] | 캔버스 페인팅과 인터랙션(pan/zoom/LOD/선택) |
//! | [`node`] | 노드 카드 한 장 |
//! | [`chrome`] | 러너 배지 · 줌 클러스터 · 미니맵 · 사이클 배너 · LOD 칩 · 빈 상태 |
//! | [`detail`] | 선택 노드 상세 **콘텐츠** — 도킹(우측 패널/하단 시트)만 호출자가 정한다 |
//!
//! # 레이아웃은 모양에서만 나온다
//!
//! 좌표 계산 입력은 **id + 의존 엣지 + 치수**뿐이다(`tasty-dag-layout`). `TaskState`
//! 는 캐시 키에도 들어가지 않는다 — 0.5 초 폴링에서 상태가 바뀔 때마다 좌표를 다시
//! 계산하면 노드가 미세하게 튀어 그래프를 읽을 수 없다. 상태 전이는 카드 한 장을
//! 다시 칠할 뿐이다.

pub mod canvas;
pub mod chrome;
pub mod detail;
pub mod model;
pub mod node;
pub mod render;
pub mod view;

pub use render::draw_dag_graph;
pub use view::{DagGraphViewStore, DagPollRequest, DagTarget};
