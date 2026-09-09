# ADR-0257: 단축키 이식 번들은 스키마 태그가 붙은 TOML 한 장이고, 미설치 plugin 의 override 는 import 에서 버린다

- **Status**: Accepted
- **Date**: 2026-09-09
- **Tags**: keybindings, portability, import-export, toml, schema, plugin, crate-boundary, warnings

## Context

단축키 구성을 다른 tasty 환경으로 옮길 수단이 없었다. 사용자 요구는 "모든 단축키
구성을 export 하고, 다른 환경에서 import 하면 그대로 가져온다" 이고, 범위는 사용자
결정으로 **호스트 `KeybindingSettings` 전량 + plugin override** 다.

옮길 대상이 두 파일에 흩어져 있다.

- `~/.tasty/config.toml` 의 `[keybindings]` — `KeybindingSettings`(`tasty-settings`)
- `~/.tasty/plugins.toml` 의 `keybindings` — `PluginsConfig.keybindings`
  (`tasty-host-plugin`)

두 크레이트의 의존은 `tasty-host-plugin` → `tasty-settings` **단방향**이라 아래쪽에서는
`ShortcutOverride` 가 안 보인다. 그리고 `Settings::save`/`load` 는 config.toml 전체를
읽고 쓸 뿐, 섹션 하나만 떼어내 옮기는 경로가 없다.

import 이 다루는 것은 **사용자가 파일 피커에서 고른 임의의 파일**이다. 아무 TOML 을
골라도 패닉하지 않아야 하고, 그것이 단축키 번들이 아니면 그렇다고 말해야 한다.

## Decision

번들은 **최상위에 스키마 태그(`schema = "tasty.keybindings"`)와 버전이 붙은 TOML
한 장**이고, 코덱은 `tasty-host-plugin` 의 `keybinding_bundle` 에 둔다 — 두 타입이
모두 보이는 가장 낮은 지점이다.

세 갈래를 이렇게 가른다.

1. **거절** — 최상위 `schema` 가 없거나 다르면 `BundleError::NotABundle`. TOML 자체가
   깨졌으면 `BundleError::Toml`. 값을 하나도 복원할 수 없는 경우만 에러다.
2. **경고하고 계속** — 모르는 최상위 키, 모르는 단축키 필드, 값의 모양이 다른 필드
   (타입 불일치·고정 배열 길이 차이), 읽히지 않는 override 항목, 이 빌드가 아는 것보다
   높은 버전. 전부 `Vec<BundleWarning>` 으로 돌려준다.
3. **버리고 경고** — **미설치 plugin 의 override 전부**, 그리고 대상이 없는
   `script_bindings` 항목. 설치 여부·script 존재 여부는 decode 인자(`DecodeEnv`)로 받는다.

미설치 plugin 을 버리는 이유: 그 override 는 이 환경에서 가리킬 command 가 없어 발화도
표시도 되지 않는데, 남겨 두면 `plugins.toml` 에 고이면서 나중에 그 plugin 을 설치했을
때 사용자가 기억 못 하는 설정이 되살아난다.

`[keybindings]` 절은 **필드 단위로** 복원한다 — 기본값 테이블에서 출발해 번들의 필드를
하나씩 얹고, 얹은 뒤 전체가 역직렬화되지 않으면 그 필드만 되돌린다. 그래서 코덱은
필드 명부도, 어느 필드가 고정 길이 배열인지도 손으로 들지 않는다. `KeybindingSettings`
자신의 직렬화 결과가 그 답이고, 필드가 늘거나 배열 길이가 바뀌어도 코덱은 안 고친다.

export 원본은 **`PluginsConfig.keybindings` 자체**다. 설정 창이 가진
`PluginShortcutSnapshot` 은 `command_registry.iter_all()` 로 만들어져 등록된 command 만
담으므로, 그것을 원본으로 쓰면 비활성·미등록 plugin 의 override 가 조용히 빠진다.

번들은 `KeybindingSettings` 를 통째로 실을 뿐 액션 이름을 따로 열거하지 않는다 —
"모든 단축키는 `KeybindingSettings` 로 노출되며 코드에 하드코딩되지 않는다" 는
프로젝트 규칙이 번들 포맷에서도 그대로 성립해야 하기 때문이다. 번들에 있는 필드는
그 타입에 있는 필드이고, 그 반대도 참이다.

## Consequences

- **얻은 것**: 구성 전량이 사람이 읽고 고칠 수 있는 파일 한 장으로 오간다. 코덱이
  UI 없이 시험 가능하고(헤드리스), 필드가 늘어도 코덱은 안 고친다. 비정상 입력이
  조용히 사라지지 않는다 — 버린 것이 전부 경고 목록에 남아 UI 가 그대로 그린다.
