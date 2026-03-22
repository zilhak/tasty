# main.rs 분할 계획

`src/main.rs` (891줄)를 main.rs + 2개 파일로 분할한다.

## 현재 구조 분석

| 줄 범위 | 내용 |
|---------|------|
| 1-13 | mod 선언 (14개 모듈) |
| 15-29 | use 선언 |
| 31-48 | `ClipboardContext` struct + impl |
| 50-55 | `AppEvent` enum |
| 57-70 | `DividerDragKind` enum, `DividerDrag` struct |
| 72-107 | `App` struct + `App::new` |
| 110-138 | `App::compute_terminal_rect_with_sidebar`, `App::paste_to_terminal` |
| 140-344 | `App::handle_shortcut` (205줄) |
| 346-366 | `App::process_ipc` |
| 368-378 | `impl ApplicationHandler for App`: `user_event` |
| 380-447 | `impl ApplicationHandler for App`: `resumed` |
| 449-865 | `impl ApplicationHandler for App`: `window_event` (417줄) |
| 868-891 | `fn main()` |

## 분할 후 구조

```
src/main.rs             — main(), App struct, resumed(), user_event()
src/event_handler.rs    — impl App: window_event (ApplicationHandler의 핵심 이벤트 처리)
src/shortcuts.rs        — impl App: handle_shortcut, paste_to_terminal
```

## 각 파일 상세

### main.rs (~250줄)

진입점과 App 정의를 담는다.

**포함 내용:**
- mod 선언 (줄 1-13) + `mod event_handler;` + `mod shortcuts;` 추가
- use 선언 (줄 15-29)
- `ClipboardContext` (줄 31-48)
- `AppEvent` enum (줄 50-55)
- `DividerDragKind`, `DividerDrag` (줄 57-70)
- `App` struct (줄 72-91)
- `App::new` (줄 93-108)
- `App::compute_terminal_rect_with_sidebar` (줄 110-114)
- `App::process_ipc` (줄 346-366)
- `impl ApplicationHandler for App`: `user_event` (줄 368-378)
- `impl ApplicationHandler for App`: `resumed` (줄 380-447)
- `fn main` (줄 868-891)

### shortcuts.rs (~220줄)

키보드 단축키 처리와 클립보드 붙여넣기.

**포함 내용:**
- `App::paste_to_terminal` (줄 116-138)
- `App::handle_shortcut` (줄 140-344)

**의존:**
- `super::{App, DividerDragKind}` — App의 self 참조
- `super::model::SplitDirection`
- winit: `Key`, `ModifiersState`, `NamedKey`

### event_handler.rs (~430줄)

winit WindowEvent 처리의 본체.

**포함 내용:**
- `impl ApplicationHandler<AppEvent> for App`: `window_event` 메서드 (줄 449-865)

이 메서드가 main.rs에서 가장 큰 단일 블록(417줄)이다. 내부적으로 다음 이벤트를 처리한다:
- `CloseRequested` (줄 461-463)
- `Resized` (줄 464-477)
- `ScaleFactorChanged` (줄 478-483)
- `Focused` (줄 484-490)
- `Occluded` (줄 491-496)
- `ModifiersChanged` (줄 497-499)
- `KeyboardInput` (줄 500-594)
- `CursorMoved` (줄 595-641)
- `CursorLeft` (줄 642-647)
- `MouseInput` (줄 648-706)
- `MouseWheel` (줄 707-737)
- `RedrawRequested` (줄 738-855)

**의존:**
- `super::{App, AppEvent, DividerDrag, DividerDragKind}`
- `super::model::SplitDirection`
- `super::{hooks, notification, terminal}`
- winit 타입

## impl 블록 분산 방식

main.rs의 `App`에 대해 세 파일에서 `impl` 블록을 분산한다.

```rust
// shortcuts.rs
use super::App;

impl App {
    pub(crate) fn paste_to_terminal(&mut self) { ... }
    pub(crate) fn handle_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool { ... }
}
```

```rust
// event_handler.rs
use super::{App, AppEvent, DividerDrag, DividerDragKind};
use winit::application::ApplicationHandler;

impl ApplicationHandler<AppEvent> for App {
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // ... 전체 이벤트 처리 로직
    }
}
```

핵심 포인트:
- `ApplicationHandler` trait의 `window_event`는 단일 메서드이지만 내부 로직이 400줄이므로 별도 파일이 합리적이다.
- `handle_shortcut`은 `window_event` 내부에서 호출되므로 (줄 524) `pub(crate)` 가시성이 필요하다.
- `paste_to_terminal`은 `handle_shortcut` 내부에서 호출되므로 (줄 282, 294, 306) 같은 파일에 둔다.
- trait impl은 Rust에서 별도 파일에 둘 수 있다. `user_event`/`resumed`는 짧으므로 main.rs에 남기고, `window_event`만 분리한다. 단, trait impl은 하나의 블록이어야 하므로 **`user_event`와 `resumed`도 event_handler.rs로 이동**하거나, main.rs에서 전체 `impl ApplicationHandler`를 유지해야 한다.

**권장 방식:** `impl ApplicationHandler<AppEvent> for App` 전체를 `event_handler.rs`에 두고, main.rs에서는 `App` struct와 고유 메서드만 남긴다.

수정된 구조:

| 파일 | 내용 |
|------|------|
| `main.rs` | App struct, new, compute_terminal_rect_with_sidebar, process_ipc, main() |
| `shortcuts.rs` | handle_shortcut, paste_to_terminal |
| `event_handler.rs` | impl ApplicationHandler for App (user_event, resumed, window_event) |
