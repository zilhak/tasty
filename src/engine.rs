pub mod command_index;
pub mod layout_persistence;
pub mod output_observer;
pub mod state;
pub mod surface_registry;

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::ipc::server::IpcServer;

/// The central engine that owns IPC server and coordinates windows.
/// EngineState is currently inside AppState (composition), and will be
/// fully extracted in a later phase when IPC handlers are updated.
pub struct Engine {
    pub ipc_server: Option<IpcServer>,
    /// The window that currently has focus (receives IPC commands targeting "focused" window).
    pub focused_window_id: Option<winit::window::WindowId>,
    pub port_file: Option<String>,
}

impl Engine {
    pub(crate) fn new(port_file: Option<String>) -> Self {
        Self {
            ipc_server: None,
            focused_window_id: None,
            port_file,
        }
    }

    /// Start the IPC server. `proxy` 는 View 가 보유한 EventLoopProxy.
    pub(crate) fn start_ipc(&mut self, proxy: &EventLoopProxy<AppEvent>) {
        let ipc_proxy = proxy.clone();
        let ipc_waker: crate::ipc::server::IpcWaker = std::sync::Arc::new(move || {
            crate::shortcuts::send_app_event(&ipc_proxy, AppEvent::IpcReady);
        });
        match IpcServer::start_with_port_file(self.port_file.take(), Some(ipc_waker)) {
            Ok(ipc) => {
                tracing::info!("IPC server started on port {}", ipc.port());
                self.ipc_server = Some(ipc);
            }
            Err(e) => {
                tracing::warn!("Failed to start IPC server: {}", e);
            }
        }
    }
}
