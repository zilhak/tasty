# GPU 활용 전략

tasty의 GPU 렌더링 아키텍처, 셰이더 설계, 버퍼 전략, 폴백 메커니즘을 정의하는 횡단 전략 문서다.

---

## 렌더링 아키텍처 개요

```
┌─────────────────────────────────────────────────────────┐
│                    winit Window                         │
│  ┌───────────────────────────────────────────────────┐  │
│  │              wgpu Surface (SwapChain)              │  │
│  │  ┌─────────────────────────────────────────────┐  │  │
│  │  │         Render Pass Pipeline                 │  │  │
│  │  │                                              │  │  │
│  │  │  1. 배경 패스 ─── 셀 배경 셰이더            │  │  │
│  │  │  2. 글리프 패스 ─── 글리프 셰이더            │  │  │
│  │  │  3. 커서 패스 ─── 커서 셰이더               │  │  │
│  │  │  4. UI 패스 ─── UI 셰이더                   │  │  │
│  │  │  5. 오버레이 패스 ─── UI 셰이더 (재사용)     │  │  │
│  │  │  6. 효과 패스 ─── 효과 셰이더               │  │  │
│  │  └─────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

전체 렌더링은 단일 `wgpu::RenderPass` 내에서 draw call 순서로 레이어를 구성한다. 각 패스는 별도의 렌더 파이프라인(`wgpu::RenderPipeline`)을 바인딩하고, 공유 uniform 버퍼(뷰포트, 시간, DPI)를 참조한다.

**합성 흐름:**

```
Terminal Grid (termwiz Surface)
    ↓ dirty cell detection
Cell Instance Buffer (GPU)
    ↓ instanced draw
배경 패스 → 글리프 패스 → 커서 패스
    ↓
UI Layout Tree
    ↓ vertex generation
UI 패스 → 오버레이 패스
    ↓
효과 패스 (post-processing)
    ↓
Surface Present
```

터미널 셀과 UI 위젯은 독립적인 버텍스/인스턴스 버퍼를 사용하되, 동일한 렌더 패스 내에서 순차적으로 그린다. 효과 패스만 필요시 별도 렌더 타겟을 사용할 수 있다.

---

## 셰이더 설계

모든 셰이더는 **WGSL (WebGPU Shading Language)** 로 작성한다. wgpu는 WGSL을 네이티브로 지원하며, Vulkan SPIR-V, Metal MSL, DX12 DXIL, OpenGL GLSL로 자동 변환한다.

### 셀 배경 셰이더 (`cell_bg.wgsl`)

셀 배경색을 렌더링하는 인스턴스드 쿼드 셰이더.

- **입력**: 인스턴스 버퍼에서 셀 위치(col, row), 배경색(RGBA), 플래그(선택 영역 하이라이트, 검색 매치)
- **버텍스**: 단위 쿼드(0,0)-(1,1) 6개 버텍스를 인스턴스 데이터로 스케일/이동
- **프래그먼트**: 배경색 출력, 선택 영역이면 하이라이트 색상과 블렌딩
- **최적화**: `@builtin(instance_index)`로 인스턴스 ID 접근, 조건부 discard로 기본 배경색 셀 스킵

```wgsl
struct CellInstance {
    @location(0) pos: vec2<f32>,       // 셀 그리드 좌표 (col, row)
    @location(1) bg_color: vec4<f32>,  // 배경색 RGBA
    @location(2) flags: u32,           // bit 0: selected, bit 1: search_match
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    instance: CellInstance,
) -> VertexOutput {
    // 단위 쿼드 → 셀 크기로 스케일 → 그리드 위치로 이동
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // 선택 영역이면 selection_color와 alpha blend
    // 기본 배경과 동일하면 discard
}
```

### 글리프 셰이더 (`glyph.wgsl`)

텍스처 아틀라스에서 글리프를 샘플링하고 서브픽셀 렌더링을 수행하는 셰이더.

- **입력**: 인스턴스 버퍼에서 셀 위치, 글리프 아틀라스 UV, 전경색, 글리프 크기/오프셋
- **버텍스**: 글리프 바운딩 박스를 셀 위치에 배치
- **프래그먼트**:
  - 그레이스케일 글리프: 아틀라스에서 알파 샘플링 → 전경색 * 알파 출력
  - 서브픽셀 렌더링: R/G/B 채널 각각 독립 샘플링 → LCD 안티앨리어싱
  - 컬러 이모지: 별도 RGBA 아틀라스에서 직접 색상 샘플링
- **서브픽셀 렌더링 방식**: 3x 수평 서브픽셀 위치에서 별도 래스터라이즈한 글리프를 아틀라스에 저장, 프래그먼트 셰이더에서 해당 서브픽셀 변형을 선택

```wgsl
@group(0) @binding(0) var glyph_atlas: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
@group(0) @binding(2) var emoji_atlas: texture_2d<f32>;

