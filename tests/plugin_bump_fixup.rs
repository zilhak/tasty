//! 합성 Cargo 저장소에서 병합 단계의 플러그인 버전 증가 도구와 pre-commit P.1 의 보류 모드를 확인한다.
//! 도구는 scripts/plugin-bump-fixup.sh, 모드 판정은 scripts/lib/plugin-bump-mode.sh 다.
//! 실제 cargo·rustfmt·git 을 쓰지만 레지스트리 의존이 없어 네트워크에 닿지 않는다. Bash 를 쓰는 Unix 전용 시험이다.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

fn repo_file(rel: &str) -> String {
    format!("{}/{rel}", env!("CARGO_MANIFEST_DIR"))
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} 실행 실패: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} 실패:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    git_out(dir, args);
}

fn write(dir: &Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().expect("부모 경로")).expect("디렉토리 생성");
    fs::write(&p, body).unwrap_or_else(|e| panic!("{rel} 쓰기 실패: {e}"));
}

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{rel} 읽기 실패: {e}"))
}

fn cargo_offline(dir: &Path, args: &[&str]) {
    let out = Command::new("cargo")
        .args(args)
        .arg("--offline")
        .current_dir(dir)
        .output()
        .expect("cargo 실행");
    assert!(
        out.status.success(),
        "cargo {args:?} 실패:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

struct Run {
    code: i32,
    output: String,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rc={} 출력:\n{}", self.code, self.output)
    }
}

fn run_script(dir: &Path, rel: &str, args: &[&str]) -> Run {
    let out = Command::new("bash")
        .arg(repo_file(rel))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("스크립트 실행");
    Run {
        code: out.status.code().unwrap_or(-1),
        output: format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

fn fixup(dir: &Path) -> Run {
    run_script(dir, "scripts/plugin-bump-fixup.sh", &[])
}

fn check_head(dir: &Path) -> Run {
    run_script(
        dir,
        "scripts/check-plugin-version-bump.sh",
        &["--range", "HEAD^", "HEAD"],
    )
}

const PLUGINS: [&str; 2] = ["alpha", "beta"];

fn manifest_of(p: &str) -> String {
    format!("crates/tasty-plugin-{p}/tasty-plugin.toml")
}

fn cargo_toml_of(p: &str) -> String {
    format!("crates/tasty-plugin-{p}/Cargo.toml")
}

fn main_rs_of(p: &str) -> String {
    format!("crates/tasty-plugin-{p}/src/main.rs")
}

/// 플러그인 둘(alpha·beta)이 매니페스트 없는 공유 크레이트 shared 에 path 의존하는 workspace 다.
fn seed() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("임시 디렉토리");
    let d = tmp.path();
    git(d, &["init", "--quiet", "-b", "main"]);
    git(d, &["config", "user.email", "fixture@example.invalid"]);
    git(d, &["config", "user.name", "fixture"]);
    // 사용자 Git 훅이 합성 저장소의 커밋과 amend 에 개입하지 않게 한다.
    git(d, &["config", "core.hooksPath", "/dev/null"]);
    write(
        d,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n\n[workspace.package]\nedition = \"2024\"\n",
    );
    // lane/ 은 아래 P.1 시험이 만드는 연결된 작업 트리 자리다.
    write(d, ".gitignore", "target\nlane/\n");
    for p in PLUGINS {
        write(
            d,
            &cargo_toml_of(p),
            &format!(
                "[package]\nname = \"tasty-plugin-{p}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                 [dependencies]\nshared = {{ path = \"../shared\" }}\n"
            ),
        );
        write(
            d,
            &manifest_of(p),
            &format!("id = \"com.tasty.{p}\"\nversion = \"0.1.0\"\n"),
        );
        write(
            d,
            &main_rs_of(p),
            "fn main() {\n    shared::hi();\n}\n\nfn a() {}\n\n\n\n\nfn b() {}\n",
        );
    }
    write(
        d,
        "crates/shared/Cargo.toml",
        "[package]\nname = \"shared\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(d, "crates/shared/src/lib.rs", "pub fn hi() {}\n");
    cargo_offline(d, &["generate-lockfile"]);
    git(d, &["add", "-A"]);
    git(d, &["commit", "--quiet", "-m", "seed"]);
    tmp
}

fn edit_line(d: &Path, rel: &str, from: &str, to: &str) {
    let body = read(d, rel);
    assert_eq!(body.matches(from).count(), 1, "{rel} 에서 바꿀 줄 {from:?}");
    write(d, rel, &body.replacen(from, to, 1));
}

fn commit_all(d: &Path, msg: &str) {
    git(d, &["commit", "--quiet", "-a", "-m", msg]);
}

fn version_line(d: &Path, rel: &str) -> String {
    read(d, rel)
        .lines()
        .find(|l| l.starts_with("version"))
        .unwrap_or_else(|| panic!("{rel} 에 version 줄이 없다"))
        .to_string()
}

fn assert_versions(d: &Path, p: &str, v: &str) {
    let want = format!("version = \"{v}\"");
    assert_eq!(version_line(d, &cargo_toml_of(p)), want, "{p} Cargo.toml");
    assert_eq!(version_line(d, &manifest_of(p)), want, "{p} 매니페스트");
    let lock = read(d, "Cargo.lock");
    assert!(
        lock.contains(&format!("name = \"tasty-plugin-{p}\"\n{want}\n")),
        "Cargo.lock 의 tasty-plugin-{p} 가 {v} 가 아니다:\n{lock}"
    );
}

fn commit_count(d: &Path) -> String {
    git_out(d, &["rev-list", "--count", "HEAD"])
}

#[test]
fn content_change_gets_its_bump_amended_into_the_same_commit() {
    let tmp = seed();
    let d = tmp.path();
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn a() {}",
        "fn a() { let _a = 1; }",
    );
    commit_all(d, "alpha");
    let before = commit_count(d);

    let r = fixup(d);
    assert_eq!(r.code, 0, "{r:?}");
    assert!(
        r.output.contains("tasty-plugin-alpha 0.1.0 → 0.1.1"),
        "{r:?}"
    );
    assert_versions(d, "alpha", "0.1.1");
    assert_versions(d, "beta", "0.1.0");
    assert_eq!(commit_count(d), before, "새 커밋이 아니라 amend 여야 한다");
    assert_eq!(git_out(d, &["log", "-1", "--format=%s"]), "alpha");
    assert_eq!(
        git_out(d, &["status", "--porcelain"]),
        "",
        "작업 트리가 깨끗해야 한다"
    );
    let changed = git_out(d, &["diff", "--name-only", "HEAD^", "HEAD"]);
    assert_eq!(
        changed.lines().collect::<Vec<_>>(),
        [
            "Cargo.lock",
            "crates/tasty-plugin-alpha/Cargo.toml",
            "crates/tasty-plugin-alpha/src/main.rs",
            "crates/tasty-plugin-alpha/tasty-plugin.toml",
        ]
    );
    let r = check_head(d);
    assert_eq!(r.code, 0, "{r:?}");
}

#[test]
fn commit_without_plugin_content_change_is_left_alone() {
    let tmp = seed();
    let d = tmp.path();
    write(d, "README.md", "notes\n");
    git(d, &["add", "README.md"]);
    commit_all(d, "docs");
    let head = git_out(d, &["rev-parse", "HEAD"]);

    let r = fixup(d);
    assert_eq!(r.code, 0, "{r:?}");
    assert!(r.output.contains("올릴 플러그인 버전이 없다"), "{r:?}");
    assert_eq!(
        git_out(d, &["rev-parse", "HEAD"]),
        head,
        "커밋이 바뀌면 안 된다"
    );
}

#[test]
fn commit_that_already_bumped_is_left_alone() {
    let tmp = seed();
    let d = tmp.path();
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn a() {}",
        "fn a() { let _a = 1; }",
    );
    for rel in [cargo_toml_of("alpha"), manifest_of("alpha")] {
        edit_line(d, &rel, "version = \"0.1.0\"", "version = \"0.1.5\"");
    }
    cargo_offline(d, &["metadata", "--format-version", "1"]);
    commit_all(d, "alpha bumped by hand");
    let head = git_out(d, &["rev-parse", "HEAD"]);

    let r = fixup(d);
    assert_eq!(r.code, 0, "{r:?}");
    assert!(r.output.contains("올릴 플러그인 버전이 없다"), "{r:?}");
    assert_eq!(git_out(d, &["rev-parse", "HEAD"]), head);
    assert_versions(d, "alpha", "0.1.5");
}

#[test]
fn every_changed_plugin_is_bumped_in_one_pass() {
    let tmp = seed();
    let d = tmp.path();
    for p in PLUGINS {
        edit_line(d, &main_rs_of(p), "fn b() {}", "fn b() { let _b = 2; }");
    }
    commit_all(d, "both plugins");

    let r = fixup(d);
    assert_eq!(r.code, 0, "{r:?}");
    for p in PLUGINS {
        assert_versions(d, p, "0.1.1");
    }
    assert!(r.output.contains("버전 증가 2 건"), "{r:?}");
}

#[test]
fn shared_path_dependency_change_bumps_every_plugin_that_links_it() {
    let tmp = seed();
    let d = tmp.path();
    write(
        d,
        "crates/shared/src/lib.rs",
        "pub fn hi() {}\npub fn bye() {}\n",
    );
    commit_all(d, "shared");

    let r = fixup(d);
    assert_eq!(r.code, 0, "{r:?}");
    for p in PLUGINS {
        assert_versions(d, p, "0.1.1");
    }
    let lock_diff = git_out(
        d,
        &["diff", "--numstat", "HEAD^", "HEAD", "--", "Cargo.lock"],
    );
    assert!(
        lock_diff.starts_with("2\t2\t"),
        "Cargo.lock 은 두 패키지의 version 줄만 바뀌어야 한다: {lock_diff}"
    );
}

#[test]
fn undecidable_range_stops_without_touching_the_commit() {
    let tmp = seed();
    let d = tmp.path();
    // 내용을 바꾸면서 버전을 내린 커밋은 검사기가 판정 불가(2)로 끝낸다.
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn a() {}",
        "fn a() { let _a = 1; }",
    );
    for rel in [cargo_toml_of("alpha"), manifest_of("alpha")] {
        edit_line(d, &rel, "version = \"0.1.0\"", "version = \"0.0.9\"");
    }
    cargo_offline(d, &["metadata", "--format-version", "1"]);
    commit_all(d, "alpha downgraded");
    let head = git_out(d, &["rev-parse", "HEAD"]);

    let r = fixup(d);
    assert_eq!(r.code, 2, "{r:?}");
    assert!(r.output.contains("앞 끝이 뒤 끝보다 새것"), "{r:?}");
    assert!(r.output.contains("버전 검사가 종료 코드"), "{r:?}");
    assert!(r.output.contains("종료 코드 2 로 끝났다"), "{r:?}");
    assert!(r.output.contains("판정 불가"), "{r:?}");
    assert_eq!(git_out(d, &["rev-parse", "HEAD"]), head);
    assert_eq!(git_out(d, &["status", "--porcelain"]), "");
}

