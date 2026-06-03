# 리팩토링 분석

현재 남아있는 코드 개선 가능성과 로드맵을 기술한다.

이전 리팩토링으로 완료된 항목(God Object 분리, Visitor 패턴 도입, 파일 분할, 클립보드 구현, DECSET 구현, `_with_cwd` 오버로드 통합 등)은 이 문서에서 제외한다.

---

## 1. 코드 중복: PaneNode / SurfaceLayout — **완료 (Phase F.F)**

`model/pane_tree.rs::PaneNode` 와 `model/surface_layout.rs::SurfaceLayout` 의
binary-tree 구조 재귀 본체가 `src/model/binary_tree.rs` 의 `BinaryTree` trait
default 메서드로 통합되었다.

**trait 표면:**

- 연관: `type Id`, `const BORDER_WIDTH: PhysicalPx`
- 필수: `split_parts(_mut)`, `leaf_id`
- default (구조 재귀, leaf-agnostic): `first_id`, `all_ids`, `next_id` /
  `prev_id`, `compute_rects`, `collect_dividers`, `find_divider_at`,
  `update_ratio_for_rect`, `directional_focus`, `build_path_to`, `edge_leaf`

**leaf-touching 메서드는 inherent 유지:** PaneNode 의 `split_pane_in_place`
/ `close_pane` / `find_pane(_mut)` / `first_pane` / `all_surface_ids`,
SurfaceLayout 의 `split_with_surface` / `close_surface` / `replace_surface`
/ `find_surface(_mut)` / `surface_regions` / `resize_all` /
`for_each_surface` / `find_surface_at` 등.

**외부 호출처 0 변경:** 각 enum 이 trait 와 동명인 메서드 (`compute_rects`
등 5 개) 와 이름이 다른 id-시리즈 (PaneNode 3 / SurfaceLayout 2) 를
inherent alias 로 보존, UFCS (`<Self as BinaryTree>::method(self, ...)`)
로 trait 본체에 위임한다. `crate::model::BinaryTree` 는 prelude 로 재노출
되어 신규 코드가 trait method 를 직접 호출할 때 import 1 줄만 필요.

---

## 2. 확장성: 단일 CellRenderer

`gpu/mod.rs`의 GpuState가 `renderer: CellRenderer` 하나만 소유한다. 모든 서피스가 동일한 CellRenderer를 공유하며, `prepare_terminal_viewport()` 호출 시마다 유니폼 버퍼를 덮어쓴다.

멀티 서피스 렌더링이 순차적이어서 draw call이 서피스 수에 비례하여 증가한다.

**개선안:**
- 서피스별 유니폼을 배열이나 동적 오프셋으로 관리
- 인스턴스 데이터에 뷰포트 오프셋을 포함시켜 단일 draw call로 렌더

---

## 3. 확장성: 고정 아틀라스 크기

`font.rs`의 GlyphAtlas는 2048x2048 고정 크기이며, 가득 차면 전체 캐시를 초기화한다.

CJK/이모지 등 유니코드 문자가 많으면 아틀라스가 자주 리셋되어 성능 저하.

**개선안:**
- 다중 아틀라스 페이지 (새 텍스처 할당)
- LRU 캐시로 사용 빈도 낮은 글리프 교체

---

## 4. 크레이트 분리 후보

현재 `src/` 내에 있던 분리 후보 3 종 중 *model* 만 trigger 도달 (G.E 에서 분리 완료), 나머지는 trigger 미도달. **trigger 재정의 + 분리 보류 사유는 [`library-separation/execution-plan.md`](library-separation/execution-plan.md) 의 Phase 4~6 참조.**

| 모듈 | 근거 | 난이도 | 현 상태 |
|------|------|--------|---------|
| `src/model/` → `tasty-model` | `tasty-terminal` 외 `use crate::` 없음 | 중 | **✅ 완료 (G.E, 2026-06-03)**. `crates/tasty-model/` (16 파일 / 3,719 LOC). 본 바이너리 `src/model.rs` 는 `pub use tasty_model::*;` shim 유지 — 옛 callsite 회귀 0. ([exec-plan Phase 5](library-separation/execution-plan.md#phase-5-tasty-model-분리--완료-ge-2026-06-03)) |
| `src/gfx/renderer/` + `src/gfx/gpu/` → `tasty-renderer` | `font`, `model`, `selection` 만 의존 | 중 | **G.E 시점 trigger 미도달 — 보류**. 실측 본 바이너리 내부 의존 13 unique 모듈 (state/settings/plugin/AppEvent/i18n/terminal_link/selection 등), wgpu 24 미안정. ([exec-plan Phase 4](library-separation/execution-plan.md#phase-4-tasty-renderer-분리--장기-과제-유지-분리-안-됨)) |
| `src/store/notification.rs` + 분산 4 곳 → `tasty-notification` | `model::Rect` 불필요, notify-rust 만 의존 | 소 | **G.E 시점 trigger 미도달 — 보류**. `grep crates/` plugin importer = 0. F.E NotificationSoundPlayer port 도입에도 외부 노출 0. ([exec-plan Phase 6](library-separation/execution-plan.md#phase-6-tasty-notification-분리--비권장-유지-재검토-필요)) |

---

## 우선순위

| 순위 | 항목 | 효과 |
|------|------|------|
| ~~P2~~ ✅ | ~~BinaryTree trait 추출~~ 완료 (§1) | ~250줄 중복 제거, 새 트리 타입 추가 용이 |
| ~~P3~~ ✅ | ~~크레이트 분리 (model)~~ 완료 (G.E, §4) | `crates/tasty-model/` 분리. renderer / notification 은 trigger 미도달 — [library-separation/execution-plan.md](library-separation/execution-plan.md) 의 Phase 4 / 6 trigger 충족 시 재진입 |
| P3 | 멀티 서피스 렌더 최적화 | 10+ 서피스에서 성능 개선 |
| P3 | 다중 아틀라스 페이지 | CJK 집약 사용 시 성능 개선 |
