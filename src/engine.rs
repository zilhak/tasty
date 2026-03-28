use crate::ipc::server::IpcServer;
use crate::AppEvent;
use winit::event_loop::EventLoopProxy;

/// The central engine that owns IPC server and coordinates windows.
/// EngineState is currently inside AppState (composition), and will be
/// fully extracted in a later phase when IPC handlers are updated.
pub struct Engine {
    pub ipc_server: Option<IpcServer>,
    pub proxy: EventLoopProxy<AppEvent>,
    /// When Some, a modal window is active and all other windows should ignore input.
    pub modal_window_id: Option<winit::window::WindowId>,
    pub port_file: Option<String>,
}

impl Engine {
    pub fn new(proxy: EventLoopProxy<AppEvent>, port_file: Option<String>) -> Self {
        Self {
            ipc_server: None,
            proxy,
            modal_window_id: None,
            port_file,
        }
    }

    /// Start the IPC server.
    pub fn start_ipc(&mut self) {
        let ipc_proxy = self.proxy.clone();
        let ipc_waker: crate::ipc::server::IpcWaker = std::sync::Arc::new(move || {
            let _ = ipc_proxy.send_event(AppEvent::IpcReady);
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

    /// Check if a modal is active.
    pub fn is_modal_active(&self) -> bool {
        self.modal_window_id.is_some()
    }
}
