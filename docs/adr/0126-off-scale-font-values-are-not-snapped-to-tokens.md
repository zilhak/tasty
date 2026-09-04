# ADR-0126: 스케일 밖 폰트 값은 토큰으로 스냅하지 않는다 — `.5` 값은 토큰이 될 수 없다

- **Status**: Accepted
- **Date**: 2026-09-04
- **Tags**: theme, design-tokens, font-size, zoom, guards, adr-0033

## Context

UI 디자인 규칙은 "폰트 크기는 `Theme` 에서 가져온다"이고, `tests/design_token_adherence.rs`
가드가 `FontId::*`/`RichText::size` 의 숫자 리터럴을 CI 에서 막는다. 리터럴을 걷어내는
작업을 하다 보면 **대응 토큰이 없는 값**을 만난다 — 코드에서 자란 9.5 · 10.5 · 11.5 ·
12.5 · 13.5, DTCG primitive 에는 있으나 semantic role 이 없어 `Theme` 필드가 없는 12 ·
16, 어느 tier 에도 없는 30.

여기서 "가장 가까운 토큰으로 보내면 된다"는 유혹이 생긴다. 실제로 한 라운드에서 열 자리가
그렇게 스냅됐고, 그 변경은 "값이 바뀌는 치환은 하나도 없다"는 주장과 함께 올라왔다.
독립 리뷰가 소스 수준 전수 대조로 그 주장을 반증했다(±0.5 ~ ±1.0, 10 자리).

핵심은 실수 열 건이 아니라 **구조**다. 토큰 폰트 크기는
`Theme::with_colors_and_zoom` 의

```rust
let zoomed = |px: LogicalPx| LogicalPx((px.value() * ui_zoom).round());
```

를 거친다. `.round()` 가 있으므로 **폰트 토큰은 어떤 `ui_scale` 에서도 정수**다. 따라서
`.5` 로 끝나는 자리에 토큰을 넣는 것은 "zoom 1 에서만 0.5 차이" 가 아니라 **모든 배율에서
값이 다르다** — 값 보존 치환이 원리적으로 불가능하다.

값이 같은 경우에도 축이 하나 더 있다. 명명 const 는 `zoomed()` 경로 밖이라 `ui_scale` 을
타지 않고, 토큰은 탄다. 그래서 "zoom 1 에서 픽셀이 같다"는 것은 무변경의 증거가 아니다.

## Decision

**어느 tier 에도 없는 폰트 값은 토큰으로 스냅하지 않는다.** 사유를 적은 명명 const 로
올려 드리프트를 눈에 보이게 두고, 어느 토큰으로 스냅할지는 디자인 판단으로 넘긴다.
semantic 이 없는 primitive(12 · 16)를 쓰는 자리도 같게 다루되 const 이름에 primitive
임을 남긴다.

**단, 명명 const 가 허용되는 것은 값이 스케일 *밖*일 때뿐이다.** 값이 UI 폰트 토큰과
같은 const 는 토큰의 복사본이고, 복사본은 `zoomed()` 경로 밖이라 `ui_scale` 을 타지
않는다 — 그 자리는 zoom 1 에서만 같고 나머지 배율에서 갈라진다. 이 ADR 은 그런 자리를
허용하지 않는다.

이 경계는 처음부터 의도였지만 본문에 적혀 있지 않았고, 그 사이에 세 자리가
**0126 이 감싸던 자리**로 남아 있었다 — `src/adapters/ui/info_modal.rs` ·
`src/adapters/ui/popup/confirm_delete_category.rs` · `src/adapters/ui/popup/approval.rs`
의 `const BODY_FONT_SIZE: f32 = 13.0` 이다. 값이 `font_size_body`(13)와 같았으므로
"스케일 밖 값의 명명 const" 가 아니라 토큰의 복사본이었고, 셋 다 `font_size_body` 로
보냈다(const 는 제거). 같은 const 를 쓰던 popup sizer 의 높이 추정도 함께 토큰으로
보냈다 — 라벨만 zoom 을 타고 상자는 안 타면 배율에서 본문이 잘린다.

**`.5` 로 끝나는 값은 그중에서도 결론이 더 강하다 — 토큰이 될 수 없다.** 위 반올림
때문에 어떤 설정에서도 값이 달라지므로, 스냅은 언제나 픽셀 변경이다. 스냅하려면 그것을
"값을 바꾸는 디자인 결정" 으로 명시적으로 승인받아야 하고, 리터럴 정리 작업이 조용히
곁다리로 할 수 있는 일이 아니다.

규칙이 서 있는 전제(UI 폰트 토큰은 늘 정수)는 `crates/tasty-type-appearance/src/theme.rs`
의 `ui_font_size_tokens_are_integers_at_every_zoom` 가 `cargo test --workspace`(CI)로
잡는다. 전제가 깨지는 길 둘 — `zoomed()` 가 반올림을 그만두는 것, 새 UI 폰트 필드가
`zoomed()` 를 우회하는 것 — 이 그 테스트에 걸린다.

