//! egui-mesh POD 와이어 코덱 (A1-S2).
//!
//! epaint 0.31 은 `serde` feature 가 꺼져 있어 (`Cargo.lock` deps 에 serde 없음,
//! bytemuck 만) `ClippedPrimitive` / `Mesh` / `TexturesDelta` / `ImageData` 를
//! `serde_json` 으로 직렬화할 수 없다. 따라서 paint 출력
//! `(Vec<ClippedPrimitive>, TexturesDelta, pixels_per_point)` 을 손으로 미러한
//! POD 바이트 레이아웃으로 인코드/디코드한다.
//!
//! 핵심 사실 — `Vertex` / `Color32` / `Pos2` / `Rect` 는 `#[repr(C)]` + `bytemuck::Pod`
//! (egui `bytemuck` feature on) 라 정점/인덱스/픽셀은 `bytemuck::cast_slice` 로 직카피된다
//! (직렬화 비용 0). 나머지(`TexturesDelta`/`ImageDelta`/`TextureOptions`/enum 태그)만 수작업.
//!
//! ## 미지원
//! - `Primitive::Callback` — plugin 프로세스엔 wgpu paint callback 이 없다. 인코드 시
//!   skip + `tracing::warn!`. 디코드 결과는 항상 `Primitive::Mesh` 만 담긴다.
//!
//! ## 버전 방어
//! 헤더에 magic / wire_version / `size_of::<Vertex>()` 를 박는다. epaint major 업글로
//! `Vertex` 레이아웃이 바뀌면 디코드가 [`MeshWireError::VertexStrideMismatch`] 로 실패하며,
//! round-trip 테스트가 와이어 파손을 컴파일/테스트 단계에서 잡는다.

use egui::Color32;
use egui::emath::{Pos2, Rect};
use egui::epaint::textures::{TextureFilter, TextureOptions, TextureWrapMode, TexturesDelta};
use egui::epaint::{
    ClippedPrimitive, ColorImage, FontImage, ImageData, ImageDelta, Mesh, Primitive, TextureId,
    Vertex,
};

/// 와이어 매직 — `b"TMSH"` (Tasty MeSH).
const MAGIC: [u8; 4] = *b"TMSH";
/// 와이어 포맷 버전. 레이아웃을 바꾸면 +1.
const WIRE_VERSION: u16 = 1;

/// 디코드한 paint 출력. 인코드 입력과 동형이되 Callback primitive 는 제거돼 있다.
///
/// `ClippedPrimitive` 는 `PartialEq` 를 구현하지 않으므로(`Primitive::Callback` 의
/// 클로저 때문) 이 타입도 `PartialEq` 를 두지 않는다 — 비교는 mesh 필드 단위로 한다.
#[derive(Clone, Debug)]
pub struct DecodedPaint {
    pub primitives: Vec<ClippedPrimitive>,
    pub textures_delta: TexturesDelta,
    pub pixels_per_point: f32,
}

/// 디코드 실패 사유.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshWireError {
    /// 입력이 헤더보다 짧거나 길이 프리픽스가 가리키는 만큼의 바이트가 없다.
    UnexpectedEof {
        /// 읽으려던 바이트 수.
        needed: usize,
        /// 실제 남은 바이트 수.
        remaining: usize,
    },
    /// 매직 4바이트 불일치 — egui-mesh 프레임이 아니다.
    BadMagic([u8; 4]),
    /// 와이어 버전 불일치.
    VersionMismatch { expected: u16, found: u16 },
    /// `size_of::<Vertex>()` 불일치 — epaint 레이아웃이 바뀌었다.
    VertexStrideMismatch { expected: u16, found: u16 },
    /// 알 수 없는 enum 태그 (TextureId / ImageData / TextureFilter / WrapMode 등).
    BadTag { what: &'static str, value: u8 },
}

impl core::fmt::Display for MeshWireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof { needed, remaining } => {
                write!(
                    f,
                    "unexpected eof: needed {needed} bytes, {remaining} remaining"
                )
            }
            Self::BadMagic(m) => write!(f, "bad magic: {m:?}"),
            Self::VersionMismatch { expected, found } => {
                write!(
                    f,
                    "wire version mismatch: expected {expected}, found {found}"
                )
            }
            Self::VertexStrideMismatch { expected, found } => write!(
                f,
                "vertex stride mismatch: expected {expected}, found {found} (epaint layout changed?)"
            ),
            Self::BadTag { what, value } => write!(f, "bad {what} tag: {value}"),
        }
    }
}

