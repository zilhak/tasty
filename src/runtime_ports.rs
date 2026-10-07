//! 이 인스턴스가 실제로 bind한 포트의 기록. IPC 서버와 웹훅 리스너가 bind에 성공한 뒤 한 번씩
//! 기록하고, 포트가 필요한 곳(자기 자신 attach 거절, 인스턴스 파일, 웹훅 URL, `webhook.config` 조회)은
//! 여기서 읽는다. 설정 파일 값은 다음 실행의 요청일 뿐이라 여기에 넣지 않는다.
//! `tasty.port` 파일은 IPC 서버가 bind 시점에 직접 쓴다. 값은 여기 기록하는 것과 같은 bind 결과다.
//!
//! 인스턴스 하나에 하나다. `AppServices`가 GUI·headless 공통 앱 초기화에서 만들어 소유하고,
//! 프로세스 전역 static으로 두지 않는다. 한 시험 프로세스가 여러 `AppServices`를 병렬로 만들 때
//! 서로의 기록을 보지 않게 하기 위해서다.

use std::net::SocketAddr;
use std::sync::OnceLock;

/// 포트를 어떻게 정했는지.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PortSource {
    /// 실행 인자로 지정했다.
    Argument,
    /// 설정 파일에 저장한 값이다.
    Config,
    /// 명시 지정 없이 기본값부터 차례로 시도해 얻었다.
    Probe,
    /// OS가 고른 빈 포트다(IPC).
    Dynamic,
}

impl PortSource {
    /// 실행 인자나 설정으로 직접 지정한 포트인지.
    pub(crate) fn is_explicit(self) -> bool {
        matches!(self, Self::Argument | Self::Config)
    }
}

/// bind에 성공한 주소와 그 출처.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoundPort {
    pub(crate) addr: SocketAddr,
    pub(crate) source: PortSource,
}

#[derive(Debug, Default)]
pub(crate) struct RuntimePorts {
    ipc: OnceLock<BoundPort>,
    webhook: OnceLock<BoundPort>,
}

impl RuntimePorts {
    /// IPC 서버 bind 뒤 한 번 기록한다. 두 번째 기록은 경고만 남기고 처음 값을 유지한다.
    pub(crate) fn record_ipc(&self, bound: BoundPort) {
        if self.ipc.set(bound).is_err() {
            tracing::warn!("runtime IPC port already recorded");
        }
    }

    /// 웹훅 리스너 bind 뒤 한 번 기록한다.
    pub(crate) fn record_webhook(&self, bound: BoundPort) {
        if self.webhook.set(bound).is_err() {
            tracing::warn!("runtime webhook port already recorded");
        }
    }

    pub(crate) fn ipc(&self) -> Option<BoundPort> {
        self.ipc.get().copied()
    }

    pub(crate) fn webhook(&self) -> Option<BoundPort> {
        self.webhook.get().copied()
    }

    pub(crate) fn ipc_port(&self) -> Option<u16> {
        self.ipc().map(|b| b.addr.port())
    }

    pub(crate) fn webhook_port(&self) -> Option<u16> {
        self.webhook().map(|b| b.addr.port())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(port: u16, source: PortSource) -> BoundPort {
        BoundPort {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            source,
        }
    }

    #[test]
    fn the_first_record_wins_and_instances_do_not_share() {
        let a = RuntimePorts::default();
        let b = RuntimePorts::default();
        a.record_webhook(bound(28430, PortSource::Config));
        a.record_webhook(bound(28431, PortSource::Config));
        assert_eq!(a.webhook_port(), Some(28430));
        assert_eq!(b.webhook_port(), None, "another instance sees nothing");
        a.record_ipc(bound(40000, PortSource::Dynamic));
        assert_eq!(a.ipc_port(), Some(40000));
    }
}
