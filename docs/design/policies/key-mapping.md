# 키 매핑 설계 (운영 상세)

tasty 의 모든 단축키는 [`KeybindingSettings`](../../features/settings/index.md) 로 노출되며 코드에 하드코딩되지 않는다. 본 문서는 바인딩 문자열의 OS별 키 매핑과 위치 기반 추상화를 기술한다.

## 핵심 원칙: 물리적 키 위치 일관성

Windows/macOS/Linux 에서 **같은 물리적 키 조합 → 같은 기능**. 표준 키보드 하단 수정자 배치:

```
Windows/Linux:  [Ctrl] [Win/Super] [Alt]  ──  [Alt] [Win/Super] [Ctrl]
macOS:          [Ctrl] [Option]    [Cmd]  ──  [Cmd] [Option]    [Ctrl]
```

macOS 의 **Cmd** 는 Windows/Linux 의 **Alt** 와 같은 물리적 위치다. 사용자가 키보드를 바꿔 써도 같은 손가락 위치에서 같은 동작을 기대하므로, tasty 는 이 물리적 위치를 기준으로 매핑한다.

## 바인딩 토큰 ↔ 실제 키

| 토큰 | Windows | Linux | macOS |
|------|---------|-------|-------|
| `ctrl` | Ctrl | Ctrl | Control (⌃) |
| `alt` | Alt | Alt | **Command (⌘)** |
| `shift` | Shift | Shift | Shift |
| `option` | (미사용) | (미사용) | Option (⌥) |

macOS 에서만 `alt` 토큰이 Cmd(⌘)에 매핑된다(물리 위치가 Win/Linux 의 Alt 와 동일하므로). 예: 프리셋 `new_tab = "alt+t"` 는 Win/Linux 에서 Alt+T, macOS 에서 ⌘+T 로 눌린다. 프리셋은 **하나의 바인딩 문자열 집합**을 쓰지만 OS별 매핑으로 각 OS 에서 자연스러운 조합으로 느껴진다.

### 캡처(설정 UI) / 매칭(런타임)

- **캡처**: winit `ModifiersState` → 토큰. macOS `super_key() → "alt"` · `alt_key() → "option"`, 기타 `alt_key() → "alt"`; `control_key() → "ctrl"`, `shift_key() → "shift"`. macOS 에서 ⌘+N 도, Windows 에서 Alt+N 도 동일하게 `"alt+n"` 저장.
- **매칭**: winit `ModifiersState` 와 비교. `"ctrl" → control_key()`, `"alt" → macOS super_key() / 기타 alt_key()`, `"shift" → shift_key()`.

## 바인딩 문자열 문법

`+` 는 구분자이자 키 이름이다. 파서는 왼쪽부터 `ctrl+`·`shift+`·`alt+` 프리픽스를 벗기고 남은 전체를 키 토큰으로 본다 — `"ctrl++"` = "Ctrl + `+` 키".

| 바인딩 | 해석 |
|--------|------|
| `ctrl++` / `ctrl+plus` | Ctrl + `+` (문자/이름 별칭 양방향 매칭) |
| `ctrl+-` / `ctrl+minus` | Ctrl + `-` |
| `ctrl+=` / `ctrl+equals` | Ctrl + `=` |
| `ctrl+shift+=` | Shift 까지 함께 요구 |
| `ctrl+` / `ctrl` | 무효 (키 부분 없음 / 모디파이어 단독) |

- **modifier 없는 일반 키 등록 방지**: 설정 캡처 시 알파벳/숫자/스페이스 등 타이핑 키는 수정자 1개 이상과 함께여야 등록된다(`w` 단독 무시). F1~F12·Tab·Enter 등 비타이핑 키는 수정자 없이 가능.
- **모디파이어 단독 입력(Ctrl/Shift/Alt/Super/Meta/Fn)은 어떤 바인딩과도 매칭 안 됨** — 매처가 구조적으로 차단.
- **Escape 는 설정 UI 녹화에서 "슬롯 비우기"로 예약** — 녹화 중 ESC 를 누르면 그 슬롯이
  지워지고 녹화가 끝난다. 따라서 `escape` 를 값으로 갖는 바인딩은 프리셋/설정 파일로만
  들어오고 녹화 버튼으로는 재지정할 수 없다(현재 해당: `fullscreen_stage_exit`). 기본값으로
  되돌리려면 프리셋을 재적용한다.