struct GlyphInstance {
    @location(0) pos: vec2<f32>,
    @location(1) uv_rect: vec4<f32>,    // atlas UV (x, y, w, h)
    @location(2) fg_color: vec4<f32>,
    @location(3) glyph_offset: vec2<f32>,
    @location(4) glyph_size: vec2<f32>,
    @location(5) flags: u32,            // bit 0: is_emoji, bit 1: is_bold, bit 2: subpixel_variant
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (is_emoji) {
        return textureSample(emoji_atlas, glyph_sampler, in.uv);
    }
    let alpha = textureSample(glyph_atlas, glyph_sampler, in.uv).r;
    return vec4(in.fg_color.rgb, alpha * in.fg_color.a);
}
```

### UI 셰이더 (`ui.wgsl`)

egui가 대부분의 UI 위젯을 처리하지만, 디바이더, 패인 테두리 등 커스텀 UI 요소를 렌더링하는 셰이더.

- **기능**: 둥근 모서리(SDF 기반), 그라데이션, 보더, 그림자
- **입력**: UI 쿼드의 위치/크기, 모서리 반경, 배경색/그라데이션 색상, 보더 두께/색상
- **SDF 라운드 렉트**: `sdRoundedBox()` 함수로 둥근 모서리 거리 계산 → 안티앨리어싱 엣지

```wgsl
fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    // 4개 코너 각각 다른 반경 지원
    let q = abs(p) - b + r.x;
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - r.x;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let d = sd_rounded_box(in.local_pos, in.half_size, in.corner_radii);
    let alpha = 1.0 - smoothstep(-0.5, 0.5, d);
    // 그라데이션: mix(color_top, color_bottom, in.local_pos.y / in.size.y)
    // 보더: d > -border_width이면 border_color
    return vec4(color.rgb, color.a * alpha);
}
```

### 효과 셰이더 (`effects.wgsl`)

알림 글로우, 디밍, 포커스 애니메이션 등 시각 효과를 처리하는 셰이더.

- **알림 글로우**: 알림 영역 주변에 가우시안 블러 기반 bloom 효과. additive 블렌딩으로 합성
- **디밍**: 비활성 패인 영역에 반투명 검정 오버레이. `alpha = dim_factor` (0.0~0.5)
- **포커스 애니메이션**: 보더 색상/두께를 `time` uniform으로 보간

```wgsl
@group(0) @binding(0) var<uniform> time: f32;

@fragment
fn fs_glow(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = length(in.uv - vec2(0.5));
    let glow = exp(-dist * dist * 8.0) * in.intensity;
    let pulse = sin(time * 3.0) * 0.3 + 0.7;
    return vec4(in.glow_color.rgb, glow * pulse);
}

