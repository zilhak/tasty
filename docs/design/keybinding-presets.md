# 단축키 프리셋 설계

## 개요

Tasty는 4개의 단축키 프리셋을 제공한다. 각 프리셋은 해당 플랫폼의 대표 터미널 앱 관례를 따른다.

| 프리셋 | 참고 앱 | 특징 |
|--------|---------|------|
| **Tasty** (기본) | 자체 설계 | 개발자가 편하다고 느끼는 자체 키 조합 |
| **Mac** | iTerm2 / Terminal.app | `alt+` (= ⌘) 중심. macOS에서 일반적으로 쓰는 조합 |
| **Windows** | Windows Terminal | `ctrl+shift+` 중심. `ctrl+c/v`로 복사/붙여넣기 |
| **Linux** | GNOME Terminal | `ctrl+shift+` 중심. `ctrl+shift+c/v`로 복사/붙여넣기 |

### 프리셋과 키 매핑의 관계

프리셋은 **바인딩 문자열의 집합**이다. 바인딩 문자열이 실제 물리 키에 어떻게 매핑되는지는 `key-mapping.md`의 OS별 매핑 레이어가 결정한다. 따라서:

- Mac 프리셋의 `alt+c`는 macOS에서 ⌘+C, Windows에서 Alt+C로 동작한다.
- Windows 프리셋의 `ctrl+c`는 어떤 OS에서든 Ctrl+C로 동작한다.

프리셋은 **특정 플랫폼에서 사용할 것을 권장**하는 것이지, 다른 플랫폼에서 사용을 금지하지는 않는다.

---

## 전체 바인딩 비교표

빈 칸은 바인딩 없음(사용자가 직접 설정 가능).

### 생성/닫기

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| new_window | alt+shift+n | alt+shift+n | ctrl+shift+n | ctrl+shift+n |
| new_workspace | alt+n | alt+n | alt+n | alt+n |
| new_tab | alt+t | alt+t | alt+t | alt+t |
| close_active | ctrl+w | alt+w | ctrl+w | ctrl+w |
| close_pane | ctrl+shift+w | alt+shift+w | ctrl+shift+w | ctrl+shift+w |
| close_surface | | | | |
| close_workspace | alt+shift+w | | alt+shift+w | alt+shift+w |
| restore_closed | ctrl+shift+t | ctrl+shift+t | ctrl+shift+t | ctrl+shift+t |

### 분할

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| split_pane_vertical | alt+e | alt+e | alt+shift+e | alt+shift+e |
| split_pane_horizontal | alt+shift+e | alt+shift+e | alt+shift+d | alt+shift+d |
| split_surface_vertical | alt+d | alt+d | alt+d | alt+d |
| split_surface_horizontal | alt+shift+d | alt+shift+d | alt+e | alt+e |

