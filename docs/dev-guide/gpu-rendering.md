# GPU 렌더링 구조

터미널은 wgpu 커스텀 셰이더 파이프라인으로 그린다 — egui UI 와 별도로, 각 셀의 배경 + 글리프를 GPU 인스턴스 렌더링한다. 핵심 렌더러는 `src/gfx/renderer.rs` 의 `CellRenderer`.

## 프레임 흐름

1. **Clear pass** — 배경색으로 클리어
2. **Terminal pass** — 아래 4단계 accumulator 로 모든 터미널을 한 번에
3. **egui pass** — 사이드바·탭바·팝업 등 UI 오버레이

## 터미널 렌더 = 누적 후 1회 flush, 단일 패스 (필수 모델)

여러 터미널을 *각각 submit* 하지 않는다. 한 프레임의 모든 surface 인스턴스를 Vec 에 **누적**하고, 버퍼에 **한 번** 쓰고, **단일 render pass** 에서 surface 별 scissor + 인스턴스 range 로 그린다.

```rust
renderer.begin_frame();                       // ① accumulator 클리어
for surface in surfaces {                      // ② 각 surface 인스턴스를 Vec 에 push
    renderer.append_terminal_viewport(term, queue, &viewport, ansi, ...);
}
renderer.flush_buffers(device, queue);         // ③ 누적분을 버퍼에 1회 write
// 단일 render pass 안에서:
renderer.render_all(&mut render_pass, w, h);   // ④ bg 패스 → glyph 패스, surface 별 scissor
```

### ① `begin_frame()`

per-frame accumulator(`bg_instances`, `glyph_instances`, `surface_ranges`)와 draw 카운터를 클리어하고 glyph atlas frame 카운터를 bump(per-page LRU stamp 일관성).

### ② `append_terminal_viewport(...)`

한 surface 의 셀들을 `BgInstance`/`GlyphInstance` 로 만들어 accumulator Vec 에 push 한다. 동시에 그 surface 의 `(scissor rect, bg range, glyph range)` 를 `surface_ranges` 에 기록한다. **viewport offset 은 per-instance 로 각 인스턴스에 baked** 되므로(전역 uniform 을 surface 마다 다시 쓰지 않는다), surface 마다 uniform 갱신/submit 이 필요 없다. theme lock(`ansi` 팔레트)은 호출자가 **프레임당 1회** 잡아 넘긴다(surface 마다 잠그지 않음).

### ③ `flush_buffers(device, queue)`

누적된 인스턴스를 `bg_instance_buffer`/`glyph_instance_buffer` 에 `write_buffer` 로 **한 번** 쓴다. 용량이 모자라면 ×2 로 키워 재할당(hard cap 16M 인스턴스 ≈ 1 GiB, 초과 시 clamp + warn).

### ④ `render_all(render_pass, w, h)`

단일 render pass 안에서 **bg 패스 전체 → glyph 패스 전체** 순으로 draw 한다. 각 패스는 `surface_ranges` 를 순회하며 surface 별 `set_scissor_rect(...)` 후 instanced `draw(0..6, range)`(6 vert = quad). 빈 range 는 skip.

> **왜 이 구조인가**: 예전엔 공유 버퍼를 offset 0 부터 덮어써서 *터미널마다 encoder+submit 을 분리* 해야 했다. 지금은 인스턴스를 누적하고 per-instance offset 을 baking 하므로, 버퍼 write 1회 + render pass 1개로 끝난다 — submit 폭증 없이 N 개 surface 를 그린다.

## 주요 버퍼

| 버퍼 | 내용 |
|------|------|
| `uniform_buffer` | **전역** cell_size + viewport_size (윈도우 리사이즈 시만 갱신) |
| `bg_instance_buffer` | 셀 배경 인스턴스 (pos + bg_color), per-instance viewport offset baked |
| `glyph_instance_buffer` | 글리프 인스턴스 (pos + uv + fg_color + glyph_size), offset baked |

per-surface 위치 정보가 인스턴스에 들어가 있으므로 uniform 은 전역 1개로 충분하다.

## 진단

- `draw_call_count()` / `active_surface_count()` — 프레임의 bg/glyph draw 수, 활성 surface 수.
- 프레임 타이밍 계측은 `src/gfx/perf.rs` / redraw 경로. `tracing::warn!` 은 임계값 초과 시만.
- 셀 색이 의심되면 debug 의 `debug.glyph_color`(렌더러가 push 하는 실제 RGBA) — [debug-ipc.md](debug-ipc.md).
