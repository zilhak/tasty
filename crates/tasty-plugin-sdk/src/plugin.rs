//! Plugin trait — plugin 작성자가 구현하는 진입점.
//!
//! 사용 예:
//!
//! ```ignore
//! struct MyExplorer;
//!
//! impl Plugin for MyExplorer {
//!     fn id(&self) -> &str { "com.example.explorer" }
//!     fn version(&self) -> &str { "0.1.0" }
//!     fn surface_kinds(&self) -> Vec<&str> { vec!["explorer"] }
//!
//!     fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
//!         // initial tree
//!         SurfaceResult { tree: Some(my_tree()), display_name: Some("Files".into()) }
//!     }
//!
//!     fn handle_event(&mut self, ctx: SurfaceEventCtx) -> SurfaceResult {
//!         // event 처리 후 새 tree 반환
//!         SurfaceResult { tree: Some(my_tree()), display_name: None }
//!     }
//! }
//!
//! fn main() -> anyhow::Result<()> {
//!     tasty_plugin_sdk::run(MyExplorer)
//! }
//! ```

use serde_json::Value;
use tasty_plugin_protocol::ui_tree::UiEvent;
pub use tasty_plugin_protocol::SurfaceResult;

#[derive(Debug, Clone)]
pub struct SurfaceCreateCtx {
    pub surface_id: u32,
    pub kind: String,
    pub params: Value,
}

#[derive(Debug, Clone)]
pub struct SurfaceEventCtx {
    pub surface_id: u32,
    pub event: UiEvent,
}

#[derive(Debug, Clone)]
pub struct SurfaceRestoreCtx {
    pub surface_id: u32,
    pub kind: String,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct SurfaceSnapshotCtx {
    pub surface_id: u32,
}

/// Plugin 생명주기 진입점. 모든 메서드는 동기적으로 호출되고, plugin 로직은
/// 내부에서 자유롭게 thread/async runtime을 사용할 수 있다.
pub trait Plugin: Send + 'static {
    /// 매니페스트 id와 일치해야 함. SDK가 hello 송신 시 사용.
    fn id(&self) -> &str;

    /// hello에 포함할 plugin 자체 버전.
    fn version(&self) -> &str {
        "0.0.0"
    }

    /// `surface.create`에 응답. 초기 tree와 display_name 반환.
    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult;

    /// `surface.event`에 응답. tree가 None이면 호스트는 이전 tree 유지.
    fn handle_event(&mut self, ctx: SurfaceEventCtx) -> SurfaceResult;

    /// `surface.restore`에 응답. 영속화된 데이터로부터 surface 복원.
    fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
        let _ = ctx;
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    /// `surface.snapshot` — 영속화할 데이터 반환. 기본 구현은 null.
    fn snapshot_surface(&mut self, ctx: SurfaceSnapshotCtx) -> Value {
        let _ = ctx;
        Value::Null
    }

    /// `surface.destroy` — 호스트가 surface를 닫을 때 호출. 자원 해제용.
    fn destroy_surface(&mut self, surface_id: u32) {
        let _ = surface_id;
    }
}
