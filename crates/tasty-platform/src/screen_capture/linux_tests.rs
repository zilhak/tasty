//! 임시 디렉터리의 도구 shim 으로 후보 순서·폴백·취소 분류를 확인한다. shim 은 OS 네이티브
//! 선택 UI를 대신하지 않는다. 실제 도구의 종료 상태는 기능 문서의 "결과 분류" 절에 기록한다.

use super::CaptureError;
use super::linux::{candidates, run_candidates};
use super::tests::png_fixture;
use std::path::PathBuf;

struct Shims {
    dir: tempfile::TempDir,
    log: PathBuf,
    fixture: PathBuf,
}

impl Shims {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("calls.log");
        let fixture = dir.path().join("fixture.png");
        std::fs::write(&fixture, png_fixture()).expect("fixture");
        Self { dir, log, fixture }
    }

    fn bin(&self) -> PathBuf {
        let bin = self.dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        bin
    }

    /// `name` 을 호출 기록 뒤 `body` 를 실행하는 shim 으로 만든다. 마지막 인자는 `$last` 로 읽는다.
    fn tool(&self, name: &str, body: &str) {
        let script = format!(
            "#!/bin/sh\necho {name} >> '{log}'\nfor last; do :; done\nFIXTURE='{fixture}'\n{body}\n",
            log = self.log.display(),
            fixture = self.fixture.display(),
        );
        tasty_test_support::write_executable(&self.bin().join(name), script).expect("shim");
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn run(&self, wayland: bool) -> (Result<(), CaptureError>, PathBuf) {
        let out = self.dir.path().join("out.png");
        let result = run_candidates(candidates(wayland), &out, self.bin().as_os_str());
        (result, out)
    }
}

const WRITE_PNG: &str = "cp \"$FIXTURE\" \"$last\"";

#[test]
fn failing_first_tool_falls_back_to_the_next_candidate() {
    let shims = Shims::new();
    shims.tool("gnome-screenshot", "echo boom >&2\nexit 1");
    shims.tool("scrot", WRITE_PNG);
    let (result, out) = shims.run(false);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(shims.calls(), ["gnome-screenshot", "scrot"]);
    assert_eq!(std::fs::read(out).unwrap(), png_fixture());
}

#[test]
fn missing_first_tool_falls_back_without_running_it() {
    let shims = Shims::new();
    shims.tool("scrot", WRITE_PNG);
    let (result, _) = shims.run(false);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(shims.calls(), ["scrot"]);
}

#[test]
fn scrot_escape_is_a_cancel_and_stops_the_chain() {
    let shims = Shims::new();
    shims.tool(
        "scrot",
        "echo 'scrot: Key was pressed, aborting shot' >&2\necho 'scrot: no image grabbed' >&2\nexit 1",
    );
    shims.tool("import", WRITE_PNG);
    let (result, out) = shims.run(false);
    assert!(matches!(result, Err(CaptureError::Cancelled)), "{result:?}");
    assert_eq!(shims.calls(), ["scrot"]);
    assert!(!out.exists());
}

#[test]
fn successful_exit_without_file_is_not_confirmed_as_cancel_and_stops_the_chain() {
    let shims = Shims::new();
    shims.tool("gnome-screenshot", "exit 0");
    shims.tool("scrot", WRITE_PNG);
    let (result, _) = shims.run(false);
    assert!(
        matches!(result, Err(CaptureError::NoOutput(_))),
        "{result:?}"
    );
    assert_eq!(shims.calls(), ["gnome-screenshot"]);
}

#[test]
fn corrupted_file_is_discarded_and_the_next_candidate_runs() {
    let shims = Shims::new();
    shims.tool("gnome-screenshot", "printf 'not a png' > \"$last\"");
    shims.tool("scrot", WRITE_PNG);
    let (result, out) = shims.run(false);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(shims.calls(), ["gnome-screenshot", "scrot"]);
    assert_eq!(std::fs::read(out).unwrap(), png_fixture());
}

