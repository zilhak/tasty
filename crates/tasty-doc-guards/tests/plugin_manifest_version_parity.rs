//! builtin plugin 의 매니페스트(`tasty-plugin.toml`)와 그 크레이트의 코드가 **같은 값을
//! 말하는지** 검증한다. 물음이 그것이고, `version` 은 그 첫 칸이었다.
//!
//! 지금 두 칸이다.
//!
//! 1. 매니페스트 `version` ↔ `Cargo.toml` `version`
//! 2. 매니페스트 `id` ↔ 코드의 `const PLUGIN_ID`
//!
//! 두 칸을 한 파일에 두는 이유가 있다. 같은 물음에 답하는 시험이 둘로 갈리면 다음 사람이
//! 하나만 보고 "그 축은 이미 지켜진다" 고 읽는다 — 실제로 이 파일이 `version` 만 보는 동안
//! `id` 는 아무도 안 봤다.
//!
//! 배경: 매니페스트 version 은 `plugin.list` / 업그레이드 판정(`upgrade_builtins`)이
//! 노출·비교하는 값인데, 과거엔 Cargo.toml 만 patch 자동 +1 되고 매니페스트는 방치돼
//! 드리프트(예: markdown Cargo 0.1.11 vs manifest 0.1.1)가 쌓였다. 버전 정책이 이제
//! 둘의 lockstep 갱신을 요구하므로(§버전 정책 > Plugin), 이 테스트가 그 집행 채널이다.
//! 자동 실행 채널이 **둘**이다: `doc-guards.yml` 이 `cargo test -p tasty-doc-guards` 를
//! main push · PR 마다 **경로 필터 없이** 돌리고, `check-headless` 의 전체 스위트
//! (`cargo test --workspace --no-default-features`)도 이 타깃을 담는다. 그리고 Windows
//! 잡이 `-p tasty-doc-guards` 로 이 크레이트의 통합 타깃을 지목해 돌린다 — 기본 조합의
//! `--lib --bins` 스텝은 여전히 통합 타깃을 안 보지만, 그 잡에 그 스텝만 있는 것은
//! 아니다. 채널 정본은 `docs/dev-guide/ci-gates.md`.
//! 앞엣것은 이 타깃이 이 크레이트로 옮겨오면서 생긴 채널이다(ADR-0138).
//!
//! 참고: 매니페스트가 없는 라이브러리/인프라 크레이트(sdk, protocol, manifest 등)는
//! 번들 대상이 아니므로 스캔에서 자동 제외된다(파일 부재 = skip).

use std::path::{Path, PathBuf};

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// `version = "..."` 최상위 필드의 따옴표 안 값을 추출한다.
/// `manifest_version` / `api_version` 처럼 접미 `version` 은 `^version = ` 앵커로 배제.
fn extract_version(contents: &str, file: &Path) -> String {
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("version") {
            // `version` 뒤 첫 비공백이 `=` 여야 최상위 필드 (manifest_version 등 배제)
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let after_eq = after_eq.trim();
                let value = after_eq
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .to_string();
                assert!(!value.is_empty(), "빈 version 필드: {}", file.display());
                return value;
            }
        }
    }
    panic!("version 필드를 찾지 못함: {}", file.display());
}

