//! `README.md` / `README.ko.md` 의 배지·본문이 담은 **소스에서 파생되는 수**가 실측과
//! 일치하는지 검증한다. 불일치 시 fail. 지금 보는 것은 둘이다 —
//! Version 배지(루트 `Cargo.toml` `[package] version`)와 Workspace 배지(크레이트 수).
//!
//! 배경: shields.io static badge 는 URL 에 값이 박혀 있어 어디서도 파생되지 않는다.
//! 릴리스 절차(`docs/dev-guide/release.md` §1)가 bump 커밋에 배지를 함께 넣도록
//! 요구하지만 절차 문구만으로는 누락을 막지 못했고, 실제로 배지가 여러 마이너 뒤처진
//! 채 방치된 적이 있다. 이 테스트가 그 집행 채널이다 —
//! `doc-guards.yml` 이 main push · PR 마다 이 타깃을 돌리고, `check-headless` 의 전체
//! 스위트에서도 돈다(`docs/dev-guide/ci-gates.md`).
//!
//! 선례: `crates/tasty-doc-guards/tests/plugin_manifest_version_parity.rs`(plugin `Cargo.toml` ↔
//! `tasty-plugin.toml` lockstep). 같은 형태의 "선언값이 두 곳에 중복 존재" 드리프트다.
//!
//! ## 이 파일이 **안 보는 것**
//!
//! 아래는 실측해서 적는다 — "구멍이 닫혀 있다" 고 쓰지 않는다.
//!
//! - **배지·본문의 문구 형태가 바뀌면 못 본다.** 두 검사 모두 고정 조각
//!   (`VERSION_PREFIX` · `WORKSPACE_PREFIX` · `BODY_SUFFIX`)으로 자리를 찾는다.
//!   조각이 사라지면 "배지가 0 개" 로 잡히지만(그건 실패다), 같은 수를 **다른 문구로**
//!   새로 적으면 그 자리는 좌변 밖이라 조용히 낡는다. 실물: 이 커밋 전의 README 는
//!   같은 수를 파일당 세 자리(배지 하나 + 본문 둘)에 적고 있었고, 셋이 모두 낡아 있었다.
//! - **`crates/` 밖의 크레이트는 안 센다.** 레포 루트의 본 바이너리(`tasty`)가 그것이다.
//!   좌변의 정의와 그 근거는 [`tasty_doc_guards::crate_layers::crate_dir_names`].
//! - **workspace `exclude` 는 좌변에서 빠지지 않는다** — `crates/tasty-plugin-sdk-wasm`
//!   은 `cargo metadata` 에 안 나오지만 이 수에는 든다. 배지가 `crates/` 로 링크하므로
//!   클릭한 사람이 그 자리에서 세는 수와 같아야 한다.
//! - **README 밖의 같은 수는 안 본다.** `docs/architecture/index.md` 의 절 제목·개요
//!   문장은 `architecture_crate_list_complete` 가 본다(같은 좌변 함수를 쓴다).
//!   `docs/dev-guide/build.md` 는 수를 아예 복제하지 않는 쪽을 골랐다.

use std::path::{Path, PathBuf};

/// 배지 URL 에서 버전 값 앞에 오는 고정 조각. shields.io static badge 문법상
/// `badge/<label>-<message>-<color>` 이므로 값의 끝은 뒤따르는 `-<color>` 다.
const VERSION_PREFIX: &str = "badge/version-";

/// Workspace 배지의 같은 조각 — `badge/workspace-<N>%20crates-<color>`. 값 뒤에는
/// URL 인코딩된 공백이 붙는다.
const WORKSPACE_PREFIX: &str = "badge/workspace-";

/// Workspace 배지 값의 끝 경계. 이것까지 붙여서 비교해야 한 자리 수가 두 자리 수
/// 배지에도 걸리는 부분일치를 막는다(Version 배지에서 `-` 를 붙이는 것과 같은 이유).
const WORKSPACE_SUFFIX: &str = "%20crates-";

/// 본문이 같은 수를 적는 형태 — `<N>-crate workspace`. 영문·한국어 README 가 이 조각을
/// 공유한다(한국어 본문도 이 영어 표현을 그대로 쓴다).
///
/// 여기에도 지금 값을 적지 않는다. 이 파일이 고치는 결함이 바로 "같은 수가 두 자리에
/// 적히고 한쪽만 갱신된 것" 이라, 예시로라도 값을 박으면 그 자리가 다음 복제본이 된다
/// (ADR-0139: 커밋마다 바뀌는 값은 적는 순간 낡는다).
const BODY_SUFFIX: &str = "-crate workspace";

const READMES: [&str; 2] = ["README.md", "README.ko.md"];