@fragment
fn fs_dim(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(0.0, 0.0, 0.0, in.dim_factor);
}
```

### 커서 셰이더 (`cursor.wgsl`)

블록, 빔(I-beam), 언더라인 커서를 렌더링하고 블링크 애니메이션을 처리한다.

- **커서 유형**: `cursor_type` uniform으로 분기
  - 블록(0): 전체 셀 크기 쿼드, 반전 색상
  - 빔(1): 셀 좌측 2px 너비 세로 바
  - 언더라인(2): 셀 하단 2px 높이 가로 바
- **블링크**: `time` uniform으로 `sin()` 기반 알파 애니메이션, 주기 = 1초
- **부드러운 블링크**: `smoothstep`으로 on/off 전환을 부드럽게

```wgsl
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var alpha = 1.0;
    if (blink_enabled) {
        let t = fract(time * 1.0);  // 1초 주기
        alpha = smoothstep(0.3, 0.4, t) - smoothstep(0.7, 0.8, t);
        alpha = mix(0.0, 1.0, alpha);
    }
    // cursor_type에 따라 쿼드 영역 마스킹
    return vec4(cursor_color.rgb, cursor_color.a * alpha);
}
```

---

## 렌더 패스 구조

단일 `wgpu::RenderPass`에서 파이프라인을 전환하며 순차적으로 그린다. 오버드로가 발생하지만, 터미널의 UI 복잡도에서는 멀티 패스보다 단일 패스가 효율적이다.

### 패스 순서와 블렌딩

| 순서 | 패스 | 파이프라인 | 블렌딩 모드 | 설명 |
|------|------|-----------|-------------|------|
| 1 | 배경 패스 | `cell_bg_pipeline` | Opaque (불투명) | 셀 배경색 + 선택 영역 하이라이트. 불투명이므로 depth test 불필요 |
| 2 | 글리프 패스 | `glyph_pipeline` | Alpha Blend (`SrcAlpha, OneMinusSrcAlpha`) | 텍스트 렌더링. 배경 위에 알파 블렌딩 |
| 3 | 커서 패스 | `cursor_pipeline` | Alpha Blend | 커서 오버레이. 블링크 시 알파 0으로 페이드 |
| 4 | UI 패스 | `ui_pipeline` | Alpha Blend | 사이드바, 디바이더, 상태 표시줄. SDF 라운드 렉트 |
| 5 | 오버레이 패스 | `ui_pipeline` (재사용) | Alpha Blend | 명령 팔레트, 검색 바, 알림 토스트. UI 패스와 동일 파이프라인 |
| 6 | 효과 패스 | `effects_pipeline` | Additive (`SrcAlpha, One`) 또는 Alpha Blend | 글로우는 additive, 디밍은 alpha blend |

### Depth/Stencil 사용

- **Depth buffer**: 사용하지 않는다. 모든 렌더링은 2D이고, draw call 순서로 레이어링을 보장한다.
- **Stencil buffer**: 패인 경계 클리핑에 사용한다.
  - 각 패인의 사각 영역을 스텐실에 기록
  - 터미널 렌더링 시 해당 패인의 스텐실 마스크 내에서만 그리기
  - 대안으로 `set_scissor_rect()`를 사용할 수 있으며, 이 쪽이 더 단순하다 (권장)

### 클리어 전략

```rust
let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: &surface_view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
                r: bg.r, g: bg.g, b: bg.b, a: bg.a,  // 기본 배경색으로 클리어
            }),
            store: wgpu::StoreOp::Store,
        },
    })],
    depth_stencil_attachment: None,  // 2D 전용이므로 depth 불필요
    ..Default::default()
});
```

---

## 버퍼 전략

### 셀 인스턴스 버퍼

인스턴스드 렌더링으로 셀 그리드를 그린다. 각 셀은 인스턴스 데이터로 표현된다.

**인스턴스 레이아웃** (셀당 40바이트):

```
struct CellInstanceData {
    position: [f32; 2],      // 8 bytes  - 셀 그리드 좌표 (col, row)
    glyph_index: u32,        // 4 bytes  - 글리프 아틀라스 인덱스
    atlas_uv: [f32; 4],      // 16 bytes - 아틀라스 UV 사각형 (x, y, w, h)
    fg_color: u32,           // 4 bytes  - 전경색 (RGBA8 packed)
    bg_color: u32,           // 4 bytes  - 배경색 (RGBA8 packed)
    flags: u32,              // 4 bytes  - 스타일 플래그 (bold, italic, underline, selected, ...)
}
// Total: 40 bytes per cell
```

**업데이트 전략:**
- 전체 셀 그리드를 하나의 `wgpu::Buffer`에 저장 (`BufferUsages::VERTEX | BufferUsages::COPY_DST`)
- termwiz가 변경된 셀(dirty cells)만 추적
- 프레임마다 dirty 영역만 `queue.write_buffer()`로 부분 업데이트
- 연속된 dirty 행은 하나의 `write_buffer` 호출로 병합

### 배경 인스턴스 버퍼

셀 배경은 별도의 간소화된 인스턴스 버퍼를 사용한다.

```
struct BgInstanceData {
    position: [f32; 2],  // 8 bytes
    bg_color: u32,       // 4 bytes (RGBA8 packed)
    flags: u32,          // 4 bytes (selected, search_match)
}
// Total: 16 bytes per cell
```

### Uniform 버퍼

모든 셰이더가 공유하는 글로벌 uniform:

```
struct GlobalUniforms {
    viewport_size: [f32; 2],   // 뷰포트 크기 (논리 픽셀)
    cell_size: [f32; 2],       // 셀 크기 (논리 픽셀)
    time: f32,                 // 애니메이션 시간 (초)
    dpi_scale: f32,            // DPI 스케일 팩터
    grid_offset: [f32; 2],     // 그리드 시작 오프셋 (사이드바 너비 등)
}
```

- `@group(0) @binding(0)`에 바인딩
- 프레임마다 `queue.write_buffer()`로 업데이트 (64바이트 미만이므로 부담 없음)

### Storage Buffer (SSBO) 대안

셀 수가 많은 경우(예: 4K 모니터 + 작은 폰트 → 400x100 = 40,000 셀), 인스턴스 버퍼 대신 Storage Buffer를 사용할 수 있다.

```wgsl
@group(1) @binding(0) var<storage, read> cells: array<CellData>;

