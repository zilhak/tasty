# Performance Benchmarks

F.G 의 GPU 최적화 (단일 draw + multi-page atlas) 가 의도대로 작동하는지 *실측* 한다.
범위: **frame-time 의 `terminals_ms` / `gpu_total_ms` + draw call 수** (10+ surface 환경).

> Scope (Phase I.H, Q4=A): atlas LRU eviction 카운터 / RSS memory profile / flamegraph 는 **포함하지 않음**. 후속 영역 `I.H-atlas`, `I.H-2 cargo bench harness` 로 분리.

---

## 1. 측정 목적

- F.G.a: 모든 surface 의 bg+glyph 를 *단일 render pass + 단일 instance buffer* 로 묶어 한 번에 issue.
- F.G.b: 멀티 페이지 atlas + LRU eviction 으로 atlas miss 시 stall 회피.

두 변경의 효과는 (1) draw call 수가 `O(surface)` 로 선형 성장만 하고 (2) `terminals_ms` 가 surface 수 증가에 따라 폭발하지 않는지로 확인한다.

---

## 2. 방법론

### 2.1 측정 segment 정의

`src/gfx/gpu.rs::GpuState::render()` 가 단계별 `Instant::now() / elapsed()` 로 측정.

| 변수 | 의미 |
|------|------|
| `terminals_ms` | `renderer.begin_frame()` + N 회 `append_terminal_viewport` + `flush_buffers()` + `render_all()`. F.G 핵심 영역. |
| `gpu_total_ms` | `render()` 함수 전체 (layout + egui + tessellate + clear + terminals + egui_pass + present). |
| `draw_calls_total` | `RenderPass::draw()` 호출 횟수. `set_pipeline` / `set_scissor_rect` 는 별도 (pipeline 스위치는 2회 고정). |
| `surfaces` | 활성 (bg 또는 glyph range 비어있지 않은) surface 수. |

### 2.2 집계 + 출력

- `src/gfx/perf.rs::PerfAggregator` — `WINDOW = 300` 프레임 ring buffer + `DUMP_EVERY = 300` 마다 p50/p99/max 한 줄 dump.
- log target: `tasty::gfx::perf`. 기본 `RUST_LOG` 에서 비활성.
- enable: `RUST_LOG=tasty::gfx::perf=info`.
- 한 줄 포맷:

  ```text
  perf n=300 surfaces=10 draws=20 terminals_ms p50=… p99=… max=… gpu_total_ms p50=… p99=… max=…
  ```

### 2.3 시나리오

`scripts/bench/perf-10-surfaces.sh`:

1. `cargo run --release` 시작, `tasty list info` 폴링으로 준비 대기.
2. 첫 surface 의 ID 를 `tasty list surfaces` 로 얻어 `tasty split --level surface --target-surface <SID> --direction vertical` 9 회 — 총 10 surface.
3. 각 surface 에 `for i in $(seq 1 5000); do echo bench_$i; done` 입력.
4. `PERF_DURATION_SECS` (기본 60) 초 대기, 그동안 stdout/stderr 가 `.claude-workspace/temp/perf-{platform}.log` 로 적재.
5. 종료 후 `grep "tasty::gfx::perf" | tail -12` 로 마지막 12 샘플 (≈60s) 추출.

### 2.4 빌드 프로필

`cargo run --release`. `--profile dist` 는 LTO 활성으로 더 빠를 수 있으나 빌드 비용이 비대칭으로 커서 본 영역에서는 사용하지 않음.

---

## 3. 측정 결과

| 플랫폼 | wgpu backend | surfaces | terminals_ms (p50 / p99 / max) | gpu_total_ms (p50 / p99 / max) | draw_calls_total |
|--------|-------------|----------|--------------------------------|--------------------------------|------------------|
| Mac (M-series) | Metal | 10 | TBD | TBD | TBD (예상 20) |
| Linux (x86_64) | Vulkan | 10 | TBD | TBD | TBD (예상 20) |
| Windows (x86_64) | DX12 | 10 | TBD | TBD | TBD |

> 본 PR 시점의 측정값은 *실측 환경 (release build, GUI) 가 회수되는 즉시* 채워진다. 인프라 (counter / aggregator / 스크립트) 는 모두 머지되어 누구든 동일한 절차로 결과를 재현·갱신할 수 있다.

---

## 4. F.G 효과 평가 (정성)

- **단일 render pass batching (F.G.a)**: pipeline 스위치는 frame 당 *2회 고정* (bg → glyph). surface 가 N 개여도 encoder/submit 횟수는 1 회로 유지. `set_scissor_rect + draw` 만 surface 수에 비례 (`draw_calls_total = 2N` 상한, 빈 surface 는 skip).
- **멀티 페이지 atlas (F.G.b)**: glyph cache miss 시 새 page 할당 (최대 `MAX_PAGES = 4`) 또는 LRU page 재사용. `flush_buffers` 가 dynamic grow 로 silent clamp 회귀를 막아 instance count 변동에도 stall 없음.
- 정량 비교 (F.G 이전 vs 이후) 는 본 영역 범위 밖. F.G 이전 빌드 재현 비용이 가치 대비 큼.

---

## 5. 회귀 임계값

- `gpu_total_ms p99 > 30.0` → 기존 `SLOW_RENDER_MS` warn 트리거 (`src/gfx/gpu.rs`).
- `draw_calls_total > 2 × surfaces` → batching 회귀 가능성. perf log 의 `draws` 와 `surfaces` 비교로 즉시 확인.
- CI 통합 (자동 임계값 검사) 은 본 영역 범위 밖. `I.H-2 cargo bench harness` 후속 영역에서 다룬다.

---

## 6. 한계 및 후속

| 항목 | 한계 | 후속 |
|------|------|------|
| atlas 측정 | LRU eviction 카운터 / page entry_count / last_access_frame 모두 미수집 | 영역 `I.H-atlas` |
| memory profile | RSS / GPU 메모리 측정 없음 | 별도 |
| flamegraph | CPU profile 미수집 | 별도 |
| Windows shell loop | `scripts/bench/perf-10-surfaces.sh` 는 bash 전제, PowerShell 변형 없음 | Windows 행은 TBD 유지, PowerShell port 후속 영역 |
| 측정 윈도우 | `WINDOW = 300 ≈ 5s @ 60fps`. GPU thermal throttling 은 잡지 못함. | 의도된 trade-off |
| present mode / vsync | wgpu 기본 present mode (Mac Metal 은 사실상 60Hz vsync 강제) | 영역 밖 |
| 빌드 프로필 | `--release` 만 측정. `dist` (LTO) 미측정 | 후속 영역 |
| `draw_call_count` 의미 | `RenderPass::draw()` 만 카운트. `set_pipeline` / `set_scissor_rect` 는 별도 비용 | 문서로 명시 |

---

## 7. 재현 방법

```bash
cargo build --release
# tasty CLI 가 PATH 에 있어야 함
./scripts/bench/perf-10-surfaces.sh
# .claude-workspace/temp/perf-{platform}.log 의 마지막 12 perf 라인이 결과
```

`PERF_DURATION_SECS` 환경변수로 측정 시간 조절 (기본 60 초).
