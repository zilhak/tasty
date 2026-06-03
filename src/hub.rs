//! `Hub` — 외부 통신 표면. IPC 서버, 포트 파일 등 *프로세스 외부* 와 주고받는
//! 인프라를 모은다.
//!
//! D.3.D.2 — `ipc_server` 는 `Option<Box<dyn IpcServerPort>>` 로 보유. production
//! 은 `TcpIpcServer` (옛 `IpcServer` 의 type alias).

use crate::adapters::production::tcp_ipc_server::TcpIpcServer;
use crate::ipc::host_call::HostIpcInjector;
use crate::ipc::server::IpcWaker;
use crate::ports::ipc_server::IpcServerPort;

pub(crate) struct Hub {
    pub ipc_server: Option<Box<dyn IpcServerPort>>,
    pub port_file: Option<String>,
}

impl Hub {
    pub(crate) fn new(port_file: Option<String>) -> Self {
        Self {
            ipc_server: None,
            port_file,
        }
    }

    /// IPC 서버 시작. `IpcWaker` 는 호출자가 직접 만든다 — gui 빌드는
    /// `EventLoopProxy<AppEvent>` 에서 변환, headless 는 `mpsc::Sender<AppEvent>`
    /// 에서 변환 (`adapters::production::{winit_waker, headless_waker}`).
    ///
    /// 반환: host→plugin sync dispatch 에 사용할 `HostIpcInjector` (서버 시작
    /// 실패 시 `None`). 호출자가 `Core::set_host_ipc_injector` 로 등록한다.
    pub(crate) fn start_ipc(&mut self, ipc_waker: IpcWaker) -> Option<HostIpcInjector> {
        match TcpIpcServer::start_with_port_file(self.port_file.take(), Some(ipc_waker.clone())) {
            Ok(ipc) => {
                tracing::info!("IPC server started on port {}", ipc.port());
                let injector = HostIpcInjector::new(ipc.command_sender(), ipc_waker);
                self.ipc_server = Some(Box::new(ipc));
                Some(injector)
            }
            Err(e) => {
                tracing::warn!("Failed to start IPC server: {}", e);
                None
            }
        }
    }
}
