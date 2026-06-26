//! `engine` 모듈 — 도메인 sub-크레이트 묶음.
//!
//! Phase C 의 strangler fig 마이그레이션 진행 중. `Engine` *struct* 는 모든
//! 필드가 Core/Hub/View 로 옮겨가 *삭제* 됐다. 모듈 이름은 sub-module 들
//! (state / output_observer / surface_registry / command_index /
//! layout_persistence) 의 컨테이너로 잠시 유지하며, 이후 sub-step 에서
//! 각각 `core/` 산하로 재배치된다.

pub mod command_index;
pub mod hook_event_registry;
pub mod layout_persistence;
pub mod output_observer;
pub mod surface_registry;