impl std::error::Error for MeshWireError {}

// ============================================================================
// 인코드
// ============================================================================

/// paint 출력을 POD 바이트로 인코드한다. `Primitive::Callback` 은 skip + warn.
pub fn encode_paint(
    primitives: &[ClippedPrimitive],
    textures_delta: &TexturesDelta,
    pixels_per_point: f32,
) -> Vec<u8> {
    let mut w = Vec::new();

    // 헤더
    w.extend_from_slice(&MAGIC);
    w.extend_from_slice(&WIRE_VERSION.to_le_bytes());
    w.extend_from_slice(&(core::mem::size_of::<Vertex>() as u16).to_le_bytes());
    w.extend_from_slice(&pixels_per_point.to_le_bytes());

    // mesh primitive 만 (Callback 은 건너뜀)
    let meshes: Vec<&ClippedPrimitive> = primitives
        .iter()
        .filter(|p| match &p.primitive {
            Primitive::Mesh(_) => true,
            Primitive::Callback(_) => {
                tracing::warn!(
                    "egui-mesh: Primitive::Callback is unsupported in plugin process; skipping"
                );
                false
            }
        })
        .collect();

    w.extend_from_slice(&(meshes.len() as u32).to_le_bytes());
    for prim in meshes {
        let Primitive::Mesh(mesh) = &prim.primitive else {
            unreachable!("filtered to meshes above");
        };
        write_rect(&mut w, &prim.clip_rect);
        write_texture_id(&mut w, mesh.texture_id);
        write_mesh_geometry(&mut w, mesh);
    }

    // textures delta
    w.extend_from_slice(&(textures_delta.set.len() as u32).to_le_bytes());
    for (id, delta) in &textures_delta.set {
        write_texture_id(&mut w, *id);
        write_image_delta(&mut w, delta);
    }
    w.extend_from_slice(&(textures_delta.free.len() as u32).to_le_bytes());
    for id in &textures_delta.free {
        write_texture_id(&mut w, *id);
    }

    w
}

fn write_rect(w: &mut Vec<u8>, r: &Rect) {
    w.extend_from_slice(&r.min.x.to_le_bytes());
    w.extend_from_slice(&r.min.y.to_le_bytes());
    w.extend_from_slice(&r.max.x.to_le_bytes());
    w.extend_from_slice(&r.max.y.to_le_bytes());
}

fn write_texture_id(w: &mut Vec<u8>, id: TextureId) {
    match id {
        TextureId::Managed(v) => {
            w.push(0);
            w.extend_from_slice(&v.to_le_bytes());
        }
        TextureId::User(v) => {
            w.push(1);
            w.extend_from_slice(&v.to_le_bytes());
        }
    }
}

fn write_mesh_geometry(w: &mut Vec<u8>, mesh: &Mesh) {
    w.extend_from_slice(&(mesh.vertices.len() as u32).to_le_bytes());
    w.extend_from_slice(&(mesh.indices.len() as u32).to_le_bytes());
    // Vertex 는 #[repr(C)] + Pod → 바이트 직카피.
    w.extend_from_slice(bytemuck::cast_slice::<Vertex, u8>(&mesh.vertices));
    w.extend_from_slice(bytemuck::cast_slice::<u32, u8>(&mesh.indices));
}

fn write_image_delta(w: &mut Vec<u8>, delta: &ImageDelta) {
    // pos: Option<[usize; 2]>
    match delta.pos {
        Some([x, y]) => {
            w.push(1);
            w.extend_from_slice(&(x as u64).to_le_bytes());
            w.extend_from_slice(&(y as u64).to_le_bytes());
        }
        None => w.push(0),
    }
    write_texture_options(w, &delta.options);
    write_image_data(w, &delta.image);
}

fn write_texture_options(w: &mut Vec<u8>, o: &TextureOptions) {
    w.push(filter_tag(o.magnification));
    w.push(filter_tag(o.minification));
    w.push(wrap_tag(o.wrap_mode));
    match o.mipmap_mode {
        Some(f) => {
            w.push(1);
            w.push(filter_tag(f));
        }
        None => {
            w.push(0);
            w.push(0);
        }
    }
}

fn filter_tag(f: TextureFilter) -> u8 {
    match f {
        TextureFilter::Nearest => 0,
        TextureFilter::Linear => 1,
    }
}

