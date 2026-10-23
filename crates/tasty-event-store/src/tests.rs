//! 실제 SQLite 파일에 대한 저장 계약 시험.

mod common;
mod progress_wait;

mod atomicity;
mod commands;
mod effects;
mod fencing;
mod group;
mod identity;
mod manifest_alias;
mod metrics;
mod projection;
mod schema;
mod snapshots;

mod endpoint;