- **잃은 것**: `[keybindings]` 복원이 필드마다 한 번씩 역직렬화를 돌려 O(필드 수) 다.
  import 은 사용자가 한 번 누르는 동작이라 비용이 문제 되는 자리가 아니다.
  그리고 값의 모양이 어긋난 필드는 **되살릴 수 없고 기본값이 된다** — 부분 복구가
  아니라 필드 단위 전부 아니면 전무다.
- **운영 비용 / 유지 부담**: 번들 구조체에 최상위 필드를 더하면 `BUNDLE_KEYS` 도 함께
  고쳐야 한다. 그 정합은 `top_level_keys_match_the_struct` 테스트가 직렬화 결과로
  대조하므로 빠뜨리면 그 자리에서 깨진다. 포맷을 비호환으로 바꾸면 `BUNDLE_VERSION`
  을 올린다 — 구버전이 신버전 번들을 읽으면 경고를 달고 아는 만큼만 복원한다.

## Alternatives Considered

- **`KeybindingSettings` 를 그대로 직렬화한다** — 포맷을 새로 정할 필요가 없다.
  안 골랐다: plugin override 가 안 들어간다. 그리고 스키마 태그가 없어 아무 TOML 이나
  "단축키 구성" 으로 읽히므로, 사용자가 `config.toml` 을 고르면 그 절이 통째로
  기본값으로 덮인다.
- **`Settings` 전체를 옮긴다** — 이미 있는 직렬화를 그대로 쓴다. 안 골랐다: 요구가
  "단축키" 범위인데 테마·터미널 설정까지 딸려 간다.
- **JSON 을 쓴다** — `Option` 필드가 있어도 안전하다. 안 골랐다: 이 레포의 사용자
  설정 파일이 전부 TOML 이고, `KeybindingSettings` 에는 `Option<T>` 필드가 없다
  (전 필드가 `Vec<String>` / `String` / `[String; N]` / `Vec<ScriptBinding>` 이고,
  `ScriptBinding` 자신도 `String` 필드 둘뿐이라 중첩까지 봐도 `Option` 이 없다).
  `ShortcutOverride` 의 TOML
  round-trip 도 `shortcut_override_serialization` 이 이미 고정하고 있다. 사람이 열어
  고치는 파일이라는 요구에도 TOML 이 낫다.
- **번들 코덱을 새 크레이트로 뺀다** — 두 타입 어디에도 안 얹힌다. 안 골랐다:
  `tasty-host-plugin` 이 이미 두 타입을 모두 보고, plugin override 절반이 그 크레이트
  것이다. 크레이트를 더하면 빌드 그래프에 노드만 늘고 경계는 안 나뉜다.
- **미설치 plugin 의 override 를 보존한다** — 나중에 그 plugin 을 설치하면 살아난다.
  안 골랐다(사용자 결정): 되살아나는 시점이 사용자가 기억하는 시점과 안 맞고,
  import 결과가 "지금 화면에 보이는 것" 과 달라진다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- `KeybindingSettings` 에 `Option<T>` 필드가 생긴다(`ScriptBinding` 같은 중첩 타입 안도
  포함 — 위 열거가 그 층을 함께 센다) — TOML 은 null 을 표현하지 못해
  그 필드가 직렬화에서 깨진다. 그때는 포맷(또는 그 필드의 표현)을 다시 정해야 한다.
  같은 함정의 선례가 preset capture 다.
- `KeybindingBundle` 에 최상위 필드가 늘었는데 `BUNDLE_KEYS` 가 안 늘었다 —
  `top_level_keys_match_the_struct` 가 잡는다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- import 에서 "모양이 어긋난 필드가 통째로 기본값이 됐다" 는 보고가 반복된다. 그러면
  필드 단위 전부-아니면-전무 대신 원소 단위 복구(길이가 다른 배열의 앞부분만 살리기
  등)를 검토한다. 재는 법: `BundleWarning::KeybindingFieldShapeMismatch` 가 실제
  import 에서 얼마나 나오는지 사용자 보고로 센다 — 그 경고는 UI 에 그대로 뜬다.

## References

- `docs/features/keybindings/index.md` "이식 번들" — 지금 어떻게 동작하는가
- `docs/design/policies/key-mapping.md` "설정 파일 이식성" — 바인딩 문자열이 이미 OS 독립
- [ADR-0254](0254-the-binding-parser-lives-with-the-setting-it-parses.md) — 이식 판정이
  쓸 파서를 어디에 뒀는가
- 코드 근거(결정이 실현된 현재 위치): `tasty_host_plugin::keybinding_bundle` 의
  `encode`·`decode`·`DecodeEnv`·`BundleWarning`, export 원본인
  `PluginsConfig::shortcut_overrides`
