# ADR-0342: 셀 렌더러는 크레이트를 직접 부르고, 본체 재수출을 거치지 않는다

- **Status**: Accepted
- **Date**: 2026-09-20
- **Tags**: architecture, layering, renderer, selection, terminal-link, naming, headless, adr-0319, adr-0324

## Context

선택·링크 타입은 이미 잎 크레이트로 내려가 있다 — 선택은 `tasty-selection`
([ADR-0319](0319-the-selection-model-drops-the-stale-gui-gate-instead-of-carrying-it.md)),
링크 검출은 `tasty-terminal-link`
([ADR-0324](0324-link-detection-and-link-opening-split-by-side-effect.md)). 두 결정은
"렌더러가 앱 상태 모듈이 아니라 크레이트를 본다" 를 얻은 것으로 적었다.

**타입은 그렇게 됐는데 표기는 안 그랬다.** 두 결정 모두 호출부를 안 건드리려고 본체에
이름을 이어 뒀고(`src/state.rs` 의 `pub use tasty_selection as selection;` ·
`src/adapters/ui/terminal_link.rs` 의 `pub use tasty_terminal_link::*;`), 크레이트
루트가 그것을 다시 `crate::selection` · `crate::terminal_link` 로 재수출한다. 그래서
셀 렌더러가 소스에 적고 있던 것은 여전히

```rust
use crate::selection::{NormalizedSelection, SelectionPoint};   // = state::selection
use crate::terminal_link::LinkHighlight;                       // = adapters::ui::terminal_link
```

였다. 컴파일되는 타입은 크레이트의 것이지만, **소스를 읽어서는 렌더러가 앱 상태와 UI
어댑터를 보는 것으로 읽힌다** — 그리고 그 두 재수출 중 하나가 사라지면 렌더러가 깨진다.
의존 방향을 재는 사람도 도구도 표기를 본다.

같은 형태가 폭 표(`tasty-cell-width`)·글리프 아틀라스(`tasty-font`)·사각형
(`tasty-model`)에도 있었다. 셋 다 본체 모듈 파일 한 줄짜리 재수출을 거치고 있었다.

## Decision

`src/gfx/renderer.rs` 와 `src/gfx/renderer/` 는 **워크스페이스 크레이트를 직접 이름으로
부른다** — `tasty_selection` · `tasty_terminal_link` · `tasty_cell_width` ·
`tasty_font` · `tasty_model`. 본체의 `crate::selection` · `crate::terminal_link` ·
`crate::font` · `crate::model` 재수출은 **렌더러 안에서 쓰지 않는다.** 타입도 동작도
바뀌지 않는다 — 같은 타입을 원래 이름으로 부를 뿐이다.

렌더 입력 타입의 표기는 호출부와 같아야 뜻이 하나로 남으므로, 그 두 타입
(`tasty_selection::*` · `tasty_terminal_link::LinkHighlight`)에 한해 `src/gfx/gpu.rs` ·
`src/gfx/gpu/render_pass.rs` 의 시그니처도 함께 옮긴다. 그 둘의 나머지 본체 참조
(`crate::state` · `crate::core` · `crate::search_state` · `crate::model`)는 **그대로
둔다** — `gpu` 는 `AppState` · `CoreState` · `PluginManager` 를 받는 호스트 접착층이고,
그 의존은 거짓이 아니라 사실이다.

그 결과 렌더러가 부르는 본체 경로는 **`crate::cell_palette` 하나**로 남는다. 그것은
지우지 않는다 — 셀 색 해석은 `gui` 게이트 밖에 있어야 하고(헤드리스
`debug.glyph_color` 핸들러가 같은 함수로 답한다), 새 크레이트를 만들지 않는 한 렌더러와
헤드리스 핸들러가 공유할 자리는 본체 모듈뿐이다.

## Consequences

- **얻은 것**: 렌더러의 의존이 **표기에서도** 크레이트다. `grep -rn 'crate::'
  src/gfx/renderer.rs src/gfx/renderer/` 가 두 줄(둘 다 `cell_palette`)이고, 그 두 줄이
  곧 "렌더러를 본체 밖으로 옮기려면 무엇이 남았는가" 의 전량이다 — 전에는 그 물음에
  답하려면 재수출 사슬을 넷 따라가야 했다.
