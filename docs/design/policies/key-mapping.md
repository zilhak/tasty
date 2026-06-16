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
| (미사용) | Win | Super | Option (⌥) |

macOS 에서만 `alt` 토큰이 Cmd(⌘)에 매핑된다(물리 위치가 Win/Linux 의 Alt 와 동일하므로). 예: 프리셋 `new_tab = "alt+t"` 는 Win/Linux 에서 Alt+T, macOS 에서 ⌘+T 로 눌린다. 프리셋은 **하나의 바인딩 문자열 집합**을 쓰지만 OS별 매핑으로 각 OS 에서 자연스러운 조합으로 느껴진다.

### 캡처(설정 UI) / 매칭(런타임)

- **캡처**: egui `Modifiers` → 토큰. macOS `mac_cmd → "alt"`, 기타 `alt → "alt"`; `ctrl → "ctrl"`, `shift → "shift"`. macOS 에서 ⌘+N 도, Windows 에서 Alt+N 도 동일하게 `"alt+n"` 저장.
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

`~/.tasty/config.toml` 의 바인딩 문자열은 **OS 독립**이다 — `"alt+n"` 은 어디서든 `"alt+n"`. 변환은 런타임 OS별 매핑 레이어가 한다. "OS 네이티브 표기로 표시"(예: macOS 에서 `⌘+N` 로 보여주기)를 한다면 **저장(항상 추상 토큰) ↔ 표시(OS별 표기)** 를 분리해야 이식성이 유지된다.

## OS 메뉴 key equivalent

tasty 가 직접 소유하는 OS 메뉴(macOS NSMenu / Windows AcceleratorTable / Linux Wayland 메뉴)의 key equivalent 도 **`KeybindingSettings` 의 대응 binding 에서 가져온다 — 가져올 수 없으면 비운다.** selector 가 OS 표준(`cut:` / `performClose:` 등)이라는 사실이 단축키 하드코딩을 정당화하지 않는다(selector 와 key equivalent 는 독립 결정). binding 이 빈 vec 이면 key equivalent 도 비워 단축키 없는 메뉴 항목으로 둔다.

**예외**: OS 자체가 박아 tasty 가 무력화/덮어쓰기/가로채기 모두 불가능한 단축키(macOS Spotlight `Cmd+Space`, OS 전역 윈도우 전환 등)는 정책 범위 밖 — tasty 가 등록할 수도 끌 수도 없다.

## 코드 위치

- `KeybindingSettings`(바인딩 필드·`format_display`), 캡처/매칭 레이어, 프리셋 기본 바인딩.
- OS 메뉴 배선: macOS NSMenu / Windows AcceleratorTable.
</content>
