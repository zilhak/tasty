//! NotificationBackend port — OS native notification (macOS / Linux / Windows).
//!
//! Headless 시 NoOp.

#[allow(dead_code)]
pub trait NotificationBackend: Send + Sync {
    fn show(&self, title: &str, body: &str, icon: Option<&IconRef>) -> anyhow::Result<()>;
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum IconRef {
    /// 디스크의 PNG 등.
    Path(std::path::PathBuf),
    /// in-memory RGBA8.
    Image {
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    /// OS 시스템 아이콘 이름.
    System(String),
}
