# 17. 설정 시스템

## cmux 구현 방식

- SwiftUI 설정 화면
- Ghostty config 호환
- UserDefaults 영속화
- 7개 카테고리: General / Appearance / Sidebar / Notifications / Browser / Automation / Shortcuts
- 라이브 리로드

## 크로스 플랫폼 구현 방안

### 설정 파일 형식

TOML (Rust 생태계 표준):

```toml
[general]
shell = "/bin/zsh"           # 기본 셸 (Windows: "powershell.exe")
startup_command = ""         # 시작 시 실행할 명령

[appearance]
theme = "dark"
font_family = "JetBrains Mono"
font_size = 14.0
background_opacity = 1.0
cursor_style = "block"       # block, underline, bar
cursor_blink = true

[sidebar]
width = 220
show_git_branch = true
show_ports = true
show_cwd = true
show_notifications = true

[notifications]
enabled = true
system_notification = true
sound = "default"            # "default", "none", 또는 파일 경로

[window]
startup_mode = "normal"      # normal, maximized, fullscreen
remember_position = true
remember_size = true

[clipboard]
copy_paste_macos_style = false    # Alt+C / Alt+V (macOS에서 기본 true)
copy_paste_linux_style = false    # Ctrl+Shift+C / Ctrl+Shift+V (Linux에서 기본 true)
copy_paste_windows_style = false  # Ctrl+C / Ctrl+V (Windows에서 기본 true)
osc52_write = true                # OSC 52 클립보드 쓰기 허용
osc52_read = false                # OSC 52 클립보드 읽기 차단 (보안)

[keybindings]
new_workspace = "ctrl+n"
split_vertical = "ctrl+d"
split_horizontal = "ctrl+shift+d"
# ...
```

### 설정 파일 경로

| OS | 경로 |
|----|------|
| **Linux** | `~/.config/tasty/config.toml` |
| **macOS** | `~/.config/tasty/config.toml` |
| **Windows** | `~/.config/tasty/config.toml` |

모든 플랫폼에서 `~/.config/tasty/config.toml`로 통일한다 (XDG 스타일).
`directories` 크레이트로 홈 디렉토리를 추상화.

### GUI 설정 윈도우

네이티브 GUI이므로 적절한 설정 UI를 제공한다. cmux의 SwiftUI 설정 화면과 유사한 UX.

```
┌─ Settings ──────────────────────────────────────┐
│                                                  │
│ ┌──────────┐  General                           │
│ │ General  │  ┌────────────────────────────────┐ │
│ │Appearance│  │ Shell:    [/bin/zsh        ▼]  │ │
│ │ Sidebar  │  │ Startup:  [                 ]  │ │
│ │ Notify   │  └────────────────────────────────┘ │
│ │ Window   │                                     │
│ │Shortcuts │  Appearance                         │
│ └──────────┘  ┌────────────────────────────────┐ │
│               │ Theme:    [Dark           ▼]   │ │
│               │ Font:     [JetBrains Mono ▼]   │ │
│               │ Size:     [- 14.0 +]           │ │
│               │ Opacity:  [████████░░] 0.85    │ │
│               │ Cursor:   ● Block ○ Bar ○ Line │ │
│               └────────────────────────────────┘ │
│                                                  │
│              [Reset to Default]    [Apply] [OK]  │
└──────────────────────────────────────────────────┘
```

### 설정 카테고리

| 카테고리 | 항목 |
|----------|------|
| General | 셸, 시작 명령, 언어 |
| Appearance | 테마, 폰트, 크기, 투명도, 커서 스타일 |
| Sidebar | 너비, 표시 항목 선택 |
| Notifications | 활성화, 시스템 알림, 사운드 |
| Clipboard | 복사/붙여넣기 방식 토글 (macOS/Linux/Windows), OSC 52 정책 |
| Window | 시작 모드, 위치/크기 기억 |
| Shortcuts | 키 바인딩 편집 (충돌 감지 포함) |

### 라이브 리로드

파일 감시(`notify` 크레이트)로 설정 파일 변경 시 자동 적용.
GUI 설정 윈도우에서 변경한 내용도 즉시 미리보기.

### 테마 시스템

```toml
# ~/.config/tasty/themes/monokai.toml
[colors]
background = "#272822"
foreground = "#f8f8f2"
cursor = "#f8f8f0"
selection = "#49483e"
# ANSI 16색
black = "#272822"
red = "#f92672"
green = "#a6e22e"
# ...
```

## 최적화 전략

- **설정 파싱 캐싱**: TOML 파싱 결과를 메모리에 캐싱하고, 파일 변경 시에만 재파싱한다. `notify` 크레이트의 파일 감시와 연동한다.
- **라이브 리로드 디바운스**: 파일 변경 감지 시 즉시 반영하지 않고 300ms 디바운스를 적용한다. 편집기에서 저장할 때 발생하는 다중 이벤트를 하나로 합친다.
- **설정 윈도우 지연 생성**: 설정 윈도우를 열 때만 생성한다. 평소에는 메모리에 로드하지 않아 리소스를 절약한다.
- **검증 캐싱**: 설정 값 검증 결과를 캐싱하여 동일한 값에 대한 반복 검증을 방지한다. 설정 구조체의 해시를 비교하여 변경 여부를 판단한다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | |
| macOS | ✅ 가능 | |
| Linux | ✅ 가능 | |

GUI 앱이므로 cmux의 SwiftUI 설정 화면과 유사한 수준의 설정 UI가 가능하다.
