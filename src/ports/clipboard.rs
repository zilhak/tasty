//! ClipboardSystem port — 시스템 clipboard (arboard wrap).
//!
//! Headless 시 NoOp adapter.

#[allow(dead_code)] // 이유: ClipboardSystem port — DI 빌더 배선 존재, read_image/write_image 호출 경로 배선 대기
pub trait ClipboardSystem: Send + Sync {
    fn read_text(&self) -> anyhow::Result<String>;
    fn write_text(&self, text: &str) -> anyhow::Result<()>;

    fn read_image(&self) -> anyhow::Result<ClipboardImage>;
    fn write_image(&self, image: &ClipboardImage) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct ClipboardImage {
    pub width: u32,
    pub height: u32,
    /// RGBA8 픽셀, row-major.
    pub pixels: Vec<u8>,
}
