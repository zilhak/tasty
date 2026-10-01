//! Engine 작업 실행·완료 대기와 runner 수명을 연결한다.
//! IPC 핸들러는 인자를 해석하고 여기서 반환한 결과를 응답으로 만든다.

pub(crate) mod completion_strategy;
pub(crate) mod event_feed;
pub(crate) mod graph_view;
pub(crate) mod hook_wait;
pub(crate) mod runner_host;
pub(crate) mod runner_thread;
pub(crate) mod task;
pub(crate) mod task_output_ref;
pub(crate) mod task_waker;