@vertex
fn vs_main(@builtin(instance_index) idx: u32) -> VertexOutput {
    let cell = cells[idx];
    // ...
}
```

- **장점**: 컴퓨트 셰이더에서 직접 셀 데이터 조작 가능 (정렬, 필터링)
- **단점**: OpenGL 폴백에서 SSBO 지원이 제한적
- **결정**: 인스턴스 버퍼를 기본으로, SSBO는 컴퓨트 셰이더 활용 시 전환

### 버퍼 업로드 전략

```
┌─────────────┐    write_buffer()    ┌─────────────┐
│  CPU 메모리   │ ──────────────────→ │  GPU 버퍼     │
│  (dirty cells)│                     │  (VERTEX)    │
└─────────────┘                      └─────────────┘
```

- **단순 경로** (기본): `queue.write_buffer()`로 직접 업로드. wgpu가 내부적으로 스테이징 버퍼를 관리한다.
- **명시적 스테이징** (대량 업데이트 시): `BufferUsages::MAP_WRITE | COPY_SRC` 스테이징 버퍼 → `encoder.copy_buffer_to_buffer()` → GPU 버퍼. 매 프레임 교대하는 더블 버퍼링으로 CPU-GPU 동기화 오버헤드를 제거한다.

**더블 버퍼링:**

```rust
struct CellBufferPool {
    buffers: [wgpu::Buffer; 2],
    current: usize,
}