## Consequences

- **얻은 것**: 리터럴 정리가 픽셀을 바꾸지 않는다는 것을 규칙으로 보장한다. "토큰으로
  바꿨다" 와 "값을 바꿨다" 가 한 커밋 안에서 섞이지 않으므로, 리뷰가 값 대조 없이도
  치환의 안전성을 신뢰할 수 있다. 규칙의 전제가 테스트로 고정돼 문서가 코드에 없는
  성질을 보증하는 상태가 재발하지 않는다.
- **잃은 것**: 명명 const 자리는 `ui_scale` 줌을 타지 않는다. 배율 0.85 / 1.2 에서 그
  자리들만 고정 크기로 남는다 — 스냅 승인 전까지 그대로다.
- **운영 비용 / 유지 부담**: 스케일 밖 값이 나올 때마다 const 이름과 사유를 적어야 한다.
- **강제는 절반만 기계가 한다 — 어느 절반인지가 중요하다.** 기계가 가르는 것은
  **경계 위반**이다: 값이 UI 폰트 토큰과 같은 명명 const 가 폰트 자리에 오면
  `src/design_token_guard.rs` 의 `no_named_const_copies_a_ui_font_token` 이 잡는다
  (판별 축은 이름이 아니라 **값 + 위치**다 — `SOMETHING_ELSE = 13.0` 도 잡히고
  `PALETTE_HINT_FONT_SIZE = 10.5` 는 안 잡힌다).
  기계가 **못** 가르는 것은 그 다음 질문이다: 스케일 밖 값 하나하나가 *어느* 토큰으로
  수렴해야 하는지, 애초에 수렴해야 하는지는 디자인 판단이라 소스에 신호가 없다.
  그 절반은 사람이 지키는 규약이고, 이 ADR 이 그 규약의 본문이다.

## Alternatives Considered

- **A: 전부 가장 가까운 토큰으로 스냅하고 "디자인 스냅" 으로 문서화** — 값 통일과 zoom
  대응을 한 번에 얻는다. 안 고른 이유: 어느 값으로 수렴할지는 디자인 결정이고, 리터럴
  정리 커밋이 그 결정을 대신할 수 없다. 픽셀이 실제로 바뀌므로 "무변경 리팩터" 라는
  커밋의 성격도 무너진다.
- **B: 스케일 밖 값마다 semantic 토큰을 신설** — 근본 해결이지만 role 배정이 디자인
  작업이고, 12 · 16 처럼 primitive 는 있는데 semantic 이 없는 자리는 "어느 역할인가" 가
  먼저 정해져야 한다. 이 ADR 은 그 결정을 막지 않는다 — 정해지면 const 가 토큰으로 간다.
- **C: `zoomed()` 에서 폰트만 반올림을 빼 `.5` 를 보존** — `.5` 자리가 토큰이 될 수 있게
  된다. 안 고른 이유: 반올림은 서브픽셀 폰트 크기가 래스터라이저 캐시를 흩뜨리지 않게
  하는 의도적 선택이고, 열 자리를 위해 전 UI 의 폰트 스케일 정책을 바꾸는 것은 비용
  방향이 반대다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- `zoomed()` 의 반올림 정책이 바뀐다 (`ui_font_size_tokens_are_integers_at_every_zoom`
  가 먼저 실패한다).
- 디자인이 `.5` 스케일을 정식 tier 로 승인해 대응 semantic 토큰이 생긴다.
- 명명 const 가 zoom 을 안 타는 것이 실제 사용자 문제로 보고된다 — 그때는 A 나 B 중
  하나를 골라야 한다.

## References

- [`docs/design/systems/theme.md`](../design/systems/theme.md) — "스케일 밖 폰트 값" ·
  "`.5` 값은 토큰이 될 수 없다" 행
- [ADR-0033](0033-ui-color-semantic-role-only.md) — 색은 semantic role 접근자로만
  읽는다. 같은 축(값이 아니라 토큰을 경유한다)의 색 쪽 결정
- `tests/design_token_adherence.rs` — 폰트/선굵기/간격 리터럴 재유입 가드와 그 한계 목록
- `src/design_token_guard.rs` — 토큰 값을 복사한 명명 const 를 폰트 자리에서 막는 가드.
  관례(`tests/*.rs`)를 깨고 본체 crate 의 `#[cfg(test)]` 모듈에 둔 이유가 그 모듈 doc 에
  있다 — 통합 테스트는 컴파일만 자동으로 검사되고 실행은 수동 트리거라, 런타임에 소스를
  읽는 스캔 가드에게는 그 채널이 무의미하기 때문이다(채널 정본은
  [`docs/dev-guide/ci-gates.md`](../dev-guide/ci-gates.md))