#[test]
fn uncommitted_tracked_change_stops_before_amending() {
    let tmp = seed();
    let d = tmp.path();
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn a() {}",
        "fn a() { let _a = 1; }",
    );
    commit_all(d, "alpha");
    let head = git_out(d, &["rev-parse", "HEAD"]);
    edit_line(
        d,
        &main_rs_of("beta"),
        "fn a() {}",
        "fn a() { let _x = 9; }",
    );

    let r = fixup(d);
    assert_eq!(r.code, 2, "{r:?}");
    assert!(r.output.contains("커밋하지 않은 추적 파일 변경"), "{r:?}");
    assert_eq!(git_out(d, &["rev-parse", "HEAD"]), head);
    assert_versions(d, "alpha", "0.1.0");
}

/// 두 lane 이 같은 플러그인을 바꾸고 버전 줄은 건드리지 않는다. 병합 단계가 rebase --exec 로 커밋마다 붙인다.
#[test]
fn lanes_that_leave_versions_alone_rebase_without_conflict() {
    let tmp = seed();
    let d = tmp.path();
    let exec = format!("bash {}", repo_file("scripts/plugin-bump-fixup.sh"));

    git(d, &["checkout", "--quiet", "-b", "lane-a", "main"]);
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn a() {}",
        "fn a() { let _a = 1; }",
    );
    commit_all(d, "A: alpha");
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn main() {",
        "fn main() { let _a2 = 3;",
    );
    commit_all(d, "A: alpha again");

    git(d, &["checkout", "--quiet", "-b", "lane-b", "main"]);
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn b() {}",
        "fn b() { let _b = 2; }",
    );
    write(
        d,
        "crates/shared/src/lib.rs",
        "pub fn hi() {}\npub fn bye() {}\n",
    );
    commit_all(d, "B: alpha and shared");

    for lane in ["lane-a", "lane-b"] {
        git(d, &["checkout", "--quiet", lane]);
        git(d, &["rebase", "--quiet", "--exec", &exec, "main"]);
        git(d, &["checkout", "--quiet", "main"]);
        git(d, &["merge", "--quiet", "--ff-only", lane]);
    }

    assert_versions(d, "alpha", "0.1.3");
    assert_versions(d, "beta", "0.1.1");
    let landed = git_out(d, &["rev-list", "--reverse", "main~3..main"]);
    assert_eq!(landed.lines().count(), 3);
    for c in landed.lines() {
        let r = run_script(
            d,
            "scripts/check-plugin-version-bump.sh",
            &["--range", &format!("{c}^"), c],
        );
        assert_eq!(r.code, 0, "착지한 커밋 {c}: {r:?}");
        assert!(r.output.contains("통과 — 판정 대상"), "{c}: {r:?}");
    }
    cargo_offline(d, &["metadata", "--locked", "--format-version", "1"]);
}