fn wrap_tag(m: TextureWrapMode) -> u8 {
    match m {
        TextureWrapMode::ClampToEdge => 0,
        TextureWrapMode::Repeat => 1,
        TextureWrapMode::MirroredRepeat => 2,
    }
}

fn write_image_data(w: &mut Vec<u8>, image: &ImageData) {
    match image {
        ImageData::Color(img) => {
            w.push(0);
            w.extend_from_slice(&(img.size[0] as u64).to_le_bytes());
            w.extend_from_slice(&(img.size[1] as u64).to_le_bytes());
            // Color32 = [u8; 4], Pod.
            w.extend_from_slice(bytemuck::cast_slice::<Color32, u8>(&img.pixels));
        }
        ImageData::Font(img) => {
            w.push(1);
            w.extend_from_slice(&(img.size[0] as u64).to_le_bytes());
            w.extend_from_slice(&(img.size[1] as u64).to_le_bytes());
            // f32 coverage, Pod.
            w.extend_from_slice(bytemuck::cast_slice::<f32, u8>(&img.pixels));
        }
    }
}

// ============================================================================
// 디코드
// ============================================================================

/// POD 바이트를 paint 출력으로 디코드한다.
pub fn decode_paint(bytes: &[u8]) -> Result<DecodedPaint, MeshWireError> {
    let mut r = Reader::new(bytes);

    let magic = r.read_array::<4>()?;
    if magic != MAGIC {
        return Err(MeshWireError::BadMagic(magic));
    }
    let version = r.read_u16()?;
    if version != WIRE_VERSION {
        return Err(MeshWireError::VersionMismatch {
            expected: WIRE_VERSION,
            found: version,
        });
    }
    let vertex_stride = r.read_u16()?;
    let expected_stride = core::mem::size_of::<Vertex>() as u16;
    if vertex_stride != expected_stride {
        return Err(MeshWireError::VertexStrideMismatch {
            expected: expected_stride,
            found: vertex_stride,
        });
    }
    let pixels_per_point = r.read_f32()?;

    let prim_count = r.read_u32()? as usize;
    let mut primitives = Vec::with_capacity(prim_count);
    for _ in 0..prim_count {
        let clip_rect = r.read_rect()?;
        let texture_id = r.read_texture_id()?;
        let mesh = r.read_mesh_geometry(texture_id)?;
        primitives.push(ClippedPrimitive {
            clip_rect,
            primitive: Primitive::Mesh(mesh),
        });
    }

    let set_count = r.read_u32()? as usize;
    let mut set = Vec::with_capacity(set_count);
    for _ in 0..set_count {
        let id = r.read_texture_id()?;
        let delta = r.read_image_delta()?;
        set.push((id, delta));
    }
    let free_count = r.read_u32()? as usize;
    let mut free = Vec::with_capacity(free_count);
    for _ in 0..free_count {
        free.push(r.read_texture_id()?);
    }

    Ok(DecodedPaint {
        primitives,
        textures_delta: TexturesDelta { set, free },
        pixels_per_point,
    })
}