/// 매니페스트를 가진 번들 plugin 디렉토리를 모은다.
///
/// 두 시험이 같은 모수를 봐야 하므로 여기 한 번만 적는다. 그리고 `read_dir` 을 두 번
/// 부르지 않기 위해서이기도 하다 — 통합 타깃의 직접 `read_dir` 은 여유 0 래칫이라
/// 사본을 하나 더 만들면 그 자리에서 빨개진다(`scripts/check-shared-walk-ratchet.sh`).
fn bundled_plugin_dirs() -> Vec<(String, PathBuf)> {
    // `CARGO_MANIFEST_DIR` 이 곧 레포 루트가 아니다(여기서는 크레이트 디렉토리다).
    // 공용 `repo_root()` 는 표지 파일 넷으로 자기가 잡은 경로를 검증한다.
    let crates_dir = tasty_doc_guards::repo_root().join("crates");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&crates_dir).expect("crates 디렉토리 read_dir 실패") {
        let entry = entry.expect("crates dir entry 읽기 실패");
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if !name.starts_with("tasty-plugin-") {
            continue;
        }
        // 번들 매니페스트 없는 라이브러리/인프라 크레이트 → 대상 아님.
        if !dir.join("tasty-plugin.toml").exists() {
            continue;
        }
        out.push((name, dir));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 매니페스트 최상위 `id = "..."` 의 값.
fn extract_id(contents: &str, file: &Path) -> String {
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("id") {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let value = after_eq.trim().trim_matches('"').to_string();
                assert!(!value.is_empty(), "빈 id 필드: {}", file.display());
                return value;
            }
        }
    }
    panic!("id 필드를 찾지 못함: {}", file.display());
}

#[test]
fn manifest_version_matches_cargo_version() {
    let mut checked = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for (name, dir) in bundled_plugin_dirs() {
        let manifest = dir.join("tasty-plugin.toml");
        let cargo = dir.join("Cargo.toml");
        let manifest_v = extract_version(
            &std::fs::read_to_string(&manifest).expect("매니페스트 read 실패"),
            &manifest,
        );
        let cargo_v = extract_version(
            &std::fs::read_to_string(&cargo).expect("Cargo.toml read 실패"),
            &cargo,
        );
        checked += 1;
        if manifest_v != cargo_v {
            mismatches.push(format!(
                "  {name}: manifest={manifest_v} vs Cargo={cargo_v}"
            ));
        }
    }

    assert!(
        checked > 0,
        "스캔된 builtin plugin 이 0 개 — 경로/네이밍이 바뀌었는지 확인"
    );
    assert!(
        mismatches.is_empty(),
        "매니페스트 version 이 Cargo.toml 과 드리프트됨 (버전 정책 §Plugin: lockstep 갱신 + 재서명 필요):\n{}",
        mismatches.join("\n")
    );
}

/// `crates/` 아래 파일 수의 하한. 이 모수는 크레이트 분리·병합으로 크게 움직이므로
/// 하한은 순회 생존만 본다 — 실제 수에 붙이면 정상적인 정리가 순회 사망으로 오진된다.
const CRATES_FLOOR: Floor = Floor {
    min: 200,
    measured: 650,
    measured_on: "2026-09-07",
    why_this_gap: "이 모수는 crates/ 아래 .rs 파일 수다. 크레이트가 갈라지고 합쳐지는 것은 \
                   정상 변경이라 하한을 실측에 붙이면 그 정리가 순회 사망으로 잘못 \
                   진단된다. 여기서 하한은 순회 생존만 본다.",
};

