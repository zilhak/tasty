# cosmic-text 0.18

GPU 가속 터미널에서 텍스트 레이아웃과 글리프 렌더링을 담당하는 라이브러리.
Unicode 지원, 폰트 셰이핑(shaping), 라인 래핑을 처리한다.

## 핵심 타입 개요

| 타입 | 역할 |
|------|------|
| `FontSystem` | 폰트 로딩 및 글리프 셰이핑 엔진 |
| `SwashCache` | 래스터화된 글리프 캐시 |
| `Buffer` | 텍스트 레이아웃 버퍼 (줄 목록 + 메트릭) |
| `Metrics` | 폰트 크기 및 줄 높이 |
| `Attrs` | 텍스트 속성 (폰트 패밀리, 굵기, 색상 등) |
| `Family` | 폰트 패밀리 지정자 |
| `Weight` | 폰트 굵기 |
| `Color` | RGBA 색상 |

## Cargo.toml

```toml
[dependencies]
cosmic-text = "0.18"
```

## FontSystem

모든 폰트 연산의 진입점. 애플리케이션 전체에서 하나만 생성한다.

```rust
use cosmic_text::FontSystem;

// 시스템 폰트를 자동으로 탐색하여 초기화
let mut font_system = FontSystem::new();

// 특정 로케일과 폰트 DB를 지정하여 초기화
use cosmic_text::fontdb;
let mut db = fontdb::Database::new();
db.load_system_fonts();
// 커스텀 폰트 추가
db.load_font_file("/path/to/font.ttf").ok();

let mut font_system = FontSystem::new_with_locale_and_db("ko-KR".into(), db);
```

메모리에서 폰트 로드:

```rust
let font_data: Vec<u8> = std::fs::read("NotoSansMono.ttf").unwrap();
font_system.db_mut().load_font_data(font_data);
```

## SwashCache

글리프 래스터라이제이션 결과를 캐싱한다. `FontSystem`과 함께 사용한다.

```rust
use cosmic_text::SwashCache;

let mut swash_cache = SwashCache::new();
```

## Metrics (폰트 크기 설정)

```rust
use cosmic_text::Metrics;

// Metrics::new(font_size, line_height)
let metrics = Metrics::new(14.0, 20.0);  // 14px 폰트, 20px 줄 높이

// 터미널 권장값: 모노스페이스 폰트는 line_height = font_size * 1.2~1.4
let terminal_metrics = Metrics::new(16.0, 24.0);
```

## Attrs (텍스트 속성)

```rust
use cosmic_text::{Attrs, Family, Weight, Style, Stretch};

// 기본 속성
let attrs = Attrs::new();

// 패밀리 지정
let attrs = Attrs::new().family(Family::Monospace);
let attrs = Attrs::new().family(Family::Name("JetBrains Mono"));

// 굵기와 스타일
let bold_attrs = Attrs::new()
    .family(Family::Monospace)
    .weight(Weight::BOLD);

let italic_attrs = Attrs::new()
    .family(Family::Monospace)
    .style(Style::Italic);

// 색상 (RGBA)
use cosmic_text::Color;
let colored_attrs = Attrs::new()
    .color(Color::rgb(0xFF, 0xA0, 0x00));  // 주황색
```

## Family 열거형

```rust
use cosmic_text::Family;

Family::Serif         // 세리프 폰트
Family::SansSerif     // 산세리프 폰트
Family::Monospace     // 모노스페이스 (터미널 기본값)
Family::Cursive       // 필기체
Family::Fantasy       // 판타지
Family::Name("JetBrains Mono")  // 특정 폰트 이름
```

## Weight 상수

```rust
use cosmic_text::Weight;

Weight::THIN          // 100
Weight::EXTRA_LIGHT   // 200
Weight::LIGHT         // 300
Weight::NORMAL        // 400 (기본값)
Weight::MEDIUM        // 500
Weight::SEMI_BOLD     // 600
Weight::BOLD          // 700
Weight::EXTRA_BOLD    // 800
Weight::BLACK         // 900
```

## Buffer (텍스트 레이아웃)

텍스트를 저장하고 레이아웃 계산을 수행한다.

```rust
use cosmic_text::{Buffer, Metrics, Attrs, Family};

let metrics = Metrics::new(14.0, 20.0);
let mut buffer = Buffer::new(&mut font_system, metrics);

// 렌더링 영역 크기 설정 (픽셀 단위)
buffer.set_size(&mut font_system, Some(800.0), Some(600.0));

// 텍스트 설정 — 단일 속성
let attrs = Attrs::new().family(Family::Monospace);
buffer.set_text(&mut font_system, "Hello, 터미널!", attrs, Shaping::Advanced);

// 텍스트 설정 — 범위별 다른 속성 (터미널 색상 하이라이팅)
use cosmic_text::AttrsList;
let mut attrs_list = AttrsList::new(attrs);
// 0..5 범위에 빨간색 적용
attrs_list.add_span(0..5, Attrs::new().color(Color::rgb(0xFF, 0x00, 0x00)));
buffer.set_rich_text(
    &mut font_system,
    [("Hello world", attrs_list.defaults())].iter().cloned(),
    attrs,
    Shaping::Advanced,
);
```

## layout_runs() — 레이아웃 순회

```rust
use cosmic_text::{Buffer, LayoutRun};

// 레이아웃 계산 후 순회
buffer.shape_until_scroll(&mut font_system, false);

for run in buffer.layout_runs() {
    // run: &LayoutRun
    println!("줄 y 오프셋: {}", run.line_y);
    println!("줄 높이: {}", run.line_height);

    for glyph in run.glyphs.iter() {
        println!(
            "글리프 id={}, x={}, y={}, w={}",
            glyph.glyph_id,
            glyph.x,
            glyph.y,
            glyph.w
        );
    }
}
```

