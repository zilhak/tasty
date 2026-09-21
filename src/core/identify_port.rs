//! 비동기 파일 식별을 **띄우는** 포트 — 도메인(`DomainIntent::DispatchFile`)이 소유한다.
//!
//! 식별 worker 의 실체(`file::identify_worker::IdentifyWorker`)는 winit event loop proxy 로
//! 결과(`AppEvent::IdentifyDone`)를 돌려보내는 GUI 부품이다. 도메인이 그 타입을 필드로 들면
//! 창 조립부에 컴파일 의존하므로, 도메인이 쓰는 연산 하나(식별 시작)만 여기 선언하고 worker
//! 가 구현한다. 요청 id 는 도메인이 추적하지 않아 반환하지 않는다.

use crate::file::dispatch::FileDispatchOrigin;
use crate::file::format::{DetectDepth, FileTarget};

pub(crate) trait IdentifySpawner: Send + Sync {
    /// 식별을 백그라운드로 시작하고 즉시 돌아온다. 결과 배달은 구현의 몫이다.
    fn spawn_identify(
        &self,
        target: FileTarget,
        depth: DetectDepth,
        origin_surface_id: Option<u32>,
        dispatch_origin: FileDispatchOrigin,
        ignore_size_limit: bool,
    );
}