### 포커스 이동

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| focus_pane_next | ctrl+] | ctrl+] | ctrl+] | ctrl+] |
| focus_pane_prev | ctrl+[ | ctrl+[ | ctrl+[ | ctrl+[ |
| focus_surface_next | alt+] | alt+] | alt+] | alt+] |
| focus_surface_prev | alt+[ | alt+[ | alt+[ | alt+[ |
| next_tab | | | | |
| prev_tab | | | | |
| tab_switch_modifier | ctrl | ctrl | ctrl | ctrl |
| workspace_switch_modifier | alt | alt | alt | alt |

> Pane/Surface 분할과 포커스 이동은 Tasty 고유 개념이므로 전 프리셋 동일.
> `next_tab` / `prev_tab`은 `ctrl+tab` / `alt+tab`이 OS 수준 단축키와 충돌하므로 기본값 없음.

### 클립보드/줌

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| copy | ctrl+c, alt+c, ctrl+shift+c | alt+c | ctrl+c | ctrl+shift+c |
| paste | ctrl+v, alt+v, ctrl+shift+v | alt+v | ctrl+v | ctrl+shift+v |
| zoom_in | ctrl+=, ctrl++, alt+=, alt++ | alt+=, alt++ | ctrl+=, ctrl++ | ctrl+=, ctrl++ |
| zoom_out | ctrl+-, alt+- | alt+- | ctrl+- | ctrl+- |
| zoom_reset | ctrl+0, alt+0 | alt+0 | ctrl+0 | ctrl+0 |

> **Tasty 프리셋의 copy/paste**: 세 플랫폼의 관례를 모두 바인딩. `ctrl+c`로 복사 시, 텍스트 선택이 있으면 복사하고 없으면 시그널(SIGINT)을 터미널에 전달하는 특수 처리가 구현되어 있다.

### UI 토글

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| toggle_settings | ctrl+, | alt+, | ctrl+, | ctrl+, |
| toggle_notifications | ctrl+shift+i | alt+shift+i | ctrl+shift+i | ctrl+shift+i |
| toggle_sidebar | ctrl+shift+b | alt+shift+b | ctrl+shift+b | ctrl+shift+b |
| toggle_sidebar_collapse | ctrl+b | alt+b | ctrl+b | ctrl+b |
| toggle_clipboard_viewer | ctrl+shift+h | alt+shift+h | ctrl+shift+h | ctrl+shift+h |

> **Mac**: UI 토글도 ⌘(alt) 기반으로 통일. macOS 관례(⌘,로 설정 열기 등)에 부합.

### 종료

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| quit | | alt+q | | ctrl+q |
| quit_immediate | | | | |
| quit_minimize | | alt+m | | |

> `quit`은 `close_behavior` 설정에 따라 확인 다이얼로그를 거치므로 기본 바인딩 허용.
> `quit_immediate`는 확인 없이 즉시 종료하므로, 실수 방지를 위해 전 프리셋 기본값 없음. 사용자가 필요 시 직접 설정.
> **Mac**: ⌘Q(종료), ⌘M(최소화)는 macOS 표준.
> **Windows**: Alt+F4가 OS 수준 종료이므로 별도 바인딩 불필요.
> **Linux**: Ctrl+Q(GNOME Terminal 종료 관례).

### Surface 변환/기타

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| convert_surface | alt+' | alt+' | alt+' | alt+' |
| convert_to_markdown | | | | |
| convert_to_explorer | | | | |
| open_markdown | | | | |
| open_explorer | | | | |

> Tasty 고유 기능(Surface 변환 등)은 다른 터미널에 대응 관례가 없으므로 모든 프리셋에서 동일.

### 레이아웃 프리셋 적용

| 필드 | Tasty | Mac | Windows | Linux |
|------|-------|-----|---------|-------|
| apply_workspace_preset | | | | |
| apply_tab_preset | | | | |
| apply_pane_preset | | | | |

> 레이아웃 프리셋(workspace/tab/pane 구조 저장본)을 적용하는 단축키. 사용자가 자주 쓰는 프리셋 이름을 직접 배정하도록 전 프리셋 기본값 없음.

---

## 설계 원칙

### 1. 수정자 키 계층 (Mac 프리셋)

macOS에서는 Cmd(⌘)가 주 수정자이므로, Mac 프리셋은 `alt+`(= ⌘)를 핵심 동작에 배정한다.

```
alt+      (⌘)         → 주요 동작: 탭/윈도우 생성, 닫기, 복사/붙여넣기
alt+shift+(⌘⇧)        → 보조 동작: 프리셋 복원, 분할, 탭 이동
ctrl+     (⌃)          → 보조 이동: surface 포커스
```

### 2. 수정자 키 계층 (Windows/Linux 프리셋)

Windows/Linux에서는 Ctrl이 주 수정자이므로 Ctrl+Shift 조합을 핵심 동작에 배정한다.

```
ctrl+shift+ → 주요 동작: 탭 생성, 닫기, 복사/붙여넣기(Linux)
ctrl+       → 보조 동작: 줌, 설정, 닫기
alt+        → 분할, pane 포커스 이동
alt+shift+  → 상위 분할(pane 레벨)
```

### 3. Tasty 고유 기능 처리

Pane/Surface 분할, Workspace, Surface 변환 등 다른 터미널에 없는 개념은:
- 각 프리셋의 수정자 키 계층에 맞춰 배정
- 가능하면 프리셋 간 동일한 문자 키 유지 (예: `d`=분할, `e`=pane 분할)
