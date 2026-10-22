//! View의 튜토리얼 진행 요청을 state.db writer에 넣는 얇은 입구. 저장 실행과 View별 버전 체인은
//! writer(`crate::db::writer`)가 맡는다. 접수한 저장은 요청을 낸 View가 닫혀도 실행된다.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) use crate::db::writer::{Reply, Save, TutorialReceipt as Receipt};
use crate::db::writer::{StateQueue, StateRequest, submit_tutorial};

#[derive(Default)]
pub(crate) struct Client {
    queue: Option<StateQueue>,
    closed: Arc<AtomicBool>,
    identity: Arc<()>,
}
impl Client {
    fn submit(&self, request: StateRequest) -> Result<Receipt, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("tutorial writer stopped".into());
        }
        let queue = self.queue.as_ref().ok_or("tutorial database unavailable")?;
        submit_tutorial(queue, request)
            .map_err(|error| format!("tutorial write admission: {error}"))
    }
    pub(crate) fn load(&self) -> Result<Receipt, String> {
        let topics = crate::adapters::ui::tutorial::all_topics()
            .iter()
            .map(|topic| (topic.id, topic.revision))
            .collect();
        self.submit(StateRequest::TutorialLoad {
            owner: self.identity.clone(),
            topics,
        })
    }
    pub(crate) fn save(&self, save: Save) -> Result<Receipt, String> {
        self.submit(StateRequest::TutorialSave {
            owner: self.identity.clone(),
            save,
        })
    }
}

/// App이 소유하는 튜토리얼 접수 게이트. 종료가 시작되면 새 튜토리얼 요청만 막고 최근 파일 저장은
/// 막지 않는다. writer의 종료는 AppServices가 맡는다.
#[derive(Default)]
pub(crate) struct TutorialProgress {
    closed: Arc<AtomicBool>,
}
impl TutorialProgress {
    /// state.db writer가 없으면 요청마다 거절하는 Client다.
    pub(crate) fn client(&self) -> Client {
        Self::client_on(crate::db::shared_queue(), self.closed.clone())
    }
    fn client_on(queue: Option<StateQueue>, closed: Arc<AtomicBool>) -> Client {
        Client {
            queue,
            closed,
            identity: Arc::new(()),
        }
    }
    pub(crate) fn stop(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
