# 기술적 분리 가능성

각 후보 크레이트의 현재 커플링, 필요한 추상화, breaking changes, 분리 난이도를 분석한다.

---

## 1. tasty-hooks (hooks.rs, 290줄)

### 현재 커플링

**없음.** tasty 내부 타입을 일절 참조하지 않는다.

```
hooks.rs:1  — use std::collections::HashSet;
hooks.rs:2  — use std::process::Command;
hooks.rs:4  — pub type HookId = u64;
hooks.rs:9  — pub surface_id: u32,   // 단순 u32, 타입 별칭 아님
```

외부 의존: `regex` (컴파일 타임 정규식), `serde` (HookEvent 직렬화).

### 필요한 trait 추상화

없음. 현재 API를 그대로 공개하면 된다.

### Breaking changes

없음. `crate::hooks` → `tasty_hooks`로 import 경로만 변경.

### 순환 의존 위험

없음. hooks.rs는 어떤 내부 모듈도 참조하지 않고, 다른 모듈(`state.rs:1`, `main.rs`, `ipc/handler.rs:3`)이 hooks를 일방적으로 import한다.

### 컴파일 타임 영향

`regex` 크레이트가 hooks에만 격리되므로, hooks를 변경하지 않는 빌드에서 regex 재컴파일 불필요. 긍정적.

### 분리 난이도: **즉시**

예상 작업 시간: 10분.

---

## 2. tasty-terminal (terminal.rs, 1,358줄)

### 현재 커플링

`terminal.rs`는 tasty 내부 타입을 참조하지 않는다:

```
terminal.rs:1   — use std::io::{Read, Write};
terminal.rs:6   — use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
terminal.rs:7-8 — use termwiz::cell::{...};
terminal.rs:16  — use termwiz::surface::{Change, CursorVisibility, Position, Surface};
```

반대로 `model.rs`가 terminal에 의존한다:

```
model.rs:1  — use crate::terminal::{Terminal, Waker};
```

`state.rs`도 terminal을 import한다:

```
state.rs:6  — use crate::terminal::{Terminal, TerminalEvent, Waker};
```

### 필요한 trait 추상화

없음. `Terminal`, `Waker`, `TerminalEvent`, `TerminalEventKind`, `MouseTrackingMode`을 그대로 공개 API로 노출.

### Breaking changes

- `crate::terminal` → `tasty_terminal` import 경로 변경 (model.rs, state.rs, main.rs, ipc/handler.rs)
- `termwiz::surface::Surface`가 공개 API에 노출됨 (`terminal.rs:47` — `pub fn surface(&self) -> &Surface`). tasty-terminal 사용자가 termwiz 버전에 종속.

### 순환 의존 위험

없음. 의존 방향이 단방향: `model.rs → terminal.rs`, `state.rs → terminal.rs`.

### 컴파일 타임 영향

`portable-pty`, `termwiz` 의존이 tasty-terminal에 격리되므로, 터미널 코드를 변경하지 않는 빌드에서 이 두 크레이트의 재컴파일 불필요. 긍정적.

### 분리 난이도: **즉시**

예상 작업 시간: 30분.

변경 대상 파일:
- `terminal.rs` → `crates/tasty-terminal/src/lib.rs`로 이동
- `model.rs:1` — import 경로 변경
- `state.rs:6` — import 경로 변경
- `main.rs` — `mod terminal` 제거, 외부 크레이트 import
- `ipc/handler.rs` — (간접 참조, state를 통해 접근하므로 변경 불필요할 수 있음)

---

## 3. tasty-ipc-protocol (ipc/protocol.rs, 131줄)

### 현재 커플링

**없음.** serde/serde_json만 의존.

```
ipc/protocol.rs:1 — use serde::{Deserialize, Serialize};
```

### 필요한 trait 추상화

없음.

### Breaking changes

import 경로 변경만. `crate::ipc::protocol` → `tasty_ipc_protocol`.

### 순환 의존 위험

없음.

### 컴파일 타임 영향

131줄이므로 무의미.

### 분리 난이도: **즉시**

예상 작업 시간: 5분. 단, 131줄짜리 파일을 별도 크레이트로 관리하는 것이 과연 이점이 있는지는 다른 관점 문서 참조.

---

## 4. tasty-ipc-server (ipc/server.rs, 196줄)

### 현재 커플링

`ipc/protocol.rs`에만 의존:

```
ipc/server.rs:11 — use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
```

`directories` 크레이트로 포트 파일 경로를 하드코딩:

```
ipc/server.rs:9 — use directories::BaseDirs;
```

포트 파일 경로 결정 로직 (`write_port_file`, `read_port_file`, `port_file_path`)이 tasty 고유 로직.

### 필요한 trait 추상화

포트 파일 경로를 매개변수로 받도록 리팩토링 필요. `directories` 의존을 선택적으로 만들거나 호출자에게 위임.

### Breaking changes

