//! Tasty 가 작업 실행을 위해 띄우는 자식(Run task·후처리 CLI·reduce 의 custom 셸)의 환경.
//!
//! 자식은 Tasty 프로세스의 환경을 받되, 바깥 세션의 신원은 넘기지 않는다. 이 Tasty 를 Claude Code
//! 나 다른 Tasty 의 터미널 안에서 띄웠으면 그 세션의 변수가 이 프로세스에 남아 있고, 자식이 그
//! 값으로 `claude`·`tasty` 를 부르면 다른 세션·인스턴스의 신원으로 동작한다.
//!
//! 지우기만 하고 이 인스턴스의 값으로 덮어쓰지는 않는다. 터미널 셸은 자기 surface 의
//! `TASTY_SURFACE_ID`·`TASTY_PARENT_HOME` 을 넣지만, 작업의 자식은 터미널 surface 가 아니고 자식이
//! 띄운 에이전트의 완료 알림은 그 알림을 기다리는 호출자가 받는다.

use std::ffi::OsString;

/// 바깥 Tasty 인스턴스가 이 프로세스에 남긴 신원 변수. 이 Tasty 를 다른 Tasty 의 터미널에서
/// 띄웠으면 그 surface·세션·완료 알림 경로를 가리킨다.
pub const OUTER_IDENTITY_ENV: &[&str] = &[
    "TASTY_SESSION_TOKEN",
    "TASTY_SURFACE_ID",
    "TASTY_PARENT_HOME",
    "TASTY_AGENT_ID",
];

/// 작업 자식의 환경. 상속 환경에서 두 종류를 뺀다.
/// - 바깥 Claude Code 세션의 표지·비밀처럼 터미널 셸에도 넘기지 않는 변수
///   (`tasty_utils::process::is_stripped_inherited_env`).
/// - 바깥 Tasty 인스턴스의 신원 변수([`OUTER_IDENTITY_ENV`]).
///
/// 나머지(`TASTY_HOME`·`TASTY_LOCALE` 등 다른 `TASTY_*` 포함)는 그대로 넘기고, Tasty 가 task 별
/// 변수를 더하지는 않는다.
pub fn child_env(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    inherited
        .into_iter()
        .filter(|(k, v)| {
            !k.to_str().is_some_and(|k| {
                OUTER_IDENTITY_ENV.contains(&k)
                    || tasty_utils::process::is_stripped_inherited_env(k, v)
            })
        })
        .collect()
}

/// 이 프로세스 환경을 [`child_env`] 로 거른 값.
pub fn inherited() -> Vec<(OsString, OsString)> {
    child_env(std::env::vars_os())
}

#[cfg(all(test, unix))]
#[path = "child_env_tests.rs"]
mod tests;