impl CellBufferPool {
    fn current_buffer(&self) -> &wgpu::Buffer {
        &self.buffers[self.current]
    }
    fn swap(&mut self) {
        self.current = 1 - self.current;
    }
}
```

- 프레임 N: `buffers[0]`에 렌더링하면서 `buffers[1]`에 데이터 업로드
- 프레임 N+1: 스왑하여 `buffers[1]`로 렌더링, `buffers[0]`에 업로드

---

## 글리프 아틀라스 상세

### 아틀라스 구조

- **페이지 크기**: 2048x2048 기본 (GPU `max_texture_dimension_2d`에 따라 4096까지 확장 가능)
- **텍스처 포맷**:
  - 일반 글리프: `R8Unorm` (그레이스케일 알파맵, 페이지당 ~4MB)
  - 컬러 이모지: `Rgba8UnormSrgb` (페이지당 ~16MB)
- **멀티 페이지**: 일반적으로 1~3 페이지 사용. 페이지 인덱스를 글리프 메타데이터에 포함

### 글리프 패킹: Shelf-First-Fit 알고리즘

```
┌────────────────────────────────┐
│ shelf 0 (h=16) [A][B][C][D]...│
│ shelf 1 (h=20) [가][나][다]... │
│ shelf 2 (h=14) [a][b][c]...   │
│                                │
│         (빈 공간)              │
│                                │
└────────────────────────────────┘
```

- 각 shelf는 동일 높이의 가로 행
- 새 글리프 삽입 시: 기존 shelf 중 높이가 맞고 여유 공간이 있는 곳에 배치
- 맞는 shelf가 없으면: 새 shelf 생성 (높이 = 글리프 높이 + 2px 패딩)
- 페이지가 가득 차면: 새 페이지 할당

### 캐시 퇴거 (LRU)

- 각 글리프에 마지막 사용 프레임 번호를 기록
- 페이지가 가득 차고 새 글리프 필요 시:
  1. LRU 기준으로 오래된 글리프 제거
  2. 빈 공간이 파편화되면 전체 페이지 재빌드 (사용 중인 글리프만 재패킹)
- 재빌드 빈도를 제한한다 (최소 60초 간격)

### 서브픽셀 렌더링

LCD 디스플레이의 RGB 서브픽셀을 활용한 수평 해상도 3배 향상:

- 각 글리프를 3가지 서브픽셀 오프셋(0, 1/3, 2/3 픽셀)으로 래스터라이즈
- 아틀라스에 3개의 변형을 저장 (메모리 3배 사용)
- 프래그먼트 셰이더에서 글리프 위치의 소수 부분에 따라 적절한 변형 선택
- **비활성화 조건**: HiDPI (scale > 1.5)에서는 서브픽셀 효과가 미미하므로 비활성화하여 메모리 절약

### 컬러 이모지 처리

- 일반 글리프와 별도의 RGBA 아틀라스 사용
- `cosmic-text`의 `SwashCache`로 컬러 글리프 래스터라이즈
- 프래그먼트 셰이더에서 `flags.is_emoji` 비트로 분기하여 해당 아틀라스 샘플링
- 이모지 크기는 셀 높이에 맞추되 가로는 2셀 폭까지 허용 (wide character)

### 아틀라스 업로드

새 글리프 추가 시 전체 텍스처를 재업로드하지 않고 부분 업로드:

```rust
queue.write_texture(
    wgpu::TexelCopyTextureInfo {
        texture: &atlas_texture,
        mip_level: 0,
        origin: wgpu::Origin3d { x: glyph_x, y: glyph_y, z: 0 },
        aspect: wgpu::TextureAspect::All,
    },
    &glyph_bitmap,
    wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(glyph_width),
        rows_per_image: Some(glyph_height),
    },
    wgpu::Extent3d {
        width: glyph_width,
        height: glyph_height,
        depth_or_array_layers: 1,
    },
);
```

- 프레임당 새 글리프 업로드 횟수를 제한한다 (예: 최대 64개/프레임)
- 초과분은 다음 프레임으로 이월하여 프레임 드롭 방지

---

## 멀티 패인 렌더링

### Option A: Viewport/Scissor 방식 (권장)

단일 렌더 패스에서 각 패인마다 `set_scissor_rect()`를 설정하고 그린다.

```rust
for pane in &panes {
    render_pass.set_scissor_rect(
        pane.x as u32,
        pane.y as u32,
        pane.width as u32,
        pane.height as u32,
    );
    // 해당 패인의 셀 인스턴스 범위만 draw
    render_pass.draw(0..6, pane.instance_range.clone());
}
```

- **장점**: 추가 GPU 메모리 불필요, 렌더 타겟 전환 없음, 구현이 단순
- **단점**: 패인 간 독립 후처리 불가 (디밍은 별도 오버레이 쿼드로 처리)
- **성능**: 패인 수 N에 대해 draw call N배이지만, 터미널 렌더링에서 draw call은 병목이 아님

### Option B: Render-to-Texture 방식

각 패인을 별도 텍스처에 렌더링한 뒤 최종 컴포지트 패스에서 합성.

- **장점**: 패인별 독립 효과 적용 가능 (블러, 줌 애니메이션)
- **단점**: 패인 수 * 텍스처 메모리, 추가 렌더 패스 오버헤드
- **적용 시점**: 패인 줌 애니메이션이나 패인별 블러 효과가 필요할 때만

**결론: Option A를 기본으로 사용하고, 특수 효과가 필요한 패인만 Option B로 전환한다.**

### 디바이더 렌더링

패인 사이의 구분선은 UI 패스에서 그린다.

- 두께: 1~4px (설정 가능)
- 색상: 테마 색상, 활성 패인 경계는 하이라이트
- 드래그 핸들: 마우스 hover 시 두께 증가 + 커서 변경 (winit 측 처리)

### 비활성 패인 디밍

비활성 패인 위에 반투명 검정 쿼드를 오버레이:

```wgsl
// 효과 패스에서
@fragment
fn fs_pane_dim(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(0.0, 0.0, 0.0, uniforms.dim_alpha);  // dim_alpha = 0.15~0.30
}
```

- 블렌딩: `SrcAlpha, OneMinusSrcAlpha` (표준 알파 블렌딩)
- `dim_alpha` 값은 설정에서 조정 가능 (기본 0.2)

---

## UI 위젯 렌더링

### 터미널과 UI의 분리

터미널 셀 렌더링과 UI 위젯 렌더링은 별도의 파이프라인과 버텍스 버퍼를 사용한다.

- **터미널**: 인스턴스드 쿼드, 고정 그리드 레이아웃, 셀 단위 업데이트
- **UI**: 자유 배치 쿼드, SDF 기반 도형, 이벤트 기반 업데이트

### UI 렌더 접근법

터미널 셀 그리드는 커스텀 wgpu 셰이더로 렌더링하고, UI 위젯(사이드바, 명령 팔레트, 설정, 검색 바, 알림 등)은 egui로 렌더링한다.

| 영역 | 렌더러 | 근거 |
|------|--------|------|
| 터미널 셀 그리드 | 커스텀 wgpu 셰이더 | 성능 핵심, 인스턴스드 렌더링으로 최적화 |
| UI 위젯 | egui (egui-wgpu) | 빠른 개발, 풍부한 위젯, 충분한 성능 |

**전략:**
1. **Phase 1 (MVP)**: egui로 모든 UI 위젯 구현. `egui-wgpu` 백엔드로 기존 렌더 파이프라인에 통합
2. **Phase 2**: 성능 병목이 되는 UI 요소가 있으면 커스텀 SDF 렌더러로 전환

### SDF (Signed Distance Field) UI

SDF를 사용하면 해상도에 독립적인 부드러운 도형을 렌더링할 수 있다:

- **둥근 사각형**: `sdRoundedBox()` (사이드바 배경, 명령 팔레트)
- **원**: `sdCircle()` (알림 뱃지, 로딩 스피너)
- **선분**: `sdSegment()` (디바이더, 밑줄)
- **안티앨리어싱**: `smoothstep()`으로 엣지를 부드럽게. DPI 스케일에 따라 smoothstep 범위 조정

---

## 투명도와 시각 효과

### 배경 투명도 (OS별 윈도우 합성)

#### Windows

```rust
// DWM blur behind window
use windows::Win32::Graphics::Dwm::{
    DwmEnableBlurBehindWindow, DWM_BLURBEHIND,
};