/// 매니페스트 `id` 와 코드의 `const PLUGIN_ID` 가 같은 값을 말하는지 본다.
///
/// ## 왜 이 값에 채널이 필요한가
///
/// plugin 은 hello 로 자기 id 를 보내고 host 는 **그 값 그대로** registry 에 등록한다 —
/// 매니페스트의 `id` 와 대조하지 않는다. 그래서 둘이 어긋나도 부팅은 조용히 성공하고,
/// 매니페스트를 근거로 도는 것(IPC 네임스페이스 · 권한 · `plugin.list`)과 hello 를 근거로
/// 도는 것이 **서로 다른 plugin 을 가리키게 된다.** 컴파일도 테스트도 그것을 안 본다.
///
/// ## 그리고 바로 옆줄이 옳은 형태다
///
/// 같은 파일에서 `PLUGIN_VERSION` 은 `env!("CARGO_PKG_VERSION")` 으로 **파생**된다 —
/// 어긋날 수가 없다. `PLUGIN_ID` 만 손으로 적은 두 번째 사본이고, 그 사실이 어디에도
/// 적혀 있지 않다. 파생으로 바꾸는 편이 이 시험보다 강하지만(매니페스트를 `include_str!`
/// 로 읽어 파싱하는 build.rs 가 필요하다) 그 비용이 이 값 하나에 비해 크다. 그래서
/// 여기서는 **두 사본이 갈라지는 것을 잡는 것**까지 한다.
#[test]
fn manifest_id_matches_the_plugin_id_constant() {
    let root = tasty_doc_guards::repo_root();
    let sources = walk_with_floor(
        &root.join("crates"),
        &root,
        &CRATES_FLOOR,
        Descend::SkipBuildCaches,
        &|w: &Walked| w.rel.ends_with(".rs") && w.rel.contains("/src/"),
    )
    .unwrap_or_else(|why| panic!("{why}"));

    let mut checked = 0usize;
    let mut blind: Vec<String> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();

    for (name, dir) in bundled_plugin_dirs() {
        let manifest = dir.join("tasty-plugin.toml");
        let manifest_id = extract_id(
            &std::fs::read_to_string(&manifest).expect("매니페스트 read 실패"),
            &manifest,
        );
        checked += 1;
        let mut found_here = 0usize;

        let prefix = format!("crates/{name}/src/");
        for walked in sources.iter().filter(|w| w.rel.starts_with(&prefix)) {
            let text = std::fs::read_to_string(&walked.path).unwrap_or_default();
            for line in text.lines() {
                // 이름 뒤에 `:` 가 와야 한다 — 부분 문자열로 찾으면 `PLUGIN_IDENT` 같은
                // 다른 상수까지 잡히고, 그러면 이 시험이 "그 상수를 찾았다" 고 잘못 센다.
                // (변이로 밟았다: 상수 이름을 바꿔도 접두가 매치돼 하한이 발화하지 않았다.)
                let Some(rest) = line.trim_start().split_once("const PLUGIN_ID:") else {
                    continue;
                };
                let Some(value) = rest.1.split('"').nth(1) else {
                    continue;
                };
                found_here += 1;
                if value != manifest_id {
                    mismatches.push(format!(
                        "  {}: manifest id={manifest_id} vs const PLUGIN_ID={value}",
                        walked.rel
                    ));
                }
            }
        }

        if found_here == 0 {
            blind.push(format!("  {name}"));
        }
    }

    assert!(
        checked > 0,
        "스캔된 builtin plugin 이 0 개 — 경로/네이밍이 바뀌었는지 확인"
    );
    // 합이 아니라 **plugin 마다** 센다. 합으로 세면 한 crate 의 여분 사본이 다른 crate 의
    // 소실을 가린다 — 실제로 `tasty-plugin-claude` 는 `lib.rs` 와 `main.rs` 두 곳에 그
    // 상수를 두고 있어, 합 판정에서는 어느 한 plugin 이 통째로 안 보여도 수가 맞는다.
    // (변이로 확인했다: 상수 이름 하나를 바꿔도 합 단정은 통과했다.)
    assert!(
        blind.is_empty(),
        "이 plugin 들에서 `const PLUGIN_ID` 를 하나도 못 찾았다 — 상수를 다른 이름으로 \
         적었거나 순회가 그 파일을 못 봤다는 뜻이다. 이 상태에서 뒤따르는 \"불일치 0\" 은 \
         일치한다는 뜻이 아니라 그 plugin 을 아예 안 봤다는 뜻이다:\n{}",
        blind.join("\n")
    );
    assert!(
        mismatches.is_empty(),
        "매니페스트 `id` 와 코드의 `const PLUGIN_ID` 가 갈라졌다.\n\
         host 는 hello 로 받은 id 를 그대로 등록하고 매니페스트와 대조하지 않으므로, \
         이 어긋남은 부팅을 막지 않고 IPC 네임스페이스·권한·`plugin.list` 가 서로 다른 \
         plugin 을 가리키게 만든다:\n{}",
        mismatches.join("\n")
    );
}