#[test]
fn all_candidates_failing_returns_a_tool_error_naming_each_failure() {
    let shims = Shims::new();
    shims.tool("gnome-screenshot", "echo 'portal gone' >&2\nexit 1");
    shims.tool(
        "scrot",
        "echo \"scrot: Saving to file $last failed\" >&2\nexit 1",
    );
    shims.tool("import", "printf 'half' > \"$last\"\nexit 0");
    let (result, out) = shims.run(false);
    let Err(CaptureError::Tool(error)) = result else {
        panic!("expected a tool error, got {result:?}");
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("gnome-screenshot: exited with"),
        "{message}"
    );
    assert!(message.contains("portal gone"), "{message}");
    assert!(message.contains("Saving to file"), "{message}");
    assert!(message.contains("import:"), "{message}");
    assert_eq!(shims.calls(), ["gnome-screenshot", "scrot", "import"]);
    assert!(!out.exists(), "partial output must be removed");
}

#[test]
fn no_installed_candidate_is_unavailable() {
    let shims = Shims::new();
    let (result, _) = shims.run(false);
    assert!(
        matches!(result, Err(CaptureError::Unavailable(_))),
        "{result:?}"
    );
    let (result, _) = shims.run(true);
    assert!(
        matches!(result, Err(CaptureError::Unavailable(_))),
        "{result:?}"
    );
}

#[test]
fn non_executable_file_is_not_a_candidate() {
    let shims = Shims::new();
    std::fs::write(shims.bin().join("gnome-screenshot"), "#!/bin/sh\nexit 1\n").unwrap();
    shims.tool("scrot", WRITE_PNG);
    let (result, _) = shims.run(false);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(shims.calls(), ["scrot"]);
}

#[test]
fn slurp_without_grim_does_not_open_the_selection_ui() {
    let shims = Shims::new();
    shims.tool("slurp", "echo '0,0 10x10'");
    shims.tool("gnome-screenshot", WRITE_PNG);
    let (result, _) = shims.run(true);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(shims.calls(), ["gnome-screenshot"]);
}

#[test]
fn grim_slurp_success_uses_the_selected_geometry() {
    let shims = Shims::new();
    shims.tool("slurp", "echo '1,2 30x40'");
    shims.tool(
        "grim",
        "[ \"$1\" = -g ] && [ \"$2\" = '1,2 30x40' ] || exit 9\ncp \"$FIXTURE\" \"$last\"",
    );
    shims.tool("gnome-screenshot", "exit 1");
    let (result, _) = shims.run(true);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(shims.calls(), ["slurp", "grim"]);
}

#[test]
fn slurp_cancel_is_a_cancel_and_stops_the_chain() {
    let shims = Shims::new();
    shims.tool("slurp", "echo 'selection cancelled' >&2\nexit 1");
    shims.tool("grim", WRITE_PNG);
    shims.tool("gnome-screenshot", WRITE_PNG);
    let (result, _) = shims.run(true);
    assert!(matches!(result, Err(CaptureError::Cancelled)), "{result:?}");
    assert_eq!(shims.calls(), ["slurp"]);
}

#[test]
fn slurp_backend_failure_falls_back() {
    let shims = Shims::new();
    shims.tool(
        "slurp",
        "echo 'compositor doesn'\"'\"'t support zwlr_layer_shell_v1' >&2\nexit 1",
    );
    shims.tool("grim", WRITE_PNG);
    shims.tool("gnome-screenshot", WRITE_PNG);
    let (result, _) = shims.run(true);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(shims.calls(), ["slurp", "gnome-screenshot"]);
}

#[test]
fn grim_error_after_selection_is_a_failure_that_falls_back() {
    let shims = Shims::new();
    shims.tool("slurp", "echo '0,0 10x10'");
    shims.tool("grim", "echo 'grim: screencopy unsupported' >&2\nexit 1");
    shims.tool("gnome-screenshot", "echo 'no shell' >&2\nexit 1");
    let (result, _) = shims.run(true);
    let Err(CaptureError::Tool(error)) = result else {
        panic!("expected a tool error, got {result:?}");
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("grim+slurp: grim: exited with"),
        "{message}"
    );
    assert!(message.contains("screencopy unsupported"), "{message}");
    assert_eq!(shims.calls(), ["slurp", "grim", "gnome-screenshot"]);
}

#[test]
fn slurp_empty_selection_is_no_output() {
    let shims = Shims::new();
    shims.tool("slurp", "exit 0");
    shims.tool("grim", WRITE_PNG);
    shims.tool("gnome-screenshot", WRITE_PNG);
    let (result, _) = shims.run(true);
    assert!(
        matches!(result, Err(CaptureError::NoOutput(_))),
        "{result:?}"
    );
    assert_eq!(shims.calls(), ["slurp"]);
}
