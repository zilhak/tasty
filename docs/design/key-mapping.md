# 키 매핑 설계

## 개요

Tasty는 Windows, macOS, Linux에서 **사용자가 동일한 물리적 키 조합을 눌러 동일한 기능을 사용**할 수 있도록 설계한다. 이를 위해 바인딩 문자열에서 사용하는 수정자 이름(`ctrl`, `alt`, `shift`)이 OS마다 다른 물리적 키에 매핑된다.

## 핵심 원칙: 물리적 키 위치 일관성

표준 키보드에서 하단 수정자 키의 물리적 배치:

```
Windows/Linux:  [Ctrl] [Win/Super] [Alt]  ────  [Alt] [Win/Super] [Ctrl]
macOS:          [Ctrl] [Option]    [Cmd]  ────  [Cmd] [Option]    [Ctrl]
```

macOS의 **Cmd 키**는 Windows/Linux의 **Alt 키**와 동일한 물리적 위치에 있다. 사용자가 키보드를 바꿔 써도 **같은 손가락 위치에서 같은 동작**을 기대하므로, Tasty는 이 물리적 위치를 기준으로 매핑한다.

## 바인딩 문자열과 실제 키의 매핑

바인딩 문자열(예: `"alt+n"`)에서 각 수정자 토큰이 실제로 어떤 키에 대응되는지:

| 바인딩 토큰 | Windows | Linux | macOS |
|-------------|---------|-------|-------|
| `ctrl` | Ctrl | Ctrl | Ctrl (⌃) |
| `alt` | Alt | Alt | **Cmd (⌘)** |
| `shift` | Shift | Shift | Shift |

macOS에서만 `alt` 토큰이 Cmd(⌘)에 매핑된다. 이는 물리적 키 위치가 Windows/Linux의 Alt와 동일하기 때문이다.

## 프리셋과 크로스 플랫폼 경험

기본 프리셋 "Tasty"의 바인딩 예시:

```toml
new_workspace = "alt+n"
new_tab = "alt+t"
split_pane_vertical = "alt+e"
toggle_settings = "ctrl+,"
close_pane = "ctrl+shift+w"
```

이 바인딩이 각 OS에서 실제로 어떤 키 조합으로 동작하는지:

| 바인딩 | Windows/Linux에서 누르는 키 | macOS에서 누르는 키 |
|--------|---------------------------|-------------------|
| `alt+n` | Alt + N | ⌘ + N |
| `alt+t` | Alt + T | ⌘ + T |
| `ctrl+,` | Ctrl + , | Ctrl + , |
| `ctrl+shift+w` | Ctrl + Shift + W | Ctrl + Shift + W |

프리셋은 **하나의 동일한 바인딩 문자열 집합**을 사용하지만, OS별 키 매핑에 의해 사용자는 각 OS에서 자연스러운 키 조합으로 느낀다. macOS 사용자에게 `alt+n`은 "⌘+N"이고, Windows 사용자에게는 "Alt+N"이다.

## 구현 세부사항

### 단축키 캡처 (설정 UI)

설정 창에서 사용자가 단축키를 녹화할 때, egui의 `Modifiers`를 바인딩 문자열로 변환한다:

```
macOS:    mac_cmd → "alt"   |  ctrl → "ctrl"  |  shift → "shift"
기타 OS:  alt     → "alt"   |  ctrl → "ctrl"  |  shift → "shift"
```

사용자가 macOS에서 ⌘+N을 누르면 `"alt+n"`이 저장된다. Windows에서 Alt+N을 누르면 동일하게 `"alt+n"`이 저장된다.

### 단축키 매칭 (런타임)

바인딩 문자열을 파싱하여 실제 키 이벤트와 비교할 때, winit의 `ModifiersState`를 사용한다:

```
"ctrl"  → mods.control_key()
"alt"   → macOS: mods.super_key()  |  기타: mods.alt_key()
"shift" → mods.shift_key()
```

