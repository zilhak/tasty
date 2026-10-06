//! runner 가 띄우는 자식(Run task·후처리 CLI)의 환경. 규칙은 [`tasty_agent::child_env`] 에 있다.

#[cfg(all(test, unix))]
#[path = "child_env_tests.rs"]
mod tests;

pub(crate) use tasty_agent::child_env::inherited;