let bb = DWM_BLURBEHIND {
    dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
    fEnable: true.into(),
    hRgnBlur: region,
    fTransitionOnMaximized: false.into(),
};
DwmEnableBlurBehindWindow(hwnd, &bb);

// Windows 11: Mica / Acrylic
use windows::Win32::UI::WindowsAndMessaging::SetWindowCompositionAttribute;
```

- `raw-window-handle` 크레이트로 winit 윈도우에서 HWND 추출
- Windows 10 1903+: `SetWindowCompositionAttribute`로 아크릴 효과
- Windows 11: Mica 효과 (더 가볍고 시스템 통합적)

#### macOS

```rust
// raw-window-handle로 NSView 추출 후 NSVisualEffectView 설정
// objc2 크레이트 사용
let ns_view: *mut Object = window.ns_view();
// NSVisualEffectView를 서브뷰로 추가, blendingMode = .behindWindow
```

#### Linux/Wayland

- `layer-shell` 프로토콜 또는 컴포지터별 투명도 지원
- Sway/Hyprland: 윈도우 룰로 opacity 설정 가능
- wgpu surface의 알파 채널을 활용 (`CompositeAlphaMode::PreMultiplied`)

#### Linux/X11

- `_NET_WM_WINDOW_OPACITY` 프로퍼티 설정
- picom 등 외부 컴포지터 필요
- XRender 컴포지팅으로 블러 효과

### 알림 글로우 효과

- **구현**: 알림 영역 주변에 확장된 쿼드를 additive 블렌딩으로 렌더링
- **가우시안 근사**: `exp(-dist^2 * factor)` 프래그먼트 셰이더에서 실시간 계산 (별도 블러 패스 불필요)
- **펄스 애니메이션**: `sin(time * speed) * 0.3 + 0.7`로 밝기 변동
- **페이드 아웃**: 알림 소멸 시 `intensity`를 0으로 보간

### 포커스 애니메이션

- 활성 패인 보더: 색상과 두께를 `lerp()`로 보간
- 전환 시간: 150ms (ease-out 커브)
- GPU 측에서 `time` uniform과 `focus_start_time`의 차이로 보간 비율 계산

---

## HiDPI / Retina 대응

### DPI 스케일 팩터

- `winit::window::Window::scale_factor()`로 현재 DPI 스케일 획득
- `ScaleFactorChanged` 이벤트로 런타임 변경 감지 (모니터 간 윈도우 이동)

### 좌표 체계

```
논리 픽셀 (Logical)          물리 픽셀 (Physical)
┌──────────────┐            ┌────────────────────────────┐
│  100 x 50    │  × 2.0 →  │       200 x 100            │
│  (사용자 좌표) │            │    (실제 렌더링 해상도)      │
└──────────────┘            └────────────────────────────┘
```

- **모든 레이아웃 계산**: 논리 픽셀 단위
- **GPU 렌더링**: 물리 픽셀 단위 (surface 해상도)
- **변환**: 셰이더의 `dpi_scale` uniform으로 처리, 또는 뷰포트 설정에서 한 번에 변환

### 글리프 래스터라이즈

- 글리프는 항상 **물리 픽셀 크기**로 래스터라이즈
- `font_size_physical = font_size_logical * scale_factor`
- 예: 14pt 폰트, scale_factor 2.0 → 28pt로 래스터라이즈 → 아틀라스에 저장

### DPI 변경 시 처리

1. `ScaleFactorChanged` 이벤트 수신
2. surface 크기 업데이트 (`configure()`)
3. 글리프 아틀라스 전체 재빌드 (새 물리 크기로 재래스터라이즈)
4. 셀 크기 재계산
5. 인스턴스 버퍼 전체 재생성
6. uniform 버퍼의 `dpi_scale` 업데이트

아틀라스 재빌드는 비용이 크므로, DPI 변경 이벤트를 디바운싱한다 (200ms).

---

## GPU 메모리 예산

### 일반적인 메모리 사용량 추정

| 항목 | 크기 | 비고 |
|------|------|------|
| 글리프 아틀라스 (그레이스케일) | ~4MB/page × 1~3 pages = 4~12MB | R8, 2048x2048 |
| 글리프 아틀라스 (컬러 이모지) | ~16MB/page × 1 page = 16MB | RGBA8, 2048x2048 |
| 셀 인스턴스 버퍼 | ~40B × cols × rows × panes | 200×50×4 = ~1.6MB |
| 배경 인스턴스 버퍼 | ~16B × cols × rows × panes | ~0.6MB |
| UI 버텍스 버퍼 | ~4MB | 사이드바, 팔레트 등 |
| Uniform 버퍼 | <1KB | 무시할 수준 |
| Depth/Stencil | ~4MB/window | 사용 시 |
| 스왑체인 텍스처 | ~8MB (1080p RGBA) ~ 33MB (4K RGBA) | OS 관리 |
| **합계 (일반)** | **30~50MB** | |
| **합계 (4K + 이모지 다수)** | **60~80MB** | |

### 예산 한도

- **목표**: 100MB 이하
- **경고 임계**: 80MB 초과 시 로그 경고
- **폴백**: 150MB 초과 시 효과 비활성화, 아틀라스 페이지 축소

---

## GPU 폴백 전략

### wgpu 백엔드 우선순위

```rust
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::all(),  // 모든 백엔드 시도
    ..Default::default()
});
```

실제 선택 우선순위:

| 우선순위 | 백엔드 | 플랫폼 |
|----------|--------|--------|
| 1 | Vulkan | Windows, Linux |
| 2 | DX12 | Windows |
| 3 | Metal | macOS |
| 4 | DX11 | Windows (레거시) |
| 5 | OpenGL ES | Linux (폴백), Android |

### 기능 감지와 그레이스풀 디그레이데이션

```rust
let features = adapter.features();
let limits = adapter.limits();