/// 길이검증 바이트 커서.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], MeshWireError> {
        let remaining = self.buf.len() - self.pos;
        if n > remaining {
            return Err(MeshWireError::UnexpectedEof {
                needed: n,
                remaining,
            });
        }
        let slice = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], MeshWireError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn read_u8(&mut self) -> Result<u8, MeshWireError> {
        Ok(self.read_array::<1>()?[0])
    }

    fn read_u16(&mut self) -> Result<u16, MeshWireError> {
        Ok(u16::from_le_bytes(self.read_array::<2>()?))
    }

    fn read_u32(&mut self) -> Result<u32, MeshWireError> {
        Ok(u32::from_le_bytes(self.read_array::<4>()?))
    }

    fn read_u64(&mut self) -> Result<u64, MeshWireError> {
        Ok(u64::from_le_bytes(self.read_array::<8>()?))
    }

    fn read_f32(&mut self) -> Result<f32, MeshWireError> {
        Ok(f32::from_le_bytes(self.read_array::<4>()?))
    }

    fn read_usize(&mut self) -> Result<usize, MeshWireError> {
        Ok(self.read_u64()? as usize)
    }

    fn read_rect(&mut self) -> Result<Rect, MeshWireError> {
        let min = Pos2::new(self.read_f32()?, self.read_f32()?);
        let max = Pos2::new(self.read_f32()?, self.read_f32()?);
        Ok(Rect::from_min_max(min, max))
    }

    fn read_texture_id(&mut self) -> Result<TextureId, MeshWireError> {
        let tag = self.read_u8()?;
        let v = self.read_u64()?;
        match tag {
            0 => Ok(TextureId::Managed(v)),
            1 => Ok(TextureId::User(v)),
            other => Err(MeshWireError::BadTag {
                what: "TextureId",
                value: other,
            }),
        }
    }

    fn read_mesh_geometry(&mut self, texture_id: TextureId) -> Result<Mesh, MeshWireError> {
        let vtx_count = self.read_u32()? as usize;
        let idx_count = self.read_u32()? as usize;

        let vtx_bytes = self.take(vtx_count * core::mem::size_of::<Vertex>())?;
        let mut vertices = vec![Vertex::default(); vtx_count];
        // 목적지 Vec 은 Vertex 정렬이 보장되므로 정렬 불문 직접 복사 가능.
        bytemuck::cast_slice_mut::<Vertex, u8>(&mut vertices).copy_from_slice(vtx_bytes);

        let idx_bytes = self.take(idx_count * core::mem::size_of::<u32>())?;
        let mut indices = vec![0u32; idx_count];
        bytemuck::cast_slice_mut::<u32, u8>(&mut indices).copy_from_slice(idx_bytes);

        Ok(Mesh {
            indices,
            vertices,
            texture_id,
        })
    }

    fn read_image_delta(&mut self) -> Result<ImageDelta, MeshWireError> {
        let pos = match self.read_u8()? {
            0 => None,
            1 => Some([self.read_usize()?, self.read_usize()?]),
            other => {
                return Err(MeshWireError::BadTag {
                    what: "ImageDelta.pos",
                    value: other,
                });
            }
        };
        let options = self.read_texture_options()?;
        let image = self.read_image_data()?;
        Ok(ImageDelta {
            image,
            options,
            pos,
        })
    }

    fn read_texture_options(&mut self) -> Result<TextureOptions, MeshWireError> {
        let magnification = self.read_filter()?;
        let minification = self.read_filter()?;
        let wrap_mode = self.read_wrap()?;
        let mipmap_mode = match self.read_u8()? {
            0 => {
                let _ = self.read_u8()?; // 패딩 (filter 자리)
                None
            }
            1 => Some(self.read_filter()?),
            other => {
                return Err(MeshWireError::BadTag {
                    what: "mipmap_mode",
                    value: other,
                });
            }
        };
        Ok(TextureOptions {
            magnification,
            minification,
            wrap_mode,
            mipmap_mode,
        })
    }

    fn read_filter(&mut self) -> Result<TextureFilter, MeshWireError> {
        match self.read_u8()? {
            0 => Ok(TextureFilter::Nearest),
            1 => Ok(TextureFilter::Linear),
            other => Err(MeshWireError::BadTag {
                what: "TextureFilter",
                value: other,
            }),
        }
    }

    fn read_wrap(&mut self) -> Result<TextureWrapMode, MeshWireError> {
        match self.read_u8()? {
            0 => Ok(TextureWrapMode::ClampToEdge),
            1 => Ok(TextureWrapMode::Repeat),
            2 => Ok(TextureWrapMode::MirroredRepeat),
            other => Err(MeshWireError::BadTag {
                what: "TextureWrapMode",
                value: other,
            }),
        }
    }

    fn read_image_data(&mut self) -> Result<ImageData, MeshWireError> {
        let kind = self.read_u8()?;
        let w = self.read_usize()?;
        let h = self.read_usize()?;
        let count = w * h;
        match kind {
            0 => {
                let bytes = self.take(count * core::mem::size_of::<Color32>())?;
                let mut pixels = vec![Color32::default(); count];
                bytemuck::cast_slice_mut::<Color32, u8>(&mut pixels).copy_from_slice(bytes);
                Ok(ImageData::Color(std::sync::Arc::new(ColorImage {
                    size: [w, h],
                    pixels,
                })))
            }
            1 => {
                let bytes = self.take(count * core::mem::size_of::<f32>())?;
                let mut pixels = vec![0f32; count];
                bytemuck::cast_slice_mut::<f32, u8>(&mut pixels).copy_from_slice(bytes);
                Ok(ImageData::Font(FontImage {
                    size: [w, h],
                    pixels,
                }))
            }
            other => Err(MeshWireError::BadTag {
                what: "ImageData",
                value: other,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    // 테스트 fixture 는 POD 바이트 round-trip 을 검증하려고 원시 픽셀값을 직접 만든다
    // (UI 색 "디자인" 이 아니라 외부 픽셀 데이터). clippy.toml 의 정상 예외 경로.
    #![allow(clippy::disallowed_methods)]

    use super::*;
    use egui::epaint::{PaintCallback, PaintCallbackInfo};

    fn vtx(x: f32, y: f32, u: f32, v: f32, c: Color32) -> Vertex {
        Vertex {
            pos: Pos2::new(x, y),
            uv: Pos2::new(u, v),
            color: c,
        }
    }

    fn sample_mesh(tex: TextureId) -> Mesh {
        Mesh {
            indices: vec![0, 1, 2, 2, 1, 3],
            vertices: vec![
                vtx(
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    Color32::from_rgba_premultiplied(1, 2, 3, 4),
                ),
                vtx(
                    10.0,
                    0.0,
                    1.0,
                    0.0,
                    Color32::from_rgba_premultiplied(5, 6, 7, 8),
                ),
                vtx(
                    0.0,
                    10.0,
                    0.0,
                    1.0,
                    Color32::from_rgba_premultiplied(9, 10, 11, 12),
                ),
                vtx(
                    10.0,
                    10.0,
                    1.0,
                    1.0,
                    Color32::from_rgba_premultiplied(250, 200, 150, 255),
                ),
            ],
            texture_id: tex,
        }
    }

    fn sample_primitives() -> Vec<ClippedPrimitive> {
        vec![
            ClippedPrimitive {
                clip_rect: Rect::from_min_max(Pos2::new(1.0, 2.0), Pos2::new(300.5, 400.25)),
                primitive: Primitive::Mesh(sample_mesh(TextureId::Managed(0))),
            },
            ClippedPrimitive {
                clip_rect: Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(64.0, 64.0)),
                primitive: Primitive::Mesh(sample_mesh(TextureId::User(42))),
            },
        ]
    }

    fn sample_textures_delta() -> TexturesDelta {
        let color = ImageDelta {
            image: ImageData::Color(std::sync::Arc::new(ColorImage {
                size: [2, 2],
                pixels: vec![
                    Color32::from_rgba_premultiplied(10, 20, 30, 40),
                    Color32::from_rgba_premultiplied(50, 60, 70, 80),
                    Color32::from_rgba_premultiplied(90, 100, 110, 120),
                    Color32::from_rgba_premultiplied(130, 140, 150, 160),
                ],
            })),
            options: TextureOptions::NEAREST,
            pos: Some([4, 8]),
        };
        let font = ImageDelta {
            image: ImageData::Font(FontImage {
                size: [3, 1],
                pixels: vec![0.0, 0.25, 1.0],
            }),
            options: TextureOptions {
                magnification: TextureFilter::Linear,
                minification: TextureFilter::Nearest,
                wrap_mode: TextureWrapMode::Repeat,
                mipmap_mode: Some(TextureFilter::Linear),
            },
            pos: None,
        };
        TexturesDelta {
            set: vec![(TextureId::Managed(0), font), (TextureId::User(7), color)],
            free: vec![TextureId::Managed(3), TextureId::User(99)],
        }
    }

    /// 핵심: 손으로 만든 primitives/textures 가 round-trip 후 완전히 동일한가.
    /// 특히 Vertex 가 byte-identical 인지 명시적으로 확인한다.
    #[test]
    fn round_trip_handmade_byte_identical() {
        let prims = sample_primitives();
        let delta = sample_textures_delta();
        let ppp = 2.0;

        let bytes = encode_paint(&prims, &delta, ppp);
        let decoded = decode_paint(&bytes).expect("decode");

        assert_eq!(decoded.pixels_per_point, ppp);
        assert_eq!(decoded.primitives.len(), prims.len());

        for (orig, got) in prims.iter().zip(&decoded.primitives) {
            assert_eq!(orig.clip_rect, got.clip_rect);
            let (Primitive::Mesh(om), Primitive::Mesh(gm)) = (&orig.primitive, &got.primitive)
            else {
                panic!("expected meshes");
            };
            assert_eq!(om.texture_id, gm.texture_id);
            assert_eq!(om.indices, gm.indices);
            // 구조적 동일 + 바이트 동일 둘 다 확인.
            assert_eq!(om.vertices, gm.vertices);
            assert_eq!(
                bytemuck::cast_slice::<Vertex, u8>(&om.vertices),
                bytemuck::cast_slice::<Vertex, u8>(&gm.vertices),
                "vertices must be byte-identical"
            );
        }

        assert_eq!(decoded.textures_delta, delta);
    }

    /// Callback primitive 는 인코드에서 제거되고 mesh 만 남아야 한다.
    #[test]
    fn callback_primitive_is_skipped() {
        let prims = vec![
            ClippedPrimitive {
                clip_rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(10.0, 10.0)),
                primitive: Primitive::Mesh(sample_mesh(TextureId::Managed(0))),
            },
            ClippedPrimitive {
                clip_rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(20.0, 20.0)),
                primitive: Primitive::Callback(PaintCallback {
                    rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(20.0, 20.0)),
                    callback: std::sync::Arc::new(
                        |_: PaintCallbackInfo, _: &mut dyn std::any::Any| {},
                    ),
                }),
            },
        ];
        let bytes = encode_paint(&prims, &TexturesDelta::default(), 1.0);
        let decoded = decode_paint(&bytes).expect("decode");
        assert_eq!(decoded.primitives.len(), 1, "callback must be skipped");
        assert!(matches!(
            decoded.primitives[0].primitive,
            Primitive::Mesh(_)
        ));
    }

    /// 실제 egui::Context tessellate 출력을 round-trip — plugin tessellate ↔ 바이트 ↔
    /// host 복원이 byte-identical Vertex 임을 진짜 데이터로 검증 (epaint 업글 방어).
    #[test]
    fn round_trip_real_tessellation_byte_identical() {
        let ctx = egui::Context::default();
        let ppp = 1.5;
        let raw_input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))),
            ..Default::default()
        };
        let full_output = ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Tasty egui-mesh");
                ui.label("round-trip POD wire codec");
                if ui.button("ok").clicked() {}
            });
        });

        let primitives = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        // 폰트 atlas 가 textures_delta.set 에 실려야 의미 있는 테스트.
        assert!(
            !full_output.textures_delta.set.is_empty(),
            "expected font atlas in textures_delta"
        );
        let mesh_count = primitives
            .iter()
            .filter(|p| matches!(p.primitive, Primitive::Mesh(_)))
            .count();
        assert!(mesh_count > 0, "expected tessellated meshes");

        let bytes = encode_paint(&primitives, &full_output.textures_delta, ppp);
        let decoded = decode_paint(&bytes).expect("decode");

        assert_eq!(decoded.pixels_per_point, ppp);
        assert_eq!(decoded.primitives.len(), mesh_count);

        let orig_meshes: Vec<&Mesh> = primitives
            .iter()
            .filter_map(|p| match &p.primitive {
                Primitive::Mesh(m) => Some(m),
                Primitive::Callback(_) => None,
            })
            .collect();
        for (om, got) in orig_meshes.iter().zip(&decoded.primitives) {
            let Primitive::Mesh(gm) = &got.primitive else {
                panic!("expected mesh");
            };
            assert_eq!(om.texture_id, gm.texture_id);
            assert_eq!(om.indices, gm.indices);
            assert_eq!(
                bytemuck::cast_slice::<Vertex, u8>(&om.vertices),
                bytemuck::cast_slice::<Vertex, u8>(&gm.vertices),
                "tessellated vertices must be byte-identical"
            );
        }
        assert_eq!(decoded.textures_delta, full_output.textures_delta);
    }

    #[test]
    fn empty_paint_round_trips() {
        let bytes = encode_paint(&[], &TexturesDelta::default(), 1.0);
        let decoded = decode_paint(&bytes).expect("decode");
        assert!(decoded.primitives.is_empty());
        assert!(decoded.textures_delta.is_empty());
        assert_eq!(decoded.pixels_per_point, 1.0);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = encode_paint(&[], &TexturesDelta::default(), 1.0);
        bytes[0] = b'X';
        assert!(matches!(
            decode_paint(&bytes),
            Err(MeshWireError::BadMagic(_))
        ));
    }

    #[test]
    fn truncated_input_is_rejected() {
        let bytes = encode_paint(&sample_primitives(), &sample_textures_delta(), 1.0);
        let truncated = &bytes[..bytes.len() - 4];
        assert!(matches!(
            decode_paint(truncated),
            Err(MeshWireError::UnexpectedEof { .. })
        ));
    }
}
