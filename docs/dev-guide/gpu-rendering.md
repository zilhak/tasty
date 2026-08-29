# GPU 렌더링 구조

터미널은 wgpu 커스텀 셰이더 파이프라인으로 그린다 — egui UI 와 별도로, 각 셀의 배경 + 글리프를 GPU 인스턴스 렌더링한다. 핵심 렌더러는 `src/gfx/renderer.rs` 의 `CellRenderer`.

## 프레임 흐름

1. **Clear pass** — 배경색으로 클리어
2. **Terminal pass** — 아래 4단계 accumulator 로 모든 터미널을 한 번에
3. **egui-mesh pass** — plugin 이 자기 프로세스에서 tessellate 한 mesh 를 surface 영역에 합성 (host chrome 아래 layer). 채널 상세는 [egui-mesh-channel](egui-mesh-channel.md)
4. **egui pass** — 사이드바·탭바·팝업 등 UI 오버레이

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

## Surface configure 치수 clamp

`GpuState`(`src/gfx/gpu.rs`)는 `surface.configure` 에 넘기는 width/height 를 winit 경계에서 `device.limits().max_texture_dimension_2d`(어댑터별 실제 한계, 런타임 조회 — 하드코딩 금지)로 **clamp** 하고, 실제로 clamp 가 걸리면 `warn!` 을 남긴다. 상한을 넘는 치수가 오면 wgpu 가 panic 하기 때문이다(예: 외부 `SetWindowPos` 가 winit `Resized` 로 `1100×65535` 유입). 하한은 `1`(configure 는 0 불가), 0 은 최소화 신호로 `resize` early-return 이 configure 를 스킵한다. `resize` 와 `new`(startup) 모두 공통 `clamp_surface_dims(w,h,max)` 헬퍼를 쓴다.

**거부+안내 계층은 없다** — IPC/CLI/시작단/split 어디에도 상한 초과 치수를 주입하는 진입점이 없어(창 크기 변경 명령·저장된 geometry 복원 부재) winit OS 이벤트 경계가 유일한 유입 경로다. OS 이벤트는 거부할 수 없으므로 방어는 clamp + `warn!` 하나로 일원화한다. 정상 범위(≤max)에서는 no-op 이라 기존 동작에 영향이 없다.

## 주요 버퍼

| 버퍼 | 내용 |
|------|------|
| `uniform_buffer` | **전역** cell_size + viewport_size (윈도우 리사이즈 시만 갱신) |
| `bg_instance_buffer` | 셀 배경 인스턴스 (pos + bg_color), per-instance viewport offset baked |
| `glyph_instance_buffer` | 글리프 인스턴스 (pos + uv + fg_color + glyph_size), offset baked |

per-surface 위치 정보가 인스턴스에 들어가 있으므로 uniform 은 전역 1개로 충분하다.

## 프레임 구동 정책 — 무엇이 프레임을 유발하고 무엇이 상한을 거는가

프레임은 **`View::mark_dirty()` 하나로 수렴**한다. 이 호출이 `base.dirty` 를 세우고 그 자리에서 `Window::request_redraw()` 를 발화하므로, `mark_dirty()` 호출 = 프레임 1 회 요청이다. 실제 렌더는 `render_if_dirty`(`src/view/main/redraw.rs`)가 `dirty` 일 때만 수행한다.

### 유발원과 상한 적용

상한은 `RepaintGate`(`src/view/repaint.rs`)가 `mark_dirty` 안에서 건다. 유발원은 `RepaintSource` 로 분류한다.

| 유발원 | `RepaintSource` | 상한 | 좌표 |
|---|---|---|---|
| 키·마우스·IME·리사이즈·포커스·구조 변경 | `Interactive` (기본값) | **없음 — 즉시 발화** | `mark_dirty()` 호출 전반 |
| PTY 출력 | `TerminalOutput` | 주사율까지 coalesce | `src/app/event_handler.rs` `handle_terminal_output` |
| egui 내부 delay 0 즉시 repaint | `EguiAnimation` | 주사율까지 coalesce | `AppEvent::EguiRepaint` 핸들러 |
| attach mirror 갱신 | `AttachMirror` | 주사율까지 coalesce | `src/app/attach_poll.rs`, `src/app/attach_client.rs` |

