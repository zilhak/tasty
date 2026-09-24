//! CLI 출력 오류를 처리한다. stdout은 outln!/out!, stderr는 errln!을 사용한다.
//! BrokenPipe는 StdoutClosed로 전달하고 CLI 진입점에서 종료 코드 0으로 처리한다.
//! 그 외 stdout 오류는 종료 코드 1로 전파한다. stderr 쓰기 실패는 버리고 기존 종료 코드를 유지한다.
//! 자세한 규칙은 docs/dev-guide/cli-structure.md#stdout-출력-outrs를 따른다.

use std::fmt;
use std::io::{self, Write};

/// stdout을 읽던 쪽이 파이프를 닫았다. quiet_if_stdout_closed가 정상 종료로 처리한다.
#[derive(Debug, thiserror::Error)]
#[error("stdout closed by reader (broken pipe)")]
pub struct StdoutClosed;

/// BrokenPipe만 StdoutClosed로 바꾸고 다른 오류는 문맥을 붙여 반환한다.
fn classify(err: io::Error) -> anyhow::Error {
    if err.kind() == io::ErrorKind::BrokenPipe {
        StdoutClosed.into()
    } else {
        anyhow::Error::new(err).context(tasty_i18n::t("cli.out.write_failed").to_string())
    }
}

/// stdout 에 직접 쓰는 외부 코드(clap `print_help` 등)의 `io::Result` 를 같은 규칙으로
/// 분류한다.
pub fn from_io(result: io::Result<()>) -> anyhow::Result<()> {
    result.map_err(classify)
}

/// `println!` 대체 — `args` 뒤에 개행. [`outln!`] 이 이 함수를 부른다.
pub fn line(args: fmt::Arguments<'_>) -> anyhow::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_fmt(args)
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(classify)
}

/// `print!` 대체 — 개행 없이 쓴다. [`out!`] 이 이 함수를 부른다. 버퍼링은 std 와
/// 같다(LineWriter: 개행 또는 [`flush`] 시점에 실제 write 가 일어난다).
pub fn text(args: fmt::Arguments<'_>) -> anyhow::Result<()> {
    io::stdout().lock().write_fmt(args).map_err(classify)
}

/// stdout flush — `std::io::stdout().flush()` 대체. 개행 없는 [`text`] 출력을 밀어낼 때
/// 쓴다. 파이프 생존 프로브가 아니다: 버퍼가 비어 있으면 write(2) 를 내지 않아 EPIPE 를
/// 감지하지 못한다 — 읽는 쪽 닫힘은 다음 실제 write([`line`]/[`text`]+flush)에서 잡힌다.
pub fn flush() -> anyhow::Result<()> {
    io::stdout().lock().flush().map_err(classify)
}

/// stderr에 개행과 함께 쓴다. 실패를 다시 보고할 곳이 없어 쓰기 오류를 무시한다.
pub fn err_line(args: fmt::Arguments<'_>) {
    let mut stderr = io::stderr().lock();
    let written = stderr
        .write_fmt(args)
        .and_then(|()| stderr.write_all(b"\n"));
    // stderr 가 닫혔다 — 이 실패를 보고할 채널이 남아 있지 않다. 종료 코드는 호출자가 정한다.
    drop(written);
}

/// `err` 가 stdout 닫힘([`StdoutClosed`])에서 비롯됐는가. `context()` 로 감싸인 경우도
/// 원인 체인을 따라 찾는다.
pub fn is_stdout_closed(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|e| e.downcast_ref::<StdoutClosed>().is_some())
}

/// CLI 진입점의 경계 처리 — stdout 닫힘은 조용한 성공(종료 코드 0), 나머지는 그대로.
pub fn quiet_if_stdout_closed(result: anyhow::Result<()>) -> anyhow::Result<()> {
    match result {
        Err(e) if is_stdout_closed(&e) => Ok(()),
        other => other,
    }
}

/// `println!` 대체. `outln!()?` / `outln!("{}", v)?` — 값은 `anyhow::Result<()>`.
macro_rules! outln {
    () => {
        $crate::out::line(::std::format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::out::line(::std::format_args!($($arg)*))
    };
}

/// `print!` 대체(개행 없음). 값은 `anyhow::Result<()>`.
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::out::text(::std::format_args!($($arg)*))
    };
}

/// `eprintln!` 대체. 값이 없다 — stderr 쓰기 실패는 [`err_line`] 이 버린다.
macro_rules! errln {
    () => {
        $crate::out::err_line(::std::format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::out::err_line(::std::format_args!($($arg)*))
    };
}

pub(crate) use {errln, out, outln};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_pipe_is_classified_as_stdout_closed() {
        let err = classify(io::Error::from(io::ErrorKind::BrokenPipe));
        assert!(is_stdout_closed(&err));
        assert!(quiet_if_stdout_closed(Err(err)).is_ok());
    }

    #[test]
    fn stdout_closed_is_found_through_context_layers() {
        let err = classify(io::Error::from(io::ErrorKind::BrokenPipe))
            .context("outer")
            .context("outermost");
        assert!(is_stdout_closed(&err));
    }

    #[test]
    fn other_io_errors_stay_errors() {
        let err = classify(io::Error::other("disk full"));
        assert!(!is_stdout_closed(&err));
        assert!(quiet_if_stdout_closed(Err(err)).is_err());
        assert!(quiet_if_stdout_closed(Err(anyhow::anyhow!("unrelated"))).is_err());
        assert!(quiet_if_stdout_closed(Ok(())).is_ok());
    }

    #[test]
    fn from_io_maps_broken_pipe_only() {
        assert!(is_stdout_closed(
            &from_io(Err(io::Error::from(io::ErrorKind::BrokenPipe))).unwrap_err()
        ));
        assert!(!is_stdout_closed(
            &from_io(Err(io::Error::from(io::ErrorKind::PermissionDenied))).unwrap_err()
        ));
        assert!(from_io(Ok(())).is_ok());
    }
}
