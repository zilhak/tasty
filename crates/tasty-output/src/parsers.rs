//! tasty-output 빌트인 파서 도메인 sub-module.

mod errors;
#[cfg(test)]
mod fuzz;
mod links;
mod progress;
mod shell;
mod test_result;
#[cfg(test)]
mod tests;

pub use errors::{CompileErrorParser, StackTraceParser};
pub use links::{OscLinkParser, PathParser, UrlParser};
pub use progress::ProgressParser;
pub use shell::{ExitCodeParser, OscNotificationParser, PromptBoundaryParser};
pub use test_result::TestResultParser;
