# state가 None일 때의 이벤트 처리

`AppState`가 아직 초기화되지 않은 상태(셸 설정 모드 등)에서도 `gpu.resize()`, `gpu.handle_egui_event()` 등 GPU/UI 관련 처리는 동작해야 한다. 기존 코드에서 `if let (Some(gpu), Some(state)) = ...` 패턴으로 묶여 있으면 state가 None일 때 gpu 호출도 함께 스킵된다. state 없이도 필요한 GPU 호출은 별도로 분리할 것.

```rust
// 잘못됨 — state가 None이면 gpu.resize()도 스킵
if let (Some(gpu), Some(state)) = (&mut self.gpu, &mut self.state) {
    gpu.resize(new_size);
    // ... state 관련 처리
}

// 올바름 — gpu.resize()는 항상 호출
if let Some(gpu) = &mut self.gpu {
    gpu.resize(new_size);
}
if let (Some(gpu), Some(state)) = (&self.gpu, &mut self.state) {
    // ... state 관련 처리
}
```
