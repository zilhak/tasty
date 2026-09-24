//! IPC 서버와 포트 파일 설정을 보관한다.

use crate::adapters::production::tcp_ipc_server::TcpIpcServer;
use crate::ipc::server::IpcWaker;
use crate::ports::ipc_server::IpcServerPort;
use tasty_ipc::host_call::HostIpcInjector;
use tasty_ipc::stream_hub::StreamContext;

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

    /// 서버와 같은 명령 큐를 쓰는 injector를 반환한다. 시작 실패는 None이다.
    /// waker와 연결 통계는 GUI·headless 호출자가 주입한다.
    pub(crate) fn start_ipc(
        &mut self,
        ipc_waker: IpcWaker,
        stream_ctx: StreamContext,
        connections: std::sync::Arc<tasty_telemetry::ConnectionStats>,
    ) -> Option<HostIpcInjector> {
        match TcpIpcServer::start_with_port_file(
            self.port_file.take(),
            Some(ipc_waker.clone()),
            stream_ctx,
            connections,
        ) {
            Ok(ipc) => {
                tracing::info!("IPC server started on port {}", ipc.port());
                // 소켓 요청과 내부 요청이 같은 큐의 바이트 한도를 공유해야 한다.
                let injector = HostIpcInjector::new(ipc.command_sender(), ipc_waker)
                    .with_admission(ipc.admission());
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
