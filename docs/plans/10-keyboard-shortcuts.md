# 10. 키보드 단축키

## cmux 구현 방식

- 33개 이상의 커스터마이징 가능한 단축키
- 카테고리: Titlebar/Navigation/Panes/Panels
- 보조 단축키 지원
- Dvorak/비US 키보드 레이아웃 지원

## 크로스 플랫폼 구현 방안

### 키 입력 처리

winit의 `WindowEvent::KeyboardInput`으로 모든 키 입력을 받는다.

```rust
WindowEvent::KeyboardInput { event, .. } => {
    let KeyEvent { physical_key, logical_key, state, .. } = event;
    if state == ElementState::Pressed {
        let action = keymap.lookup(logical_key, modifiers);
        // ...
    }
}
```

winit는 물리 키(`physical_key`)와 논리 키(`logical_key`)를 구분하여 제공한다.
키보드 레이아웃이 다른 경우(Dvorak, AZERTY 등) 논리 키를 사용하면 자연스러운 매핑이 된다.

### 기본 단축키 (계획)

| 단축키 | 동작 |
|--------|------|
| Ctrl+N (Cmd+N) | 새 워크스페이스 |
| Ctrl+1~9 (Cmd+1~9) | 워크스페이스 전환 |
| Ctrl+D (Cmd+D) | 수직 분할 |
| Ctrl+Shift+D | 수평 분할 |
| Alt+방향키 | 패인 포커스 이동 |
| Ctrl+Shift+P (Cmd+Shift+P) | 명령 팔레트 |
| Ctrl+W (Cmd+W) | 패인 닫기 |
| Ctrl+Shift+W | 워크스페이스 닫기 |
| Ctrl+F (Cmd+F) | 검색 |
| Ctrl+I | 알림 패널 |
| Ctrl+= / Ctrl+- | 폰트 줌 인/아웃 |
| Ctrl+0 | 폰트 줌 리셋 |
| F11 (Cmd+Enter) | 전체화면 토글 |

### 복사/붙여넣기 키 정책

세 가지 방식을 제공하며, 각각 설정에서 독립적으로 ON/OFF 할 수 있다.

| 방식 | 복사 | 붙여넣기 | 기본 ON |
|------|------|----------|---------|
| macOS 방식 | Alt+C | Alt+V | macOS만 |
| Linux 방식 | Ctrl+Shift+C | Ctrl+Shift+V | Linux만 |
| Windows 방식 | Ctrl+C (선택 있을 때) / Ctrl+C (선택 없으면 PTY 전달) | Ctrl+V | Windows만 |

**동작 규칙:**

- 세 가지를 모두 활성화하면 Alt+C/V, Ctrl+C/V, Ctrl+Shift+C/V 전부 복사/붙여넣기로 동작한다
- Cmd 키는 Alt에 매핑한다 (물리적 위치 일치). Ctrl에 매핑하지 않는다
- 각 OS는 자신의 네이티브 방식이 기본 ON이지만, 사용자가 원하는 조합을 자유롭게 활성화할 수 있다
- Windows 방식이 ON이고 텍스트 선택이 없을 때: Ctrl+C → PTY에 SIGINT 전달

### 커스터마이징

설정 파일(TOML)에서 키 바인딩 재정의:

```toml
[keybindings]
new_workspace = "ctrl+n"
split_vertical = "ctrl+d"
split_horizontal = "ctrl+shift+d"
font_zoom_in = "ctrl+equal"
fullscreen = "f11"
```

### OS별 수정자 키 매핑

| 수정자 | Windows/Linux | macOS |
|--------|-------------|-------|
| Primary | Ctrl | Ctrl |
| Secondary | Alt | Alt (= Cmd 물리 위치) |
| Tertiary | Win (Super) | Ctrl |

Cmd 키는 물리적 위치 기준으로 Alt에 매핑한다. 이는 macOS 키보드의 Cmd 위치가 Windows/Linux의 Alt 위치와 일치하기 때문이다.

### 네이티브 GUI의 이점

OS 레벨 키 이벤트를 직접 받으므로:

- **모든 수정자 조합** 사용 가능 (Ctrl+Shift+Alt+키 등)
- **시그널 충돌 없음**: Ctrl+C/Z/D를 자유롭게 바인딩 또는 PTY에 전달 선택 가능
- **물리/논리 키 구분**: 키보드 레이아웃에 무관한 물리 키 바인딩 지원
- **글로벌 단축키**: OS별 API로 앱이 백그라운드일 때도 단축키 가능 (선택적)

## 최적화 전략

- **키맵 조회 O(1)**: HashMap 기반 키 바인딩 조회로 선형 탐색을 회피한다. `(KeyCode, Modifiers)` 튜플을 키로 사용한다.
- **키 이벤트 빠른 경로**: 일반 문자 입력은 키맵 조회 없이 바로 PTY로 전달한다. 수정자 키가 없는 단순 문자 이벤트는 바인딩 확인을 스킵한다.
- **수정자 키 필터**: 수정자 키만 눌린 이벤트(Ctrl, Shift, Alt 단독)는 즉시 무시하여 불필요한 처리를 방지한다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | Ctrl 기반 기본 매핑 |
| macOS | ✅ 가능 | Cmd 기반 매핑 |
| Linux | ✅ 가능 | Ctrl 기반 기본 매핑, Wayland/X11 모두 지원 |
