# Performance Benchmarks

F.G 의 GPU 최적화 (단일 draw + multi-page atlas) 가 의도대로 작동하는지 *실측* 한다.
범위: **frame-time 의 `terminals_ms` / `gpu_total_ms` + draw call 수** (10+ surface 환경).

> Scope: I.H 가 release × 10-surface ASCII 시나리오를 깐 뒤, **J.F** 가 (a) **dist (full LTO) 프로필 분기**, (b) **CJK 4-surface atlas eviction 시나리오**, (c) **atlas eviction / active-page / entry-count 카운터** 를 추가했다. RSS memory profile / flamegraph / scrollback viewport scroll / cargo bench harness 는 *여전히* 본 문서 범위 밖 (§6 참조).

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
| `atlas_evictions` | `GlyphAtlas::eviction_count()` — 누적 (단조 증가) 페이지 eviction 횟수. frame 당 최대 1회 가산. |
| `atlas_active_pages` | `GlyphAtlas::active_page_count()` — `entry_count > 0` 인 페이지 수 (0..=`MAX_PAGES`). |
| `atlas_entry_count_sum` | `GlyphAtlas::entry_count_sum()` — 모든 페이지의 `entry_count` 합. live 캐시 항목 근사치. |

### 2.2 집계 + 출력

- `src/gfx/perf.rs::PerfAggregator` — `WINDOW = 300` 프레임 ring buffer + `DUMP_EVERY = 300` 마다 p50/p99/max 한 줄 dump.
- log target: `tasty::gfx::perf`. 기본 `RUST_LOG` 에서 비활성.
- enable: `RUST_LOG=tasty::gfx::perf=info`.
- 한 줄 포맷:

  ```text
  perf n=300 surfaces=10 draws=20 terminals_ms p50=… p99=… max=… gpu_total_ms p50=… p99=… max=… atlas_evictions=… atlas_pages=… atlas_entries=…
  ```

  - `atlas_*` 세 값은 *마지막 sample* (window 의 가장 최근 frame) 의 절대치이며 percentile 이 아니다. `atlas_evictions` 는 누적이라 *전후 차이* 가 의미를 갖는다.

### 2.3 시나리오

두 시나리오가 있다. 둘 다 `PERF_DURATION_SECS` (기본 60) / `PERF_PROFILE` (`release` 기본, 또는 `dist`) env 를 받는다.

| 시나리오 | 스크립트 | surface 수 | 입력 부하 | 핵심 관찰 |
|----------|----------|-----------|-----------|-----------|
| 10-surface ASCII | `scripts/bench/perf-10-surfaces.sh` | 10 | `seq 1 5000 → echo` 각 surface | `terminals_ms` / `draw_calls_total` 의 surface-scaling |
| CJK 4-surface | `scripts/bench/perf-cjk-atlas.sh` | 4 | 각 surface 에 한자 / 히라가나 / 한글 음절 3 batch (batch 당 3000 unique 코드포인트 × ~16 회 반복) | `atlas_evictions` delta + `atlas_pages` 평균 |

**10-surface ASCII**:

1. `cargo run "${CARGO_FLAGS[@]}"` 시작 (release 또는 dist), `tasty list info` 폴링으로 준비 대기.
2. 첫 surface 의 ID 를 `tasty list surfaces` 로 얻어 `tasty split --level surface --target-surface <SID> --direction vertical` 9 회 — 총 10 surface.
3. 각 surface 에 `for i in $(seq 1 5000); do echo bench_$i; done` 입력.
4. `PERF_DURATION_SECS` 초 대기, 그동안 stdout/stderr 가 `.claude-workspace/temp/perf-{platform}-{profile}.log` 로 적재.
5. 종료 후 `grep "tasty::gfx::perf" | tail -12` 로 마지막 12 샘플 (≈60s) 추출.

**CJK 4-surface (J.F 신설)**:

1. `cargo run` 기동 후 ready 대기 (동일).
2. **CJK fallback 폰트 사전 점검**: 첫 surface 에 "한국어 中文 日本語" 한 줄 전송 → 로그에 `font fallback missing` / `no glyph for codepoint` warning 이 잡히면 즉시 abort. visual check 의존 X (`RUST_LOG="…,tasty::font=warn,…"`).
3. surface 3 개 추가 (`tasty split --level surface --target-surface <FIRST_SID> --direction vertical`) — 총 4 surface.
4. 각 surface 에 3 batch 입력 (batch 당 `python3 -c "print(''.join(chr(BASE + i % 3000) for i in range(50000)))"`):
   - `BASE = 0x4E00` — CJK Unified Ideographs (한자)
   - `BASE = 0x3040` — Hiragana
   - `BASE = 0xAC00` — Hangul Syllables
5. `PERF_DURATION_SECS` 초 대기 후 `grep "tasty::gfx::perf" | tail -12` 추출.

