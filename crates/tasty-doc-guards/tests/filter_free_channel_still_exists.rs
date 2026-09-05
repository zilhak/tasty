//! 이 디렉토리의 가드들이 **매 push 도는 채널**을 실제로 갖고 있는지 본다.
//!
//! ADR-0138 의 결정은 "읽는 것이 전부 `docs/**` 인 가드를 의존 0 크레이트로 옮긴다" 였고,
//! 그 결정이 값을 갖는 근거는 **옮긴 자리에 경로 필터가 없다**는 것 하나다. 그런데 그
//! 근거를 확인하는 것이 아무 데도 없었다 — 실측으로 확인했다(2026-09-05):
//!
//! - `filtered_guards_are_not_totally_blind` 는 이 디렉토리를 이름으로 **건너뛴다**
//!   (`FILTER_FREE_DIR`). 채널이 있다고 **가정**하는 것이지 재는 것이 아니다.
//! - `ci_channel_claims_match_workflows` 의 `automatic_job_bodies` 는 경로 필터를
//!   모델하지 않는다. `push:` 만 있으면 자동으로 세므로, 필터가 생겨도 그 잡은 여전히
//!   "자동" 이다.
//! - `src/source_guards` 의 `EXPECTED_TEST_INVOCATIONS` 는 파일별 **호출 건수**를
//!   고정한다. 필터가 붙어도 건수는 그대로고, 호출을 한 타깃으로 좁혀도 그대로다.
//!
//! 변이 둘로 그 셋을 동시에 확인했다 — ① `push:` 에 `paths:` 를 달기 ② 호출을
//! `--test <이름>` 하나로 좁히기. **두 변이 모두 세 판정기 전부에서 살아남았다.**
//! 그 상태가 되면 이 디렉토리의 가드들은 자기가 깨질 수 있는 유일한 종류의 push
//! (문서만 담은 push)에서 안 도는 자리로 조용히 돌아간다 — ADR-0138 이 벗어나려던
//! 바로 그 상태이고, 되돌아간 것을 아무도 못 본다.
//!
//! **이름이 아니라 성질로 판정한다**(ADR-0175 와 같은 이유). 물음은 "`doc-guards.yml`
//! 이 있는가" 가 아니라 "경로 필터 없는 잡 중 이 패키지를 **좁히지 않고** 돌리는 것이
//! 있는가" 다. 워크플로 이름이 바뀌거나 잡이 다른 파일로 옮겨가도 채널이 남아 있으면
//! 통과해야 한다 — 이름으로 박으면 옮기는 것 자체가 거짓 실패가 된다.
//!
//! **이 가드 자신의 채널**: 여기서 잡는 변경은 `.github/workflows/**` 를 건드린다.
//! 그 경로는 `crossplatform-check.yml` 의 `paths-ignore`(`docs/**` · `site/**` ·
//! `**/*.md`) 밖이라, 필터가 붙는 그 push 에서 `check-headless` 가 전체 스위트를 돌며
//! 이 타깃을 실행한다. 즉 자기 채널이 사라지는 변경은 다른 채널이 본다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tasty_doc_guards::workflow_triggers::push_trigger;

/// 채널이 지켜 주는 대상이 사는 곳. `filtered_guards_are_not_totally_blind` 의
/// `FILTER_FREE_DIR` 과 **같은 값이어야 한다** — 아래에서 그 정합을 함께 본다.
const GUARD_DIR: &str = "crates/tasty-doc-guards/tests";

/// 그 디렉토리의 패키지 이름. 채널 판정은 이 이름을 좁히지 않고 부르는가로 한다.
const PACKAGE: &str = "tasty-doc-guards";

/// 채널이 지켜 주는 순수 스캔 가드 수의 하한. 실측 17 (2026-09-05).
/// 모수가 비면 "채널이 있다" 는 아무것도 안 지키는 참이 된다.
const MIN_GUARDED: usize = 12;

