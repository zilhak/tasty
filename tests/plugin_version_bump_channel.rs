//! `scripts/check-plugin-version-bump.sh` 의 판정을 **합성 git 저장소**로 양극성 고정한다.
//!
//! 왜 합성 저장소인가: 이 저장소 **자신의** 이력을 읽는 테스트는 배포 tarball(비-git),
//! shallow clone, 첫 커밋에서 판정 불가가 되고, 그 셋이 CI 에서 조용히 통과로 세어질
//! 위험이 크다. 여기서는 매번 저장소를 만들어 쓰므로 그 셋에 의존하지 않는다.
//!
//! **한 극성만 보면 항등식이다.** "bump 안 한 사본에서 FAIL" 만 보면 항상 FAIL 하는
//! 게이트도 통과하고, "bump 한 사본에서 PASS" 만 보면 아무것도 안 하는 게이트도
//! 통과한다. 그래서 최소 네 극성을 본다 — 부채(FAIL) · bump(PASS) · 포맷만(PASS) ·
//! 판정 불가(exit 2, 통과 아님).
//!
//! 판별식의 근거·측정·대안은 `docs/adr/0137-plugin-version-bump-is-judged-by-content-not-file-count.md`.

use std::fs;
use std::path::Path;
use std::process::Command;

fn script() -> String {
    format!(
        "{}/scripts/check-plugin-version-bump.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn run_git(dir: &Path, args: &[&str]) {
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
}

fn write(dir: &Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().expect("부모 경로")).expect("디렉토리 생성");
    fs::write(&p, body).unwrap_or_else(|e| panic!("{rel} 쓰기 실패: {e}"));
}

/// 스크립트를 돌리고 (exit code, stdout+stderr) 를 돌려준다.
fn check(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new("bash")
        .arg(script())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("게이트 스크립트 실행");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

const PLUGIN: &str = "crates/tasty-plugin-fixture";

/// 루트 Cargo.toml(스크립트가 edition 을 읽는다) + plugin 한 벌을 담은 저장소를 만들고
/// 첫 커밋까지 마친다.
fn seed_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("임시 디렉토리");
    let d = tmp.path();
    run_git(d, &["init", "--quiet"]);
    run_git(d, &["config", "user.email", "fixture@example.invalid"]);
    run_git(d, &["config", "user.name", "fixture"]);
    // 훅이 이 저장소에 끼어들면 판정이 아니라 훅을 재게 된다.
    run_git(d, &["config", "core.hooksPath", "/dev/null"]);

    write(
        d,
        "Cargo.toml",
        "[workspace]\n[package]\nedition = \"2024\"\n",
    );
    write(
        d,
        &format!("{PLUGIN}/Cargo.toml"),
        "[package]\nname = \"tasty-plugin-fixture\"\nversion = \"0.1.0\"\n",
    );
    write(
        d,
        &format!("{PLUGIN}/tasty-plugin.toml"),
        "id = \"com.tasty.fixture\"\nversion = \"0.1.0\"\n",
    );
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    let msg = \"a  b\";\n}\n",
    );
    run_git(d, &["add", "-A"]);
    run_git(d, &["commit", "--quiet", "-m", "seed"]);
    tmp
}

fn bump_to(d: &Path, v: &str) {
    write(
        d,
        &format!("{PLUGIN}/Cargo.toml"),
        &format!("[package]\nname = \"tasty-plugin-fixture\"\nversion = \"{v}\"\n"),
    );
    write(
        d,
        &format!("{PLUGIN}/tasty-plugin.toml"),
        &format!("id = \"com.tasty.fixture\"\nversion = \"{v}\"\n"),
    );
}

fn commit_all(d: &Path, msg: &str) {
    run_git(d, &["add", "-A"]);
    run_git(d, &["commit", "--quiet", "-m", msg]);
}

#[test]
fn a_content_change_without_a_bump_is_rejected() {
    let tmp = seed_repo();
    let d = tmp.path();
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    let msg = \"changed\";\n}\n",
    );
    commit_all(d, "feat(fixture): change behaviour");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(code, 1, "부채인데 통과했다:\n{text}");
    assert!(
        text.contains("version 이 안 올랐다"),
        "위반 사유가 메시지에 없다:\n{text}"
    );
    assert!(
        text.contains("src/main.rs"),
        "어느 파일 때문인지 메시지에 없다:\n{text}"
    );
}

