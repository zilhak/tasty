//! 문서가 "CI 가 돌린다" 고 말하는 명령이 **실제로 자동으로 도는가** 를 대조한다.
//!
//! 배경: `cargo test --workspace` 전체 스위트는 `.github/workflows/test.yml` 의
//! `test-linux-x64` 잡에 있지만 그 잡은 `if: github.event_name == 'workflow_dispatch'`
//! 라 **수동 전용**이다. 그런데 레포 곳곳이 "`tests/X.rs` 가 `cargo test --workspace`(CI)
//! 로 강제한다" 라고 적어 왔다 — 자동으로 돌지 않는 채널을 강제 장치로 부른 것이다.
//! 그 서술을 읽은 사람은 자기가 아무것도 돌리지 않아도 어딘가가 잡아 준다고 믿는다.
//!
//! 이 가드가 없으면 같은 문장이 계속 새로 쓰인다: 컴파일도 통과하고, 그 주장이 틀렸다는
//! 사실은 워크플로 파일을 직접 열어야만 보인다. 채널 매트릭스 자체는
//! `docs/dev-guide/ci-gates.md`.
//!
//! **판정은 워크플로에서 읽는다**(문서를 문서로 검사하지 않는다):
//! - 자동 트리거(`push` / `pull_request` / `schedule`)를 가진 워크플로의 잡 중,
//!   `workflow_dispatch` 로 좁혀지지 않은 잡을 모은다.
//! - 그 잡들이 **전체 스위트**(`cargo test --workspace` 를 `--lib`/`--bins`/`--test` 로
//!   좁히지 않은 형태)를 돌리면 → 주장은 참이므로 이 가드는 조용히 통과한다.
//! - 돌리지 않으면 → "전체 스위트를 CI 가 돌린다" 는 서술은 전부 위반이다.
//!
//! 즉 전체 스위트가 자동화되는 날 이 가드는 스스로 잠잠해진다. 문서를 손으로 다시
//! 훑을 필요가 없다.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 스캔에서 제외할 디렉토리 — 빌드 산출물과 커밋되지 않는 로컬 폴더.
const SKIP_DIRS: &[&str] = &["target", ".git", "_site", "node_modules"];

/// 텍스트로 읽을 확장자.
const TEXT_EXTS: &[&str] = &["rs", "md", "toml", "yml", "yaml", "sh"];

/// 그 자리가 **부재를 함께 말하고 있는가** — 이것이 주장을 정당하게 만드는 유일한
/// 조건이다.
///
/// 경로 allowlist 를 쓰지 않는다. 정당한 서술의 조건은 "어느 파일이냐" 가 아니라
/// "자동으로 돌지 않는다는 사실을 같이 적었느냐" 이고, 그것은 파일과 무관하게 판정할 수
/// 있다. 규칙으로 두면 앞으로 정확히 쓴 문장은 등록 없이 통과하고, 등록만 해 두고 문장은
/// 틀린 채로 두는 형태도 생기지 않는다.
const ABSENCE_MARKERS: &[&str] = &[
    "수동 전용",
    "수동 실행",
    "수동 트리거",
    "자동 채널 없음",
    "자동 채널이 없다",
    "자동 채널은 아니다",
    "자동으로 돌지 않는다",
    "자동으로 도는 채널은 없다",
    "그 채널도 수동",
    "workflow_dispatch",
];

/// 위 규칙으로도 걸러지지 않는 예외 — (경로, 창에 함께 있어야 하는 말, 이유).
///
/// 파일 통째를 빼지 않는다: 경로만으로 면제하면 그 파일이 나중에 들이는 진짜 위반까지
/// 함께 새어 나간다(`tests/no_todo_file_citation.rs` 와 같은 이유).
const ALLOWLIST: &[(&str, &str, &str)] = &[(
    "tests/ci_channel_claims_match_workflows.rs",
    "automatic_job_bodies",
    "이 가드 자신 — 검사 로직이 그 명령 문자열을 리터럴로 다룬다",
)];

/// `cargo test --workspace` 인용 지점 주변에서 "CI 가 돌린다" 는 표지를 찾는다.
///
/// **줄 단위로 보지 않는다** — 주석과 마크다운은 문장을 예사로 줄바꿈하고, 실제로
/// `tests/macos_bundle_codesign.rs` 는 명령과 "채널" 을 다른 줄에 뒀다. 줄로 끊어 보면
/// 그런 주장이 그대로 빠져나간다. 그래서 인용 지점 앞뒤 창을 한 덩어리로 읽는다.
const CLAIM_WINDOW: usize = 220;

/// CI 가 자동으로 돌린다는 뜻으로 읽히는 표지.
const CI_MARKERS: &[&str] = &[
    "(CI)",
    "CI 강제",
    "CI 채널",
    "CI channel",
    "CI 의",
    "CI 에서",
    "Linux CI",
    "test.yml",
    ".github/workflows",
];

/// 인용된 명령이 **좁혀진 조합**인가 — `--lib`/`--bins`/`--test` 로 좁힌 형태는 실제로
/// 자동으로 돈다(`crossplatform-check.yml` 의 Windows·headless 잡, semver 가드).
fn is_narrowed(tail: &str) -> bool {
    tail.split_whitespace()
        .take(4)
        .any(|w| w.starts_with("--lib") || w.starts_with("--bins") || w.starts_with("--test"))
}