- **modifier 없는 바인딩은 modifier-hint 오버레이에 뜨지 않는다** — 오버레이는 홀드 중인
  modifier 조합에 속한 바인딩만 나열하므로, 조합이 없는 바인딩은 속할 섹션이 없다.

## 복사/붙여넣기 키 정책

세 방식을 독립적으로 on/off:

| 방식 | 복사 / 붙여넣기 | macOS 실제 키 |
|------|----------------|---------------|
| macOS | `alt+c` / `alt+v` | ⌘+C / ⌘+V |
| Linux | `ctrl+shift+c` / `ctrl+shift+v` | Ctrl+Shift+C/V |
| Windows | `ctrl+c` / `ctrl+v` | Ctrl+C/V |

## OS 고유 키 이름 혼용 금지

바인딩 토큰 `ctrl`/`alt`/`shift` 는 **물리적 위치를 추상화한 이름**이며 OS 가 인식하는 키 이름과 다를 수 있다. tasty 는 각 키를 OS 고유 이름으로 인식하고 바인딩 문자열과의 변환만 OS별로 다르게 한다:

- macOS **Command(⌘)** 은 Command 다. Alt 가 아니다. **Option(⌥)** 은 Option 이다. Alt 가 아니다.
- Windows **Alt** 는 Alt, **Win** 은 Win 이다.

토큰 `"alt"` 가 macOS 에서 Command 에 매핑되는 것은 OS 고유 키를 다른 이름으로 부르는 게 아니라 **물리적 위치 기반 추상화**다.

## 설정 파일 이식성

`~/.tasty/config.toml` 의 바인딩 문자열은 **OS 독립**이다 — `"alt+n"` 은 어디서든 `"alt+n"`. 변환은 런타임 OS별 매핑 레이어가 한다. **저장(항상 추상 토큰) ↔ 표시(OS별/사용자 선택 표기)** 를 분리해 이식성을 유지한다.

macOS 사용자를 위한 표시 커스터마이징: `GeneralSettings::{alt,option,shift}_display_style` 3 개 필드(설정 > 일반 > 표시, mac 전용 UI)로 Alt/Option/Shift 토큰의 화면 표기를 텍스트("Alt"/"Option"/"Shift", 기본값)와 macOS 심볼("⌘"/"⌥"/"⇧") 사이에서 독립적으로 고를 수 있다(`alt` 는 추가로 "Cmd" 텍스트도 선택 가능). `KeybindingSettings::format_display`/`format_display_parts` 가 이 설정을 받아 표시 문자열을 만든다 — 필드는 크로스플랫폼으로 존재하지만(직렬화 단순성), 값을 바꿀 수 있는 UI 는 macOS 에서만 노출된다. 저장 포맷(바인딩 문자열)에는 전혀 영향을 주지 않는다.

"symbol" 표시에는 두 경로가 있다. `format_display`·`format_display_parts`는 `⌘`·`⌥`·`⇧` 문자를 반환한다. 명령 팔레트·상태바·단축키 탭 등에서 이 문자열을 쓰며, egui 폰트에 U+2325(⌥) 글리프가 없으면 빈 사각형으로 보일 수 있다. 아직 해결되지 않은 문제다.

설정의 3개 표시 방식 드롭다운과 modifier-hint의 키캡(`combo_keycap_parts`, `src/adapters/ui/modifier_hint_overlay.rs`)은 `tasty_icons::{CMD_KEY,OPTION_KEY,SHIFT_KEY}`와 `tasty_ui_widgets::{KbdKey,kbd_parts}`로 벡터 아이콘을 그린다. 이 경로는 글꼴의 해당 글리프를 필요로 하지 않는다.

### 이식 시 `option` 처리

`option`은 macOS의 물리적 Option 키를 뜻하며 다른 OS에서는 매칭되지 않는다. `tasty-key-match`의 winit·egui 경로 모두 비-macOS에서 `option_matches = !parsed.option`을 사용한다. macOS 구성을 Windows·Linux로 가져오면 해당 바인딩은 화면에 표시돼도 실행되지 않는다. `alt` 등 다른 토큰은 OS별 매핑을 그대로 사용할 수 있다.

네 기본 프리셋에는 `option` 바인딩이 없다. 사용자가 macOS에서 녹화하거나 빠른 전환 수정자로 지정한 바인딩을 이관 대상으로 검사한다. 판정과 대체는 `tasty_host_plugin::keybinding_bundle::option_migration`이 담당한다.

