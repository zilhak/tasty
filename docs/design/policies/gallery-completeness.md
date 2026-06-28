# 갤러리 완전성 정책 (운영 상세)

> *왜* 이렇게 결정했는지(근거·대안·재검토 조건)는 [ADR-0020](../../adr/0020-gallery-complete-component-source.md). 본 문서는 결정의 *현재 운영 상태* 만 기술한다.

**갤러리(`crates/tasty-gallery`)는 본체의 모든 UI 컴포넌트를 노출한다. 어떤 컴포넌트도 갤러리에서 빠지지 않는다 — cut 금지.**

## 핵심 규칙

- 본체에 존재하는 **modal · popup · 공용 위젯 · 레이아웃 idiom** 은 빠짐없이 갤러리 카탈로그(`catalog.rs::all()`)에 specimen 으로 등록한다.
- 디자인 산출물(`Tasty Design System`)의 gallery 페이지가 일부 컴포넌트를 카탈로그에서 **생략(cut)** 하더라도, 그걸 근거로 갤러리에서 제거하지 않는다. 생략은 디자인 측 결함으로 본다.
- 카탈로그 1차 분류는 디자인 gallery 의 5분류(**Foundations / Components / Icons / Overlays / Layouts**)에 더해, 플러그인 유래 specimen 을 네이티브와 분리해 모으는 **Plugins** 페이지를 둔다(총 6분류). 플러그인이 제공하는 viewer/popup(clipboard · git · markdown · image · html)은 네이티브 섹션이 아니라 Plugins 아래에 등록한다.

## demo=main — 갤러리가 곧 본체

갤러리 specimen 은 본체와 **같은 view-only 함수**를 호출한다(mirror 아님). 따라서 갤러리에서 디자인과 맞으면 본체도 맞는다. 이 등가성이 갤러리를 "본체 전체의 거울"로 만들고, 그래서 **거울에 빠진 컴포넌트가 있으면 그만큼 본체에 검증 사각이 생긴다.** 완전성은 demo=main 의 가치를 지키기 위한 전제다.

- 위젯의 집·demo=main 구조: [shared-widgets](shared-widgets.md), [architecture/ui-widgets-crate](../../architecture/ui-widgets-crate.md).
- 본체 view 가 props 분리(view-only)돼 있어야 갤러리가 직접 호출할 수 있다: [dev-guide/model-view-split](../../dev-guide/model-view-split.md). 분리가 안 된 컴포넌트는 분리가 선행 과제이며, 그 전까지도 갤러리에서 빼지 않고 시각 복제 specimen 으로 둔다(본체 의존 0, 로컬 mock props).

## 디자인이 cut 했을 때 — 소스가 아니라 디자인을 고친다

디자인 카탈로그가 본체 컴포넌트를 누락하면:

1. 갤러리 소스에서 그 컴포넌트를 **빼지 않는다.**
2. 누락분을 디자인에 다시 포함하도록 [디자인 request](../../../.claude-workspace/design-request/) 를 작성한다(`.claude/CLAUDE.md` 디자인 변경 워크플로).
3. 갱신된 디자인을 받은 뒤 그 기준으로 갤러리 specimen 을 정합한다.

즉 디자인↔갤러리 항목 불일치는 **항상 디자인 측을 보강해서** 해소한다.

## 관련

- [ADR-0020](../../adr/0020-gallery-complete-component-source.md) — 결정 근거.
- [dev-guide/gallery-first](../../dev-guide/gallery-first.md) — 새 컴포넌트는 디자인→갤러리→본체 순서.
- [design/systems/design-gallery-mapping](../systems/design-gallery-mapping.md) — 디자인 jsx ↔ 갤러리 항목 ↔ 본체 함수 3자 매핑.
- [shared-widgets](shared-widgets.md) — 보편 컴포넌트는 공용 위젯으로(완전성의 부품 단위 기반).