#[test]
fn the_same_change_with_a_bump_passes() {
    let tmp = seed_repo();
    let d = tmp.path();
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    let msg = \"changed\";\n}\n",
    );
    bump_to(d, "0.1.1");
    commit_all(d, "feat(fixture): change behaviour + bump");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(code, 0, "bump 했는데 막혔다:\n{text}");
    // **통과가 무동작과 구분되어야 한다** — 판정 대상이 0 이면 게이트가 죽어도 초록이다.
    assert!(
        text.contains("판정 대상 1 건"),
        "판정 대상 건수가 1 이 아니다(게이트가 이 변경을 아예 안 봤을 수 있다):\n{text}"
    );
}

#[test]
fn a_formatting_only_change_needs_no_bump() {
    let tmp = seed_repo();
    let d = tmp.path();
    // rustfmt 가 되돌릴 수 있는 차이만 준다. 문자열 리터럴 안의 두 칸 공백은 그대로 둔다 —
    // 공백을 통째로 제거하는 정규화였다면 여기서 리터럴 차이까지 삼켰을 자리다.
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn  main( )\n{\n        let  msg =   \"a  b\" ;\n}\n",
    );
    commit_all(d, "style(fixture): reformat");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(code, 0, "포맷만 바뀌었는데 bump 를 요구했다:\n{text}");
    assert!(
        text.contains("판정 대상 0 건"),
        "포맷 변경이 판정 대상으로 세어졌다:\n{text}"
    );
    // 비영 대조: 변경 자체는 있었다. 0 건이 "아무것도 안 바뀌었다" 가 아님을 고정한다.
    assert!(
        text.contains("변경된 crates 파일 1 개"),
        "변경 파일 수가 안 찍힌다 — 0 건이 무변경인지 배제인지 구분되지 않는다:\n{text}"
    );
}

#[test]
fn a_literal_only_change_is_not_swallowed_by_normalization() {
    let tmp = seed_repo();
    let d = tmp.path();
    // 공백 제거 정규화였다면 `"a  b"` 와 `"ab"` 가 같아져 **거짓 음성**이 됐을 변경.
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    let msg = \"ab\";\n}\n",
    );
    commit_all(d, "fix(fixture): collapse the literal");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(
        code, 1,
        "문자열 리터럴 안의 공백 변화가 정규화에 삼켜졌다 — 거짓 음성이다:\n{text}"
    );
}

#[test]
fn a_docs_only_file_outside_the_build_output_needs_no_bump() {
    let tmp = seed_repo();
    let d = tmp.path();
    write(d, &format!("{PLUGIN}/README.md"), "설명이 늘었다\n");
    commit_all(d, "docs(fixture): add a readme");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(code, 0, "산출물 밖 파일에 bump 를 요구했다:\n{text}");
    assert!(text.contains("판정 대상 0 건"), "{text}");
}

#[test]
fn staged_mode_sees_the_index_before_a_commit_exists_for_it() {
    let tmp = seed_repo();
    let d = tmp.path();
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    let msg = \"staged\";\n}\n",
    );
    run_git(d, &["add", "-A"]);

    let (code, text) = check(d, &["--staged"]);
    assert_eq!(code, 1, "staged 부채를 못 봤다:\n{text}");

    bump_to(d, "0.1.1");
    run_git(d, &["add", "-A"]);
    let (code, text) = check(d, &["--staged"]);
    assert_eq!(code, 0, "staged 에서 bump 했는데 막혔다:\n{text}");
    assert!(text.contains("판정 대상 1 건"), "{text}");
}

#[test]
fn a_new_plugin_has_nothing_to_bump_from() {
    let tmp = seed_repo();
    let d = tmp.path();
    let other = "crates/tasty-plugin-newcomer";
    write(
        d,
        &format!("{other}/Cargo.toml"),
        "[package]\nname = \"tasty-plugin-newcomer\"\nversion = \"0.1.0\"\n",
    );
    write(
        d,
        &format!("{other}/tasty-plugin.toml"),
        "id = \"com.tasty.newcomer\"\nversion = \"0.1.0\"\n",
    );
    write(d, &format!("{other}/src/main.rs"), "fn main() {}\n");
    commit_all(d, "feat(newcomer): add a plugin");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(
        code, 0,
        "새 plugin 의 최초 버전에 bump 를 요구했다:\n{text}"
    );
}

