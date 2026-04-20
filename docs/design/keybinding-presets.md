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
| new_window | alt+shift+n | alt+n | ctrl+shift+n | ctrl+shift+n |
| new_workspace | alt+n | alt+shift+n | alt+n | alt+n |
| new_tab | alt+t | alt+t | ctrl+shift+t | ctrl+shift+t |
| close_active | ctrl+w | alt+w | ctrl+w | ctrl+w |
| close_pane | ctrl+shift+w | alt+shift+w | ctrl+shift+w | ctrl+shift+w |
| close_surface | | | | |
| close_workspace | alt+shift+w | | alt+shift+w | alt+shift+w |
| restore_closed | ctrl+shift+t | alt+shift+t | ctrl+shift+r | ctrl+shift+r |

> **Windows/Linux의 restore_closed**: Tasty 기본값 `ctrl+shift+t`는 `new_tab`과 충돌하므로 `ctrl+shift+r`(Restore)로 변경.

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
| focus_pane_next | ctrl+] | alt+] | alt+] | alt+] |
| focus_pane_prev | ctrl+[ | alt+[ | alt+[ | alt+[ |
| focus_surface_next | alt+] | ctrl+] | ctrl+] | ctrl+] |
| focus_surface_prev | alt+[ | ctrl+[ | ctrl+[ | ctrl+[ |
| next_tab | ctrl+tab | alt+shift+] | ctrl+tab | ctrl+tab |
| prev_tab | ctrl+shift+tab | alt+shift+[ | ctrl+shift+tab | ctrl+shift+tab |
| tab_switch_modifier | ctrl | alt | ctrl | alt |
| workspace_switch_modifier | alt | ctrl | alt | ctrl |

> **Mac의 next/prev_tab**: iTerm2 관례(⌘⇧] / ⌘⇧[)를 따름.

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
| quit_immediate | | alt+shift+q | | ctrl+shift+q |
| quit_minimize | | alt+m | | |

> **Mac**: ⌘Q(종료), ⌘⇧Q(즉시 종료), ⌘M(최소화)는 macOS 표준.
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

### 3. 충돌 회피

각 프리셋 내에서 동일 바인딩이 두 기능에 할당되지 않도록 설계했다. 특히:

- Windows/Linux: `ctrl+shift+t`를 `new_tab`에 사용하므로 `restore_closed`는 `ctrl+shift+r`로 변경
- Mac: `alt+n`을 `new_window`(⌘N)에 사용하므로 `new_workspace`는 `alt+shift+n`(⌘⇧N)으로 변경

### 4. Tasty 고유 기능 처리

Pane/Surface 분할, Workspace, Surface 변환 등 다른 터미널에 없는 개념은:
- 각 프리셋의 수정자 키 계층에 맞춰 배정
- 가능하면 프리셋 간 동일한 문자 키 유지 (예: `d`=분할, `e`=pane 분할)