// ── 시작 조건이 맞지 않으면 아무것도 바꾸지 않고 판정 불가(2)로 멈춘다 ──────────────

fn assert_refused(r: &Run, words: &str) {
    assert_eq!(r.code, 2, "{r:?}");
    assert!(r.output.contains(words), "{words:?} 가 없다: {r:?}");
}

#[test]
fn fixup_takes_no_arguments() {
    let tmp = seed();
    let r = run_script(tmp.path(), "scripts/plugin-bump-fixup.sh", &["HEAD~1"]);
    assert_refused(&r, "인자를 받지 않는다");
}

#[test]
fn fixup_refuses_a_merge_commit() {
    let tmp = seed();
    let d = tmp.path();
    git(d, &["checkout", "--quiet", "-b", "side"]);
    edit_line(
        d,
        &main_rs_of("alpha"),
        "fn a() {}",
        "fn a() { let _s = 1; }",
    );
    commit_all(d, "side");
    git(d, &["checkout", "--quiet", "main"]);
    edit_line(
        d,
        &main_rs_of("beta"),
        "fn a() {}",
        "fn a() { let _m = 1; }",
    );
    commit_all(d, "main");
    git(d, &["merge", "--quiet", "--no-ff", "--no-edit", "side"]);
    let head = git_out(d, &["rev-parse", "HEAD"]);

    assert_refused(&fixup(d), "merge 커밋은 다루지 않는다");
    assert_eq!(git_out(d, &["rev-parse", "HEAD"]), head);
}