> 4 surface × 3 batch × 3000 unique = 36000 unique 코드포인트 호출. `MAX_PAGES = 4 × ATLAS_SIZE = 2048` 의 페이지 한도를 자연스럽게 넘어 eviction 을 유도하는 설계.

### 2.4 빌드 프로필

`PERF_PROFILE` env 로 두 프로필을 분기 실행한다.

| 프로필 | cargo 플래그 | LTO | 빌드 시간 | 권장 |
|--------|-------------|-----|-----------|------|
| `release` (기본) | `--release` | thin | 1× | 일상 측정 |
| `dist` | `--profile dist` | full | ~3.5× | LTO 영향 검증 1 회만 |

> `dist` 빌드 산출물은 `target/dist/` 로 별도 디렉토리에 적재된다. 측정 후 `cargo clean -p tasty --profile dist` 로 해제 가능 (수 GB 점유). 회귀 측정은 동일 프로필 안에서 한다.

---

## 3. 측정 결과

표는 (시나리오 × 플랫폼 × 프로필) 의 cross product. 실측이 회수되는 행만 값이 들어가고 나머지는 `TBD`.

### 3.1 10-surface ASCII

| 플랫폼 | wgpu backend | 프로필 | terminals_ms (p50 / p99 / max) | gpu_total_ms (p50 / p99 / max) | draws | atlas_evictions Δ | atlas_pages |
|--------|-------------|--------|--------------------------------|--------------------------------|-------|-------------------|-------------|
| Mac (M-series) | Metal | release | TBD | TBD | TBD (예상 20) | TBD | TBD |
| Mac (M-series) | Metal | dist | TBD | TBD | TBD | TBD | TBD |
| Linux (x86_64) | Vulkan | release | TBD | TBD | TBD | TBD | TBD |
| Linux (x86_64) | Vulkan | dist | TBD | TBD | TBD | TBD | TBD |
| Windows (x86_64) | DX12 | release | TBD | TBD | TBD | TBD | TBD |
| Windows (x86_64) | DX12 | dist | TBD | TBD | TBD | TBD | TBD |

### 3.2 CJK 4-surface (atlas eviction)

| 플랫폼 | wgpu backend | 프로필 | terminals_ms (p50 / p99 / max) | gpu_total_ms (p50 / p99 / max) | atlas_evictions Δ | atlas_pages (평균) | atlas_entries (마지막) |
|--------|-------------|--------|--------------------------------|--------------------------------|-------------------|--------------------|------------------------|
| Mac (M-series) | Metal | release | TBD | TBD | TBD | TBD | TBD |
| Mac (M-series) | Metal | dist | TBD | TBD | TBD | TBD | TBD |
| Linux (x86_64) | Vulkan | release | TBD | TBD | TBD | TBD | TBD |
| Linux (x86_64) | Vulkan | dist | TBD | TBD | TBD | TBD | TBD |
| Windows (x86_64) | DX12 | release | TBD | TBD | TBD | TBD | TBD |
| Windows (x86_64) | DX12 | dist | TBD | TBD | TBD | TBD | TBD |

> 측정 인프라 (counter / aggregator / 스크립트 / fallback 점검) 는 모두 머지되어 누구든 동일한 절차로 결과를 재현·갱신할 수 있다. `atlas_evictions Δ` 는 측정 시작 직후 sample 과 마지막 sample 의 누적값 차이.

---

## 4. F.G 효과 평가 (정성)

- **단일 render pass batching (F.G.a)**: pipeline 스위치는 frame 당 *2회 고정* (bg → glyph). surface 가 N 개여도 encoder/submit 횟수는 1 회로 유지. `set_scissor_rect + draw` 만 surface 수에 비례 (`draw_calls_total = 2N` 상한, 빈 surface 는 skip).
- **멀티 페이지 atlas (F.G.b)**: glyph cache miss 시 새 page 할당 (최대 `MAX_PAGES = 4`) 또는 LRU page 재사용. `flush_buffers` 가 dynamic grow 로 silent clamp 회귀를 막아 instance count 변동에도 stall 없음.
- 정량 비교 (F.G 이전 vs 이후) 는 본 영역 범위 밖. F.G 이전 빌드 재현 비용이 가치 대비 큼.

---

## 5. 회귀 임계값

- `gpu_total_ms p99 > 30.0` → 기존 `SLOW_RENDER_MS` warn 트리거 (`src/gfx/gpu.rs`).
- `draw_calls_total > 2 × surfaces` → batching 회귀 가능성. perf log 의 `draws` 와 `surfaces` 비교로 즉시 확인.
- `atlas_evictions Δ/min`: **TBD — 측정 후 결정**. CJK 시나리오 실측 데이터 (평균 + 분산) 가 회수되면 §7 의 후속 영역에서 임계 수치를 박는다. 사변으로 임의값을 박지 않는다.
- CI 통합 (자동 임계값 검사) 은 본 영역 범위 밖. `I.H-2 cargo bench harness` 후속 영역에서 다룬다.

