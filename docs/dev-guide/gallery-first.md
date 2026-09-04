# Gallery-first — 새 UI 컴포넌트 추가 순서

**새 modal · popup · 공용 위젯은 본체에 넣기 전에 갤러리(`crates/tasty-gallery`)에 먼저 만든다.** 갤러리는 본체의 모든 UI 컴포넌트를 노출하는 단일 출처이고([gallery-completeness](../design/policies/gallery-completeness.md), [ADR-0020](../adr/0020-gallery-complete-component-source.md)), 컴포넌트는 그 출처를 *거쳐서* 본체로 들어온다.

## 순서 (필수)

### 0. 디자인 확보 (acquisition)
디자인에 없는 새 요소(디자인에 없는 컴포넌트를 추가하거나, 디자인과 다른 형태로 바꾸는 것)라면 **소스부터 고치지 않는다.** [디자인 변경 워크플로](design-change-workflow.md)대로 디자인 요청문서를 작성해 claude design 에게서 디자인을 먼저 받는다. (이 단계는 디자인을 *확보*하는 것이고, 받은 디자인을 소스 구조로 옮기는 *구조 전사*는 1 단계다 — 아래 참고.)

> 디자인에 *이미 있는데* 소스만 못 따라간 경우(구현 누락/불일치)는 디자인 변경 불필요 — 1 단계로 바로 간다.

### 1. 갤러리 specimen 먼저
받은 디자인으로 갤러리에 specimen 을 만든다: `catalog/{components,widgets}/<name>.rs` 의 `draw(ui, &Theme)` + `catalog.rs::pages()` 의 해당 페이지에 `section(...)`/`spec(...)` 등록. 정합은 **두 개의 독립 축**으로 맞춘다:

- **토큰 정합**: 색·간격·치수·보더는 모두 Theme 토큰에서 가져온다([theme UI 규칙](../design/systems/theme.md#ui-디자인-규칙-필수)). 보편 이름이 붙는 부품(버튼/입력/표 등)은 [공용 위젯](../design/policies/shared-widgets.md)을 호출한다.
- **구조 전사(structural transcription)**: 디자인의 **레이아웃 구조**(grid·컬럼·패딩·정렬·요소 경계)를 egui 소스에 **1:1 전사**한다 — egui flow 로 눈대중 흉내 내지 않는다. 토큰만 맞고 구조가 어긋나면 specimen 이 드리프트한다(전사 절차·함정은 [`design-parity-notes`](../design/systems/design-parity-notes.md) 의 "구조 전사" 원칙, 매핑은 [`design-gallery-mapping`](../design/systems/design-gallery-mapping.md)).

### 2. 본체 반영
그 후 본체 앱에 넣는다(팝업이면 [popup-implementation](popup-implementation.md) 의 `PopupDef` 3단계). demo=main 이므로 본체와 갤러리가 **같은 view-only 함수**를 호출하도록 props 를 분리한다([model-view-split](model-view-split.md)). 새로 그리지 말고 1 단계에서 만든 함수를 본체에서 호출한다.

## 이미 본체에만 있는 view 를 갤러리로 끌어올릴 때

gallery-first 이전에 만들어져 본체 binary 에만 있는 view 는 갤러리가 복제할 수밖에 없다. 복제를 없애려면 **view 함수를 `crates/tasty-ui-widgets` 로 옮겨** 본체 wrapper 와 갤러리 specimen 이 같은 함수를 호출하게 한다. 이때 crate 로 **넘기지 않는 것**이 정해져 있다:

- **`egui::Area` / `LayerId`** — 부유 배치와 z-order 는 본체 정책이다. crate view 는 넘겨받은 `egui::Ui` 안에 크기를 할당하고 **그 rect 기준**으로만 그린다(화면 절대 좌표 사용 금지 — 갤러리 카드는 임의 위치에 있다).
- **i18n** — 위젯 crate 는 `tasty-i18n` 을 의존하지 않는다. 라벨·tooltip 은 props 필드로 올려 본체 wrapper 가 주입한다. 그리기와 폭 계산이 **같은 필드**를 읽게 해서 문자열이 두 곳에서 독립 조립되지 않게 한다.
- **글로벌 `theme()` 접근** — crate 함수는 항상 `&Theme` 을 명시적으로 받는다.

선례: StatusBar(`crates/tasty-ui-widgets/src/status_bar.rs` ↔ `src/adapters/ui/status_bar.rs` ↔ `catalog/components/status_bar.rs`).

## 왜 이 순서인가

- **검증을 먼저 세운다**: 갤러리는 본체 앱을 다 띄우지 않고 컴포넌트 하나만 격리 렌더한다. 디자인 정합을 빠르게 반복할 검증대를 본체 배선보다 먼저 갖는다.
- **완전성이 절차로 보장된다**: 컴포넌트가 갤러리를 거쳐야만 본체로 들어오므로, "본체엔 있는데 갤러리엔 없는" 누락이 구조적으로 안 생긴다([gallery-completeness](../design/policies/gallery-completeness.md)).
- **demo=main 이 공짜로 유지된다**: 갤러리용으로 분리한 view-only 함수를 본체가 그대로 호출하므로 거울이 자동으로 맞는다.

## 관련

- [design-change-workflow](design-change-workflow.md) — 0단계 "디자인 확보"의 전체 절차(요청문서→시안→정합 루프).
- [ADR-0020](../adr/0020-gallery-complete-component-source.md) — cut 금지 + gallery-first 결정 근거.
- [design/policies/gallery-completeness](../design/policies/gallery-completeness.md) — 갤러리 완전성 운영 상태.
- [popup-implementation](popup-implementation.md) — 본체 팝업 추가(`PopupDef`).
- [design/policies/shared-widgets](../design/policies/shared-widgets.md) · [model-view-split](model-view-split.md) — 부품 단위 공용화 + view 분리.
- [design/systems/design-gallery-mapping §공용 crate view specimen](../design/systems/design-gallery-mapping.md) — 이미 이 경로로 올라간 항목 목록.