#[test]
fn fixup_refuses_a_repository_without_commits() {
    let tmp = tempfile::tempdir().expect("임시 디렉토리");
    git(tmp.path(), &["init", "--quiet"]);
    assert_refused(&fixup(tmp.path()), "HEAD 커밋이 없다");
}

#[test]
fn fixup_refuses_outside_a_repository_and_in_a_bare_one() {
    let plain = tempfile::tempdir().expect("임시 디렉토리");
    assert_refused(&fixup(plain.path()), "git 저장소 안에서 실행해라");

    let bare = tempfile::tempdir().expect("임시 디렉토리");
    git(bare.path(), &["init", "--quiet", "--bare"]);
    assert_refused(&fixup(bare.path()), "작업 트리 루트로 옮겨 가지 못했다");
}

#[test]
fn fixup_refuses_when_the_checker_is_not_beside_it() {
    let tmp = seed();
    let lone = tempfile::tempdir().expect("임시 디렉토리");
    let copy = lone.path().join("plugin-bump-fixup.sh");
    fs::copy(repo_file("scripts/plugin-bump-fixup.sh"), &copy).expect("사본");
    let out = Command::new("bash")
        .arg(&copy)
        .current_dir(tmp.path())
        .output()
        .expect("사본 실행");
    let r = Run {
        code: out.status.code().unwrap_or(-1),
        output: String::from_utf8_lossy(&out.stderr).into_owned(),
    };
    assert_refused(&r, "검사 스크립트가 없다");
}