- **잃은 것**: 본체 재수출 이름과 렌더러의 이름이 **갈렸다.** `state::selection` 을 다른
  크레이트로 갈아끼우면 렌더러는 따라오지 않는다 — 그것이 이 결정이 의도한 바지만,
  "한 곳만 고치면 전부 따라온다" 는 성질은 그 자리에서 사라진다.
- **운영 비용 / 유지 부담**: 없다. 새 타입·새 인자·새 크레이트·새 feature 가 없고,
  `Cargo.toml` 도 안 바뀐다(다섯 크레이트 전부 이미 무조건 의존이다).

## Alternatives Considered

- **A: 그대로 둔다 — 타입이 이미 크레이트니 표기는 취향이다** — 컴파일 결과는 같지만
  의존 방향을 재는 모든 수단(사람의 grep, 이동 계획, 재수출 삭제)이 표기를 본다.
  ADR-0319 가 "얻은 것" 으로 적은 문장이 소스에서 확인되지 않는 상태가 남는다. 기각.
- **B: `cell_palette` 까지 잎 크레이트로 내려 렌더러의 본체 참조를 0 으로 만든다** —
  가장 깨끗하지만 새 크레이트가 필요하고, 크레이트 수 lockstep 자리를 함께 움직인다.
  이 회차에서 크레이트 수를 바꾸는 lane 은 따로 정해져 있어 동시에 손대면 같은 네 자리를
  두 곳에서 고치게 된다. 보류 — 좌변이 한 모듈뿐이라 나중에 해도 비용이 같다.
- **C: 반대로 본체 재수출을 지우고 모든 호출부를 크레이트 이름으로 바꾼다** — 렌더러 밖
  호출부가 열 자리가 넘고 그중 다수가 이 회차의 다른 소유 경로다. 이 결정이 묻는 것은
  "렌더러가 무엇을 보는가" 하나라 거기까지 넓힐 이유가 없다. 기각.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- `crate::cell_palette` 가 렌더러에서 사라진다 — 대안 B 가 실행됐다는 뜻이고, 그러면 이
  ADR 이 남긴 "하나" 라는 수가 0 이 된다. 그때 이 문서의 Decision 마지막 문단을 다시 본다.
- 본체가 `tasty-selection` · `tasty-terminal-link` 를 optional 의존으로 바꾼다. 그러면
  렌더러가 직접 부르는 이름이 feature 뒤로 들어가고, 재수출을 거치던 때와 게이트가 달라진다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- 렌더러가 다시 본체 모듈을 이름으로 부르게 되는가. **이 사실을 재는 자동 채널은 없다** —
  새 `use crate::…` 한 줄은 컴파일되고 아무 시험도 안 깬다. 재는 법:
  `grep -rn 'crate::' src/gfx/renderer.rs src/gfx/renderer/` 가 `cell_palette` 두 줄
  말고 무엇을 더 내는지 본다. 이 회차는 새 가드를 만들지 않기로 해서 붙이지 않았다.
- 표기를 갈라 둔 값이 실제로 있었는가 — 즉 렌더러를 본체 밖으로 옮기는 작업이 이 결정
  덕에 짧아졌는가. 재는 법: 그 이동을 실제로 시도할 때 렌더러 쪽에서 고칠 `use` 줄이
  몇이었는지 세어, 이 ADR 이 말한 "두 줄" 과 대조한다.

## References

- 타입 소속을 내린 두 결정: [ADR-0319](0319-the-selection-model-drops-the-stale-gui-gate-instead-of-carrying-it.md) ·
  [ADR-0324](0324-link-detection-and-link-opening-split-by-side-effect.md)
- 크레이트 분할이 의존 방향을 따른다는 결정: [ADR-0089](0089-crate-split-follows-dependency-direction.md)
- 코드 근거(결정이 실현된 **현재 위치**): `src/gfx/renderer.rs` 머리 주석과 그 모듈의
  `use` 목록, `src/cell_palette.rs` 의 `compute_cell_colors`
- 렌더 구조: [gpu-rendering](../dev-guide/gpu-rendering.md)
