//! Plugin Canvas SharedBuffer → wgpu texture cache.
//!
//! 한 plugin 인스턴스가 [`tasty_plugin_protocol::UiNode::Canvas`]를 표시하면, 호스트는
//! 해당 SharedBuffer 영역을 GPU 텍스처에 부분 업로드해 egui 합성기로 그린다. 이 모듈은
//! (plugin_id, SharedBufferId) → wgpu::Texture / egui::TextureId 매핑과 staging upload를
//! 책임진다.
//!
//! # 라이프사이클
//!
//! - [`CanvasTextureCache::ensure`]: 캔버스 폭/높이/포맷/필터 중 하나라도 변하면 기존 텍스처를
//!   삭제하고 새로 생성하며 [`egui_wgpu::Renderer::register_native_texture`]도 재호출한다.
//! - [`CanvasTextureCache::upload_if_dirty`]: plugin이 보고한 commit generation이 이전과
//!   같으면 noop. 다르면 dirty rect만 staging 벡터로 잘라 `queue.write_texture`로 업로드.
//! - [`CanvasTextureCache::release_plugin`]: plugin 종료 시 호출.
//!
//! # bytes_per_row
//!
//! `queue.write_texture`는 내부 staging buffer를 통해 256바이트 row alignment를 자동
//! 처리하므로, 본 모듈은 *src* row stride를 `dirty_w × bpp`로 그대로 보낸다. dirty rect
//! 전체 row를 사용자 슬라이스에서 한 줄씩 떼어내 staging vec에 쌓는다.
//!
//! # 검증
//!
//! 픽셀 영역이 사용자 슬라이스 범위를 넘어서면 업로드를 거부하고 warn 로그를 남긴다
//! (data UB 방지). 호스트 manager 측에서 `width × height × bpp + footer ≤ buffer.len()`을
//! 추가로 검증하므로 본 모듈의 검사는 second line of defence.

use std::collections::HashMap;

use egui::TextureId;
use tasty_plugin_protocol::{PixelFilter, PixelFormat, Rect, SharedBufferId};

/// Plugin id + SharedBufferId의 페어. 호스트 내부에서 한 cache entry를 식별.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanvasKey {
    pub plugin_id: String,
    pub buffer_id: SharedBufferId,
}

struct Entry {
    /// 텍스처 자체. drop 시 wgpu가 해제.
    _texture: wgpu::Texture,
    /// 텍스처 view. egui_renderer에 등록할 때 참조.
    view: wgpu::TextureView,
    /// egui가 사용하는 핸들. dimensions/format/filter가 바뀌면 재등록 필요.
    egui_id: TextureId,
    width: u32,
    height: u32,
    format: PixelFormat,
    filter: PixelFilter,
    /// 마지막 GPU upload에 사용한 plugin commit generation 값. 같은 값이면 upload 생략.
    last_uploaded_gen: u64,
}

/// Plugin Canvas 텍스처 전역 캐시. 한 [`crate::gpu::GpuState`]에 하나.
pub struct CanvasTextureCache {
    entries: HashMap<CanvasKey, Entry>,
}

