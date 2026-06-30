# ADR-0020: 갤러리는 본체 UI 컴포넌트의 완전한 단일 출처 — cut 금지, gallery-first

- **Status**: Accepted
- **Date**: 2026-06-24
- **Tags**: gallery, design-parity, demo-main, component-catalog, workflow, ui

## Context

`crates/tasty-gallery` 는 본체 UI 컴포넌트(modal/popup/공용 위젯/레이아웃 idiom)를 격리 렌더해 디자인 정합·토큰·시각을 검증하는 도구다. demo=main 원칙([shared-widgets](../design/policies/shared-widgets.md))상 갤러리 specimen 은 본체와 같은 view-only 함수를 호출하므로 *갤러리에서 맞으면 본체에서 맞는다*.

claude design 산출물(`Tasty Design System`)의 gallery 페이지는 컴포넌트 카탈로그의 디자인 기준이다. 그런데 디자인 (3) 의 gallery 재구성은 README "What was cut" 에 명시된 대로 본체에 실재하는 다수 컴포넌트(Convert · Port Scanner 통팝업 · File Handler Picker · Apply Preset · Toast Stack · Markdown Open · Update · Search Bar · Tools Menu · Divider · Multi-tier Tab · hint_text)를 카탈로그에서 **의도적으로 제거(cut)** 했다.

문제: 갤러리가 컴포넌트를 누락하면 그 컴포넌트는 디자인 정합·토큰·시각을 검증할 단일 경로를 잃는다. "대표만 보여주는" 카탈로그는 빠진 컴포넌트가 본체에서 디자인과 어긋나도 드러나지 않는 **검증 사각**을 만든다.

## Decision

**① 갤러리는 본체의 모든 UI 컴포넌트를 노출한다(cut 금지).** 본체에 존재하는 modal/popup/공용 위젯/레이아웃 idiom 은 빠짐없이 갤러리 카탈로그에 specimen 으로 등록한다. 디자인 산출물이 일부 컴포넌트를 카탈로그에서 생략하더라도, 그것을 근거로 갤러리에서 제거하지 않는다 — 생략은 디자인 측 결함으로 보고 디자인 request 로 보강한다.

**② 새 UI 컴포넌트는 gallery-first 로 들어온다.** 새 modal/popup/공용 위젯을 본체에 넣기 전에 (a) 디자인을 먼저 받고 (b) 갤러리에 specimen 을 먼저 만들어 토큰·치수를 맞춘 뒤 (c) 본체에 반영한다. 절차는 [dev-guide/gallery-first](../dev-guide/gallery-first.md).

## Consequences

- **얻은 것**: 갤러리 카탈로그 = 본체 컴포넌트 전수. 모든 컴포넌트가 디자인 정합·토큰 검증의 단일 경로를 갖는다. 디자인이 cut 해도 검증 사각이 생기지 않는다.
- **잃은 것**: 디자인 카탈로그와 갤러리 항목 집합이 어긋날 수 있다(디자인이 cut 한 경우). 이 어긋남은 "갤러리에서 빼서" 가 아니라 "디자인에 다시 넣어달라" 로만 해소한다.
- **운영 비용 / 유지 부담**: 본체에 컴포넌트를 추가할 때마다 갤러리 specimen 도 함께 유지해야 한다(gallery-first 가 이를 절차로 강제). 디자인이 cut 하면 디자인 request 왕복 비용이 든다 — 본 ADR 제정 직후의 `2026-06-24-gallery-completeness` request 가 첫 적용 사례.

## Alternatives Considered

- **A. 디자인 카탈로그를 100% 추종(cut 수용)**: 갤러리를 디자인 4페이지에 맞춰 cut 한 컴포넌트를 제거. — 빠진 컴포넌트의 검증 경로가 사라져 검증 사각이 생긴다. demo=main 의 가치(갤러리가 본체 전체의 거울)를 훼손하므로 기각.
- **B. "대표 컴포넌트만" 노출**: 핵심 위젯만 갤러리에 두고 나머지는 생략. — "대표" 의 기준이 임의적이고, 생략된 컴포넌트가 조용히 디자인과 어긋난다. 기각.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- 갤러리 specimen 유지 비용이 그 컴포넌트의 디자인 정합 가치를 명백히 넘어서는 컴포넌트 부류가 생길 때.
- demo=main 구조가 폐기되어 갤러리가 더 이상 본체의 거울이 아니게 될 때.

## References

- [design/policies/gallery-completeness](../design/policies/gallery-completeness.md) — 운영 상태(이 ADR 의 결정을 현재 어떻게 운영하나).
- [dev-guide/gallery-first](../dev-guide/gallery-first.md) — gallery-first 워크플로 절차.
- [design/policies/shared-widgets](../design/policies/shared-widgets.md) · [dev-guide/model-view-split](../dev-guide/model-view-split.md) — demo=main 의 기반.
- [design/systems/design-gallery-mapping](../design/systems/design-gallery-mapping.md) — 디자인↔갤러리↔본체 3자 매핑.