## draw() — 픽셀 버퍼에 렌더링

CPU 소프트웨어 렌더링 시 사용. GPU 렌더링에서는 글리프 텍스처 업로드로 대체한다.

```rust
use cosmic_text::{Buffer, Color, SwashCache};

// 픽셀 버퍼 (RGBA, width * height * 4 바이트)
let width = 800usize;
let height = 600usize;
let mut pixels = vec![0u8; width * height * 4];

let text_color = Color::rgb(0xFF, 0xFF, 0xFF);  // 흰색

buffer.draw(
    &mut font_system,
    &mut swash_cache,
    text_color,
    |x, y, w, h, color| {
        // 각 글리프 픽셀을 버퍼에 직접 기록
        for row in 0..h as usize {
            for col in 0..w as usize {
                let px = (y as usize + row) * width + (x as usize + col);
                if px < pixels.len() / 4 {
                    let base = px * 4;
                    let alpha = color.a() as u32;
                    // 알파 블렌딩
                    pixels[base]     = ((color.r() as u32 * alpha) / 255) as u8;
                    pixels[base + 1] = ((color.g() as u32 * alpha) / 255) as u8;
                    pixels[base + 2] = ((color.b() as u32 * alpha) / 255) as u8;
                    pixels[base + 3] = color.a();
                }
            }
        }
    },
);
```

## 터미널 글리프 렌더링 패턴

터미널 에뮬레이터에서의 전형적인 사용 패턴.

### 셀 기반 렌더링

```rust
use cosmic_text::{Attrs, AttrsList, Buffer, Color, Family, FontSystem,
                  Metrics, Shaping, SwashCache, Weight};

pub struct TerminalRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    cell_width: f32,
    cell_height: f32,
    metrics: Metrics,
}

impl TerminalRenderer {
    pub fn new(font_size: f32) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        // 셀 크기 측정: 'M' 문자 기준 (모노스페이스)
        let line_height = font_size * 1.4;
        let metrics = Metrics::new(font_size, line_height);

        // 단일 문자로 셀 너비 측정
        let mut probe = Buffer::new(&mut font_system, metrics);
        probe.set_size(&mut font_system, Some(1000.0), None);
        let attrs = Attrs::new().family(Family::Monospace);
        probe.set_text(&mut font_system, "M", attrs, Shaping::Advanced);
        probe.shape_until_scroll(&mut font_system, false);

        let cell_width = probe
            .layout_runs()
            .next()
            .and_then(|r| r.glyphs.first())
            .map(|g| g.w)
            .unwrap_or(font_size * 0.6);

        Self {
            font_system,
            swash_cache,
            cell_width,
            cell_height: line_height,
            metrics,
        }
    }

    pub fn render_cell(
        &mut self,
        text: &str,
        fg: (u8, u8, u8),
        bold: bool,
        italic: bool,
    ) -> Buffer {
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
        buffer.set_size(
            &mut self.font_system,
            Some(self.cell_width * text.chars().count() as f32 + 4.0),
            Some(self.cell_height),
        );

        let weight = if bold { Weight::BOLD } else { Weight::NORMAL };
        let style = if italic {
            cosmic_text::Style::Italic
        } else {
            cosmic_text::Style::Normal
        };

        let attrs = Attrs::new()
            .family(Family::Monospace)
            .weight(weight)
            .style(style)
            .color(Color::rgb(fg.0, fg.1, fg.2));

        buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }
}
```

### GPU 텍스처 업로드 패턴 (wgpu 연동)

```rust
use cosmic_text::{SwashContent, SwashImage};

// SwashCache에서 글리프 이미지 가져오기
fn upload_glyph_to_texture(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cache_key: cosmic_text::CacheKey,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) {
    if let Some(image) = swash_cache.get_image(font_system, cache_key) {
        match image.content {
            SwashContent::Mask => {
                // 단색 글리프 (일반 텍스트)
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: image.placement.left as u32,
                            y: image.placement.top as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &image.data,
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(image.placement.width),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: image.placement.width,
                        height: image.placement.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            SwashContent::Color => {
                // 컬러 이모지
                // RGBA 데이터로 처리
            }
            SwashContent::SubpixelMask => {
                // LCD 서브픽셀 렌더링
            }
        }
    }
}
```

## 메트릭 계산 유틸리티

```rust
/// 터미널 열/행 수에서 픽셀 크기 계산
pub fn terminal_size_px(cols: u32, rows: u32, cell_w: f32, cell_h: f32) -> (f32, f32) {
    (cols as f32 * cell_w, rows as f32 * cell_h)
}

/// 픽셀 좌표에서 터미널 셀 위치 계산
pub fn px_to_cell(x: f32, y: f32, cell_w: f32, cell_h: f32) -> (u32, u32) {
    let col = (x / cell_w).floor() as u32;
    let row = (y / cell_h).floor() as u32;
    (col, row)
}
```

## 주의사항

- `FontSystem`은 스레드 안전하지 않으므로 `Arc<Mutex<FontSystem>>`으로 감싸야 멀티스레드 사용 가능.
- `Buffer::shape_until_scroll()`은 보이는 영역만 셰이핑하므로 전체 텍스트가 필요하면 `Buffer::shape_until_cursor()`를 사용.
- 대용량 터미널 화면(예: 220열 × 50행)은 셀 단위가 아닌 줄 단위 `Buffer`로 관리하면 성능이 좋다.
- `SwashCache`는 LRU 캐시이므로 자주 쓰이는 글리프는 자동으로 유지된다.
