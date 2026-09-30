//! decide 계약. 상태와 명령만 보고 이벤트·effect·응답을 정하며 저장소·시계·ID 생성기를 직접 부르지 않는다.
//!
//! 실행기(재시도 키 조회·확정·적용·응답)는 root runtime이 이 계약에 대해 generic하게 구현한다.

use std::fmt;

use crate::ids::{IdSupplier, Revision};

/// decide에 주는 입력. 새 ID와 시각은 여기서만 받아 decide를 결정적으로 유지한다.
pub struct DecisionContext<'a> {
    pub ids: &'a mut dyn IdSupplier,
    pub command_id: &'a str,
    pub now_ms: u64,
}

/// decide 결과. 실행기가 이벤트·effect·명령 기록을 한 transaction으로 확정한다.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision<E, F> {
    pub events: Vec<E>,
    pub effects: Vec<F>,
    /// 최초 해소한 대상·입력. 재요청은 이 기록을 쓰고 대상을 다시 해소하지 않는다.
    pub resolved: Vec<u8>,
    pub response: Vec<u8>,
}

/// 한 stream의 상태·명령·이벤트를 정하는 순수 규칙.
pub trait Decider {
    type State;
    type Command;
    type Event;
    type Effect;
    /// 도메인 거절. 이벤트를 만들지 않는다.
    type Rejection: Clone + fmt::Debug;

    /// 상태에 마지막으로 적용한 이 stream의 revision.
    fn revision(&self, state: &Self::State) -> Option<Revision>;

    /// 같은 재시도 키의 재요청이 원래 요청과 같은지 판정하는 값.
    fn request_digest(&self, command: &Self::Command) -> Vec<u8>;

    fn decide(
        &self,
        state: &Self::State,
        command: &Self::Command,
        ctx: &mut DecisionContext<'_>,
    ) -> Result<Decision<Self::Event, Self::Effect>, Self::Rejection>;
}