#[test]
fn a_version_that_goes_down_is_rejected() {
    let tmp = seed_repo();
    let d = tmp.path();
    bump_to(d, "0.1.5");
    commit_all(d, "chore(fixture): move to 0.1.5");
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    let msg = \"down\";\n}\n",
    );
    bump_to(d, "0.1.4");
    commit_all(d, "feat(fixture): change with a lower version");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(code, 1, "version 이 내려갔는데 통과했다:\n{text}");
}

#[test]
fn a_version_change_alone_is_not_evidence_that_the_artifact_changed() {
    // 되돌림의 형태: 내용은 그대로 두고 값만 되돌린다. version 줄을 내용 증거로 세면
    // 그 줄 하나가 "달라졌다" 를 만들고 게이트는 되돌림에 또 한 번의 bump 를 요구한다 —
    // 올리면 되돌림이 아니다. 분할 착지에서 병합하는 쪽이 최종 값을 다시 정하는 흐름은
    // 규칙이 정상으로 규정한 것이라, 이 순환은 예외 상황이 아니다.
    let tmp = seed_repo();
    let d = tmp.path();
    bump_to(d, "0.1.5");
    commit_all(d, "chore(fixture): move to 0.1.5");
    bump_to(d, "0.1.4");
    commit_all(d, "revert(fixture): put the value back");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(code, 0, "값만 되돌렸는데 또 올리라고 했다:\n{text}");
    assert!(
        text.contains("판정 대상 0 건"),
        "version 줄 자신이 판정 대상을 만들었다:\n{text}"
    );
}

#[test]
fn the_rest_of_the_manifest_is_still_content() {
    // 위 고침이 뺀 것은 **version 줄 한 줄**이다. `Cargo.toml` 을 통째로 뺐다면 feature·
    // 의존 변경이 산출물을 바꾸고도 조용히 통과한다 — 그 방향을 여기서 막는다.
    let tmp = seed_repo();
    let d = tmp.path();
    write(
        d,
        &format!("{PLUGIN}/Cargo.toml"),
        "[package]\nname = \"tasty-plugin-fixture\"\nversion = \"0.1.0\"\n\n\
         [features]\ndefault = [\"extra\"]\nextra = []\n",
    );
    commit_all(d, "feat(fixture): turn on a feature");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(
        code, 1,
        "version 줄 말고 다른 줄이 바뀌었는데 통과했다:\n{text}"
    );
    assert!(
        text.contains("Cargo.toml"),
        "어느 파일 때문인지 메시지에 없다:\n{text}"
    );
}

#[test]
fn a_non_git_directory_is_undecidable_not_a_pass() {
    let tmp = tempfile::tempdir().expect("임시 디렉토리");
    let (code, text) = check(tmp.path(), &["--range", "HEAD^", "HEAD"]);
    assert_eq!(
        code, 2,
        "비-git 환경에서 판정 불가가 아니라 다른 코드를 냈다 — 0 이면 판정 불가를 \
         통과로 세는 것이다:\n{text}"
    );
    assert!(text.contains("판정 불가"), "{text}");
}

#[test]
fn a_missing_rev_is_undecidable_not_a_pass() {
    let tmp = seed_repo();
    let (code, text) = check(tmp.path(), &["--range", "deadbeefdeadbeef", "HEAD"]);
    assert_eq!(
        code, 2,
        "없는 rev(shallow clone 의 형태)에서 판정 불가가 아니다:\n{text}"
    );
    assert!(
        text.contains("shallow"),
        "shallow 단서가 메시지에 없다:\n{text}"
    );
}

