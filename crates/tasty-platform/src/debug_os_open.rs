//! debug 빌드에서 OS 열기를 실행하지 않고 TASTY_DEBUG_OS_OPEN_LOG에 기록한다.
//! 별도 TASTY_HOME·디스플레이를 써도 기본 브라우저가 사용자 인스턴스로 요청을 전달할 수 있다.
//! 변수가 있으면 값이 비었거나 기록에 실패해도 열기를 억제하며 경고만 남긴다.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

/// 이 변수가 있으면 OS 열기를 기록만 한다. 값은 기록 파일 경로다.
pub const ENV: &str = "TASTY_DEBUG_OS_OPEN_LOG";

/// OS 열기를 가로챘으면 `true` — 호출부는 그때 아무것도 띄우지 않고 성공으로 돌아간다.
///
/// `via` 는 어느 자리가 열려 했는지(`open_uri` · `open_path` …), `target` 은 넘기려던 값이다.
/// 기록 줄은 `<via>\t<target>\n` 이다.
pub fn intercepted(via: &str, target: &str) -> bool {
    record(std::env::var_os(ENV), via, target)
}

/// 환경변수를 직접 변경하지 않고 억제 규칙을 시험할 수 있도록 분리한다.
fn record(dest: Option<OsString>, via: &str, target: &str) -> bool {
    let Some(dest) = dest else {
        return false;
    };
    if dest.is_empty() {
        tracing::warn!(
            via,
            target,
            "{ENV} is empty; OS open suppressed without a record"
        );
        return true;
    }
    let path = Path::new(&dest);
    let appended = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| writeln!(f, "{via}\t{target}"));
    if let Err(err) = appended {
        tracing::warn!(%err, via, target, path = %path.display(), "OS open suppressed but not recorded");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_variable_lets_the_open_through() {
        assert!(!record(None, "open_uri", "https://example.com"));
    }

    #[test]
    fn present_variable_records_one_line_per_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("os-open.log");
        assert!(record(
            Some(log.clone().into_os_string()),
            "open_uri",
            "file:///tmp/a"
        ));
        assert!(record(
            Some(log.clone().into_os_string()),
            "open_path",
            "/tmp/b"
        ));
        let text = std::fs::read_to_string(&log).expect("log written");
        assert_eq!(text, "open_uri\tfile:///tmp/a\nopen_path\t/tmp/b\n");
    }

    /// 기록 실패는 억제를 풀지 않는다 — 풀면 경로 오타 하나가 사용자 브라우저를 연다.
    #[test]
    fn empty_or_unwritable_destination_still_suppresses() {
        assert!(record(Some(OsString::new()), "open_uri", "https://x"));
        let dir = tempfile::tempdir().expect("tempdir");
        let unwritable = dir.path().join("missing-dir").join("os-open.log");
        assert!(record(
            Some(unwritable.clone().into_os_string()),
            "open_uri",
            "https://x"
        ));
        assert!(!unwritable.exists());
    }
}
