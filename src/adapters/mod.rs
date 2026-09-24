//! 앱 진입점과 외부 시스템 연동.
//! ipc/ui/cli/plugin은 요청을 받고 production은 외부 서비스를 구현한다.
//! test에는 시험용 구현을 둔다.

pub mod cli;
pub mod ipc;
pub mod plugin;
pub mod production;
#[cfg(test)]
pub mod test;
#[cfg(feature = "gui")]
pub mod ui;
