# egui 레이아웃 주의사항

- `CentralPanel` 안에서 `add_space`로 수동 중앙 배치하면 자식 Frame이 부모 너비를 무시하고 넘칠 수 있다.
- 정중앙 배치가 필요하면 `egui::Window`에 `.anchor(Align2::CENTER_CENTER, vec2(0,0))`을 쓰는 것이 안정적이다.
- wgpu 24에서 egui render pass에 `forget_lifetime()`이 필요하다 (`'static` 라이프타임 요구).
- **레이어 순서**: `LayerId::background()`는 모든 egui 패널 뒤에 렌더링된다. egui로 터미널 위에 무언가를 그리려면 `LayerId::new(Order::Foreground, ...)`를 사용해야 한다.
