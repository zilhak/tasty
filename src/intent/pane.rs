//! Pane 도메인 Intent 핸들러.
//!
//! 정책:
//! - **SplitPane**: focused 의존 (사용자 단축키 전용). `state.split_pane` 호출.
//! - ratio / focus 변경 API 는 S3=B 결정으로 마이그레이션 범위 외 — 사용자 단축키
//!   전용 cascade (`close_active_pane` 등) 도 그대로 직접 호출 유지.

use super::{DispatchedIntent, Intent};
use crate::engine_state::CoreState;
use crate::model::SplitDirection;
use crate::state::AppState;

pub fn handle(state: &mut AppState, engine: &mut CoreState, intent: &DispatchedIntent) {
    if let Intent::SplitPane { direction } = &intent.body {
        split(state, engine, *direction);
    }
}

fn split(state: &mut AppState, engine: &mut CoreState, direction: SplitDirection) {
    if let Err(e) = state.split_pane(engine, direction) {
        tracing::warn!("split_pane failed: {e}");
    }
}