### 복사/붙여넣기 키 정책

복사/붙여넣기는 세 가지 방식을 독립적으로 활성화/비활성화할 수 있다:

| 방식 | 복사 | 붙여넣기 | 바인딩 토큰 | macOS 실제 키 |
|------|------|----------|------------|--------------|
| macOS 방식 | `alt+c` | `alt+v` | alt | ⌘+C / ⌘+V |
| Linux 방식 | `ctrl+shift+c` | `ctrl+shift+v` | ctrl+shift | Ctrl+Shift+C/V |
| Windows 방식 | `ctrl+c` | `ctrl+v` | ctrl | Ctrl+C/V |

### modifier 없는 키 등록 방지

설정 UI에서 단축키를 캡처할 때, 일반 타이핑에 사용되는 키(알파벳, 숫자, 스페이스 등)는 **반드시 하나 이상의 수정자 키와 함께** 눌러야 등록된다. 수정자 없이 `w`만 누르는 것은 무시된다.

기능 키(F1~F12), Tab, Enter 등 타이핑에 직접 사용되지 않는 키는 수정자 없이도 등록 가능하다.

## 내부 표현과 OS 고유 키 이름

바인딩 문자열에서 사용하는 `ctrl`, `alt`, `shift` 토큰은 **물리적 키 위치를 추상화한 이름**이다. 이것은 OS가 인식하는 키 이름과 다를 수 있다.

각 OS에서 실제 키 이름과의 대응:

| 바인딩 토큰 | Windows 실제 키 | Linux 실제 키 | macOS 실제 키 |
|-------------|----------------|--------------|--------------|
| `ctrl` | Ctrl | Ctrl | Control (⌃) |
| `alt` | Alt | Alt | Command (⌘) |
| `shift` | Shift | Shift | Shift |
| (미사용) | Win | Super | Option (⌥) |

### 금지 사항: OS 고유 키 이름 혼용

macOS의 Option 키를 "alt"로 해석하는 프로그램들이 있지만, **Tasty에서는 이를 절대 허용하지 않는다.** 각 OS의 키는 고유한 이름으로 기록되어야 하며, OS 간 이동 시에만 변환이 일어난다:

- macOS의 **Command(⌘)** 는 Command이다. Alt가 아니다.
- macOS의 **Option(⌥)** 은 Option이다. Alt가 아니다.
- Windows의 **Alt** 는 Alt이다. Option이 아니다.
- Windows의 **Win** 은 Win이다. Super가 아니다.

바인딩 토큰 `"alt"`가 macOS에서 Command에 매핑되는 것은, OS 고유 키를 다른 이름으로 부르는 것이 아니라 **물리적 위치 기반 추상화**이다. 내부적으로 Tasty는 각 키를 OS 고유 이름으로 인식하고, 바인딩 문자열과의 변환만 OS별로 다르게 수행한다.

## 설정 파일 이식성

`~/.tasty/config.toml`의 바인딩 문자열은 **OS에 독립적**이다. 동일한 설정 파일을 Windows, macOS, Linux에서 공유하면, 각 OS에서 물리적 키 위치가 동일한 조합으로 동작한다.

### 설정 Export/Import 시 변환

설정을 다른 OS로 이동(export/import)할 때, 바인딩 문자열 자체는 변환이 필요 없다. `"alt+n"`은 어디서든 `"alt+n"`이다. 변환은 런타임에 OS별 매핑 레이어가 처리한다.

단, 향후 "OS 네이티브 형식으로 표시" 기능(예: macOS에서 `"alt+n"`을 `"⌘+N"`으로 표시)을 구현할 때는, 표시(display)와 저장(storage)을 분리해야 한다:

- **저장**: 항상 추상화된 바인딩 토큰 (`"alt+n"`)
- **표시**: OS별 네이티브 표기 (`⌘+N`, `Alt+N`)

이 분리가 되어 있어야 설정 파일의 크로스 플랫폼 호환성이 유지된다.