`port_file_path()` 시그니처 변경. `IpcServer::start()` 파라미터에 포트 파일 경로 추가.

### 분리 난이도: **중간**

포트 파일 로직 리팩토링이 필요. 예상 작업 시간: 1시간.

---

## 5. tasty-notification (notification.rs, 239줄)

### 현재 커플링

`model.rs`의 타입 별칭만 참조:

```
notification.rs:3 — use crate::model::{SurfaceId, WorkspaceId};
```

`SurfaceId`와 `WorkspaceId`는 모두 `u32` 타입 별칭(`model.rs:3-4`)이므로, 분리 시 직접 `u32`를 사용하거나 자체 타입 별칭을 정의하면 된다.

`notify-rust` 의존이 OS 알림 기능에 필요:

```
notification.rs:160 — let _ = notify_rust::Notification::new()
```

### 필요한 trait 추상화

없음. `u32`로 대체하면 충분.

### Breaking changes

`WorkspaceId`/`SurfaceId` 타입이 `u32`에서 자체 타입으로 변경될 수 있음. 단, 현재 `u32` 별칭이므로 실질적 변경 없음.

### 분리 난이도: **즉시**

예상 작업 시간: 15분.

---

## 6. tasty-settings (settings.rs, 326줄)

### 현재 커플링

```
settings.rs — use directories::BaseDirs; (설정 파일 경로)
settings.rs — use toml;
settings.rs — use serde::{Deserialize, Serialize};
```

`Settings` 구조체의 필드가 tasty 고유 설정(폰트, 사이드바 너비, 키바인딩 등)을 직접 정의한다. 범용적이지 않다.

### 분리 난이도: **중간**

타 프로젝트에서 재사용할 가치가 없으므로 기술적으로 가능하더라도 분리 동기 부재.

---

## 7. tasty-model (model.rs, 1,775줄)

### 현재 커플링

**핵심 의존: `terminal::Terminal`과 `terminal::Waker`**

```
model.rs:1 — use crate::terminal::{Terminal, Waker};
```

`Terminal` 타입은 `SurfaceNode` 구조체의 필드로 직접 소유됨:

```
model.rs:838-841:
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Terminal,
}
```

### 제네릭 전파 문제 (8단계)

`Terminal`을 trait으로 추상화하면 `SurfaceNode`가 제네릭이 되고, 이것이 전체 데이터 모델 계층을 오염시킨다:

```
단계 1: SurfaceNode<T: TerminalBackend>           (model.rs:838)
단계 2: SurfaceGroupLayout<T: TerminalBackend>     (model.rs:980)
         ├─ Single(SurfaceNode<T>)
         └─ Split { first: Box<SurfaceGroupLayout<T>>, ... }
단계 3: SurfaceGroupNode<T: TerminalBackend>       (model.rs:844)
         └─ layout_opt: Option<SurfaceGroupLayout<T>>
단계 4: Panel<T: TerminalBackend>                  (model.rs:714)
         ├─ Terminal(SurfaceNode<T>)
         └─ SurfaceGroup(SurfaceGroupNode<T>)
단계 5: Tab<T: TerminalBackend>                    (model.rs:678)
         └─ panel_opt: Option<Panel<T>>
단계 6: Pane<T: TerminalBackend>                   (model.rs:481)
         └─ tabs: Vec<Tab<T>>
단계 7: PaneNode<T: TerminalBackend>               (model.rs:148)
         ├─ Leaf(Pane<T>)
         └─ Split { first: Box<PaneNode<T>>, ... }
단계 8: Workspace<T: TerminalBackend>              (model.rs:74)
         └─ pane_layout_opt: Option<PaneNode<T>>
```

모든 메서드 시그니처에 `<T: TerminalBackend>`가 추가된다. 예시:

```rust
// 현재
impl PaneNode {
    pub fn all_terminals(&self) -> Vec<&Terminal> { ... }
}

// 분리 후
impl<T: TerminalBackend> PaneNode<T> {
    pub fn all_terminals(&self) -> Vec<&T> { ... }
}
```

**영향 범위:** `model.rs`에서 `Terminal`을 참조하는 메서드 약 30개 (all_terminals, all_terminals_mut, process_all, for_each_terminal, for_each_terminal_mut, focused_terminal, active_terminal, render_regions 등), `state.rs`의 `AppState`, `main.rs`의 `App`, `gpu.rs`의 `GpuState`, `ui.rs`, `renderer.rs`까지 전파.

### trait object 대안

제네릭 대신 `Box<dyn TerminalBackend>`를 사용하면 전파가 없다:

```rust
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Box<dyn TerminalBackend>,
}
```

단점:
- `TerminalBackend`가 `&Surface` 를 반환하는 `surface()` 메서드를 포함 (`terminal.rs:47` — `pub fn surface(&self) -> &Surface`)
- dynamic dispatch 오버헤드 (매 프레임 `surface()` 호출 시 vtable lookup)
- `Clone`, `Debug` 등 trait bound 제약