사용자 조작발을 통과시키는 이유는 반응성이다 — 여기에 상한을 걸면 타이핑·클릭 지연이 그대로 늘어난다. 나머지는 사람이 개별 프레임을 구분하지 못하므로 묶어도 체감이 없다.

### 상한값

**모니터가 보고하는 주사율**(`MonitorHandle::refresh_rate_millihertz()`)에서 최소 프레임 간격을 구한다. 5 초 TTL 로 재조회하므로 창을 다른 모니터로 옮기면 따라간다. 보고값이 없으면 60Hz 폴백, 상식 범위를 벗어나면 24~480Hz 로 clamp 한다. 고정 상수를 박지 않는 이유는 그 상수가 고주사율 환경에서 **오히려 상한**이 되기 때문이다.

상한 상태는 `ViewBase` 필드라 **창마다 독립**이다.

### 미뤄진 요청은 반드시 되살아난다

게이트는 `request_redraw()` 를 미룰 뿐 `dirty` 를 지우거나 요청을 버리지 않는다. 미뤄진 요청의 만기 시각은 `about_to_wait`(`drive_deferred_repaints`)이 `ControlFlow::WaitUntil` 로 재예약하며, 만기 tick 에서 `request_redraw()` 를 발화한다. 이 재예약이 유일한 복구 경로다 — 빠뜨리면 아무 이벤트도 오지 않는 순간에 그 프레임이 영영 오지 않는다.

`dirty` 를 **억제하지 않는** 것은 계약이다. `render_if_dirty` 의 doc 주석이 명시하듯 attach 서버의 원격 mirror 중계가 `dirty` 프레임에 종속돼 있어, 프레임을 없애면 원격 사용자 화면이 굶는다. 상한은 cadence 만 주사율에 맞추고 프레임 자체는 계속 흐르게 한다.

### 왜 present 층이 아니라 요청 층인가

`present_mode` 는 `Mailbox` 가 가능하면 `Mailbox`, 아니면 `Fifo` 다(`src/gfx/gpu.rs`, 선택 결과를 `info!` 로 남긴다). `Fifo` 로 고정하면 상한은 서지만 **모든** 리페인트에 최대 한 프레임의 present 블록이 실려 입력 반응성이 함께 나빠진다. 게다가 가상 디스플레이(xrdp 계열)에는 하드웨어 vblank 가 없어 `Fifo` 가 실제로 프레임을 묶어 준다는 보장도 없다. 요청 층에서 걸면 유발원별로 갈라 처리할 수 있어 반응성을 지키면서 초과 프레임만 없앤다.

원격 데스크톱 경유에서 이 상한이 특히 중요한 이유는 프레임당 비용이다. GPU 스캔아웃 경로가 없어 present 마다 GPU→CPU readback → X11 `PutImage` → 서버측 재인코딩을 타므로, 프레임당 화면 전체(1920×1080×4B ≈ 8MB)가 소켓으로 흐른다. 주사율을 넘겨 그린 프레임은 화면에 나타나지 못한 채 그 비용만 물고 버려진다.

## 진단

- `draw_call_count()` / `active_surface_count()` — 프레임의 bg/glyph draw 수, 활성 surface 수.
- 프레임 타이밍 계측은 `src/gfx/perf.rs` / redraw 경로. `tracing::warn!` 은 임계값 초과 시만.
- 셀 색이 의심되면 debug 의 `debug.glyph_color`(렌더러가 push 하는 실제 RGBA) — [debug-ipc.md](debug-ipc.md).
- 리페인트 유발원별 요청 수 / 지연 수 / 실제 present 수는 1 초 창 dump: `TASTY_LOG=tasty::view::repaint=info`(dev 빌드는 `~/.tasty-debug/debug-dev.log` 에 그냥 남는다). 요청이 많아도 실제 렌더가 합쳐졌는지를 이 한 줄로 구분한다.
