# ADR-0343: OS 경계는 폴더가 아니라 크레이트다 — 본체는 별칭으로 부른다

- **Status**: Accepted
- **Date**: 2026-09-20
- **Tags**: architecture, platform, crate-split, layering, features, headless, cross-platform, adr-0331

## Context

[ADR-0331](0331-the-platform-folder-holds-the-os-call-not-the-app-meaning.md) 이
플랫폼 폴더의 계약을 문장으로 정했다 — **OS 를 부르는 코드만 담고, 그 신호가 App 에서
무엇이 되는지는 부르는 쪽이 정한다.** 그 ADR 의 Alternative A 가 "폴더 전체를 크레이트로
뗀다" 였고, 안 고른 이유로 적힌 것은 배제가 아니라 **순서**였다: 크레이트 하나가 lockstep
네 자리를 끌고 오므로 무엇을 떼는지부터 정해야 한다는 것.

그 결정이 착지한 뒤 폴더를 다시 재면 이렇다(좌변은 이 트리의 실측이다).

```
$ find <플랫폼 폴더> -name '*.rs' | wc -l                       → 16
$ grep -rn 'crate::' <플랫폼 폴더>                              → 26 줄 / 9 파일
$ grep -rn 'crate::app\|AppEvent\|AppState' <플랫폼 폴더>       → 0
```

`crate::` 26 줄이 무엇을 보는지 전수로 갈라 보면 **본체 모듈은 하나도 없다.**

| 보는 것 | 실체 | 줄 |
|---|---|---|
| `crate::i18n` | `tasty-i18n` 의 재수출(`src/i18n.rs` 는 `pub use tasty_i18n::…` 다) | 3 |
| `crate::settings` | `tasty-settings`(`lib.rs` 의 `pub use`) | 8 |
| `crate::paths` · `crate::poison` | `tasty-utils`(`lib.rs` 의 `pub use`) | 5 |
| `crate::test_support` | `tasty-test-support`(dev-dependency) | 4 |
| 형제 플랫폼 모듈 | 같은 폴더 안 | 5 |
| `crate::shortcuts` | 산문 안의 intra-doc 링크 한 줄 (ADR-0331 §3 이 "크레이트가 갈릴 때 함께 푼다" 로 미뤄 둔 자리) | 1 |

즉 **본체에 남을 이유가 있는 파일이 0 이다.** 폴더가 본체 크레이트 안에 있다는 사실만으로
`crate::` 한 줄이면 App 상태에 닿을 수 있고, 계약을 지키는지는 사람이 매번 다시 읽어야
한다. ADR-0331 이 Alternative B 에서 이미 적었듯 그 판정을 텍스트 게이트로 대신하려는
시도는 오탐을 낸다.

## Decision

**플랫폼 폴더 전체를 `tasty-platform` 크레이트로 뺀다. 16 파일 전부이고, 본체에 남기는
파일은 없다.** 계약은 이제 문장이 아니라 **크레이트 경계**가 강제한다 — `tasty-platform`
의 `Cargo.toml` 에 본체가 없으므로 `AppEvent`·`AppState` 를 부르는 것이 컴파일되지 않는다.

**호출부는 한 줄도 안 바꾼다.** 본체 `src/lib.rs` 가 `mod platform;` 대신
`pub(crate) use tasty_platform as platform;` 를 걸어, `crate::platform::native_menu::…`
같은 기존 경로가 그대로 풀린다. 루트 별칭(`crate::crash_report` · `crate::system_tray` 등)
여덟도 `platform::` 을 거치므로 함께 유지된다. 이 회차의 기본값이 **행동 보존**이라
경계를 옮기는 것과 호출 형태를 바꾸는 것을 같은 커밋에 넣지 않는다.

**`gui` feature 는 새 크레이트에도 같은 이름으로 둔다.** 본체의 `gui` 가
`tasty-platform/gui` 를 켜고, 그 뒤에 winit·png·tray-icon·GTK·AppKit 이 있다. headless
빌드가 이 크레이트에서 컴파일하는 것은 `crash_report` 하나다 — 그 모듈만 feature 밖이다.

**크레이트의 절 소속은 "OS 경계" 를 새로 만들어 UI primitive 뒤에 둔다.** 네이티브 메뉴·
트레이는 UI 지만 egui 를 한 줄도 안 쓰고 갤러리 소비자가 없어 그 절의 규칙에 안 맞는다.

## Consequences

- **얻은 것**: ADR-0331 의 계약에 채널이 생겼다. 그 결정이 "게이트로는 못 잰다" 고 적은
  것을 컴파일러가 잰다 — 본체 타입을 부르면 `unresolved import` 다.
  본체 `src/` 의 `.rs` 가 **623 → 606**(−17: 모듈 16 + 모듈 선언 파일 하나)이고,
  크레이트 수가 **59 → 60** 이다. 새 크레이트의 `.rs` 는 **17** 개다.
- **잃은 것**: lockstep 자리가 넷 늘었다(아키텍처 문서의 개요 수·절 제목 수, README 둘의
  배지와 본문, 루트 `CLAUDE.md` 의 복제 문장). 판정기 둘이 그 넷을 본다 —
  `architecture_crate_list_complete` 와 `readme_badge_parity`.
  그리고 `macos_permissions` 의 항목 열여덟이 `pub(crate)` 에서 `pub` 이 됐다. 크레이트
  밖에서 부르려면 그래야 하고, 그만큼 공개 표면이 넓어졌다.