#[test]
fn checker_refuses_a_missing_or_unwritable_violations_file() {
    let tmp = seed();
    let d = tmp.path();
    let check = "scripts/check-plugin-version-bump.sh";
    let r = run_script(d, check, &["--range", "HEAD", "HEAD", "--violations-out"]);
    assert_refused(&r, "violations-out 에 파일 경로가 없다");

    let r = run_script(
        d,
        check,
        &[
            "--range",
            "HEAD",
            "HEAD",
            "--violations-out",
            "no/such/dir/list",
        ],
    );
    assert_refused(&r, "violations-out 파일을 쓸 수 없다");
}

#[test]
fn checker_writes_the_violation_list_relative_to_the_caller() {
    let tmp = seed();
    let d = tmp.path();
    edit_line(
        d,
        &main_rs_of("beta"),
        "fn a() {}",
        "fn a() { let _v = 1; }",
    );
    commit_all(d, "beta");
    let sub = d.join("crates");
    let r = run_script(
        &sub,
        "scripts/check-plugin-version-bump.sh",
        &["--range", "HEAD^", "HEAD", "--violations-out", "list.txt"],
    );
    assert_eq!(r.code, 1, "{r:?}");
    assert_eq!(read(&sub, "list.txt"), "crates/tasty-plugin-beta\n");
}

// ── P.1 보류 모드 ───────────────────────────────────────────────

fn mode(dir: &Path) -> Run {
    let out = Command::new("bash")
        .arg("-c")
        .arg(format!(
            ". '{}' && plugin_bump_mode",
            repo_file("scripts/lib/plugin-bump-mode.sh")
        ))
        .current_dir(dir)
        .output()
        .expect("모드 판정 실행");
    Run {
        code: out.status.code().unwrap_or(-1),
        output: format!(
            "{}|{}",
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

/// 주 저장소와 연결된 작업 트리 하나를 만든다. 반환한 경로가 연결된 작업 트리다.
fn with_linked_worktree(d: &Path) -> std::path::PathBuf {
    let wt = d.join("lane");
    git(
        d,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "lane",
            wt.to_str().expect("UTF-8 경로"),
        ],
    );
    git(d, &["config", "extensions.worktreeConfig", "true"]);
    wt
}

#[test]
fn mode_defers_only_for_worktree_scope_in_a_linked_worktree() {
    let tmp = seed();
    let d = tmp.path();
    let wt = with_linked_worktree(d);

    // 설정이 없으면 두 곳 모두 강제한다.
    for dir in [d, wt.as_path()] {
        let r = mode(dir);
        assert!(r.output.starts_with("enforced|"), "{r:?}");
    }

    git(
        &wt,
        &["config", "--worktree", "tasty.pluginBump", "deferred"],
    );
    let r = mode(&wt);
    assert_eq!(r.code, 0, "{r:?}");
    assert!(r.output.starts_with("deferred|"), "{r:?}");
    // 연결된 작업 트리의 설정은 주 작업 트리로 새지 않는다.
    let r = mode(d);
    assert!(r.output.starts_with("enforced|"), "{r:?}");
}

#[test]
fn mode_refuses_shared_scope_main_worktree_and_unknown_values() {
    let tmp = seed();
    let d = tmp.path();
    let wt = with_linked_worktree(d);

    // 공유 설정(.git/config)에 둔 값은 모든 작업 트리에 보이지만 따르지 않는다.
    git(d, &["config", "--local", "tasty.pluginBump", "deferred"]);
    for dir in [d, wt.as_path()] {
        let r = mode(dir);
        assert!(r.output.starts_with("enforced|"), "{r:?}");
        assert!(r.output.contains("worktree 범위"), "{r:?}");
    }
    git(d, &["config", "--local", "--unset", "tasty.pluginBump"]);

    // 주 작업 트리의 config.worktree 에 둔 값도 따르지 않는다.
    git(d, &["config", "--worktree", "tasty.pluginBump", "deferred"]);
    let r = mode(d);
    assert!(r.output.starts_with("enforced|"), "{r:?}");
    assert!(r.output.contains("주 작업 트리"), "{r:?}");

    git(&wt, &["config", "--worktree", "tasty.pluginBump", "later"]);
    let r = mode(&wt);
    assert!(r.output.starts_with("enforced|"), "{r:?}");
    assert!(r.output.contains("알 수 없는 값"), "{r:?}");
}

/// 실제 pre-commit 을 돌리되 bash·cargo 를 BASH_ENV 의 함수로 바꿔 호출만 기록한다.
fn run_pre_commit(dir: &Path, version_rc: i32) -> (Run, String) {
    let stub_dir = tempfile::tempdir().expect("임시 디렉토리");
    let calls = stub_dir.path().join("calls");
    let env_file = stub_dir.path().join("stubs.sh");
    fs::write(
        &env_file,
        r#"
bash() {
    printf '%s\n' "$*" >> "$HOOK_CALLS"
    case "$1" in
        *check-plugin-version-bump.sh) return "$HOOK_VERSION_RC" ;;
        *) return 0 ;;
    esac
}
cargo() {
    printf 'cargo %s\n' "$*" >> "$HOOK_CALLS"
    return 0
}
"#,
    )
    .expect("스텁 쓰기");
    let out = Command::new("bash")
        .arg(repo_file(".githooks/pre-commit"))
        .current_dir(dir)
        .env("BASH_ENV", &env_file)
        .env("HOOK_CALLS", &calls)
        .env("HOOK_VERSION_RC", version_rc.to_string())
        .output()
        .expect("pre-commit 실행");
    let run = Run {
        code: out.status.code().unwrap_or(-1),
        output: format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    };
    (run, fs::read_to_string(&calls).unwrap_or_default())
}