/// 호출을 좁혀 패키지 일부만 돌게 만드는 플래그. 하나라도 있으면 그 호출은
/// 이 디렉토리 전체의 채널이 아니다.
const NARROWING: &[&str] = &["--test", "--lib", "--bins", "--bin", "--doc", "--example"];

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 주석·스텝 이름을 지우고 한 줄로 편다.
///
/// 스텝 이름을 지우는 이유는 이 레포의 스텝 이름이 명령을 그대로 쓰기 때문이다
/// (`- name: cargo test -p tasty-doc-guards`). 안 지우면 **이름이 채널로 읽혀**,
/// 실행 스텝이 좁혀졌는데도 이름이 안 좁혀진 옛 명령을 그대로 들고 있으면 통과한다.
fn flatten(yaml: &str) -> String {
    yaml.replace("\r\n", "\n")
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with("- name:") && !t.starts_with("name:")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `jobs:` 아래의 잡 본문들. 잡 헤더는 들여쓰기 2 의 `<이름>:` 이다.
///
/// **잡 단위로 갈라야 하는 이유**: 워크플로에 필터가 없어도 그 안의 잡이
/// `if: github.event_name == 'workflow_dispatch'` 로 수동 전용일 수 있다. 파일 단위로
/// 보면 그 잡의 명령이 자동 채널로 읽힌다 — 실측으로 이 함정에 걸렸다(2026-09-05):
/// `test.yml` 은 필터가 없고 `cargo test --workspace` 를 들고 있지만 그 잡은 수동
/// 전용이라, 잡을 안 가른 첫 판에서 이 가드가 **잘못된 이유로 초록**이었다.
fn automatic_job_bodies(yaml: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut current = String::new();
    let mut in_jobs = false;
    for line in yaml.replace("\r\n", "\n").lines() {
        if line.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        let is_job_head =
            line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':');
        if is_job_head && !current.is_empty() {
            bodies.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.is_empty() {
        bodies.push(current);
    }
    // `ci_channel_claims_match_workflows::automatic_job_bodies` 와 같은 술어다.
    // 같은 물음에 답을 둘로 만들지 않으려는 것이고, 방향도 안전한 쪽이다 — 이 문자열을
    // 담은 자동 잡을 잘못 빼면 "채널이 없다" 로 시끄럽게 실패한다.
    bodies
        .into_iter()
        .filter(|b| !b.contains("github.event_name == 'workflow_dispatch'"))
        .collect()
}

/// 평탄화된 본문에서 `cargo test` 호출을 잘라낸다. 각 조각은 다음 `cargo ` 직전까지라
/// 한 스텝에 명령이 여럿이어도 플래그가 섞이지 않는다.
fn cargo_test_invocations(flat: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = flat[from..].find("cargo test") {
        let start = from + rel;
        let rest = &flat[start + "cargo test".len()..];
        let end = rest
            .find("cargo ")
            .map_or(flat.len(), |n| start + "cargo test".len() + n);
        out.push(&flat[start..end]);
        from = start + "cargo test".len();
    }
    out
}

/// 이 호출이 [`PACKAGE`] 를 **통째로** 돌리는가.
fn covers_the_package_whole(inv: &str) -> bool {
    let words: Vec<&str> = inv.split_whitespace().collect();
    let names_package = words
        .windows(2)
        .any(|w| (w[0] == "-p" || w[0] == "--package") && w[1] == PACKAGE)
        || words.contains(&format!("-p{PACKAGE}").as_str())
        || words.contains(&"--workspace");
    names_package && !words.iter().any(|w| NARROWING.contains(w))
}

fn workflow_files(root: &Path) -> Vec<PathBuf> {
    let dir = root.join(".github/workflows");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    out.sort();
    out
}

/// 이 디렉토리에 사는 **순수 소스 스캔** 타깃. 술어는
/// `filtered_guards_are_not_totally_blind::is_pure_source_scan` 과 같은 성질이다 —
/// 읽는 **행위**로 세고, 프로세스를 띄우는 것은 뺀다.
fn guarded_targets(root: &Path) -> BTreeSet<String> {
    let dir = root.join(GUARD_DIR);
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .flatten()
    {
        let p = e.path();
        if !p.extension().is_some_and(|x| x == "rs") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&p) else {
            continue;
        };
        let reads = src.contains("CARGO_MANIFEST_DIR")
            || src.contains("repo_root()")
            || src.contains("read_to_string")
            || src.contains("read_dir");
        let spawns = [
            "Command::new",
            "spawn_diag",
            "TASTY_E2E_BIN",
            "CARGO_BIN_EXE",
        ]
        .iter()
        .any(|m| src.contains(m));
        if reads && !spawns {
            out.insert(p.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    out
}

#[test]
fn a_filter_free_job_runs_this_package_whole() {
    let root = repo_root();
    let files = workflow_files(&root);
    assert!(
        files.len() >= 5,
        "워크플로를 {}개밖에 못 읽었다 — 수집이 깨졌다. 그대로 두면 '필터 없는 채널이 \
         없다' 가 측정이 아니라 사고로 참이 된다",
        files.len()
    );

    let mut unreadable = Vec::new();
    let mut filter_free = Vec::new();
    let mut filtered = 0usize;
    let mut tags_only = 0usize;
    for path in &files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        let Some(t) = push_trigger(&text) else {
            unreadable.push(name.to_string());
            continue;
        };
        if !t.present {
            continue;
        }
        if t.path_filtered {
            filtered += 1;
        } else if t.tags_only {
            // 필터는 없지만 일상 커밋에서는 안 뜬다 — 매 push 채널이 아니다.
            tags_only += 1;
        } else {
            filter_free.push((name.to_string(), text));
        }
    }

    assert!(
        unreadable.is_empty(),
        "`on:` 을 못 읽은 워크플로가 있다 — 판정 불가는 통과가 아니다. 인라인 표기\
         (`on: [push]`)면 블록 표기로 바꾸거나 판독기를 넓혀라:\n  {}",
        unreadable.join("\n  ")
    );
    // 양성 대조: 판독기가 모든 파일에 "필터 없음" 을 내고 있으면 아래 단언은
    // 아무것도 안 본다. 이 레포에는 필터를 가진 워크플로가 실제로 여럿 있다.
    assert!(
        filtered > 0,
        "경로 필터를 가진 워크플로를 하나도 못 찾았다 — 판독기가 한쪽으로만 답하고 \
         있다. 그 상태에서는 아래 '필터 없는 채널이 있다' 가 언제나 참이다"
    );

    let providers: Vec<&str> = filter_free
        .iter()
        .filter(|(_, text)| {
            automatic_job_bodies(text).iter().any(|body| {
                let flat = flatten(body);
                cargo_test_invocations(&flat)
                    .into_iter()
                    .any(covers_the_package_whole)
            })
        })
        .map(|(n, _)| n.as_str())
        .collect();

    let guarded = guarded_targets(&root);
    assert!(
        guarded.len() >= MIN_GUARDED,
        "`{GUARD_DIR}` 의 순수 스캔 가드를 {}개밖에 못 셌다(하한 {MIN_GUARDED}) — \
         모수가 줄면 '채널이 있다' 는 아무것도 안 지킨다",
        guarded.len()
    );

    assert!(
        !providers.is_empty(),
        "경로 필터 없이 `{PACKAGE}` 를 **좁히지 않고** 돌리는 잡이 하나도 없다. \
         `{GUARD_DIR}` 의 가드 {}개는 읽는 것이 대부분 문서라, 자기가 깨질 수 있는 \
         유일한 종류의 push(문서만 담은 push)에서 안 도는 자리로 돌아갔다 — ADR-0138 이 \
         벗어나려던 그 상태다. 필터를 떼거나, 그 패키지를 좁히지 않고 돌리는 잡을 \
         경로 필터 없는 워크플로에 두어라. 본 것: 필터 없는 워크플로 {:?} · \
         경로 필터를 가진 것 {filtered} 개 · 태그 전용이라 매 push 가 아닌 것 \
         {tags_only} 개",
        guarded.len(),
        filter_free
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
    );
}

/// 채널이 지키는 자리와, 그 자리를 **면제로 쓰는** 판정기의 상수가 같은 값인가.
///
/// 둘이 갈라지면 조용히 아무도 안 보는 구간이 생긴다:
/// `filtered_guards_are_not_totally_blind` 는 옛 경로를 면제하고, 이 가드는 새 경로의
/// 채널을 지킨다 — 그 사이에 낀 가드는 양쪽 어디에도 안 든다.
#[test]
fn the_exempting_guard_still_names_this_same_directory() {
    let root = repo_root();
    let path = root
        .join(GUARD_DIR)
        .join("filtered_guards_are_not_totally_blind.rs");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} — 면제하는 판정기가 사라졌으면 이 가드의 \
             전제도 사라진 것이다. 함께 다시 판단해라",
            path.display()
        )
    });
    let decl = format!("const FILTER_FREE_DIR: &str = \"{GUARD_DIR}\";");
    assert!(
        src.contains(&decl),
        "면제하는 판정기의 `FILTER_FREE_DIR` 이 `{GUARD_DIR}` 가 아니다. 그 상수는 \
         '여기는 채널이 있으니 검사에서 뺀다' 는 뜻이고, 이 가드가 지키는 것이 바로 그 \
         채널이다 — 두 값이 갈라지면 면제된 자리와 지켜지는 자리가 어긋난다"
    );
}

/// 좁힘 판정이 실제로 좁힘을 잡는가. 이 판정이 무너지면 위 단언은 좁혀진 호출을
/// 온전한 채널로 세고, 그때가 정확히 이 가드가 쓸모없어지는 순간이다.
#[test]
fn a_narrowed_invocation_is_not_a_whole_package_channel() {
    assert!(covers_the_package_whole(
        " -p tasty-doc-guards --locked --no-fail-fast"
    ));
    assert!(covers_the_package_whole(" --workspace --locked"));
    assert!(!covers_the_package_whole(
        " -p tasty-doc-guards --test no_checkbox_in_docs --locked"
    ));
    assert!(!covers_the_package_whole(" -p tasty-doc-guards --lib"));
    assert!(!covers_the_package_whole(
        " -p tasty-plugin-markdown --locked"
    ));
    assert!(!covers_the_package_whole(" --locked --no-fail-fast"));
}
