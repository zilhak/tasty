//! `Hub` — 외부 통신 표면. IPC 서버, 포트 파일 등 *프로세스 외부* 와 주고받는
//! 인프라를 모은다.

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::ipc::server::IpcServer;

pub(crate) struct Hub {
    pub ipc_server: Option<IpcServer>,
    pub port_file: Option<String>,
}

impl Hub {
    pub(crate) fn new(port_file: Option<String>) -> Self {
        Self {
            ipc_server: None,
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
