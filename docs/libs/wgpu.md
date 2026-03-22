# wgpu 24 사용 가이드

wgpu는 WebGPU API 기반의 크로스 플랫폼 GPU 추상화 라이브러리입니다. Vulkan, Metal, D3D12, OpenGL, WebGL/WebGPU 백엔드를 단일 API로 지원합니다.

---

## 목차

1. [초기화: Instance, Adapter, Device, Queue](#초기화-instance-adapter-device-queue)
2. [Surface 설정](#surface-설정)
3. [SurfaceConfiguration과 PresentMode](#surfaceconfiguration과-presentmode)
4. [RenderPass](#renderpass)
5. [Buffer 생성 및 데이터 업로드](#buffer-생성-및-데이터-업로드)
6. [셰이더 모듈 (WGSL)](#셰이더-모듈-wgsl)
7. [RenderPipeline](#renderpipeline)
8. [BindGroup과 BindGroupLayout](#bindgroup과-bindgrouplayout)
9. [Texture와 TextureView](#texture와-textureview)
10. [SurfaceError 처리](#surfaceerror-처리)
11. [winit 통합 패턴](#winit-통합-패턴)

---

## 초기화: Instance, Adapter, Device, Queue

### 초기화 순서

```
Instance → Surface (선택) → Adapter → Device + Queue
```

### Instance

`Instance`는 wgpu의 진입점입니다. 백엔드와 유효성 검사 레이어를 설정합니다.

```rust
use wgpu::{Instance, InstanceDescriptor, Backends, Dx12Compiler, Gles3MinorVersion};

let instance = Instance::new(&InstanceDescriptor {
    // 사용할 백엔드 지정
    backends: Backends::all(), // Vulkan + Metal + D3D12 + OpenGL 전부
    // 또는 플랫폼별 최적 선택:
    // backends: Backends::PRIMARY, // Vulkan + Metal + D3D12

    dx12_shader_compiler: Dx12Compiler::default(),
    flags: wgpu::InstanceFlags::default(),
    gles_minor_version: Gles3MinorVersion::default(),
});
```

### Surface (창과 연결)

Surface는 반드시 Adapter를 요청하기 전에 생성해야 합니다. (Adapter가 Surface를 지원하는지 확인해야 하므로)

```rust
use wgpu::Surface;

// winit Window로부터 Surface 생성
// SAFETY: window는 Surface보다 오래 살아야 함
let surface = instance.create_surface(window).unwrap();
```

### Adapter 요청

Adapter는 물리적 GPU를 나타냅니다.

```rust
use wgpu::{Adapter, RequestAdapterOptions, PowerPreference};

let adapter = instance.request_adapter(&RequestAdapterOptions {
    // 전력 선호도
    power_preference: PowerPreference::HighPerformance, // 외장 GPU 선호
    // power_preference: PowerPreference::LowPower,    // 내장 GPU 선호 (배터리 절약)

    // Surface와 호환되는 Adapter만 선택
    compatible_surface: Some(&surface),

    // 소프트웨어 렌더러도 허용할지 (없는 경우 fallback)
    force_fallback_adapter: false,
})
.await
.expect("GPU 어댑터를 찾을 수 없습니다");

// Adapter 정보 출력
let info = adapter.get_info();
println!("GPU: {} ({:?})", info.name, info.backend);
```

### Device와 Queue 생성

`Device`는 논리적 GPU 디바이스, `Queue`는 커맨드 제출 큐입니다.

```rust
use wgpu::{Device, Queue, DeviceDescriptor, Features, Limits};

let (device, queue) = adapter.request_device(
    &DeviceDescriptor {
        label: Some("Main Device"),

        // 필요한 GPU 기능 지정
        required_features: Features::empty(),
        // 또는 특정 기능 활성화:
        // required_features: Features::TEXTURE_COMPRESSION_BC | Features::POLYGON_MODE_LINE,

        // 리소스 한도 설정
        required_limits: Limits::default(),
        // 저사양 기기 대응:
        // required_limits: Limits::downlevel_defaults(),

        memory_hints: Default::default(),
        trace_path: None,
    },
    None, // trace path (디버깅용)
)
.await
.expect("디바이스 생성 실패");
```

### 전체 초기화 흐름

```rust
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
}

impl GpuContext {
    pub async fn new(window: Arc<winit::window::Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: window Arc가 surface보다 오래 삶
        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await.unwrap();

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            },
            None,
        ).await.unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps.formats.iter()
            .find(|f| !f.is_srgb()) // sRGB 피하기 (아래 설명 참고)
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        Self { instance, surface, adapter, device, queue, surface_config }
    }
}
```

---

## Surface 설정

### sRGB vs non-sRGB 선택 주의

Surface 포맷 선택은 색상 처리에 직접적인 영향을 미칩니다.

#### 이중 감마 보정 버그

sRGB 포맷(`Bgra8UnormSrgb`, `Rgba8UnormSrgb`)을 사용하면 GPU가 렌더링 출력을 자동으로 선형 공간에서 sRGB 공간으로 변환합니다.

**문제:** 셰이더에서 이미 sRGB 색상값(예: `#FF5733` = `(1.0, 0.341, 0.2)`)을 직접 사용하면, GPU가 또 한 번 감마 변환을 적용하여 색상이 어두워지거나 바래 보입니다.

```
버그 발생 과정:
셰이더에 sRGB 값 입력 → GPU sRGB 포맷이 감마 보정 적용 → 이중 감마 보정 → 색상 왜곡
```

**해결책 두 가지:**

```rust
// 방법 1: non-sRGB 포맷 사용 (권장 — 터미널 에뮬레이터에 적합)
// 셰이더에서 sRGB 값을 그대로 사용 가능, GPU가 변환하지 않음
let format = surface_caps.formats.iter()
    .find(|f| !f.is_srgb())  // Bgra8Unorm, Rgba8Unorm 등
    .copied()
    .unwrap_or(surface_caps.formats[0]);

// 방법 2: sRGB 포맷 사용 + 셰이더에서 선형 색상 사용
// 셰이더에서 sRGB → 선형 변환 후 출력해야 올바른 색상
// let format = surface_caps.formats.iter()
//     .find(|f| f.is_srgb())
//     .copied()
//     .unwrap_or(surface_caps.formats[0]);
```

**터미널 에뮬레이터 권장:** non-sRGB 포맷(`Bgra8Unorm` 또는 `Rgba8Unorm`) 사용. 터미널 색상은 대부분 sRGB 16진수 값으로 정의되므로, 셰이더에서 그대로 사용하기 편합니다.

### 주요 텍스처 포맷

| 포맷 | 설명 | 용도 |
|------|------|------|
| `Bgra8Unorm` | BGRA 8비트, 선형 | 터미널, UI |
| `Rgba8Unorm` | RGBA 8비트, 선형 | 범용 |
| `Bgra8UnormSrgb` | BGRA 8비트, sRGB 자동 변환 | 3D 그래픽 |
| `Rgba8UnormSrgb` | RGBA 8비트, sRGB 자동 변환 | 3D 그래픽 |
| `Rgba16Float` | 16비트 부동소수점 | HDR |

---

## SurfaceConfiguration과 PresentMode

```rust
use wgpu::{SurfaceConfiguration, PresentMode, TextureUsages, CompositeAlphaMode};

let config = SurfaceConfiguration {
    // 용도: 렌더 출력으로 사용
    usage: TextureUsages::RENDER_ATTACHMENT,

    // 픽셀 포맷
    format: wgpu::TextureFormat::Bgra8Unorm,

    // 창 내부 크기 (0이면 패닉 — 항상 max(1) 적용)
    width: window_size.width.max(1),
    height: window_size.height.max(1),

    // 화면 표시 방식
    present_mode: PresentMode::Fifo,

    // 투명도 합성 방식
    alpha_mode: CompositeAlphaMode::Auto,

    // 추가 뷰 포맷 (보통 비어있음)
    view_formats: vec![],

    // 프레임 지연 설정 (낮을수록 응답성 좋음, 높을수록 부드러움)
    desired_maximum_frame_latency: 2,
};

surface.configure(&device, &config);
```

### PresentMode 비교

| 모드 | 설명 | 용도 |
|------|------|------|
| `Fifo` | VSync 켜짐, 찢김 없음, 지연 있음 | 기본값, 터미널 권장 |
| `FifoRelaxed` | VSync, 단 늦으면 즉시 표시 | 게임 |
| `Immediate` | VSync 꺼짐, 즉시 표시, 찢김 가능 | 저지연 필요 시 |
| `Mailbox` | 최신 프레임만 큐, 찢김 없음 | 게임 (트리플 버퍼링) |
| `AutoVsync` | 지원되면 Fifo, 아니면 Immediate | 자동 선택 |
| `AutoNoVsync` | Mailbox 또는 Immediate | 자동 선택 |

### 창 크기 변경 시 재구성

```rust
fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
    if new_size.width == 0 || new_size.height == 0 {
        return; // 최소화 시 무시
    }
    self.config.width = new_size.width;
    self.config.height = new_size.height;
    self.surface.configure(&self.device, &self.config);
}
```

---

## RenderPass

`RenderPass`는 GPU에 렌더링 커맨드를 기록하는 핸들입니다. 프레임마다 새로 생성합니다.

### 기본 렌더 루프

```rust
fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
    // 1. 현재 프레임의 Surface 텍스처 획득
    let output = self.surface.get_current_texture()?;
    let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

    // 2. CommandEncoder 생성 (GPU 커맨드 녹화기)
    let mut encoder = self.device.create_command_encoder(
        &wgpu::CommandEncoderDescriptor { label: Some("Render Encoder") }
    );

    // 3. RenderPass 시작
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Main Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None, // MSAA 사용 시 설정
                ops: wgpu::Operations {
                    // 프레임 시작 시 배경색으로 초기화
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1, g: 0.1, b: 0.1, a: 1.0, // 어두운 회색
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        // 4. 파이프라인 바인딩
        render_pass.set_pipeline(&self.render_pipeline);

        // 5. BindGroup 바인딩 (uniform, texture 등)
        render_pass.set_bind_group(0, &self.bind_group, &[]);

        // 6. 버텍스 버퍼 바인딩
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

        // 7. 시저 렉트 설정 (선택사항)
        render_pass.set_scissor_rect(0, 0, self.config.width, self.config.height);

        // 8. 드로우 커맨드
        render_pass.draw(0..self.vertex_count, 0..1);
        // 또는 인덱스 버퍼 사용 시:
        // render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        // render_pass.draw_indexed(0..self.index_count, 0, 0..1);
    } // render_pass 드롭 → RenderPass 종료

    // 9. 커맨드 제출
    self.queue.submit(std::iter::once(encoder.finish()));

    // 10. 화면에 표시
    output.present();

    Ok(())
}
```

### set_pipeline

```rust
// 렌더 파이프라인 바인딩
render_pass.set_pipeline(&pipeline);
```

### set_bind_group

```rust
// 인덱스 0번 슬롯에 bind_group 바인딩
// 동적 오프셋은 uniform 버퍼 배열에서 사용
render_pass.set_bind_group(0, &bind_group, &[]);

// 동적 오프셋 예시 (대형 uniform 버퍼에서 오프셋으로 접근)
render_pass.set_bind_group(0, &bind_group, &[offset]);
```

### set_vertex_buffer

```rust
// 슬롯 0에 전체 버퍼 바인딩
render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));

// 범위 지정
render_pass.set_vertex_buffer(0, vertex_buffer.slice(0..size));

// 여러 버텍스 버퍼 (다중 슬롯)
render_pass.set_vertex_buffer(0, position_buffer.slice(..));
render_pass.set_vertex_buffer(1, color_buffer.slice(..));
```

### set_scissor_rect — 검증 규칙

시저 렉트는 렌더링 영역을 직사각형으로 제한합니다. **검증 규칙을 지키지 않으면 패닉이 발생합니다.**

```rust
// 시그니처
render_pass.set_scissor_rect(x: u32, y: u32, width: u32, height: u32);
```

**검증 규칙 (wgpu 내부 검증기가 강제):**

```
1. width > 0 && height > 0           — 0 크기 금지
2. x + width <= surface_config.width  — 오른쪽 경계 초과 금지
3. y + height <= surface_config.height — 아래쪽 경계 초과 금지
```

```rust
// 안전한 시저 렉트 설정 헬퍼
fn set_scissor_safe(
    render_pass: &mut wgpu::RenderPass,
    x: u32, y: u32,
    width: u32, height: u32,
    surface_width: u32, surface_height: u32,
) {
    // 경계 클리핑
    let x = x.min(surface_width);
    let y = y.min(surface_height);
    let width = width.min(surface_width - x);
    let height = height.min(surface_height - y);

    // 0 크기 방지
    if width == 0 || height == 0 {
        return;
    }

    render_pass.set_scissor_rect(x, y, width, height);
}

// 사용 예
set_scissor_safe(&mut render_pass, cell_x, cell_y, cell_w, cell_h,
                 config.width, config.height);
```

**주의:** 창 크기 변경 직후 이전 크기로 시저 렉트를 설정하면 검증 실패합니다. `Resized` 이벤트에서 surface를 재구성한 후 새 크기를 사용해야 합니다.

### set_viewport

뷰포트는 시저 렉트와 다르게 NDC 공간을 물리 픽셀 공간으로 매핑합니다.

```rust
render_pass.set_viewport(
    x as f32, y as f32,
    width as f32, height as f32,
    0.0, 1.0, // depth range
);
```

---

## Buffer 생성 및 데이터 업로드

### 버퍼 생성

```rust
use wgpu::{Buffer, BufferDescriptor, BufferUsages};

// 용도별 사용 플래그
let vertex_buffer = device.create_buffer(&BufferDescriptor {
    label: Some("Vertex Buffer"),
    size: (vertices.len() * std::mem::size_of::<Vertex>()) as u64,
    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
    mapped_at_creation: false,
});

let index_buffer = device.create_buffer(&BufferDescriptor {
    label: Some("Index Buffer"),
    size: (indices.len() * 4) as u64, // u32 = 4바이트
    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
    mapped_at_creation: false,
});

let uniform_buffer = device.create_buffer(&BufferDescriptor {
    label: Some("Uniform Buffer"),
    size: std::mem::size_of::<Uniforms>() as u64,
    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    mapped_at_creation: false,
});
```

### 초기 데이터와 함께 생성

```rust
use wgpu::util::{DeviceExt, BufferInitDescriptor};

// 데이터와 함께 버퍼 생성 (DeviceExt 트레이트 필요)
let vertex_buffer = device.create_buffer_init(&BufferInitDescriptor {
    label: Some("Vertex Buffer"),
    contents: bytemuck::cast_slice(&vertices), // &[u8]로 변환
    usage: BufferUsages::VERTEX,
});
```

### 데이터 업로드

```rust
// Queue::write_buffer — 가장 간단한 방법
queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

// 오프셋 지정
queue.write_buffer(&buffer, offset_bytes, data);
```

### bytemuck을 이용한 안전한 캐스팅

```rust
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
    color: [f32; 4],
}

// &[Vertex] → &[u8]
let bytes: &[u8] = bytemuck::cast_slice(&vertices);
```

### BufferUsages 요약

| 플래그 | 설명 |
|--------|------|
| `VERTEX` | 버텍스 버퍼 |
| `INDEX` | 인덱스 버퍼 |
| `UNIFORM` | Uniform 버퍼 |
| `STORAGE` | 스토리지 버퍼 (컴퓨트 셰이더) |
| `COPY_SRC` | 복사 소스 |
| `COPY_DST` | 복사 대상 (write_buffer 사용 시 필수) |
| `MAP_READ` | CPU에서 읽기 (readback) |
| `MAP_WRITE` | CPU에서 쓰기 (staging buffer) |
| `INDIRECT` | 간접 드로우 커맨드 |

---

## 셰이더 모듈 (WGSL)

WGSL(WebGPU Shading Language)은 wgpu의 기본 셰이더 언어입니다.

### 셰이더 모듈 생성

```rust
let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some("Terminal Shader"),
    source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
});
```

### WGSL 셰이더 예제

```wgsl
// shader.wgsl

// Uniform 구조체
struct Uniforms {
    // 뷰포트 크기 (픽셀)
    viewport_size: vec2<f32>,
    // 셀 크기 (픽셀)
    cell_size: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// 텍스처와 샘플러
@group(0) @binding(1)
var glyph_texture: texture_2d<f32>;

@group(0) @binding(2)
var glyph_sampler: sampler;

// 버텍스 입력
struct VertexInput {
    @location(0) position: vec2<f32>,    // 픽셀 좌표
    @location(1) tex_coords: vec2<f32>,  // UV 좌표
    @location(2) fg_color: vec4<f32>,    // 전경색
    @location(3) bg_color: vec4<f32>,    // 배경색
}

// 버텍스 출력
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
    @location(2) bg_color: vec4<f32>,
}

// 픽셀 좌표 → NDC 변환 함수
fn pixel_to_ndc(pixel: vec2<f32>, viewport: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        (pixel.x / viewport.x) * 2.0 - 1.0,
        1.0 - (pixel.y / viewport.y) * 2.0  // Y축 반전
    );
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(
        pixel_to_ndc(in.position, uniforms.viewport_size),
        0.0, 1.0
    );
    out.tex_coords = in.tex_coords;
    out.fg_color = in.fg_color;
    out.bg_color = in.bg_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(glyph_texture, glyph_sampler, in.tex_coords).r;
    // alpha가 1.0이면 전경색, 0.0이면 배경색
    return mix(in.bg_color, in.fg_color, alpha);
}
```

---

## RenderPipeline

RenderPipeline은 GPU 렌더링 파이프라인의 전체 상태를 정의합니다.

```rust
use wgpu::{
    RenderPipeline, RenderPipelineDescriptor,
    VertexState, FragmentState, PrimitiveState,
    MultisampleState, BlendState, ColorWrites,
    VertexBufferLayout, VertexAttribute, VertexStepMode,
    PipelineLayoutDescriptor,
};

// 버텍스 버퍼 레이아웃 정의
let vertex_buffer_layout = VertexBufferLayout {
    array_stride: std::mem::size_of::<Vertex>() as u64,
    step_mode: VertexStepMode::Vertex, // Vertex | Instance
    attributes: &wgpu::vertex_attr_array![
        0 => Float32x2,  // position
        1 => Float32x2,  // tex_coords
        2 => Float32x4,  // fg_color
        3 => Float32x4,  // bg_color
    ],
};

// PipelineLayout (BindGroupLayout 묶음)
let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
    label: Some("Pipeline Layout"),
    bind_group_layouts: &[&bind_group_layout],
    push_constant_ranges: &[],
});

let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
    label: Some("Terminal Pipeline"),
    layout: Some(&pipeline_layout),

    // 버텍스 셰이더
    vertex: VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        buffers: &[vertex_buffer_layout],
        compilation_options: Default::default(),
    },

    // 프래그먼트 셰이더
    fragment: Some(FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
            format: surface_config.format,
            blend: Some(BlendState::ALPHA_BLENDING), // 투명도 합성
            write_mask: ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
    }),

    // 기본 도형 (삼각형)
    primitive: PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None, // 2D UI는 백면 제거 불필요
        polygon_mode: wgpu::PolygonMode::Fill,
        unclipped_depth: false,
        conservative: false,
    },

    // 깊이/스텐실 (2D는 불필요)
    depth_stencil: None,

    // 멀티샘플링 (MSAA 사용 시 설정)
    multisample: MultisampleState {
        count: 1, // MSAA 4x: count: 4
        mask: !0,
        alpha_to_coverage_enabled: false,
    },

    multiview: None,
    cache: None,
});
```

### BlendState 주요 옵션

| 상수 | 설명 |
|------|------|
| `BlendState::REPLACE` | 블렌딩 없음, 덮어쓰기 |
| `BlendState::ALPHA_BLENDING` | 알파 블렌딩 (SrcAlpha, OneMinusSrcAlpha) |
| `BlendState::PREMULTIPLIED_ALPHA_BLENDING` | 프리멀티플라이드 알파 |

---

## BindGroup과 BindGroupLayout

### BindGroupLayout 정의

```rust
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    ShaderStages, BindingType, BufferBindingType, TextureSampleType,
    TextureViewDimension, SamplerBindingType,
};

let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
    label: Some("Bind Group Layout"),
    entries: &[
        // 바인딩 0: Uniform 버퍼
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        // 바인딩 1: 텍스처
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        // 바인딩 2: 샘플러
        BindGroupLayoutEntry {
            binding: 2,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
            count: None,
        },
    ],
});
```

### BindGroup 생성

```rust
use wgpu::{BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource};

let bind_group = device.create_bind_group(&BindGroupDescriptor {
    label: Some("Bind Group"),
    layout: &bind_group_layout,
    entries: &[
        BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        },
        BindGroupEntry {
            binding: 1,
            resource: BindingResource::TextureView(&texture_view),
        },
        BindGroupEntry {
            binding: 2,
            resource: BindingResource::Sampler(&sampler),
        },
    ],
});
```

### BindGroup 업데이트

BindGroup은 불변입니다. 리소스가 바뀌면 새로 생성해야 합니다.

```rust
// 텍스처 아틀라스 업데이트 시 bind_group 재생성
self.bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
    layout: &self.bind_group_layout,
    entries: &[
        // ... 새 리소스로 재생성
    ],
    label: None,
});
```

---

## Texture와 TextureView

### Texture 생성

```rust
use wgpu::{
    Texture, TextureDescriptor, TextureDimension, TextureUsages,
    TextureFormat, Extent3d,
};

// 텍스처 생성 (글리프 아틀라스 예시)
let texture = device.create_texture(&TextureDescriptor {
    label: Some("Glyph Atlas"),
    size: Extent3d {
        width: 1024,
        height: 1024,
        depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: TextureDimension::D2,
    format: TextureFormat::R8Unorm, // 알파 채널만 (글리프에 적합)
    // 또는 TextureFormat::Rgba8Unorm (컬러 이미지)
    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
    view_formats: &[],
});
```

### 텍스처에 데이터 업로드

```rust
use wgpu::{ImageCopyTexture, ImageDataLayout, Origin3d, TextureAspect};

// 픽셀 데이터 업로드
queue.write_texture(
    ImageCopyTexture {
        texture: &texture,
        mip_level: 0,
        origin: Origin3d { x: 0, y: 0, z: 0 }, // 업로드 시작 위치
        aspect: TextureAspect::All,
    },
    &pixel_data, // &[u8]
    ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(1024), // 한 행의 바이트 수 (R8Unorm: width * 1)
        rows_per_image: Some(1024),
    },
    Extent3d {
        width: 1024,
        height: 1024,
        depth_or_array_layers: 1,
    },
);

// 부분 업데이트 (글리프 추가 시)
queue.write_texture(
    ImageCopyTexture {
        texture: &texture,
        mip_level: 0,
        origin: Origin3d { x: glyph_x, y: glyph_y, z: 0 },
        aspect: TextureAspect::All,
    },
    &glyph_pixels,
    ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(glyph_width),
        rows_per_image: Some(glyph_height),
    },
    Extent3d {
        width: glyph_width,
        height: glyph_height,
        depth_or_array_layers: 1,
    },
);
```

### TextureView 생성

```rust
let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

// 상세 설정이 필요한 경우
let texture_view = texture.create_view(&wgpu::TextureViewDescriptor {
    label: Some("Glyph Atlas View"),
    format: Some(wgpu::TextureFormat::R8Unorm),
    dimension: Some(wgpu::TextureViewDimension::D2),
    aspect: wgpu::TextureAspect::All,
    base_mip_level: 0,
    mip_level_count: None,
    base_array_layer: 0,
    array_layer_count: None,
    ..Default::default()
});
```

### Sampler 생성

```rust
let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
    label: Some("Glyph Sampler"),
    address_mode_u: wgpu::AddressMode::ClampToEdge,
    address_mode_v: wgpu::AddressMode::ClampToEdge,
    address_mode_w: wgpu::AddressMode::ClampToEdge,
    mag_filter: wgpu::FilterMode::Linear,  // 확대 필터
    min_filter: wgpu::FilterMode::Linear,  // 축소 필터
    mipmap_filter: wgpu::FilterMode::Nearest,
    ..Default::default()
});
```

---

## SurfaceError 처리

`surface.get_current_texture()`는 여러 오류를 반환할 수 있습니다.

```rust
fn render(&mut self) -> Result<(), ()> {
    let output = match self.surface.get_current_texture() {
        Ok(texture) => texture,
        Err(wgpu::SurfaceError::Lost) => {
            // Surface가 손실됨 (창 최소화 해제, 화면 잠금 해제 등)
            // Surface 재구성으로 복구 가능
            self.surface.configure(&self.device, &self.config);
            return Ok(()); // 이번 프레임 건너뜀
        }
        Err(wgpu::SurfaceError::OutOfMemory) => {
            // GPU 메모리 부족 — 복구 불가, 종료 필요
            eprintln!("GPU 메모리 부족, 종료합니다");
            return Err(());
        }
        Err(wgpu::SurfaceError::Outdated) => {
            // Surface가 구식 (창 크기 변경 직후 발생 가능)
            // 보통 다음 프레임에서 자동 해결
            return Ok(()); // 이번 프레임 건너뜀
        }
        Err(wgpu::SurfaceError::Timeout) => {
            // 프레임 획득 타임아웃 — 일시적 현상
            // 경고 로그 후 건너뜀
            eprintln!("Surface 타임아웃, 프레임 건너뜀");
            return Ok(());
        }
        Err(wgpu::SurfaceError::Other) => {
            // 기타 오류
            eprintln!("Surface 오류");
            return Ok(());
        }
    };

    // 정상 렌더링...
    let view = output.texture.create_view(&Default::default());
    // ...
    output.present();
    Ok(())
}
```

### SurfaceError 요약표

| 오류 | 원인 | 처리 방법 |
|------|------|-----------|
| `Lost` | Surface 손실 (OS 이벤트) | `surface.configure()` 재호출 |
| `Outdated` | Surface 구식 (리사이즈 직후) | 프레임 건너뜀, 자동 회복 |
| `OutOfMemory` | GPU 메모리 부족 | 앱 종료 |
| `Timeout` | 타임아웃 | 경고 후 건너뜀 |
| `Other` | 기타 | 경고 후 건너뜀 |

---

## winit 통합 패턴

winit의 `ApplicationHandler`와 wgpu를 통합하는 표준 패턴입니다.

```rust
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

struct WgpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
}

impl WgpuState {
    async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: window Arc는 surface보다 오래 삼
        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await.unwrap();

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            },
            None,
        ).await.unwrap();

        let surface_caps = surface.get_capabilities(&adapter);

        // non-sRGB 포맷 선택 (이중 감마 보정 버그 방지)
        let format = surface_caps.formats.iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self { surface, device, queue, config, render_pipeline }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 { return; }
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("Encoder") }
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.render_pipeline);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}

// winit ApplicationHandler 통합
struct App {
    window: Option<Arc<Window>>,
    gpu: Option<WgpuState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop.create_window(Window::default_attributes()).unwrap()
        );
        // 비동기 초기화 (pollster 또는 tokio 사용)
        let gpu = pollster::block_on(WgpuState::new(window.clone()));
        self.gpu = Some(gpu);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(new_size);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &mut self.gpu {
                    match gpu.render() {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            if let Some(window) = &self.window {
                                let size = window.inner_size();
                                gpu.resize(size);
                            }
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            event_loop.exit();
                        }
                        Err(e) => eprintln!("렌더 오류: {:?}", e),
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { window: None, gpu: None };
    event_loop.run_app(&mut app).unwrap();
}
```

### Arc<Window> 사용 이유

`wgpu::Surface<'static>`은 `'static` 라이프타임을 요구합니다. `Window`에 대한 참조를 `Surface`에 전달할 때, `Window`가 `Surface`보다 오래 살아있음을 보장하기 위해 `Arc<Window>`를 사용합니다.

```rust
// 'static surface 생성
let surface: wgpu::Surface<'static> = instance.create_surface(arc_window)?;
// arc_window는 App 구조체에 Arc로 보관
```

### Cargo.toml 의존성

```toml
[dependencies]
winit = "0.30"
wgpu = "24"
bytemuck = { version = "1", features = ["derive"] }
pollster = "0.3"  # 비동기 초기화 블로킹용
log = "0.4"
env_logger = "0.11"
```