#[test]
fn the_first_commit_has_no_baseline_and_says_so() {
    let tmp = tempfile::tempdir().expect("임시 디렉토리");
    let d = tmp.path();
    run_git(d, &["init", "--quiet"]);
    run_git(d, &["config", "user.email", "fixture@example.invalid"]);
    run_git(d, &["config", "user.name", "fixture"]);
    write(
        d,
        "Cargo.toml",
        "[workspace]\n[package]\nedition = \"2024\"\n",
    );
    write(
        d,
        &format!("{PLUGIN}/tasty-plugin.toml"),
        "id = \"com.tasty.fixture\"\nversion = \"0.1.0\"\n",
    );
    run_git(d, &["add", "-A"]);

    let (code, text) = check(d, &["--staged"]);
    assert_eq!(code, 0, "첫 커밋에서 막혔다:\n{text}");
    assert!(
        text.contains("첫 커밋"),
        "첫 커밋이라는 것이 출력에 안 나온다 — 조용한 통과와 구분되지 않는다:\n{text}"
    );
}

// ── 판정기의 신선도 ───────────────────────────────────────────────────
//
// 출하 범위를 좁히는 판정기(`strip-cfg-test`)는 **빌드 산출물**이다. 그래서 그것을 고친
// 사람과 판정을 돌리는 사람이 다르면 고침이 소스에 있는데 판정은 옛 규칙으로 돈다 —
// 그 오진은 조용하다(실측으로 밟았다). 없을 때와 같은 방향(좁히기를 끄고 넓게)으로
// 처리하되, **말은 한다.**
//
// 판정은 **판정기 자신에게 묻는다**(`--check-fresh`). mtime 으로 재던 판을 버린 이유는
// git 이 파일을 다시 쓰기만 해도 낡은 것으로 나왔기 때문이다 — 내용이 같아도 그렇고,
// 이 저장소의 표준 브랜치 왕복이 정확히 그것을 만든다. 아래 둘은 그 **계약**(종료코드로
// 답한다)을 고정한다; 지문 계산 자체는 `tasty_doc_guards::freshness` 의 단위 테스트가 본다.

/// `<repo>/target/debug/strip-cfg-test` 에 껍데기를 놓는다. `--check-fresh` 에 무엇으로
/// 답할지는 호출자가 정한다.
fn plant_judge(d: &Path, fresh: bool) {
    let bin = d.join("target/debug/strip-cfg-test");
    fs::create_dir_all(bin.parent().expect("부모")).expect("target 디렉토리");
    let body = if fresh {
        "#!/bin/sh\nexit 0\n"
    } else {
        "#!/bin/sh\n[ \"$1\" = \"--check-fresh\" ] && exit 1\nexit 0\n"
    };
    fs::write(&bin, body).expect("껍데기 판정기");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("실행 권한");
    }
}

#[test]
fn a_judge_that_reports_itself_stale_is_announced_and_widens() {
    let tmp = seed_repo();
    let d = tmp.path();
    plant_judge(d, false);
    let (_, text) = check(d, &["--staged"]);
    assert!(
        text.contains("지금 소스로 지어진 것이 아니다"),
        "낡은 판정기가 조용히 쓰였다 — 옛 규칙으로 돈 판정을 아무도 못 본다:\n{text}"
    );
    assert!(
        text.contains("cargo build -p tasty-doc-guards"),
        "무엇을 하라는지가 없다:\n{text}"
    );
}

/// 반대 방향. 이것이 없으면 "항상 낡았다고 말하는" 검사도 위 테스트를 통과한다.
/// 그리고 이 갈래가 실제로 깨진 적이 있다 — mtime 판정이 내용이 같은 파일에도 켜졌다.
#[test]
fn a_judge_that_reports_itself_fresh_is_not_called_stale() {
    let tmp = seed_repo();
    let d = tmp.path();
    plant_judge(d, true);
    let (_, text) = check(d, &["--staged"]);
    assert!(
        !text.contains("지금 소스로 지어진 것이 아니다"),
        "신선한 판정기를 낡았다고 했다 — 매번 넓게 보면 좁히기가 죽은 것과 같다:\n{text}"
    );
}

// ── 산출물의 범위: plugin 디렉토리 밖 ─────────────────────────────────
//
// 번들 plugin 은 워크스페이스 크레이트를 링크한다. 그 크레이트가 바뀌면 plugin 바이너리가
// 달라지는데, 매니페스트가 없어 판정 대상에서 자연히 빠져 있었다. 아래 둘이 **양극성**이다 —
// 링크한 것은 요구하고 안 한 것은 안 요구한다. 한쪽만 보면 "전부 요구하는" 게이트도 통과한다.

