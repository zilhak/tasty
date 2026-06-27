//! ClipboardSystem port — 시스템 clipboard (arboard wrap).
//!
//! Headless 시 NoOp adapter.

pub trait ClipboardSystem: Send + Sync {
    fn read_text(&self) -> anyhow::Result<String>;
    fn write_text(&self, text: &str) -> anyhow::Result<()>;
}