---

## 6. 한계 및 후속

| 항목 | 한계 | 후속 |
|------|------|------|
| atlas counter 정밀도 | `atlas_active_pages` / `atlas_entry_count_sum` 은 *마지막 sample* 의 절대치 (frame 평균 아님). `atlas_evictions` 는 누적이라 *전후 차이* 만 의미 있음. | 의도된 trade-off — dump 한 줄 비용 최소화 |
| scrollback viewport scroll | tasty GUI 의 viewport scroll 은 단축키 / 마우스 wheel 로만 트리거 (원칙 1 ②). release IPC 비노출이라 `tasty send key pageup` 으로는 PTY 에 ESC[5~ 만 inject 되어 GUI viewport 가 안 움직임. → 본 문서로는 측정 불가. | 별 후속: debug 빌드 전용 IPC 로 GUI viewport scroll 트리거 + 30k 출력 시나리오 + 회귀 임계 결정 |
| memory profile | RSS / GPU 메모리 측정 없음 | 별도 |
| flamegraph | CPU profile 미수집 | 별도 |
| Windows shell loop | `scripts/bench/perf-10-surfaces.sh` / `perf-cjk-atlas.sh` 는 bash 전제, PowerShell 변형 없음 | Windows 행은 TBD 유지, PowerShell port 후속 영역 |
| 측정 윈도우 | `WINDOW = 300 ≈ 5s @ 60fps`. GPU thermal throttling 은 잡지 못함. | 의도된 trade-off |
| present mode / vsync | wgpu 기본 present mode (Mac Metal 은 사실상 60Hz vsync 강제) | 영역 밖 |
| `draw_call_count` 의미 | `RenderPass::draw()` 만 카운트. `set_pipeline` / `set_scissor_rect` 는 별도 비용 | 문서로 명시 |
| CJK fallback 폰트 환경 의존 | `perf-cjk-atlas.sh` 는 사전 점검으로 fallback 부재를 abort 처리하지만, 시스템에 CJK 폰트 자체가 없으면 측정 불가 (Windows EN-US default 등) | 측정 환경 사전 셋업 — docs §7 의 후속에서 표준 환경 listing |

---

## 7. 후속 최적화 후보

본 문서의 §3 측정 결과를 보고 *실측 데이터에 근거하여* 채운다. 사변으로 항목을 박지 않는다 (CLAUDE.md §1 "Don't assume").

### 7.1 사실 항목 (측정 무관, 본 영역에서 분리 결정)

- **scrollback 측정 영역 (별 후속)**: §6 에 적혀 있듯 GUI viewport scroll 은 release IPC 비노출 (원칙 1 ②). debug 빌드 전용 IPC 로 viewport scroll 트리거를 추가한 뒤 30k 라인 출력 + 빠른 scroll 시나리오를 짜고 `terminals_ms` p99 회귀 임계값을 결정한다.

### 7.2 측정 후 채움 (실측 시점 이전엔 작성 금지)

다음 항목들은 §3 표가 채워진 후 데이터에 근거해 작성한다 — 현재는 placeholder:

- `atlas_evictions Δ/min` 이 임계 수준이면: `MAX_PAGES` 상한 재검토 또는 페이지 동적 할당.
- `dist` 와 `release` 의 `gpu_total_ms p99` 차이가 미미 (<5%) 면: `dist` 프로필 LTO 의 비용 (3.5× 빌드 시간) 대비 가치 부족을 문서에 명시.
- Windows 행이 끝까지 TBD 면: PowerShell port 영역 신설.
- §5 회귀 임계값 (`atlas_evictions Δ/min`) 의 구체 수치: 측정 데이터의 평균 + 분산을 보고 결정.

---

## 8. 재현 방법

```bash
# 10-surface ASCII (기본 release)
./scripts/bench/perf-10-surfaces.sh

# 10-surface ASCII (dist / LTO)
PERF_PROFILE=dist ./scripts/bench/perf-10-surfaces.sh

# CJK 4-surface (atlas eviction)
./scripts/bench/perf-cjk-atlas.sh
PERF_PROFILE=dist ./scripts/bench/perf-cjk-atlas.sh

# 결과: .claude-workspace/temp/perf-{platform}-{profile}.log
#        .claude-workspace/temp/perf-cjk-{platform}-{profile}.log
# 마지막 12 `tasty::gfx::perf` 라인이 측정 샘플.
```

환경변수:

- `PERF_DURATION_SECS` — 측정 시간 (기본 60 초).
- `PERF_PROFILE` — `release` (기본) 또는 `dist`.

dist 빌드 산출물 정리: `cargo clean -p tasty --profile dist`.
