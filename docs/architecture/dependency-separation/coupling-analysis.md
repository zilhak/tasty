# 커플링 핫스팟 분석

파일 분할과 라이브러리 추출 시 고려해야 할 타입별 참조 관계를 분석한다.

## 가장 많이 참조되는 타입 Top 15

| # | 타입 | 정의 위치 | 참조하는 파일 | 배치 권장 |
|---|------|----------|-------------|----------|
| 1 | `Terminal` | terminal.rs:56 | model.rs, state.rs, ipc/handler.rs (간접), main.rs | `tasty-terminal` crate |
| 2 | `Waker` | terminal.rs:19 | model.rs:1, state.rs:6, main.rs:419 | `tasty-terminal` crate |
| 3 | `Rect` | model.rs:10 | main.rs:28, gpu.rs:7, renderer.rs:6, ui.rs:3, state.rs:2 | `tasty-model` crate (공통 타입) |
| 4 | `SplitDirection` | model.rs:1366 | main.rs:28, state.rs:2, ipc/handler.rs:5 | `tasty-model` crate |
| 5 | `AppState` | state.rs | main.rs, gpu.rs, ui.rs, ipc/handler.rs | 앱 바이너리 (분리 불필요) |
| 6 | `Workspace` | model.rs:74 | state.rs:2, ipc/handler.rs (간접) | `tasty-model` crate |
| 7 | `PaneNode` | model.rs:148 | state.rs (간접) | `tasty-model` crate |
| 8 | `Pane` | model.rs:481 | state.rs, ipc/handler.rs (간접) | `tasty-model` crate |
| 9 | `Panel` | model.rs:714 | ipc/handler.rs:244 | `tasty-model` crate |
| 10 | `SurfaceGroupLayout` | model.rs:980 | ipc/handler.rs:266 | `tasty-model` crate |
| 11 | `DividerInfo` | model.rs:1358 | main.rs:28, state.rs:2 | `tasty-model` crate |
| 12 | `TerminalEvent` | terminal.rs:22 | state.rs:6, main.rs (간접) | `tasty-terminal` crate |
| 13 | `TerminalEventKind` | terminal.rs:29 | main.rs:759,781,803,807,811,817 | `tasty-terminal` crate |
| 14 | `MouseTrackingMode` | terminal.rs:49 | terminal.rs 내부 | `tasty-terminal` crate |
| 15 | `CellRenderer` | renderer.rs:196 | gpu.rs (간접) | `tasty-renderer` crate |

## 의존성 방향 그래프

현재 모듈 간 import 방향:

```
terminal.rs  ←─── model.rs ←─── state.rs ←─── main.rs
    │                │               │             │
    │                │               │             ├── gpu.rs ←── renderer.rs
    │                │               │             │                  │
    │                │               │             │              font.rs
    │                │               │             │
    │                │               │             ├── ui.rs
    │                │               │             │
    │                │               │             ├── ipc/handler.rs
    │                │               │             │
    │                │               │             ├── hooks.rs
    │                │               │             │
    │                │               │             └── notification.rs
    │                │               │
    └── settings.rs ─┘               │
                                     └── settings_ui.rs
```

## 라이브러리 추출 시 끊어야 할 의존성

### terminal.rs → settings.rs (줄 1092)

```rust
fn default_shell() -> String {
    crate::settings::GeneralSettings::detect_shell()
}
```

**해결:** `Terminal::new_with_shell`의 shell 인자를 필수로 변경하거나, `default_shell` 로직을 crate 외부로 이동.

### model.rs → terminal.rs (줄 1)

```rust
use crate::terminal::{Terminal, Waker};
```

model.rs의 `Pane`, `Tab`, `Panel`, `SurfaceNode`이 `Terminal` 타입을 직접 소유한다. 이것은 `tasty-model` crate가 `tasty-terminal` crate에 의존해야 함을 의미한다.

**해결 옵션:**
1. **직접 의존 허용**: `tasty-model`이 `tasty-terminal`에 의존 → 가장 간단
2. **제네릭화**: `SurfaceNode<T>`로 터미널 타입을 매개변수화 → 과잉 설계
3. **trait 추상화**: `trait TerminalLike`을 정의하고 trait object 사용 → 성능 오버헤드

권장: 옵션 1. `model → terminal` 방향은 자연스러운 의존이다.

### state.rs → 다수 모듈

state.rs는 model, terminal, settings, notification, hooks, settings_ui를 모두 참조한다. AppState는 앱 바이너리의 통합 레이어이므로 crate 추출 대상이 아니다.

## 순환 의존성 검사

현재 코드에 **순환 의존성은 없다**.

| 관계 | 방향 | 순환 여부 |
|------|------|----------|
| terminal ↔ model | model → terminal (단방향) | 없음 |
| model ↔ state | state → model (단방향) | 없음 |
| terminal ↔ state | state → terminal (단방향) | 없음 |
| renderer ↔ model | renderer → model (단방향) | 없음 |
| handler ↔ state | handler → state (단방향) | 없음 |
| handler ↔ model | handler → model (단방향) | 없음 |
| handler ↔ hooks | handler → hooks (단방향) | 없음 |

모든 의존성이 DAG(Directed Acyclic Graph)를 형성한다.

## 분할 후 import 그래프

파일 분할 후 모듈 경계가 변해도 import 그래프의 방향은 유지된다.

```
src/terminal/
    mod.rs  ────────── events.rs
    │                  (Waker, TerminalEvent,
    │                   MouseTrackingMode)
    ├── vte_handler.rs
    └── modes.rs

         ▲
         │ use crate::terminal::{Terminal, Waker}
         │
src/model/
    mod.rs  ────────── (Rect, SplitDirection, DividerInfo, type aliases)
    │
    ├── workspace.rs   (Workspace)
    ├── pane.rs        (PaneNode, Pane, Tab)
    ├── panel.rs       (Panel, SurfaceNode)
    └── surface_group.rs (SurfaceGroupNode, SurfaceGroupLayout)

         ▲
         │ use crate::model::{...}
         │
    ┌────┴────────────────────────────────────┐
    │                                          │
src/state.rs                          src/renderer/
    │                                     mod.rs
    │                                     ├── types.rs
    ▲                                     ├── shaders.rs
    │                                     └── palette.rs
    │
src/main.rs
    ├── event_handler.rs
    └── shortcuts.rs
    │
    ├── src/ipc/handler/
    │       mod.rs
    │       ├── surface.rs
    │       └── hooks.rs
    │
    ├── gpu.rs
    ├── ui.rs
    ├── notification.rs
    ├── hooks.rs
    ├── settings.rs
    ├── settings_ui.rs
    ├── font.rs
    └── cli.rs
```

## 핵심 관찰

1. **Terminal은 가장 깊은 기반 타입이다.** 다른 어떤 모듈도 Terminal에 의존하지만, Terminal은 settings.rs 외에 다른 앱 모듈에 의존하지 않는다. crate 추출에 가장 적합한 후보.

2. **Rect는 가장 넓게 사용되는 타입이다.** 5개 파일에서 참조된다. model crate에 두되 re-export가 필요하다.

3. **AppState는 분리 불가능한 통합 지점이다.** 모든 모듈을 조합하므로 crate 추출 대상이 아니다. 앱 바이너리에 남긴다.

4. **handler → model/state 의존은 약하다.** handler.rs가 model의 Panel, SurfaceGroupLayout에 직접 접근하는 부분 (줄 244-286)을 state.rs의 메서드로 캡슐화하면 handler → model 직접 의존을 제거할 수 있다.
