//! 시스템 클립보드 접근. 헤드리스에서는 동작하지 않는 어댑터를 주입한다.

pub trait ClipboardSystem: Send + Sync {
    fn read_text(&self) -> anyhow::Result<String>;
    fn write_text(&self, text: &str) -> anyhow::Result<()>;
}