`parse_binding`·`Combo::parse_modifiers`로 파싱한다. 단순 문자열 검색은 키 이름, 대소문자, 토큰 순서를 오해할 수 있다. 검사 대상은 다음 다섯 곳이다.

1. 일반 조합 필드의 각 항목
2. 빠른 전환의 수정자 세 종류
3. 개별 지정 모드인 빠른 전환의 슬롯·다음·이전. 규칙 기반 모드의 슬롯은 키 하나이므로 조합으로 보지 않는다.
4. `script_bindings[].combo`
5. plugin override의 `Key { value }`. `Inherit`·`None`에는 조합이 없다.

대체 입력은 `ReplacementKind`에 따라 받는다. 일반 조합은 녹화하고, 수정자만 바꾸는 항목은 `all_modifier_combos()`의 비-macOS 조합 7개 중 고른다. 녹화는 수정자 단독 입력을 받지 않기 때문이다. `"individual"`은 수정자 조합이 아니어서 거절하며 이관 과정에서 빠른 전환 모드를 바꾸지 않는다. 대체값에 `option`이 다시 들어가도 거절한다.

충돌은 적용 전후 전체 조합을 비교해 새로 생긴 것만 보고한다. 빠른 전환 수정자를 바꾸면 슬롯과 다음·이전 조합도 함께 달라지므로 해당 필드 하나만 비교하지 않는다. 호스트 액션·빠른 전환·스크립트는 한 충돌 범위로 묶고 plugin은 각각 별도로 검사한다. 호스트와 plugin, 서로 다른 plugin의 중복은 아래 우선순위 규칙을 따른다.

`Resolution::Unbind`로 사용하지 않을 바인딩을 비울 수도 있다. 일반 조합은 해당 항목만 제거하고, plugin은 남은 키가 없으면 `None`, 빠른 전환의 슬롯·다음·이전은 빈값으로 둔다. 수정자 항목 자체는 조합이 하나 필요하므로 비울 수 없다(`CannotUnbind`).

새 충돌 처리 방법은 호출자가 `ConflictPolicy`로 선택한다.

- `Reject`: 충돌 목록을 반환하고 변경하지 않는다.
- `UnbindOther`: 이관 계획에 없는 충돌 상대를 비우고 적용한다. 설정 가져오기의 충돌 확인을 수락하면 사용한다. 양쪽 모두 계획에 포함됐다면 비울 쪽을 고를 수 없어 거절한다.

`introduced_conflicts`·`preview_resolution`은 선택이 끝나지 않은 계획도 미리 확인한다. 정한 항목만 대체하고 나머지는 원래 값을 유지해 비교한다. 실제 적용인 `resolve_migration`은 모든 항목의 해결 방법이 정해져야 한다.

대상 OS는 `cfg!(target_os = "macos")`로 판단한다. macOS이면 이관 목록은 비어 있다.

## OS 메뉴 key equivalent

tasty 가 직접 소유하는 OS 메뉴의 key equivalent 도 **`KeybindingSettings` 의 대응 binding 에서 가져온다 — 가져올 수 없으면 비운다.** 지금 key equivalent 를 배선한 메뉴는 macOS NSMenu 하나이고, Windows AcceleratorTable · Linux Wayland 메뉴도 등록하게 되면 같은 규칙을 따른다. selector 가 OS 표준(`cut:` / `performClose:` 등)이라는 사실이 단축키 하드코딩을 정당화하지 않는다(selector 와 key equivalent 는 독립 결정). binding 이 빈 vec 이면 key equivalent 도 비워 단축키 없는 메뉴 항목으로 둔다.

**예외**: OS 자체가 박아 tasty 가 무력화/덮어쓰기/가로채기 모두 불가능한 단축키(macOS Spotlight `Cmd+Space`, OS 전역 윈도우 전환 등)는 정책 범위 밖 — tasty 가 등록할 수도 끌 수도 없다.