### 분리 난이도: **어려움**

제네릭 전파의 코드 복잡도 증가 vs dynamic dispatch의 성능 트레이드오프. 현재 시점에서는 비용이 이점을 초과.

---

## 8. tasty-renderer (font.rs + renderer.rs, 1,108줄)

### 현재 커플링

**termwiz::surface::Surface에 직접 의존:**

```
renderer.rs:3  — use termwiz::surface::Surface;
renderer.rs:515 — pub fn prepare(&mut self, surface: &Surface, queue: &wgpu::Queue) {
renderer.rs:635 — pub fn prepare_viewport(&mut self, surface: &Surface, ...
```

`prepare()` 메서드 내부에서 Surface의 다음 API를 사용:

```
renderer.rs:516 — let (cols, rows) = surface.dimensions();
renderer.rs:517 — let lines = surface.screen_lines();
renderer.rs:526 — for cell_ref in line.visible_cells() {
renderer.rs:527 — let col_idx = cell_ref.cell_index();
renderer.rs:532 — let attrs = cell_ref.attrs();
renderer.rs:533 — let bg_color = color_attr_to_rgba(&attrs.background(), DEFAULT_BG);
renderer.rs:534 — let fg_color = color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG);
renderer.rs:543 — let text = cell_ref.str();
renderer.rs:549 — let bold = attrs.intensity() == termwiz::cell::Intensity::Bold;
renderer.rs:550 — let italic = attrs.italic();
```

**model::Rect에 의존:**

```
renderer.rs:6   — use crate::model::Rect;
renderer.rs:622 — pub fn grid_size_for_rect(&self, rect: &Rect) -> (usize, usize) {
renderer.rs:637 — viewport: &Rect,
renderer.rs:661 — viewport: &Rect,
```

### TerminalSurface trait 설계 복잡도

분리하려면 `termwiz::surface::Surface` 대신 추상 trait을 받아야 한다:

```rust
pub trait TerminalSurface {
    fn dimensions(&self) -> (usize, usize);
    fn screen_lines(&self) -> Vec<ScreenLine>;  // 문제: 반환 타입
}

pub struct ScreenLine {
    pub cells: Vec<ScreenCell>,
}

pub struct ScreenCell {
    pub col_index: usize,
    pub text: String,
    pub foreground: CellColor,
    pub background: CellColor,
    pub bold: bool,
    pub italic: bool,
}
```

문제점:

1. **반환 타입 변환 비용**: `surface.screen_lines()`가 반환하는 termwiz 내부 타입 (`Line`, `Cell`)을 중간 표현 (`ScreenLine`, `ScreenCell`)으로 매 프레임 변환해야 함. 60fps에서 80x24 = 1,920셀, 매 프레임 1,920개 `ScreenCell` 할당.

2. **lifetime 문제**: termwiz의 `visible_cells()`는 iterator를 반환하고, 각 셀이 Line에 대한 참조. trait에서 이를 추상화하려면 GAT(Generic Associated Types)나 `Vec` 복사가 필요.

3. **Color 타입 추상화**: `termwiz::color::ColorAttribute`의 4가지 variant (Default, PaletteIndex, TrueColorWithPaletteFallback, TrueColorWithDefaultFallback)를 자체 `CellColor` enum으로 재정의해야 함. renderer.rs:178-192의 `color_attr_to_rgba` 함수 로직 전체가 관련.

4. **셀 속성**: `Intensity::Bold` (`renderer.rs:549`)와 `italic()` (`renderer.rs:550`) 접근을 추상화해야 함.

### Rect 의존 해결

`Rect`는 간단한 데이터 구조이므로 렌더러 크레이트에 자체 정의하거나, 별도 `tasty-types` 크레이트로 추출 가능. 이 부분은 난이도 낮음.

### 분리 난이도: **어려움**

TerminalSurface trait 설계가 핵심 난관. 매 프레임 데이터 변환 비용, lifetime 문제, Color/Attribute 타입 추상화. 예상 작업 시간: 2~3일.

---

## 분리 난이도 종합

| 후보 | 난이도 | 핵심 장벽 | 예상 시간 |
|------|--------|-----------|-----------|
| `tasty-hooks` | 즉시 | 없음 | 10분 |
| `tasty-ipc-protocol` | 즉시 | 없음 | 5분 |
| `tasty-notification` | 즉시 | 타입 별칭 교체 | 15분 |
| `tasty-terminal` | 즉시 | Surface 공개 API 노출 | 30분 |
| `tasty-ipc-server` | 중간 | 포트 파일 로직 리팩토링 | 1시간 |
| `tasty-settings` | 중간 | 범용화 가치 없음 | 1시간 |
| `tasty-model` | 어려움 | 8단계 제네릭 전파 | 1~2일 |
| `tasty-renderer` | 어려움 | TerminalSurface trait 설계 | 2~3일 |