- **운영 비용 / 유지 부담**: 플랫폼 코드가 본체 타입을 쓰려면 이제 **그 타입을 leaf 로
  내리거나 콜백으로 받아야** 한다. ADR-0331 이 콜백을 규칙으로 정했으므로 그 비용은 새로
  생긴 것이 아니라 강제된 것이다. 그리고 새 OS 의존을 더할 때 만질 매니페스트가 둘이다.

## Alternatives Considered

- **A: 폴더를 그대로 두고 텍스트 게이트로 `crate::app` 참조를 막는다** — ADR-0331 의
  Alternative B 와 같다. 안 고른 이유도 같다: 형제 모듈의 루트 별칭이 같은 접두사를 가져
  좌변이 오탐을 내고, 오탐을 피하려면 결국 사람이 판정한다.
- **B: 일부만 뗀다 — `crate::` 가 0 인 파일만** — 가장 싸다. 안 고른 이유는 그 기준으로
  남는 것이 `i18n`·`settings`·`utils` 재수출을 보는 파일들뿐인데 **셋 다 이미 leaf
  크레이트**라, 남기는 근거가 "이름이 `crate::` 로 시작한다" 밖에 없다. 경계가 실제
  의존이 아니라 표기를 따라 그어진다.
- **C: 호출부를 전부 `tasty_platform::…` 로 고쳐 별칭을 없앤다** — 별칭 한 줄이 안 남아
  더 깨끗하다. 안 고른 이유는 이 회차가 **행동 보존** 회차이고, 호출부 32 자리를 같은
  커밋에서 고치면 경계 이동과 이름 변경이 한 diff 에 섞여 회귀의 출처를 가를 수 없기
  때문이다. 별칭은 다른 leaf 재수출 여덟과 같은 형태라 새 관행도 아니다.
- **D: 크레이트를 UI primitive 절에 넣는다** — 절을 새로 안 만들어도 된다. 안 고른 이유는
  그 절의 정의가 "본체·갤러리가 공유하는 egui 위젯" 이고 이 크레이트는 egui 를 안 쓰며
  갤러리 소비자가 없기 때문이다. 소속이 틀리면 절 순서 판정의 뜻도 흐려진다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- `crates/tasty-platform/Cargo.toml` 의 `[dependencies]` 에 leaf 가 아닌 워크스페이스
  크레이트가 들어온다. 이 결정이 산 것이 "본체 타입에 못 닿는다" 이고, 그 성질은 의존
  목록으로만 유지된다. 지금은 `tasty-i18n` · `tasty-settings` · `tasty-utils` 셋이다.
  재는 법: `grep -n '^tasty-' crates/tasty-platform/Cargo.toml`.
- `src/lib.rs` 에서 `pub(crate) use tasty_platform as platform;` 가 사라진다. 그 줄이
  사라지는 경우는 둘뿐이다 — 호출부를 전부 고쳐 Alternative C 로 갔거나, 크레이트를
  되돌렸거나. 어느 쪽이든 이 결정의 전제가 바뀐 것이다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- **macOS·Windows 에서 이 크레이트가 실제로 컴파일되는가.** 이 결정을 내린 트리에는 macOS
  타깃을 컴파일하는 채널이 없다(clang 부재). 특히 macOS 전용 의존의 **feature 목록**은
  본체 매니페스트에서 베낀 값이라, 이 크레이트가 단독으로 그 목록만으로 서는지는
  미측정이다. 재는 법: clang 이 있는 머신에서
  `cargo check -p tasty-platform --features gui --target aarch64-apple-darwin`,
  그리고 `cargo check -p tasty-platform --features gui --target x86_64-pc-windows-msvc`.
- **`pub` 으로 넓어진 표면이 실제로 오용되는가.** `macos_permissions` 의 항목이 크레이트
  밖에서 의도 밖으로 불리면 경계를 좁힐 근거가 된다. 재는 법: `git grep -n
  'macos_permissions::' -- src crates | grep -v crates/tasty-platform` 로 호출부를 세고,
  이 결정 시점의 자리(설정 탭 · 부팅 · 캡처 · 키 주입 판정)와 견준다.

## References

- [ADR-0331](0331-the-platform-folder-holds-the-os-call-not-the-app-meaning.md) — 이
  결정이 실행하는 Alternative A 를 적어 둔 선행 결정. 폴더의 계약 자체는 그쪽이 정본이다.
- [ADR-0089](0089-crate-split-follows-dependency-direction.md) — 크레이트 분리의 기준이
  크기가 아니라 의존 방향이라는 결정
- `docs/architecture/index.md` — "OS 경계" 절과 크레이트 수 정본
- `docs/dev-guide/build.md` — 워크스페이스 구조와 크레이트 분리 가이드
- 코드 근거(결정이 실현된 현재 위치): `crates/tasty-platform/src/lib.rs` ·
  `src/lib.rs` 의 `pub(crate) use tasty_platform as platform;` · 루트 `Cargo.toml` 의
  `tasty-platform` 의존과 `gui` feature 줄
