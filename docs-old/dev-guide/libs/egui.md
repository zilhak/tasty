# egui 0.31 사용 가이드

egui 0.31 + egui-winit 0.31 + egui-wgpu 0.31 기준.

---

## 목차

1. [Context](#1-context)
2. [Window](#2-window)
3. [Ui 위젯](#3-ui-위젯)
4. [Layout, Align, vec2](#4-layout-align-vec2)
5. [고급 위젯](#5-고급-위젯)
6. [Response](#6-response)
7. [egui-winit State](#7-egui-winit-state)
8. [egui-wgpu Renderer](#8-egui-wgpu-renderer)
9. [이벤트 흐름도](#9-이벤트-흐름도)

---

## 1. Context

`egui::Context`는 egui 상태 전체를 보유하는 핵심 객체다. `Arc` 내부로 Clone이 저렴하다.

```rust
use egui::Context;

let ctx = Context::default();
```

### 주요 메서드

#### `run(raw_input, run_ui) -> FullOutput`

한 프레임을 처리한다. raw_input을 소비하고 UI 클로저를 실행한 뒤 `FullOutput`을 반환한다.

```rust
let full_output = ctx.run(raw_input, |ctx| {
    egui::Window::new("내 창").show(ctx, |ui| {
        ui.label("Hello");
    });
});
```

`FullOutput`에는 다음이 포함된다:
- `platform_output` — 클립보드 복사, 커서 변경 등 플랫폼 명령
- `textures_delta` — 텍스처 추가/삭제 정보
- `shapes` — 렌더링할 도형 목록
- `pixels_per_point` — DPI 배율

#### `begin_pass(raw_input) -> &mut Ui`  /  `end_pass() -> FullOutput`

`run()`을 둘로 쪼갠 저수준 API. 두 호출 사이에서 직접 UI를 구성할 때 사용한다.

```rust
ctx.begin_pass(raw_input);
// … UI 구성 …
let full_output = ctx.end_pass();
```

#### `wants_keyboard_input() -> bool`

egui가 키보드 입력을 소비하길 원하면 `true`. 텍스트 필드에 포커스가 있을 때 해당된다.
터미널 에뮬레이터에서는 이 값이 `false`일 때만 키 이벤트를 PTY로 전달해야 한다.

```rust
if !ctx.wants_keyboard_input() {
    // 키 이벤트를 터미널로 전달
}
```

#### `wants_pointer_input() -> bool`

egui가 마우스/터치 입력을 소비하길 원하면 `true`. 위젯 위에 포인터가 올라가 있을 때 해당된다.

```rust
if !ctx.wants_pointer_input() {
    // 마우스 이벤트를 터미널로 전달
}
```

#### `request_repaint()`

다음 프레임을 즉시 요청한다. 비동기 작업 완료 후 UI를 갱신할 때 호출한다.

```rust
ctx.request_repaint();
```

`request_repaint_after(Duration)` — 지연 시간 후 repaint를 예약한다.

---

## 2. Window

`egui::Window`는 드래그 가능한 플로팅 패널이다.

### 생성

```rust
egui::Window::new("창 제목")
```

제목 문자열은 ID로도 사용된다. 같은 제목의 창이 두 개 필요하면 `.id(egui::Id::new("unique"))`로 구분한다.

### 빌더 메서드

| 메서드 | 설명 |
|--------|------|
| `.open(&mut bool)` | 닫기 버튼을 표시하고 bool을 연동한다 |
| `.fixed_size(vec2)` | 크기를 고정한다. 사용자가 리사이즈 불가 |
| `.collapsible(bool)` | 제목 더블클릭으로 접기 허용 여부 |
| `.anchor(Align2, vec2)` | 화면 기준 앵커 위치와 오프셋 |
| `.interactable(bool)` | `false`이면 클릭/드래그 무시 |
| `.resizable(bool)` | 리사이즈 핸들 표시 여부 |
| `.default_pos(pos2)` | 첫 프레임 위치 |
| `.default_size(vec2)` | 첫 프레임 크기 |
| `.min_width(f32)` | 최소 너비 |
| `.frame(Frame)` | 배경, 테두리, 패딩 커스터마이즈 |

### `show()` 반환값

```rust
let response: Option<egui::InnerResponse<Option<R>>> =
    egui::Window::new("제목").show(ctx, |ui| { /* … */ });
```

반환 타입이 중첩된 `Option`이므로 주의가 필요하다:

| 경우 | 반환값 |
|------|--------|
| 창이 표시되지 않음 (`open`이 false) | `None` |
| 창이 접혀 있음 (collapsed) | `Some(InnerResponse { inner: None, … })` |
| 창이 보임 | `Some(InnerResponse { inner: Some(R), … })` |

```rust
if let Some(inner_response) = egui::Window::new("설정")
    .open(&mut self.show_settings)
    .show(ctx, |ui| {
        ui.label("설정 내용");
    })
{
    // inner_response.inner 는 None(접힘) 또는 Some(())(표시됨)
    let _ = inner_response.response; // 창 자체의 Response
}
```

### 예시: 앵커 고정 창

```rust
egui::Window::new("상태바")
    .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(0.0, 0.0))
    .fixed_size(egui::vec2(200.0, 30.0))
    .collapsible(false)
    .interactable(false)
    .show(ctx, |ui| {
        ui.label("연결됨");
    });
```

---

## 3. Ui 위젯

`egui::Ui`는 위젯을 배치하는 컨텍스트다. `show()` 클로저 인자로 전달된다.

### 기본 위젯

#### `label`

```rust
ui.label("텍스트");
ui.label(egui::RichText::new("굵게").strong());
ui.label(egui::RichText::new("색상").color(egui::Color32::RED));
```

#### `button`

```rust
if ui.button("클릭").clicked() {
    // 처리
}
```

#### `checkbox`

```rust
ui.checkbox(&mut self.enabled, "활성화");
```

#### `text_edit_singleline`

```rust
ui.text_edit_singleline(&mut self.input_text);
```

멀티라인은 `text_edit_multiline`. 포커스 제어:

```rust
let response = ui.text_edit_singleline(&mut self.text);
if response.gained_focus() {
    // 포커스 획득
}
```

#### `heading`

```rust
ui.heading("섹션 제목");
```

#### `separator`

```rust
ui.separator(); // 수평선
```

#### `add_space`

```rust
ui.add_space(8.0); // 픽셀 단위 여백
```

### 레이아웃 컨테이너

#### `horizontal`

```rust
ui.horizontal(|ui| {
    ui.label("A");
    ui.label("B");
});
```

#### `vertical`

```rust
ui.vertical(|ui| {
    ui.label("위");
    ui.label("아래");
});
```

#### `with_layout`

```rust
ui.with_layout(
    egui::Layout::right_to_left(egui::Align::Center),
    |ui| {
        ui.button("우측 정렬");
    },
);
```

---

## 4. Layout, Align, vec2

### Layout

레이아웃은 위젯의 배치 방향과 정렬을 결정한다.

```rust
egui::Layout::left_to_right(egui::Align::Center)
egui::Layout::right_to_left(egui::Align::Center)
egui::Layout::top_down(egui::Align::LEFT)
egui::Layout::bottom_up(egui::Align::Center)
```

`Layout::left_to_right(align)` — 가로 배치, `align`은 교차축(세로) 정렬.

### Align

```rust
egui::Align::LEFT
egui::Align::Center
egui::Align::RIGHT
egui::Align::Min   // LEFT와 동일
egui::Align::Max   // RIGHT와 동일
```

### Align2

2D 정렬. `Window::anchor()` 등에서 사용한다.

```rust
egui::Align2::LEFT_TOP
egui::Align2::LEFT_CENTER
egui::Align2::LEFT_BOTTOM
egui::Align2::CENTER_TOP
egui::Align2::CENTER_CENTER
egui::Align2::CENTER_BOTTOM
egui::Align2::RIGHT_TOP
egui::Align2::RIGHT_CENTER
egui::Align2::RIGHT_BOTTOM
```

### vec2 / pos2

```rust
let size = egui::vec2(100.0, 50.0);  // Vec2
let pos  = egui::pos2(10.0, 20.0);   // Pos2
```

`Rect::from_min_size(pos2, vec2)` — 사각형 생성.

---

## 5. 고급 위젯

### Grid

열 정렬이 맞는 테이블형 레이아웃.

```rust
egui::Grid::new("내_그리드")
    .num_columns(2)
    .spacing(egui::vec2(8.0, 4.0))
    .striped(true)
    .show(ui, |ui| {
        ui.label("이름");
        ui.text_edit_singleline(&mut self.name);
        ui.end_row();

        ui.label("나이");
        ui.add(egui::DragValue::new(&mut self.age));
        ui.end_row();
    });
```

`ui.end_row()`를 잊으면 다음 행으로 넘어가지 않는다.

### ScrollArea

```rust
egui::ScrollArea::vertical()
    .max_height(200.0)
    .auto_shrink([false, true])
    .show(ui, |ui| {
        for item in &self.items {
            ui.label(item);
        }
    });
```

`ScrollArea::both()` — 양방향 스크롤.
`ScrollArea::horizontal()` — 가로 전용.

스크롤 위치 제어:

```rust
egui::ScrollArea::vertical()
    .scroll_offset(egui::vec2(0.0, self.scroll_y))
    .show(ui, |ui| { /* … */ });
```

### DragValue

드래그로 숫자를 조정하는 인라인 위젯.

```rust
ui.add(egui::DragValue::new(&mut self.value)
    .speed(0.1)
    .range(0.0..=100.0)
    .suffix(" px"));
```

### Slider

슬라이더.

```rust
ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0)
    .text("볼륨")
    .show_value(true));
```

### radio_value

열거형 선택에 적합.

```rust
ui.radio_value(&mut self.mode, Mode::Normal, "일반");
ui.radio_value(&mut self.mode, Mode::Insert, "삽입");
```

### selectable_label

토글 가능한 레이블. 탭 UI에 유용.

```rust
if ui.selectable_label(self.tab == Tab::Settings, "설정").clicked() {
    self.tab = Tab::Settings;
}
```

---

## 6. Response

거의 모든 위젯 메서드는 `egui::Response`를 반환한다.

| 메서드 | 설명 |
|--------|------|
| `.clicked()` | 이번 프레임에 클릭됐으면 true |
| `.double_clicked()` | 더블클릭 여부 |
| `.changed()` | 값이 변경됐으면 true (checkbox, slider 등) |
| `.hovered()` | 포인터가 올라가 있으면 true |
| `.has_focus()` | 키보드 포커스를 갖고 있으면 true |
| `.gained_focus()` | 이번 프레임에 포커스를 얻으면 true |
| `.lost_focus()` | 이번 프레임에 포커스를 잃으면 true |
| `.dragged()` | 드래그 중이면 true |
| `.drag_delta()` | 이번 프레임의 드래그 이동량 `Vec2` |
| `.rect` | 위젯이 차지한 영역 `Rect` |
| `.interact_rect` | 실제 인터랙션 영역 `Rect` |

```rust
let r = ui.button("전송");
if r.clicked() {
    self.send();
}
if r.hovered() {
    r.on_hover_text("클릭하면 전송합니다");
}
```

`on_hover_text()`는 `Response`를 소비한 뒤 반환하므로 체이닝 가능하다.

---

## 7. egui-winit State

`egui_winit::State`는 winit 이벤트를 egui `RawInput`으로 변환한다.

### 초기화

```rust
use egui_winit::State;

let state = State::new(
    ctx.clone(),
    egui::ViewportId::ROOT,
    &window,        // &winit::window::Window
    None,           // Option<f32> — DPI override
    None,           // Option<usize> — max texture side override
);
```

### `on_window_event(window, event) -> EventResponse`

winit `WindowEvent`를 처리하고 `EventResponse`를 반환한다.

```rust
use egui_winit::EventResponse;

let EventResponse { consumed, repaint } =
    state.on_window_event(&window, &event);
```

| 필드 | 의미 |
|------|------|
| `consumed` | egui가 이 이벤트를 소비했으므로 터미널에 전달하면 안 됨 |
| `repaint` | 이 이벤트로 인해 다음 프레임 렌더링이 필요함 |

### 터미널에서의 올바른 사용 패턴

```rust
fn handle_window_event(
    &mut self,
    window: &winit::window::Window,
    event: &winit::event::WindowEvent,
) {
    // 1. egui-winit에 먼저 전달
    let egui_winit::EventResponse { consumed, repaint } =
        self.egui_state.on_window_event(window, event);

    if repaint {
        window.request_redraw();
    }

    // 2. egui가 소비하지 않은 이벤트만 터미널로 전달
    if !consumed {
        self.terminal.handle_window_event(event);
    }
}
```

**주의:** `wants_keyboard_input()` / `wants_pointer_input()`과 `consumed`는 다르다.
`consumed`는 이미 처리된 이벤트 여부, `wants_*`는 앞으로의 의향이다.
이벤트 라우팅에는 `consumed`를 사용하는 것이 더 정확하다.

### `take_egui_input(window) -> RawInput`

매 프레임 호출해 `RawInput`을 수집한다.

```rust
let raw_input = state.take_egui_input(&window);
let full_output = ctx.run(raw_input, |ctx| { /* UI */ });
state.handle_platform_output(&window, full_output.platform_output);
```

### `handle_platform_output(window, platform_output)`

egui가 요청한 클립보드 복사, IME 위치 업데이트, 커서 변경 등을 처리한다. 매 프레임 호출 필수.

---

## 8. egui-wgpu Renderer

`egui_wgpu::Renderer`는 egui 도형을 wgpu로 렌더링하고, 커스텀 wgpu 렌더링 위에 오버레이하는 용도로 사용한다.

### 초기화

```rust
use egui_wgpu::Renderer;

let renderer = Renderer::new(
    &device,
    surface_format,   // wgpu::TextureFormat
    None,             // Option<wgpu::TextureFormat> — depth format
    1,                // msaa sample count
    false,            // dithering
);
```

### 매 프레임 처리

```rust
// 1. egui 프레임 실행
let full_output = ctx.run(raw_input, |ctx| { /* UI */ });

// 2. 텍스처 업데이트
let clipped_primitives = ctx.tessellate(
    full_output.shapes,
    full_output.pixels_per_point,
);

for (id, image_delta) in &full_output.textures_delta.set {
    renderer.update_texture(&device, &queue, *id, image_delta);
}
for id in &full_output.textures_delta.free {
    renderer.free_texture(id);
}

// 3. 스크린 디스크립터
let screen_descriptor = egui_wgpu::ScreenDescriptor {
    size_in_pixels: [surface_width, surface_height],
    pixels_per_point: full_output.pixels_per_point,
};

// 4. 커스텀 렌더링 + egui 오버레이
let mut encoder = device.create_command_encoder(&Default::default());

// 4a. 터미널 렌더 패스 (커스텀 wgpu)
{
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &surface_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    terminal_renderer.render(&mut render_pass);
}

// 4b. egui 업데이트 (렌더 패스 외부)
renderer.update_buffers(
    &device,
    &queue,
    &mut encoder,
    &clipped_primitives,
    &screen_descriptor,
);

// 4c. egui 렌더 패스 (Load::Load 로 기존 내용 유지)
{
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &surface_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,  // 터미널 위에 오버레이
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    renderer.render(&mut render_pass, &clipped_primitives, &screen_descriptor);
}

queue.submit([encoder.finish()]);
```

**핵심:** 터미널 렌더 패스는 `LoadOp::Clear`, egui 렌더 패스는 `LoadOp::Load`.
순서가 반대가 되면 egui 내용이 지워진다.

### 커스텀 wgpu 콜백

egui UI 내부에 wgpu 렌더링 결과를 삽입할 수 있다.

```rust
use egui_wgpu::CallbackFn;

let callback = egui_wgpu::Callback::new_paint_callback(
    rect,
    MyWgpuCallback { /* 커스텀 데이터 */ },
);
ui.painter().add(callback);
```

`MyWgpuCallback`은 `egui_wgpu::CallbackTrait`을 구현해야 한다.

---

## 9. 이벤트 흐름도

```
winit EventLoop
    │
    ▼
WindowEvent (KeyboardInput, CursorMoved, MouseInput, …)
    │
    ▼
egui_winit::State::on_window_event()
    │
    ├─ EventResponse.consumed = true  ──────────────────────────────┐
    │                                                                │
    └─ EventResponse.consumed = false                               │
         │                                                          │
         ▼                                                          │
    터미널 입력 처리                                                │
    (PTY에 키 전달 등)                                              │
                                                                    │
                                                                    ▼
                                               egui가 이벤트 소비 완료
                                               (텍스트 필드 입력 등)

매 프레임:
    │
    ├─ state.take_egui_input(&window)
    │       │
    │       ▼
    │   RawInput (mouse pos, keys, screen rect, …)
    │       │
    │       ▼
    │   ctx.run(raw_input, |ctx| { UI 구성 })
    │       │
    │       ▼
    │   FullOutput
    │       │
    │       ├─ shapes → tessellate → clipped_primitives
    │       ├─ textures_delta → renderer.update_texture()
    │       └─ platform_output → state.handle_platform_output()
    │
    ▼
wgpu 렌더링:
    1. 터미널 렌더 패스 (LoadOp::Clear)
    2. renderer.update_buffers()
    3. egui 렌더 패스 (LoadOp::Load)
    4. queue.submit()
```

### 입력 우선순위 정리

| 이벤트 종류 | egui 우선 처리 | 터미널 전달 조건 |
|-------------|---------------|-----------------|
| 키보드 입력 | 텍스트 필드 포커스 시 | `consumed == false` |
| 마우스 클릭 | 위젯 위에 있을 때 | `consumed == false` |
| 마우스 이동 | 항상 egui에 전달됨 | `consumed == false` |
| 스크롤 | ScrollArea 위에 있을 때 | `consumed == false` |
