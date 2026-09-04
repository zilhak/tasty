//! `check-headless` 의 명명 `--skip` 이 **정확히 하나의 테스트**를 가리키는지 고정한다.
//!
//! libtest 의 `--skip` 은 **부분일치**다 — 테스트 경로(`모듈::이름`) 어디에든 그 문자열이
//! 들어 있으면 빠진다. 그래서 두 방향으로 조용히 어긋난다.
//!
//! - **과소(0 건)**: 이름이 바뀌거나 사라지면 그 `--skip` 은 아무것도 안 잡는다. rc 에도
//!   본문에도 안 나온다 — 오타난 `--skip` 은 경고 없이 무시된다(실측).
//! - **과대(2 건 이상)**: 나중에 그 문자열을 품는 이름이 생기면 **의도 없이 함께 빠진다.**
//!   초록인데 검증 범위가 준다. `check-headless` 는 이 저장소에서 전체 스위트를 자동으로
//!   도는 **유일한** 조합이라, 여기서 조용히 빠지면 어디서도 안 돈다.
//!
//! 실물이 있다 — `sweep_stale` 을 skip 하면 `sweep_stale_prompt_files*` 넷과
//! `sweep_stale_turns` 까지 함께 빠진다. 과대는 가상의 위험이 아니다.
//!
//! **이 가드는 `--skip` 하나가 테스트 하나를 가리킨다는 것을 불변식으로 박는다.** 언젠가
//! 모듈을 통째로 빼고 싶어지면 여기가 먼저 빨개진다 — **조용히 넓어지는 것보다 시끄럽게
//! 막히는 쪽**을 고른 것이다. 그때는 skip 이 아니라 `#[ignore]` 나 cfg 로 가르거나, 이
//! 가드의 불변식을 의도적으로 고쳐야 한다.
//!
//! **사거리(R16)**: 테스트 이름 집합을 `cargo test -- --list` 가 아니라 **소스 텍스트**에서
//! 얻는다. 그래서 매크로가 만들어내는 테스트 이름은 못 본다 — 이 가드는 **양성(과소·과대가
//! 있다)은 말할 수 있고, "완벽히 하나뿐" 은 텍스트가 보는 범위에서만 말한다.** 실행 기반
//! 대조는 `--list` 를 뽑는 별도 단계가 필요하고, 그것은 이 가드가 대신하지 않는다.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const WORKFLOW: &str = ".github/workflows/crossplatform-check.yml";
const STEP_ANCHOR: &str = "- name: cargo test (headless)";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 워크플로의 `cargo test (headless)` 스텝에서 `--skip` 인자를 **읽어온다**.
///
/// 값을 이 파일에 박아두면 워크플로가 바뀌는 순간 만료되고, 만료된 가드는 CI 보다 약한
/// 초록을 낸다. 앵커를 못 찾으면 0 건이 되는데, 그 0 은 "skip 이 없다" 가 아니라
/// "파서가 죽었다" 이므로 구분해서 죽는다.
fn skips_from_workflow() -> Vec<String> {
    let text = fs::read_to_string(repo_root().join(WORKFLOW))
        .unwrap_or_else(|e| panic!("{WORKFLOW} 를 읽을 수 없다: {e}"));
    let start = text
        .find(STEP_ANCHOR)
        .unwrap_or_else(|| panic!("워크플로에서 `{STEP_ANCHOR}` 스텝을 못 찾았다 — 앵커가 깨졌다"));
    let rest = &text[start + STEP_ANCHOR.len()..];
    // 다음 스텝(`- name:`) 전까지가 이 스텝의 블록이다.
    let block = match rest.find("- name:") {
        Some(i) => &rest[..i],
        None => rest,
    };

    let mut out = Vec::new();
    let mut cursor = block;
    while let Some(i) = cursor.find("--skip") {
        let after = &cursor[i + "--skip".len()..];
        let name: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push(name);
        }
        cursor = after;
    }
    assert!(
        !out.is_empty(),
        "`{STEP_ANCHOR}` 블록에서 `--skip` 을 하나도 못 읽었다 — \
         진짜 0 건인지 파서가 죽은 건지 구분되지 않으므로 실패로 다룬다"
    );
    out
}

/// `.rs` 파일을 훑어 `fn <이름>(` 과 `mod <이름>` 중 `needle` 을 **부분문자열로 품는** 것을 모은다.
///
/// libtest 의 `--skip` 은 `모듈::이름` 전체를 보므로 모듈 이름도 대상이다.
fn identifiers_containing(needle: &str) -> BTreeSet<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if p.is_dir() {
                // 빌드 산출물과 vendored 자산은 소스가 아니다.
                if name == "target" || name == ".git" || name == "assets" {
                    continue;
                }
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&repo_root(), &mut files);

    let mut found = BTreeSet::new();
    for f in files {
        let Ok(text) = fs::read_to_string(&f) else {
            continue;
        };
        for kw in ["fn ", "mod "] {
            let mut cursor = text.as_str();
            while let Some(i) = cursor.find(kw) {
                let after = &cursor[i + kw.len()..];
                let ident: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if ident.contains(needle) {
                    found.insert(ident);
                }
                cursor = after;
            }
        }
    }
    found
}

#[test]
fn every_named_skip_matches_exactly_one_identifier() {
    for skip in skips_from_workflow() {
        let hits = identifiers_containing(&skip);
        assert!(
            !hits.is_empty(),
            "`--skip {skip}` 이 아무 테스트 이름과도 일치하지 않는다 — 죽은 skip 이다. \
             이름이 바뀌었거나 테스트가 사라졌고, 그동안 그 skip 은 조용히 무시돼 왔다. \
             워크플로에서 지우거나 현재 이름으로 고쳐라."
        );
        assert_eq!(
            hits.len(),
            1,
            "`--skip {skip}` 이 이름 {}개와 일치한다: {hits:?} — 부분일치라 의도하지 않은 \
             테스트까지 함께 빠진다. skip 문자열을 더 길게(모듈 경로를 포함해) 적거나, \
             정말 여럿을 빼야 한다면 이 가드의 불변식(skip 하나 = 테스트 하나)을 \
             먼저 고쳐라 — 조용히 넓어지게 두지 않는다.",
            hits.len()
        );
    }
}

#[test]
fn the_parser_reads_the_workflow_rather_than_a_hardcoded_list() {
    // 이 가드 자신이 만료되지 않는지 본다: 워크플로에서 읽은 목록이 비어 있지 않아야 하고,
    // 그 이름들이 이 파일 안에 리터럴로 박혀 있지 않아야 한다.
    let skips = skips_from_workflow();
    assert!(!skips.is_empty());
    let own_source = fs::read_to_string(repo_root().join("tests/headless_skip_names_are_exact.rs"))
        .expect("자기 소스를 읽을 수 있어야 한다");
    for s in &skips {
        assert!(
            !own_source.contains(s.as_str()),
            "skip 이름 `{s}` 이 이 가드 소스에 박혀 있다 — 워크플로에서 읽는 의미가 없어진다"
        );
    }
}
