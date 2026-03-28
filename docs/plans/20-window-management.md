# 20. 윈도우 관리

> **참고**: 멀티 윈도우의 최종 설계는 `docs/design/multi-window-architecture.md`와 `docs/design/focus-policy.md`에 기술되어 있다. 이 문서는 초기 계획으로, 최종 설계와 다른 부분이 있을 수 있다.

## cmux 구현 방식

- 다중 윈도우 (NSWindow)
- 전체화면 (macOS 네이티브)
- 트래픽 라이트 커스텀 오프셋
- 윈도우 사이클링
- 포커스-follows-마우스

## 크로스 플랫폼 구현 방안

### 네이티브 GUI 윈도우 관리

네이티브 GUI 앱이므로 cmux의 윈도우 관리 기능을 그대로 구현할 수 있다.
다중 OS 윈도우, 전체화면, 위치 기억 등이 모두 가능하다.

### 핵심 기능

| 기능 | 구현 방법 |
|------|----------|
| 다중 윈도우 | winit `EventLoop`에서 여러 `Window` 생성 |
| 전체화면 | `window.set_fullscreen()` (Exclusive / Borderless) |
| 윈도우 위치/크기 | `window.set_outer_position()`, `window.set_inner_size()` |
| 최소화/최대화 | `window.set_minimized()`, `window.set_maximized()` |
| Always on Top | `window.set_window_level(AlwaysOnTop)` |
| 타이틀바 커스텀 | `window.set_decorations(false)` + 자체 렌더링 타이틀바 |
| 윈도우 사이클링 | Ctrl+` 또는 Cmd+` 로 윈도우 전환 |
| 포커스 관리 | `window.focus_window()` |

### 다중 윈도우 아키텍처

```rust
struct App {
    windows: HashMap<WindowId, TastyWindow>,
    workspaces: Vec<Workspace>,
}

struct TastyWindow {
    window: winit::window::Window,
    surface: wgpu::Surface,
    renderer: Renderer,
    active_workspace: WorkspaceId,
}
```

각 윈도우는 독립적인 wgpu Surface와 렌더러를 가진다.
워크스페이스는 윈도우 간에 이동할 수 있다.

### 타이틀바 커스터마이징

OS 기본 타이틀바를 숨기고 자체 렌더링으로 커스텀 타이틀바 구현:

```
┌─[●][●][●]── agent-1 — ~/project (main) ──────────────┐
│  ┌─ Sidebar ─┐  ┌─────────────────────────────────┐  │
│  │           │  │ Terminal                         │  │
│  ...
```

| OS | 타이틀바 | 비고 |
|----|---------|------|
| **Windows** | 커스텀 타이틀바 (DWM 통합 가능) | `window.set_decorations(false)` |
| **macOS** | 트래픽 라이트 위치 조정 가능 | `window.set_title_bar_transparent(true)` |
| **Linux** | CSD/SSD 모두 지원 | Wayland: CSD 기본, X11: SSD 기본 |

### 멀티 모니터

winit의 `MonitorHandle`로 모니터 정보 조회:
- 윈도우가 어떤 모니터에 있었는지 기억
- 전체화면 시 현재 모니터에서 전체화면
- 모니터 DPI 스케일링 대응

### 윈도우 상태 저장

세션 복원(08-session-persistence.md)과 연동:
- 각 윈도우의 위치, 크기, 모니터, 전체화면 상태 저장
- 앱 재시작 시 동일한 윈도우 배치 복원

## 최적화 전략

- **비활성 윈도우 렌더링 중단**: 최소화되거나 가려진 윈도우의 렌더링을 완전 중단한다. `winit`의 `Occluded` 이벤트를 활용한다.
- **윈도우 생성 풀링**: 윈도우 생성 비용을 줄이기 위해 사전 할당된 리소스 풀을 활용한다. wgpu Surface/Device를 미리 준비한다.
- **포커스 이벤트 기반 최적화**: 포커스를 잃은 윈도우는 프레임레이트를 크게 낮춘다 (예: 1fps). 포커스 복귀 시 즉시 정상 프레임레이트로 복원한다.
- **멀티 모니터 DPI**: 모니터 간 이동 시 DPI 변경을 감지하여 글리프 아틀라스를 재구축한다. DPI가 동일하면 재구축을 스킵한다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | 다중 윈도우, 전체화면, DPI 스케일링 |
| macOS | ✅ 가능 | 트래픽 라이트, 네이티브 전체화면, Retina |
| Linux | ✅ 가능 | Wayland/X11, CSD/SSD |

네이티브 GUI이므로 cmux의 윈도우 관리와 동등한 수준을 구현할 수 있다.
