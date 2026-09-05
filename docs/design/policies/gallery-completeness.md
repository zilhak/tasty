# 갤러리 완전성 정책 (운영 상세)

> *왜* 이렇게 결정했는지(근거·대안·재검토 조건)는 [ADR-0020](../../adr/0020-gallery-complete-component-source.md). 본 문서는 결정의 *현재 운영 상태* 만 기술한다.

**갤러리(`crates/tasty-gallery`)는 본체의 모든 UI 컴포넌트를 노출한다. 어떤 컴포넌트도 갤러리에서 빠지지 않는다 — cut 금지.**

## 핵심 규칙

- 본체에 존재하는 **modal · popup · 공용 위젯 · 레이아웃 idiom** 은 빠짐없이 갤러리 카탈로그(`catalog.rs::pages()`)에 specimen 으로 등록한다.
- 디자인 산출물(`Tasty Design System`)의 gallery 페이지가 일부 컴포넌트를 카탈로그에서 **생략(cut)** 하더라도, 그걸 근거로 갤러리에서 제거하지 않는다. 생략은 디자인 측 결함으로 본다.
- 카탈로그 1차 분류는 디자인 gallery 의 5분류(**Foundations / Components / Icons / Overlays / Layouts**)에 더해, 플러그인 유래 specimen 을 네이티브와 분리해 모으는 **Plugins** 페이지와, 완결 조립 화면(위젯 아님) 단위 specimen 을 모으는 **Chrome** 페이지를 둔다(총 7분류). 플러그인이 제공하는 viewer/popup(clipboard · git · markdown · image · html)은 네이티브 섹션이 아니라 Plugins 아래에, 부팅 로딩 화면처럼 여러 위젯이 조립된 앱 크롬 단위 화면은 Chrome 아래에 등록한다.

## demo=main — 갤러리가 곧 본체

갤러리 specimen 은 본체와 **같은 view-only 함수**를 호출한다(mirror 아님). 따라서 갤러리에서 디자인과 맞으면 본체도 맞는다. 이 등가성이 갤러리를 "본체 전체의 거울"로 만들고, 그래서 **거울에 빠진 컴포넌트가 있으면 그만큼 본체에 검증 사각이 생긴다.** 완전성은 demo=main 의 가치를 지키기 위한 전제다.

- 위젯의 집·demo=main 구조: [shared-widgets](shared-widgets.md), [architecture/ui-widgets-crate](../../architecture/ui-widgets-crate.md).
- 본체 view 가 props 분리(view-only)돼 있어야 갤러리가 직접 호출할 수 있다: [dev-guide/model-view-split](../../dev-guide/model-view-split.md). 분리가 안 된 컴포넌트는 분리가 선행 과제이며, 그 전까지도 갤러리에서 빼지 않고 시각 복제 specimen 으로 둔다(본체 의존 0, 로컬 mock props).
- 본체 binary(`tasty`) 안에만 있는 view 는 props 가 분리돼 있어도 갤러리가 **호출**할 수는 없다 — 물리적 위치를 `crates/tasty-ui-widgets` 로 옮겨야 같은 함수 호출이 성립한다. 끌어올릴 때 crate 로 넘기지 않는 것(Area/z-order · i18n · 글로벌 `theme()`)과 선례는 [gallery-first §이미 본체에만 있는 view 를 갤러리로 끌어올릴 때](../../dev-guide/gallery-first.md).

## 미러가 불가피한 곳은 기계로 대조한다

`demo=main` 이 성립하지 않는 자리가 하나 있다 — 정본 타입이 갤러리에 넣기엔 무거운
크레이트에 있을 때다. `ToastKind` 가 그렇다: 정본은 `tasty-model` 에 있는데 그
크레이트는 termwiz/터미널 모델까지 끌고 오므로 갤러리 산출물에 넣지 않고, 같은 모양의
enum 을 `catalog::toast_card` 에 하나 더 둔다.

미러는 갈린다. 실제로 갤러리에만 있던 `ToastKind::Agent` 가 **본체가 만들 수 없는
토스트**를 카탈로그에 전시한 적이 있다 — 완전성이 반대 방향으로 깨진 형태다(빠진 게
아니라 없는 것을 보여줬다). 그래서 미러를 두는 자리는 **양방향 대조를 기계로 고정한다**:
`crates/tasty-gallery/src/catalog/toast_card.rs` 의 `mod tests` 가 두 `ToastKind::ALL` 을
런타임에 열거해, 어느 한쪽에만 변종이 있으면 실패한다. 정본 크레이트는 그 테스트의
`dev-dependencies` 로만 들어가므로 갤러리 산출물의 의존은 그대로다.

그 대조는 통합 테스트가 아니라 **lib 유닛 테스트**로 둔다. 통합 테스트(`tests/*.rs`)는
헤드리스 조합 하나에서만 실행되고 기본 조합의 자동 잡은 `--lib --bins` 라 못 보는데, 이
가드의 본체는 런타임 열거라 실행되지 않으면 아무것도 보지 않기 때문이다 — 어느 한쪽에만
변종을 더해도 컴파일은 통과한다.
lib 에 두면 `--lib --bins` 자동 잡에서 함께 실행된다([dev-guide/ci-gates](../../dev-guide/ci-gates.md)
가 채널 정본). 채널이 있다는 것이 그 채널이 지금 초록이라는 뜻은 아니므로, 자동 채널을
근거로 로컬 확인을 건너뛰지 마라. 같은 이유로, 미러 대조를 새로 만들 때도 `tests/` 로
내리지 마라.

새로 미러를 만들어야 하면 같은 형태의 대조를 함께 만든다 — 미러 자체보다 **미러가
갈렸을 때 아무도 모르는 것**이 문제다.

그리고 **그 미러를 소비하는 쪽까지 함께 본다.** 열거를 대조해도 그 열거를 받아 색·치수로
가는 매핑이 갈리면 화면 말고는 신호가 없고, 가드가 하나 있으면 그 옆도 덮인 것처럼 읽힌다.
`toast_card` 의 `accent_color` 가 그 자리다 — 열거 대조는 있는데 매핑은 아무것도 안 봤다.
지금은 `src/source_guards/gallery_copied_rules.rs` 가 (갈래, 부르는 이름) 짝을 본체와 맞춘다.
값(치수) 쪽 대응은 `gallery_copied_dimensions.rs` 다.

## 디자인이 cut 했을 때 — 소스가 아니라 디자인을 고친다

디자인 카탈로그가 본체 컴포넌트를 누락하면:

1. 갤러리 소스에서 그 컴포넌트를 **빼지 않는다.**
2. 누락분을 디자인에 다시 포함하도록 [디자인 변경 워크플로](../../dev-guide/design-change-workflow.md)에 따라 디자인 요청문서를 작성한다.
3. 갱신된 디자인을 받은 뒤 그 기준으로 갤러리 specimen 을 정합한다.

즉 디자인↔갤러리 항목 불일치는 **항상 디자인 측을 보강해서** 해소한다.

## 관련

- [ADR-0020](../../adr/0020-gallery-complete-component-source.md) — 결정 근거.
- [dev-guide/gallery-first](../../dev-guide/gallery-first.md) — 새 컴포넌트는 디자인→갤러리→본체 순서.
- [design/systems/design-gallery-mapping](../systems/design-gallery-mapping.md) — 디자인 jsx ↔ 갤러리 항목 ↔ 본체 함수 3자 매핑.
- [shared-widgets](shared-widgets.md) — 보편 컴포넌트는 공용 위젯으로(완전성의 부품 단위 기반).