let quality_preset = if features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
    QualityPreset::High
} else if limits.max_texture_dimension_2d >= 4096 {
    QualityPreset::Medium
} else {
    QualityPreset::Low
};
```

| 기능 | 있음 | 없음 (폴백) |
|------|------|-------------|
| 컴퓨트 셰이더 | SSBO 기반 셀 처리 | 버텍스 셰이더 기반 (기본) |
| 텍스처 4096+ | 대형 아틀라스 페이지 | 2048 페이지 (더 많은 페이지) |
| MSAA | 4x MSAA 안티앨리어싱 | SDF 기반 소프트 엣지 |
| 서브픽셀 렌더링 | LCD 안티앨리어싱 | 그레이스케일 안티앨리어싱 |
| Additive 블렌딩 | 글로우 효과 | 알파 블렌딩 글로우 (근사) |

### 소프트웨어 렌더러 폴백

GPU가 전혀 없는 환경 (VM, 원격 서버 등):

- wgpu의 `llvmpipe` (Mesa) 또는 `SwiftShader` (Google) 소프트웨어 백엔드
- `adapter.get_info().device_type == DeviceType::Cpu`로 감지
- 소프트웨어 모드에서는:
  - 모든 효과 비활성화 (글로우, 블러, 애니메이션)
  - 아틀라스 크기 축소 (1024x1024)
  - 프레임 레이트 제한 (30fps)
  - UI를 최소한으로 단순화

### 품질 프리셋 요약

| 프리셋 | 대상 | 아틀라스 | 효과 | FPS 목표 |
|--------|------|---------|------|----------|
| `Ultra` | RTX 3060+ | 4096, 서브픽셀 | 전체 | 144+ |
| `High` | GTX 1060+ | 2048, 서브픽셀 | 전체 | 60+ |
| `Medium` | Intel HD 630+ | 2048, 서브픽셀 없음 | 글로우 없음 | 60 |
| `Low` | Intel HD 4000+ | 1024 | 효과 없음 | 30~60 |
| `Software` | CPU 전용 | 1024 | 효과 없음 | 30 |