**라벨은 `t()` 다** — key equivalent 와 독립으로, OS 메뉴 항목의 **제목**은 `lang/*.toml` 번역 키에서 온다: macOS NSMenu `menu.macos.*`(앱 이름을 결합하는 About / Hide / Quit 은 `{}` placeholder + `t_fmt` 로 언어별 어순 대응), 시스템 트레이 메뉴·툴팁 `tray.*`, Windows Jump List `jump_list.*`. OS 네이티브 표면도 [i18n](../../dev-guide/i18n.md) 예외가 아니다. macOS 메뉴는 `KeybindingSettings` 변경 시 통째로 rebuild 되면서 라벨도 다시 조회하므로 키바인딩 변경 후에도 현재 언어가 유지된다.

## Plugin 커맨드 단축키 우선순위

plugin의 `[[contributes.commands]]`(`CommandDecl`)는 호스트 `KeybindingSettings`보다 먼저 매칭한다. `App::try_plugin_shortcut`이 `dispatch_window_event_to_view`보다 먼저 실행되며, plugin 명령을 찾으면 이벤트를 소비해 호스트로 넘기지 않는다. 사용자가 활성화하고 설정한 plugin의 단축키 선택을 우선하는 정책이다.

매칭 대상은 포커스에 따라 다르다.

| 포커스된 surface | 후보 명령 |
|---|---|
| plugin 소유 `RemoteSurface` | 해당 plugin의 `Global`·`Surface` 명령 |
| plugin surface가 아님 | 등록된 모든 plugin의 `CommandScope::Global` 명령 |

`Surface` 명령은 소유 plugin의 surface가 포커스됐을 때만 실행한다. 여러 plugin이 같은 Global 키를 등록하면 레지스트리 순회에서 처음 일치한 명령을 사용한다. 내부 저장소는 HashMap이므로 plugin 간 등록 순서나 실행 우선순위를 보장하지 않는다. 이 충돌을 해결하는 전용 UI는 아직 없다.

`CommandDecl.action`이 있으면 호스트가 `ToolAction::Event`·`OpenSurface`·`OpenPopup`을 도구 메뉴와 같은 방식으로 실행한다. 이때 `command.invoke`(`handle_command`)는 보내지 않아 중복 실행을 막는다. `action`이 없으면 기존 `handle_command` IPC를 사용한다.

Event Bus의 `command.invoked`는 소유 plugin에 보내는 알림이다. `action`과 대상 surface의 유무에 관계없이 명령이 매칭되면 보낸다.

## 텍스트 입력과 단축키의 우선순위

explorer 같은 egui surface에 포커스가 있으면 한 번의 키 입력이 두 곳에 도달한다.
`handle_event`가 텍스트 이벤트를 egui 큐에 먼저 넣고 그 다음에 단축키를 처리하는데,
단축키가 처리되더라도 이미 큐에 들어간 텍스트 이벤트를 되돌릴 방법이 없다. 그래서 수식
키 없이(또는 shift만 붙여) 영숫자에 등록된 단축키는 그 글자를 텍스트로도 흘려보낸다.

