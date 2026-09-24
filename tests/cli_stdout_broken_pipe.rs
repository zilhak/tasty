//! CLI의 stdout·stderr 파이프를 일찍 닫아도 panic과 crash report가 생기지 않는지 확인한다.
//! stdout은 정상 종료, stderr는 같은 명령의 정상 오류 코드를 유지해야 한다(ADR-0043).
//! 서버가 필요 없는 명령과 임시 TASTY_HOME을 사용하며 출력 매크로의 우회도 소스에서 검사한다.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

fn tasty(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tasty"));
    cmd.env("TASTY_HOME", home)
        // 부모 인스턴스의 surface 정보 때문에 augmented help로 전환되지 않도록 제거한다.
        .env_remove("TASTY_SURFACE_ID")
        .env_remove("TASTY_SESSION_TOKEN")
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    cmd
}

/// 자식을 띄운 직후 stdout 읽기 끝을 닫는다. 첫 쓰기 전후의 정확한 실행 순서는 스케줄링에 따라 달라질 수 있다.
fn run_with_closed_stdout(mut cmd: Command) -> (ExitStatus, String) {
    let mut child = cmd.stdout(Stdio::piped()).spawn().expect("spawn tasty");
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("wait tasty");
    (
        output.status,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// 같은 명령이 열린 stdout에 실제로 출력하는지도 확인해 출력 없는 경로와 구별한다.
fn run_with_open_stdout(mut cmd: Command) -> (ExitStatus, String) {
    let output = cmd.stdout(Stdio::piped()).output().expect("run tasty");
    (
        output.status,
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

fn crash_report_count(home: &Path) -> usize {
    std::fs::read_dir(home.join("crash-reports"))
        .map(|d| d.count())
        .unwrap_or(0)
}

fn assert_quiet_exit_zero(label: &str, home: &Path, status: ExitStatus, stderr: &str) {
    assert_eq!(
        status.code(),
        Some(0),
        "{label}: 종료 코드가 0 이 아님 ({status}); stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("Tasty crashed") && !stderr.contains("panicked"),
        "{label}: panic 흔적이 stderr 에 있음:\n{stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("broken pipe"),
        "{label}: EPIPE 가 에러로 노출됨:\n{stderr}"
    );
    assert_eq!(
        crash_report_count(home),
        0,
        "{label}: crash report 가 생성됨 ({})",
        home.join("crash-reports").display()
    );
}

fn check_command(label: &str, args: &[&str]) {
    let home = tempfile::tempdir().expect("tempdir");
    let home = home.path();

    let (status, stdout) = run_with_open_stdout({
        let mut c = tasty(home);
        c.args(args);
        c
    });
    assert_eq!(
        status.code(),
        Some(0),
        "{label}: 대조군(stdout 열림) 종료 코드"
    );
    assert!(
        !stdout.trim().is_empty(),
        "{label}: 대조군이 stdout 에 아무것도 쓰지 않음 — EPIPE 시나리오가 성립하지 않는다"
    );

    let (status, stderr) = run_with_closed_stdout({
        let mut c = tasty(home);
        c.args(args);
        c
    });
    assert_quiet_exit_zero(label, home, status, &stderr);
}

#[test]
fn subcommand_output_with_closed_stdout_exits_zero() {
    check_command(
        "tool remote-profile list",
        &["tool", "remote-profile", "list"],
    );
}

#[test]
fn command_tree_with_closed_stdout_exits_zero() {
    check_command("-a", &["-a"]);
}

#[test]
fn root_help_with_closed_stdout_exits_zero() {
    check_command("--help", &["--help"]);
}

/// clap 자체의 help 종료 경로도 EPIPE를 처리해야 한다.
#[test]
fn subcommand_help_with_closed_stdout_exits_zero() {
    check_command("list --help", &["list", "--help"]);
}

fn run_with_closed_stderr(home: &Path, args: &[&str]) -> ExitStatus {
    let mut cmd = tasty(home);
    cmd.args(args).stdout(Stdio::null());
    let mut child = cmd.spawn().expect("spawn tasty");
    drop(child.stderr.take());
    child.wait().expect("wait tasty")
}

fn check_stderr_command(label: &str, args: &[&str]) {
    let home = tempfile::tempdir().expect("tempdir");
    let home = home.path();

    let control = tasty(home)
        .args(args)
        .stdout(Stdio::null())
        .output()
        .expect("run tasty");
    let control_stderr = String::from_utf8_lossy(&control.stderr);
    assert!(
        !control_stderr.trim().is_empty(),
        "{label}: 대조군이 stderr 에 아무것도 쓰지 않음 — EPIPE 시나리오가 성립하지 않는다"
    );
    assert_ne!(
        control.status.code(),
        Some(0),
        "{label}: 대조군이 실패 경로가 아니다"
    );

    let status = run_with_closed_stderr(home, args);
    assert_eq!(
        status.code(),
        control.status.code(),
        "{label}: stderr 가 닫히자 종료 코드가 바뀜 ({status}) — panic 이면 101 · abort 면 신호"
    );
    assert_eq!(
        crash_report_count(home),
        0,
        "{label}: crash report 가 생성됨 ({})",
        home.join("crash-reports").display()
    );
}

#[test]
fn parse_error_with_closed_stderr_keeps_its_exit_code() {
    check_stderr_command("unknown subcommand", &["nosuchcmd"]);
    check_stderr_command(
        "missing required argument",
        &["read", "since-mark", "--surface", "1", "--cursor", "5"],
    );
}

#[test]
fn unreachable_host_with_closed_stderr_keeps_its_exit_code() {
    check_stderr_command("no port file", &["list", "surfaces"]);
}

#[test]
fn cli_crate_has_no_direct_stdout_print() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/tasty-cli/src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(
        !files.is_empty(),
        "tasty-cli 소스를 찾지 못함: {}",
        root.display()
    );

    let mut offenders = Vec::new();
    for path in files {
        if path.file_name().is_some_and(|n| n == "out.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        for (i, line) in src.lines().enumerate() {
            let code = strip_comment_and_strings(line);
            if invokes_macro(&code, "println") || invokes_macro(&code, "print") {
                offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "tasty-cli 에 std 의 stdout 출력 매크로가 있음 — `crate::out::{{outln, out}}` 을 쓴다:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn cli_crate_has_no_direct_stderr_print() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/tasty-cli/src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(
        !files.is_empty(),
        "tasty-cli 소스를 찾지 못함: {}",
        root.display()
    );

    let mut offenders = Vec::new();
    for path in files {
        if path.file_name().is_some_and(|n| n == "out.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        for (i, line) in src.lines().enumerate() {
            let code = strip_comment_and_strings(line);
            if invokes_macro(&code, "eprintln") || invokes_macro(&code, "eprint") {
                offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "tasty-cli 에 std 의 stderr 출력 매크로가 있음 — `crate::out::errln` 을 쓴다:\n{}",
        offenders.join("\n")
    );
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 줄의 첫 // 뒤와 일반 문자열 내용을 제외한다. 문자열 속 //·raw 문자열 등은 완전히 해석하지 않는다.
fn strip_comment_and_strings(line: &str) -> String {
    let code = line.split("//").next().unwrap_or("");
    let mut out = String::new();
    let mut in_str = false;
    let mut esc = false;
    for c in code.chars() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// 매크로 이름 전체를 비교한다. 이름과 !를 따로 받아 시험 코드가 원시 출력 매크로 검사에 걸리지 않도록 한다.
use tasty_doc_guards::source_text::invokes_macro;