/// 텍스트에서 "전체 스위트를 CI 가 돌린다" 는 주장의 바이트 오프셋들.
fn claim_offsets(text: &str, exempt: &[&str]) -> Vec<usize> {
    const NEEDLE: &str = "cargo test --workspace";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(NEEDLE) {
        let at = from + rel;
        from = at + NEEDLE.len();
        if is_narrowed(&text[from..]) {
            continue;
        }
        let lo = text[..at]
            .char_indices()
            .rev()
            .nth(CLAIM_WINDOW)
            .map_or(0, |(i, _)| i);
        let hi = text[from..]
            .char_indices()
            .nth(CLAIM_WINDOW)
            .map_or(text.len(), |(i, _)| from + i);
        let window = &text[lo..hi];
        if !CI_MARKERS.iter().any(|m| window.contains(m)) {
            continue;
        }
        // 같은 창이 부재를 함께 말하면 정당한 서술이다.
        if ABSENCE_MARKERS.iter().any(|m| window.contains(m)) {
            continue;
        }
        if exempt.iter().any(|e| window.contains(e)) {
            continue;
        }
        found.push(at);
    }
    found
}

/// 바이트 오프셋 → 1-기준 줄 번호.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].lines().count().max(1)
}

/// 자동 트리거를 가진 워크플로의, `workflow_dispatch` 로 좁혀지지 않은 잡 본문.
///
/// yml 을 파싱하지 않고 들여쓰기로 잡 경계를 잡는다 — 이 레포의 워크플로는 전부
/// `jobs:` 아래 2 칸 들여쓰기의 평평한 잡 목록이고, 파싱기를 들이는 것보다 이 구조를
/// 깨뜨렸을 때 눈에 띄는 편이 낫다.
fn automatic_job_bodies(workflows: &Path) -> Vec<String> {
    let mut bodies = Vec::new();
    let Ok(entries) = std::fs::read_dir(workflows) else {
        panic!("워크플로 디렉토리를 읽지 못했다: {}", workflows.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        // 트리거 판정: `on:` 블록에 push/pull_request/schedule 중 하나라도 있으면 자동.
        let head: String = text
            .lines()
            .take_while(|l| !l.starts_with("jobs:"))
            .collect();
        if !(head.contains("push:") || head.contains("pull_request:") || head.contains("schedule:"))
        {
            continue;
        }
        let mut in_jobs = false;
        let mut current = String::new();
        for line in text.lines() {
            if line.starts_with("jobs:") {
                in_jobs = true;
                continue;
            }
            if !in_jobs {
                continue;
            }
            let is_job_head = line.starts_with("  ")
                && !line.starts_with("   ")
                && line.trim_end().ends_with(':');
            if is_job_head {
                if !current.is_empty() {
                    bodies.push(std::mem::take(&mut current));
                }
            }
            current.push_str(line);
            current.push('\n');
        }
        if !current.is_empty() {
            bodies.push(current);
        }
    }
    bodies
        .into_iter()
        .filter(|body| !body.contains("github.event_name == 'workflow_dispatch'"))
        .collect()
}

/// 자동으로 도는 잡이 전체 스위트를 돌리는가.
fn ci_actually_runs_the_full_suite(root: &Path) -> bool {
    automatic_job_bodies(&root.join(".github/workflows"))
        .iter()
        .any(|body| {
            body.split("cargo test --workspace").skip(1).any(|tail| {
                !tail.split_whitespace().take(4).any(|w| {
                    w.starts_with("--lib") || w.starts_with("--bins") || w.starts_with("--test")
                })
            })
        })
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("디렉토리를 읽지 못했다: {}", dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) || name.starts_with('.') && name != ".github" {
                continue;
            }
            collect_files(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TEXT_EXTS.contains(&e))
        {
            out.push(path);
        }
    }
}

/// 문서가 "CI 가 전체 스위트를 돌린다" 고 말하면, 실제로 그런지 워크플로와 대조한다.
#[test]
fn no_file_claims_ci_runs_the_full_suite_while_it_does_not() {
    let root = repo_root();
    if ci_actually_runs_the_full_suite(&root) {
        // 전체 스위트가 자동 채널에 올라갔다 — 주장이 참이 됐으므로 검사할 것이 없다.
        return;
    }

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    assert!(
        !files.is_empty(),
        "스캔 대상 파일이 하나도 없다 — 수집이 깨졌다"
    );

    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let exempt: Vec<&str> = ALLOWLIST
            .iter()
            .filter(|(p, _, _)| rel_str == *p)
            .map(|(_, needle, _)| *needle)
            .collect();
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for at in claim_offsets(&text, &exempt) {
            violations.push(format!("{rel_str}:{}", line_of(&text, at)));
        }
    }

    assert!(
        violations.is_empty(),
        "전체 스위트(`cargo test --workspace`)는 자동으로 돌지 않는다 — `test.yml` 의 \
         `test-linux-x64` 는 `workflow_dispatch` 전용이다. 아래는 그것을 CI 강제 장치로 \
         서술한 자리다. 실제 채널은 `docs/dev-guide/ci-gates.md` 를 보고, 서술을 \
         '자동 채널 없음' 으로 고쳐라:\n  {}",
        violations.join("\n  ")
    );
}