이 중복을 막는 곳은 텍스트를 소비하는 쪽이다. explorer 타입어헤드는
`unmodified_binding_chars`로 수식 키 없이 등록된 영숫자를 모아 두고, 그 글자는 소비하지
않고 단축키에 넘긴다([explorer](../../features/explorer/index.md)의 "타입어헤드로 항목
선택"). Tasty 기본 프리셋에는 그런 단축키가 없으므로 기본 상태에서 넘기는 글자도 없다.

`shift`만 붙은 단축키도 같이 처리한다. shift 조합은 대문자 텍스트 이벤트를 만들어 수식
키가 없을 때와 똑같이 두 번 동작한다. `binding_has_modifier`가 shift를 수식 키로 세지
않는 것이 여기서는 오히려 맞는 판단이다. Ctrl과 Cmd 조합은 egui-winit이 텍스트 이벤트를
아예 만들지 않으므로 이 문제가 없다.

## 합성 키 이벤트 (winit)

winit은 X11·Windows에서 창이 포커스를 얻으면 이미 눌린 키의 `Pressed`, 잃으면 `Released`를 합성한다(`WindowEvent::KeyboardInput { is_synthetic: true, .. }`). macOS·Wayland에서는 합성하지 않는다.

Tasty는 합성 키를 사용자 입력으로 처리하지 않는다. 예를 들어 다른 앱을 `Alt+F4`로 닫은 뒤 Tasty에 합성 `F4`가 들어오면 사용자가 누르지 않은 `rename_tab`이 실행될 수 있다.

### 차단 지점 — 이벤트 진입부 단 한 곳

`App::window_event`의 진입부에서 `is_synthetic_key_event`로 차단한다. 셸 설정·종료·부팅·모달·plugin 단축키·View 위임보다 먼저 검사해야 한다. 어느 분기든 검사 전에 반환하면 그 경로에서 합성 키가 처리될 수 있다.

입력 처리에는 두 경로가 있다.

- **직접 해석**: `WindowEvent::KeyboardInput`을 읽어 단축키·PTY·녹화로 전달한다. X11에서는 차단 전 합성 `F4`로 탭 이름 변경이 열리고, `Escape`를 누른 채 종료 확인창으로 옮기면 창이 닫혔다. 차단 뒤에는 두 현상이 발생하지 않았고 직접 누른 키는 동작했다.
- **egui 전달**: `handle_egui_event`로 이벤트를 넘긴다. `MainView`·`SettingsView`·`PresetView`·`PluginsView`·`QuitView` 다섯 View가 사용하며 PresetView·PluginsView는 이 경로만 사용한다. `KeyboardInput` 패턴 검색만으로는 찾을 수 없다.

확인한 egui-winit 0.31.1은 합성 `Pressed`를 자체적으로 버리지만 `Released`는 버리지 않는다. 따라서 합성 `Enter`가 egui의 종료 확인 버튼을 누르는 현상은 재현되지 않았다. 다만 이는 의존성의 구현 세부이며 위젯 포커스가 안전을 보장하는 것은 아니다. Tasty는 두 이벤트 모두 자기 진입부에서 차단한다.

이 위치는 새 View에도 적용된다. `crates/tasty-doc-guards/tests/synthetic_key_event_guard.rs`가 검사 위치를 확인한다.

### 버려도 modifier 상태가 깨지지 않는 이유

양쪽 백엔드 모두 합성 키와 **별개로** `ModifiersChanged` 를 보낸다 — X11 은 포커스 획득
처리 말미의 `update_mods_from_query`, Windows 는 `gain_active_focus` 의 `update_modifiers`
가 담당한다. 따라서 합성 modifier Pressed 를 버려도 포커스 획득 직후의 modifier 조합
단축키는 정상 매칭된다.

### double-tap detector 는 포커스 전환마다 초기화한다

합성 키를 버리면 포커스 전환 중 누름과 뗌이 짝을 이루지 않을 수 있다. `Alt+Tab`으로 나갈 때 실제 press만 들어오고 합성 release는 버려진다. 돌아온 뒤 Alt를 떼는 입력을 첫 탭으로 오인하면 다음 한 번의 입력이 더블탭으로 처리될 수 있다.

`WindowEvent::Focused`의 획득·상실 양쪽에서 `DoubleTapDetector::reset`을 호출한다. `MainView`와 `SettingsView`는 별도 인스턴스를 가지므로 두 곳 모두 초기화한다.

### `#[cfg]` 분기를 두지 않는 이유

`is_synthetic` 은 플랫폼 중립 필드고 발현 원인도 winit 의 공통 계약이다. 합성을 하지 않는
macOS·Wayland 에서는 항상 `false` 로 들어와 동작이 바뀌지 않으므로, OS 별 근본 원인이
다를 때 요구되는 분기 케이스가 아니다.

## 코드 위치

- `KeybindingSettings`(바인딩 필드·`format_display`), 캡처/매칭 레이어, 프리셋 기본 바인딩.
- OS 메뉴 배선: macOS NSMenu(`crates/tasty-platform/src/native_menu/macos.rs`).
- 합성 키 차단: `src/adapters/ui/input/synthetic.rs`(`is_synthetic_key_event`),
  `src/app/event_handler.rs`(`App::window_event` 진입부 게이트),
  `src/adapters/ui/input/double_tap.rs`(`DoubleTapDetector::reset`).
- 텍스트 입력과 겹치는 경우의 처리: `src/adapters/ui/surface/explorer/type_ahead.rs`
  (`unmodified_binding_chars`).
- Plugin 커맨드: `crates/tasty-host-plugin/src/command_registry.rs`(`PluginCommandRegistry`,
  `effective_binding`), `src/plugin_bridge/key_dispatch.rs`(`match_plugin_shortcut`/
  `match_global_shortcut`/`dispatch_plugin_command`), `src/app/plugin_glue/shortcut.rs`
  (`App::try_plugin_shortcut`).
