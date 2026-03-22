use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct Uniforms {
    pub cell_size: [f32; 2],
    pub grid_offset: [f32; 2],
    pub viewport_size: [f32; 2],
    pub _padding: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct BgInstance {
    pub pos: [f32; 2],
    pub bg_color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct GlyphInstance {
    pub pos: [f32; 2],
    pub uv_offset: [f32; 2],
    pub uv_size: [f32; 2],
    pub fg_color: [f32; 4],
    pub glyph_offset: [f32; 2],
    pub glyph_size: [f32; 2],
}