impl CanvasTextureCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// (plugin, buffer)에 대해 텍스처를 보장하고 egui TextureId를 돌려준다.
    ///
    /// dimensions/format/filter가 바뀌었으면 기존 entry를 정리하고 새로 만들며,
    /// [`egui_wgpu::Renderer::register_native_texture`]도 재호출한다.
    pub fn ensure(
        &mut self,
        key: &CanvasKey,
        device: &wgpu::Device,
        egui_renderer: &mut egui_wgpu::Renderer,
        width: u32,
        height: u32,
        format: PixelFormat,
        filter: PixelFilter,
    ) -> TextureId {
        if let Some(e) = self.entries.get(key) {
            if e.width == width && e.height == height && e.format == format && e.filter == filter {
                return e.egui_id;
            }
        }
        if let Some(old) = self.entries.remove(key) {
            egui_renderer.free_texture(&old.egui_id);
        }
        let entry = create_entry(device, egui_renderer, width, height, format, filter);
        let id = entry.egui_id;
        self.entries.insert(key.clone(), entry);
        id
    }

    /// SharedBuffer user 영역에서 dirty rect 부분만 GPU에 업로드.
    ///
    /// `atomic_gen`이 직전 업로드 generation과 같으면 noop. dirty rect이 텍스처 또는
    /// user_data 범위를 벗어나면 클램프 후 가능한 만큼만 업로드하며 (out-of-range는 skip),
    /// generation은 갱신해 같은 frame이 재시도되지 않게 한다.
    pub fn upload_if_dirty(
        &mut self,
        key: &CanvasKey,
        queue: &wgpu::Queue,
        user_data: &[u8],
        atomic_gen: u64,
        dirty: Option<Rect>,
    ) {
        let Some(e) = self.entries.get_mut(key) else {
            tracing::warn!(?key, "canvas upload: entry missing");
            return;
        };
        if atomic_gen == e.last_uploaded_gen {
            return;
        }
        let bpp = e.format.bytes_per_pixel();
        let rect = dirty.unwrap_or(Rect {
            x: 0,
            y: 0,
            w: e.width,
            h: e.height,
        });
        let Some(clipped) = clip_rect(rect, e.width, e.height) else {
            // rect이 텍스처 밖이면 generation만 갱신하고 skip.
            e.last_uploaded_gen = atomic_gen;
            return;
        };

        // staging vec: dirty rect의 row들을 user_data에서 떼어내 차곡차곡 쌓는다.
        let src_stride = (e.width * bpp) as usize;
        let row_bytes = (clipped.w * bpp) as usize;
        let mut staging = Vec::with_capacity(row_bytes * clipped.h as usize);
        for row in 0..clipped.h {
            let src_y = (clipped.y + row) as usize;
            let src_off = src_y * src_stride + (clipped.x * bpp) as usize;
            if src_off + row_bytes > user_data.len() {
                tracing::warn!(
                    ?key,
                    gen = atomic_gen,
                    "canvas upload: dirty rect overflows user_data, aborting"
                );
                return;
            }
            staging.extend_from_slice(&user_data[src_off..src_off + row_bytes]);
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &e._texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: clipped.x,
                    y: clipped.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &staging,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(clipped.w * bpp),
                rows_per_image: Some(clipped.h),
            },
            wgpu::Extent3d {
                width: clipped.w,
                height: clipped.h,
                depth_or_array_layers: 1,
            },
        );
        e.last_uploaded_gen = atomic_gen;
        // view는 별도 사용 안 함 — egui_renderer가 등록 시점에 참조를 가져갔다.
        let _ = &e.view;
    }

    /// 한 plugin이 종료되면 그 plugin이 보유한 모든 캔버스 텍스처를 풀어준다.
    pub fn release_plugin(&mut self, plugin_id: &str, egui_renderer: &mut egui_wgpu::Renderer) {
        self.entries.retain(|k, e| {
            if k.plugin_id == plugin_id {
                egui_renderer.free_texture(&e.egui_id);
                false
            } else {
                true
            }
        });
    }

    /// 디버그용: 현재 cache 크기.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// (plugin_id, buffer_id)에 대해 등록된 egui TextureId를 반환. 미등록이면 None.
    ///
    /// linear scan: 한 윈도우 내 활성 plugin canvas 개수는 보통 < 10이라 충분히 빠르다.
    /// hash 기반 키로 바꾸려면 `Cow<str>` 또는 `Borrow` 트릭이 필요해서 의도적으로 단순 구현.
    pub fn get(&self, plugin_id: &str, buffer_id: SharedBufferId) -> Option<TextureId> {
        self.entries
            .iter()
            .find(|(k, _)| k.plugin_id == plugin_id && k.buffer_id == buffer_id)
            .map(|(_, e)| e.egui_id)
    }
}

impl Default for CanvasTextureCache {
    fn default() -> Self {
        Self::new()
    }
}