/// 레포 루트 — 이 크레이트가 `crates/` 아래 살아서 `CARGO_MANIFEST_DIR` 이 레포 루트가
/// 아니다. 해석과 검증을 [`tasty_doc_guards::repo_root`] 한 곳에 모은다(ADR-0138).
fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// 루트 `Cargo.toml` 의 `[package]` 절에서 `version` 을 뽑는다.
///
/// 첫 `version = ` 라인을 그냥 집지 않고 절을 특정한다 — 루트 매니페스트에는
/// `[workspace]` 가 `[package]` 보다 앞에 있고, 나중에 `[workspace.package]` 같은
/// 절이 생기면 첫 매치가 엉뚱한 값을 가리키게 된다.
fn package_version(cargo_toml: &str) -> String {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("version") else {
            continue;
        };
        let Some(after_eq) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        // 값 뒤 인라인 주석(`version = "X" # note`)을 먼저 잘라낸다 — 안 자르면 닫는
        // 따옴표를 못 벗겨 값에 주석이 섞이고, 정상 배지가 불일치로 오탐된다.
        let value = after_eq
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches('"')
            .trim_end_matches('"')
            .to_string();
        assert!(
            !value.is_empty(),
            "루트 Cargo.toml [package] version 이 비어 있음"
        );
        return value;
    }
    panic!("루트 Cargo.toml 에서 [package] version 을 찾지 못함");
}

/// `needle` 을 담은 라인들 — 실패 메시지에 "지금 뭐라고 적혀 있는지" 를 그대로 보인다.
fn lines_with(contents: &str, needle: &str) -> Vec<String> {
    contents
        .lines()
        .filter(|l| l.contains(needle))
        .map(|l| l.trim().to_string())
        .collect()
}

#[test]
fn readme_version_badge_matches_cargo_version() {
    let root = repo_root();
    let version = package_version(&read(&root.join("Cargo.toml")));
    // 값의 끝 경계까지 포함해 비교한다. 뒤의 `-` 가 없으면 `0.1` 이 `0.10.2` 배지에도
    // 걸려 드리프트를 놓친다.
    let expected = format!("{VERSION_PREFIX}{version}-");

    let mut problems: Vec<String> = Vec::new();
    for name in READMES {
        let path = root.join(name);
        let contents = read(&path);
        let lines = lines_with(&contents, VERSION_PREFIX);
        let matches = contents.matches(expected.as_str()).count();

        if lines.len() != 1 {
            problems.push(format!(
                "  {name}: Version 배지가 {} 개 (정확히 1 개여야 함):\n{}",
                lines.len(),
                lines
                    .iter()
                    .map(|l| format!("      {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
            continue;
        }
        if matches != 1 {
            problems.push(format!(
                "  {name}: 배지 값이 Cargo.toml 과 다름 — 기대 `{expected}`, 실제:\n      {}",
                lines[0]
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "README Version 배지가 루트 Cargo.toml version({version}) 과 드리프트됨.\n\
         릴리스 절차상 배지는 bump 커밋에 함께 갱신한다 (docs/dev-guide/release.md §1):\n{}",
        problems.join("\n")
    );
}

#[test]
fn readme_workspace_badge_matches_crate_directories() {
    let root = repo_root();
    // 좌변은 손으로 적은 상수가 아니라 실측이다 — 상수를 적으면 지금 고치는 결함을
    // 한 겹 더 만드는 것이다. 정의와 그 근거는 crate_dir_names 의 doc.
    let actual = tasty_doc_guards::crate_layers::crate_dir_names(&root).len();
    let expected = format!("{WORKSPACE_PREFIX}{actual}{WORKSPACE_SUFFIX}");
    let body = format!("{actual}{BODY_SUFFIX}");

    let mut problems: Vec<String> = Vec::new();
    for name in READMES {
        let contents = read(&root.join(name));

        let badges = lines_with(&contents, WORKSPACE_PREFIX);
        if badges.len() != 1 {
            problems.push(format!(
                "  {name}: Workspace 배지가 {} 개 (정확히 1 개여야 함):\n{}",
                badges.len(),
                badges
                    .iter()
                    .map(|l| format!("      {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        } else if contents.matches(expected.as_str()).count() != 1 {
            problems.push(format!(
                "  {name}: 배지 값이 crates/ 실측과 다름 — 기대 `{expected}`, 실제:\n      {}",
                badges[0]
            ));
        }

        // 본문도 같은 수를 적는다. 배지만 보면 본문이 따로 낡는다 — 실제로 그렇게
        // 낡았다(파일당 배지 하나 + 본문 둘이 전부 같은 옛 값이었다). 그래서 발견되는
        // 자리 **전부**가 실측과 같아야 하고, 하나도 없으면 그것도 실패다(문구가
        // 바뀌었거나 문장이 사라진 것이고, 어느 쪽이든 이 검사가 눈을 감는다).
        let mentions = lines_with(&contents, BODY_SUFFIX);
        if mentions.is_empty() {
            problems.push(format!(
                "  {name}: 본문에 `N{BODY_SUFFIX}` 표현이 없다 — 문구가 바뀌었다면 이 \
                 테스트의 BODY_SUFFIX 도 함께 옮겨라(안 옮기면 그 자리가 좌변 밖이 된다)"
            ));
        }
        let stale: Vec<&String> = mentions.iter().filter(|l| !l.contains(&body)).collect();
        if !stale.is_empty() {
            problems.push(format!(
                "  {name}: 본문의 크레이트 수가 실측({actual}) 과 다름 — 기대 `{body}`:\n{}",
                stale
                    .iter()
                    .map(|l| format!("      {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "README 의 크레이트 수가 crates/ 실측({actual}) 과 드리프트됨.\n\
         배지·본문은 shields.io static badge 와 평문이라 어디서도 파생되지 않는다 —\n\
         크레이트를 추가·삭제했으면 두 README 를 같은 값으로 함께 고쳐라:\n{}",
        problems.join("\n")
    );
}
