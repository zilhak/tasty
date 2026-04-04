# 리팩토링 분석

현재 남아있는 코드 개선 가능성과 로드맵을 기술한다.

이전 리팩토링으로 완료된 항목(God Object 분리, Visitor 패턴 도입, 파일 분할, 클립보드 구현, DECSET 구현, `_with_cwd` 오버로드 통합 등)은 이 문서에서 제외한다.

---

## 1. 코드 중복: PaneNode / SurfaceGroupLayout

`model/pane_tree.rs`의 PaneNode과 `model/surface_layout.rs`의 SurfaceGroupLayout은 둘 다 바이너리 트리(Leaf/Split)이고 다음 메서드가 구조적으로 동일하다:

| PaneNode | SurfaceGroupLayout |
|----------|-------------------|
| `compute_rects()` | `render_regions()` |
| `find_divider_at()` | `find_divider_at()` |
| `update_ratio_for_rect()` | `update_ratio_for_rect()` |
| `all_pane_ids()` | `all_surface_ids()` |
| `find_pane()` / `find_pane_mut()` | `find_terminal()` / `find_terminal_mut()` |
| `collect_dividers()` | `collect_dividers()` |
| `directional_focus()` | `directional_focus()` |

공통 `BinaryTree<LeafId, Leaf>` trait로 추출하면 ~250줄 중복 제거 가능.

**미착수 이유:** 리프 타입이 다르고(Pane vs SurfaceNode), 분할 간격도 다르며(PANE_BORDER_WIDTH vs SURFACE_BORDER_WIDTH), generic 도입 시 가독성이 오히려 나빠질 수 있다. 비용 대비 효과 검토 필요.

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

## 4. 미구현 설정

| 필드 | 상태 |
|------|------|
| `notification.sound` | UI 체크박스만 있고, 사운드 재생 미구현 |

---

## 5. 크레이트 분리 후보

현재 `src/` 내에 있지만 독립 크레이트로 추출할 수 있는 모듈:

| 모듈 | 근거 | 난이도 |
|------|------|--------|
| `model/` → `tasty-model` | `tasty-terminal` 외에 `use crate::` 없음 | 중 |
| `renderer/` → `tasty-renderer` | `font`, `model`, `selection`만 의존 | 중 |
| `notification.rs` | `model::Rect` 불필요, notify-rust만 의존 | 소 |

---

## 우선순위

| 순위 | 항목 | 효과 |
|------|------|------|
| P1 | notification.sound 구현 | 설정 UI와 동작 일치 |
| P2 | BinaryTree trait 추출 | ~250줄 중복 제거, 새 트리 타입 추가 용이 |
| P2 | 크레이트 분리 (model, renderer) | 빌드 병렬화, API 경계 명확화 |
| P3 | 멀티 서피스 렌더 최적화 | 10+ 서피스에서 성능 개선 |
| P3 | 다중 아틀라스 페이지 | CJK 집약 사용 시 성능 개선 |