fn create_entry(
    device: &wgpu::Device,
    egui_renderer: &mut egui_wgpu::Renderer,
    width: u32,
    height: u32,
    format: PixelFormat,
    filter: PixelFilter,
) -> Entry {
    let wgpu_format = match format {
        PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8UnormSrgb,
        PixelFormat::Bgra8 => wgpu::TextureFormat::Bgra8UnormSrgb,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("plugin_canvas"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu_format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let wgpu_filter = match filter {
        PixelFilter::Nearest => wgpu::FilterMode::Nearest,
        PixelFilter::Linear => wgpu::FilterMode::Linear,
    };
    let egui_id = egui_renderer.register_native_texture(device, &view, wgpu_filter);
    Entry {
        _texture: texture,
        view,
        egui_id,
        width,
        height,
        format,
        filter,
        last_uploaded_gen: 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClippedRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// rect을 (0,0)-(tex_w,tex_h) 사각형에 클램프. 결과 너비/높이가 0이면 None.
fn clip_rect(r: Rect, tex_w: u32, tex_h: u32) -> Option<ClippedRect> {
    if r.x >= tex_w || r.y >= tex_h {
        return None;
    }
    let x = r.x;
    let y = r.y;
    let w = r.w.min(tex_w - x);
    let h = r.h.min(tex_h - y);
    if w == 0 || h == 0 {
        return None;
    }
    Some(ClippedRect { x, y, w, h })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// 32×32 RGBA를 plugin SharedMemory에 채우고 footer.fetch_add → host가 wgpu
    /// 텍스처에 업로드 → 다시 staging buffer로 readback → 픽셀이 보존되는지 확인.
    ///
    /// `#[ignore]`로 표시: 일부 CI 환경에 GPU adapter가 없거나 unsafe driver라 hang/abort.
    /// 로컬에서 `cargo test --bin tasty -- --ignored canvas_e2e` 로 검증.
    #[test]
    #[ignore = "GPU adapter 필요 — 로컬에서만 실행"]
    fn canvas_e2e_upload_and_readback() {
        const W: u32 = 32;
        const H: u32 = 32;
        const BPP: usize = 4;
        const ROW_BYTES: u32 = W * (BPP as u32);
        // wgpu copy_texture_to_buffer는 bytes_per_row를 256 align 요구.
        const PADDED_ROW_BYTES: u32 = 256;

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("no GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("canvas_e2e_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            },
            None,
        ))
        .expect("device");

        // egui renderer는 ensure에 필요하지만 본 테스트는 readback만 본다.
        let mut egui_renderer =
            egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, None, 1, false);

        // SharedMemory 생성: user data 영역 = W*H*BPP, footer 별도.
        let total = (W as usize) * (H as usize) * BPP + tasty_shm::footer::SIZE;
        let (mem, _send) = tasty_shm::create(total).expect("shm create");

        // Plugin 측: 픽셀 채우고 footer fetch_add(Release).
        // SAFETY: 본 테스트가 SharedMemory의 유일한 사용자. 단일 스레드에서 write 후 read만 한다.
        unsafe {
            let raw = mem.as_mut_slice();
            let user = tasty_shm::footer::user_slice_mut(raw);
            // 픽셀당 (R=0xAB, G=0xCD, B=0xEF, A=0xFF).
            for px in user.chunks_exact_mut(BPP) {
                px[0] = 0xAB;
                px[1] = 0xCD;
                px[2] = 0xEF;
                px[3] = 0xFF;
            }
            let prev = tasty_shm::footer::fetch_add(raw, 1, Ordering::Release);
            assert_eq!(prev, 0);
        }

        // Host 측: ensure + upload.
        let key = CanvasKey {
            plugin_id: "test".to_string(),
            buffer_id: SharedBufferId(1),
        };
        let mut cache = CanvasTextureCache::new();
        cache.ensure(
            &key,
            &device,
            &mut egui_renderer,
            W,
            H,
            PixelFormat::Rgba8,
            PixelFilter::Linear,
        );
        // SAFETY: 동일 SharedMemory에 다른 writer가 없다.
        unsafe {
            let raw = mem.as_slice();
            let generation = tasty_shm::footer::load(raw, Ordering::Acquire);
            assert_eq!(generation, 1);
            let user = tasty_shm::footer::user_slice(raw);
            cache.upload_if_dirty(&key, &queue, user, generation, None);
        }

        // Readback: texture → padded staging buffer → map.
        let entry = cache.entries.get(&key).expect("cache entry");
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canvas_e2e_readback"),
            size: (PADDED_ROW_BYTES * H) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("canvas_e2e_encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &entry._texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(PADDED_ROW_BYTES),
                    rows_per_image: Some(H),
                },
            },
            wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let buffer_slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().expect("map_async result").expect("map ok");
        let data = buffer_slice.get_mapped_range();

        // 각 row의 첫 ROW_BYTES만 유효, 나머지는 padding.
        for row in 0..H as usize {
            let off = row * (PADDED_ROW_BYTES as usize);
            assert_eq!(data[off], 0xAB, "row {row} pixel 0 R");
            assert_eq!(data[off + 1], 0xCD, "row {row} pixel 0 G");
            assert_eq!(data[off + 2], 0xEF, "row {row} pixel 0 B");
            assert_eq!(data[off + 3], 0xFF, "row {row} pixel 0 A");
            // row의 마지막 픽셀도 확인.
            let last_px_off = off + (ROW_BYTES as usize) - BPP;
            assert_eq!(data[last_px_off], 0xAB, "row {row} last R");
        }
        drop(data);
        staging.unmap();
    }

    #[test]
    fn clip_rect_within_bounds() {
        let r = Rect {
            x: 5,
            y: 10,
            w: 20,
            h: 30,
        };
        let c = clip_rect(r, 100, 100).unwrap();
        assert_eq!(c, ClippedRect { x: 5, y: 10, w: 20, h: 30 });
    }

    #[test]
    fn clip_rect_clamps_overflow() {
        let r = Rect {
            x: 90,
            y: 90,
            w: 50,
            h: 50,
        };
        let c = clip_rect(r, 100, 100).unwrap();
        assert_eq!(c, ClippedRect { x: 90, y: 90, w: 10, h: 10 });
    }

    #[test]
    fn clip_rect_origin_out_returns_none() {
        let r = Rect {
            x: 100,
            y: 50,
            w: 10,
            h: 10,
        };
        assert_eq!(clip_rect(r, 100, 100), None);

        let r = Rect {
            x: 50,
            y: 100,
            w: 10,
            h: 10,
        };
        assert_eq!(clip_rect(r, 100, 100), None);
    }

    #[test]
    fn clip_rect_zero_size_returns_none() {
        let r = Rect {
            x: 10,
            y: 10,
            w: 0,
            h: 5,
        };
        assert_eq!(clip_rect(r, 100, 100), None);
    }
}
