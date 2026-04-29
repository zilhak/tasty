# GPU 렌더링 구조

Tasty의 터미널 렌더링은 wgpu 기반 커스텀 셰이더 파이프라인으로 수행된다. egui UI와 별도로 동작하며, 각 터미널의 셀(배경 + 글리프)을 GPU 인스턴스 렌더링으로 그린다.

## 렌더 파이프라인 흐름

1. **Clear pass** — 배경색으로 화면 클리어
2. **Terminal pass** — 각 터미널 surface를 순회하며 셀 렌더링
3. **egui pass** — 사이드바, 탭 바, 팝업 등 UI 오버레이

## 터미널 렌더링 구조 (`render_terminals`)

각 터미널은 다음 두 단계로 렌더된다:

1. **`prepare_terminal_viewport`** — 터미널 데이터를 공유 GPU 버퍼(uniform, bg_instance, glyph_instance)에 기록. `queue.write_buffer()`로 수행.
2. **`render_scissored`** — scissor rect를 설정하고 해당 영역에 렌더 패스 실행.

## 공유 GPU 버퍼와 submit 분리 규칙 (필수)

**각 터미널은 반드시 별도의 command encoder + `queue.submit()`으로 렌더해야 한다.**

`prepare_terminal_viewport`는 `queue.write_buffer()`로 공유 버퍼에 데이터를 쓴다. wgpu에서 `write_buffer`는 즉시 GPU에 반영되지 않고 다음 `queue.submit()` 시점에 일괄 실행된다. 따라서:

```
// ❌ 금지: 하나의 encoder에 여러 터미널을 묶으면 마지막 터미널만 표시됨
let mut encoder = device.create_command_encoder(...);
for terminal in terminals {
    prepare_terminal_viewport(terminal, queue, ...);  // write_buffer → 이전 데이터 덮어씀
    encoder.begin_render_pass(...);                    // 기록만 함, 아직 실행 안 됨
}
queue.submit(encoder.finish());  // 모든 write_buffer가 여기서 실행 → 마지막 것만 남음

// ✅ 올바름: 터미널마다 encoder + submit 분리
for terminal in terminals {
    prepare_terminal_viewport(terminal, queue, ...);
    let mut encoder = device.create_command_encoder(...);
    encoder.begin_render_pass(...);
    queue.submit(encoder.finish());  // 이 터미널의 write_buffer가 여기서 반영됨
}
```

submit 횟수가 늘어나지만, 터미널 수는 일반적으로 10개 이하이므로 성능 영향은 무시할 수준이다.

## 주요 GPU 버퍼

| 버퍼 | 용도 |
|------|------|
| `uniform_buffer` | cell_size, grid_offset(뷰포트 위치), viewport_size |
| `bg_instance_buffer` | 셀 배경색 인스턴스 (pos + bg_color) |
| `glyph_instance_buffer` | 글리프 인스턴스 (pos + uv + fg_color + glyph_size) |

이 버퍼들은 `prepare_terminal_viewport` 호출마다 offset 0부터 덮어쓰므로, 반드시 submit으로 분리해야 한다.