/// 링크된 크레이트 하나와 그것을 쓰는 plugin, 그리고 **아무도 안 쓰는** 크레이트 하나를
/// 담은 진짜 cargo 워크스페이스. `cargo tree` 가 읽을 수 있어야 하므로 매니페스트가
/// 형식만 흉내 낸 것이면 안 된다.
fn seed_workspace() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("임시 디렉토리");
    let d = tmp.path();
    run_git(d, &["init", "--quiet"]);
    run_git(d, &["config", "user.email", "fixture@example.invalid"]);
    run_git(d, &["config", "user.name", "fixture"]);
    run_git(d, &["config", "core.hooksPath", "/dev/null"]);

    write(
        d,
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\n\
         members = [\"crates/tasty-plugin-fixture\", \"crates/tasty-shared\", \"crates/tasty-lonely\"]\n\
         [workspace.package]\nedition = \"2024\"\n\
         [package]\nname = \"root-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         [lib]\npath = \"lib.rs\"\n",
    );
    write(d, "lib.rs", "pub fn nothing() {}\n");
    write(
        d,
        &format!("{PLUGIN}/Cargo.toml"),
        "[package]\nname = \"tasty-plugin-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         [dependencies]\ntasty-shared = { path = \"../tasty-shared\" }\n",
    );
    write(
        d,
        &format!("{PLUGIN}/tasty-plugin.toml"),
        "id = \"com.tasty.fixture\"\nversion = \"0.1.0\"\n",
    );
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    tasty_shared::greet();\n}\n",
    );
    for c in ["tasty-shared", "tasty-lonely"] {
        write(
            d,
            &format!("crates/{c}/Cargo.toml"),
            &format!("[package]\nname = \"{c}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        write(
            d,
            &format!("crates/{c}/src/lib.rs"),
            "pub fn greet() {\n    let _ = 1;\n}\n",
        );
    }
    run_git(d, &["add", "-A"]);
    run_git(d, &["commit", "--quiet", "-m", "seed"]);
    tmp
}

#[test]
fn a_change_in_a_linked_workspace_crate_demands_a_bump() {
    let tmp = seed_workspace();
    let d = tmp.path();
    write(
        d,
        "crates/tasty-shared/src/lib.rs",
        "pub fn greet() {\n    let _ = 2;\n}\n",
    );
    commit_all(d, "fix(shared): change behaviour");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(
        code, 1,
        "링크된 크레이트가 바뀌었는데 통과했다 — 이 자리가 여태 안 보이던 구멍이다:\n{text}"
    );
    assert!(
        text.contains("tasty-shared/src/lib.rs"),
        "어느 파일 때문인지 메시지에 없다:\n{text}"
    );
}

#[test]
fn a_change_in_an_unlinked_workspace_crate_does_not() {
    let tmp = seed_workspace();
    let d = tmp.path();
    write(
        d,
        "crates/tasty-lonely/src/lib.rs",
        "pub fn greet() {\n    let _ = 2;\n}\n",
    );
    commit_all(d, "fix(lonely): change behaviour");

    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(
        code, 0,
        "아무 plugin 도 링크하지 않는 크레이트가 bump 를 요구했다:\n{text}"
    );
}

#[test]
fn a_missing_shipping_judge_widens_and_says_so() {
    let tmp = seed_repo();
    let d = tmp.path();
    write(
        d,
        &format!("{PLUGIN}/src/main.rs"),
        "fn main() {\n    let msg = \"changed\";\n}\n",
    );
    commit_all(d, "feat(fixture): change behaviour");

    // 합성 저장소에는 `target/` 이 없으므로 판정기가 없는 경로가 그대로 재현된다.
    let (code, text) = check(d, &["--range", "HEAD^", "HEAD"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("출하 범위를 못 좁힌다"),
        "판정이 넓어진 것을 말하지 않는다 — 조용히 달라지면 다음 사람이 못 본다:\n{text}"
    );
}
