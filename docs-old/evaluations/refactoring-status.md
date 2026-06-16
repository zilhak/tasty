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

## 2. 확장성: 단일 CellRenderer — **완료 (Phase F.G.a)**

`gpu/mod.rs` 의 GpuState 는 여전히 단일 `CellRenderer` 만 소유하지만, F.G.a
에서 *서피스별 유니폼 덮어쓰기 + 서피스별 encoder/submit 분리* 구조를
폐기했다. 옛 권고였던 "서피스별 유니폼 배열 / 인스턴스에 뷰포트 오프셋
포함" 중 **후자** 가 채택됨.

- `Uniforms` 는 cell_size + viewport_size 만 보유 (서피스 간 공통, resize
  시 1 회 write). `grid_offset` 필드는 제거됨.
- `BgInstance` / `GlyphInstance` 가 `viewport_offset` attribute 를 보유 —
  push 시 per-surface viewport rect 에서 baking. BG/GLYPH 셰이더가
  `instance.viewport_offset` 를 read.
- `CellRenderer` API: `begin_frame` (per-frame 인스턴스 vec + range record
  초기화) → `append_terminal_viewport` (서피스 인스턴스 누적 + scissor +
  bg/glyph range 기록) → `flush_buffers` (kind 당 `queue.write_buffer` 1 회).
  GPU 버퍼 + CPU Vec capacity 가 누적량보다 작으면 자동 grow.

**미해소 trigger**: 다른 VTE 백엔드 (alacritty_terminal 등) 도입 시 서피스
별 *별도 CellRenderer* 가 필요해질 수 있음. 외부 wgpu 사용자 요구도 trigger
후보.

---

## 3. 확장성: 다중 페이지 아틀라스 — **완료 (Phase F.G.b)**

`crates/tasty-font` 의 `GlyphAtlas` 가 D2Array 다중 페이지 + LRU eviction
으로 확장되었다 (옛 *2048x2048 단일 페이지 + 전체 캐시 리셋* 정책 폐기).

- D2Array `MAX_PAGES = 4` layer (각 2048×2048 R8, ~4 MiB / layer).
- `AtlasEntry` 가 `page: u32` 필드 보유 (D2Array layer index).
- `AtlasPage` 마다 shelf 상태 + `last_access_frame` 타임스탬프 — 캐시 hit /
  삽입 시 갱신.
- `get_or_insert` 는 활성 페이지 → wrap-around 으로 shelf 공간 탐색.
  4 페이지 모두 full 이면 *활성 페이지 제외* `last_access_frame` 최소
  페이지를 victim 으로 선정 → 해당 layer 의 캐시 entry drop + shelf reset
  + zero-clear. 한 프레임에 최대 1 evict (thrashing 방지) — 같은 프레임의
  2 번째 over-cap 글리프는 defer.
- `CellRenderer::begin_frame` 이 `GlyphAtlas::begin_frame` 호출로
  프레임 카운터 bump 와 동기.

**미해소 trigger**: 4 layer ~16 MiB 한계를 넘는 *극단적 폰트셋 + 다국어
동시 사용* 시나리오. 현재 미관측. 발생 시 `MAX_PAGES` 상향 또는 페이지
수 동적 확장 검토.

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
| ~~P3~~ ✅ | ~~멀티 서피스 렌더 최적화~~ 완료 (Phase F.G.a, §2) | 단일 submit cycle batching + 인스턴스 viewport_offset 으로 draw 호출 1 회로 통합 |
| ~~P3~~ ✅ | ~~다중 아틀라스 페이지~~ 완료 (Phase F.G.b, §3) | D2Array 4 layer + LRU eviction 도입, 옛 전체 reset 폐기 |