/// 훅은 저장소 루트의 scripts/lib 에서 모드 판정을 읽으므로 합성 저장소에 사본을 둔다.
fn stage_plugin_change_with_mode_lib(dir: &Path) {
    let lib = fs::read_to_string(repo_file("scripts/lib/plugin-bump-mode.sh")).expect("판정 파일");
    write(dir, "scripts/lib/plugin-bump-mode.sh", &lib);
    write(
        dir,
        "crates/tasty-plugin-alpha/assets/note.txt",
        "changed asset\n",
    );
    git(dir, &["add", "-A"]);
}

#[test]
fn pre_commit_skips_p1_in_a_deferred_lane_worktree() {
    let tmp = seed();
    let d = tmp.path();
    let wt = with_linked_worktree(d);
    git(
        &wt,
        &["config", "--worktree", "tasty.pluginBump", "deferred"],
    );
    stage_plugin_change_with_mode_lib(&wt);

    // 검사기가 위반(1)을 낼 상황이어도 보류 모드에서는 호출하지 않는다.
    let (r, calls) = run_pre_commit(&wt, 1);
    assert_eq!(r.code, 0, "{r:?}\ncalls:\n{calls}");
    assert!(
        !calls.contains("check-plugin-version-bump.sh --staged"),
        "{calls}"
    );
    assert!(r.output.contains("tasty.pluginBump=deferred"), "{r:?}");
}

#[test]
fn pre_commit_still_enforces_p1_without_the_worktree_setting() {
    let tmp = seed();
    let d = tmp.path();
    let wt = with_linked_worktree(d);
    for dir in [d, wt.as_path()] {
        stage_plugin_change_with_mode_lib(dir);
        let (r, calls) = run_pre_commit(dir, 1);
        assert_eq!(r.code, 1, "{r:?}\ncalls:\n{calls}");
        assert!(
            calls.contains("check-plugin-version-bump.sh --staged"),
            "{calls}"
        );
        assert!(r.output.contains("[P.1]"), "{r:?}");

        let (r, calls) = run_pre_commit(dir, 0);
        assert_eq!(r.code, 0, "{r:?}\ncalls:\n{calls}");
        assert!(
            calls.contains("check-plugin-version-bump.sh --staged"),
            "{calls}"
        );
    }
}

#[test]
fn pre_commit_enforces_p1_when_shared_config_asks_for_deferral() {
    let tmp = seed();
    let d = tmp.path();
    let wt = with_linked_worktree(d);
    git(d, &["config", "--local", "tasty.pluginBump", "deferred"]);
    stage_plugin_change_with_mode_lib(&wt);

    let (r, calls) = run_pre_commit(&wt, 1);
    assert_eq!(r.code, 1, "{r:?}\ncalls:\n{calls}");
    assert!(
        calls.contains("check-plugin-version-bump.sh --staged"),
        "{calls}"
    );
}
