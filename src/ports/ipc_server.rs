//! Hub가 보유하는 IPC 서버 인터페이스. 서버 생성은 어댑터별로 처리한다.
//! 요청 타입은 IPC 어댑터를 거치지 않고 tasty_ipc::server에서 직접 사용한다.

use std::sync::mpsc;

use tasty_ipc::server::IpcCommand;

/// Hub에서만 요청을 꺼낸다. mpsc::Receiver가 Sync가 아니므로 Send만 요구한다.
pub trait IpcServerPort: Send {
    /// 대기하지 않고 큐에서 요청 하나를 꺼낸다.
    fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError>;
    /// 서버가 사용하는 포트 번호.
    fn port(&self) -> u16;
    /// 메인 스레드 밖에서 호스트→플러그인 동기 요청을 큐에 넣을 때 쓰는 sender.
    fn command_sender(&self) -> mpsc::Sender<IpcCommand>;
}
