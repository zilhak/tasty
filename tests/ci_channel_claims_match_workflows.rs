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
//!
//! **두 번째 축 — 명령을 적지 않는 형태.** 위 검사는 명령 리터럴 주변만 본다. 그런데
//! 같은 거짓말이 명령 없이, 테스트 파일 이름과 집행 주장만으로도 쓰인다. 이 축이 보는
//! 구분은 "CI 가 도는가" 가 아니라 **그 테스트가 자동 잡의 사정거리 안에 있는가** 다:
//! 자동 잡은 `--lib --bins`(= `src/` 안의 유닛 테스트)와 `--test <이름>` 으로 이름을
//! 지목한 것만 돌린다. 그러므로 **`tests/*.rs` 통합 테스트**를 자동 집행 장치로 부르는
//! 서술은 그 열거에 이름이 없는 한 거짓이고, 반대로 **lib 유닛 테스트**에 붙은 같은
//! 서술은 참이다 — 이 가드는 후자를 건드리지 않는다.
//!
//! 열거는 이 파일이 복사해 갖고 있지 않고 워크플로에서 **런타임에** 읽는다. 복사본을
//! 들면 워크플로가 바뀐 날 가드가 조용히 낡는다.
//!
//! **세 번째 축 — 반대 방향.** `--lib --bins` 잡은 자동으로 돈다. 그러므로 `src/` 안의
//! 유닛 테스트를 두고 부재를 적으면 그것도 거짓이다(사실보다 약하다). 강한 부정은 강한
//! 긍정만큼 검증이 필요하다 — 한 방향만 잡는 가드는 틀린 방향 하나를 굳힌다.
//!
//! **가드가 막지 못하는 것** — 조용히 통과하는 형태를 적어 둔다. 여기 적힌 것은 사각인
//! 줄 알고 남긴 것이고, 적히지 않은 형태가 새 사각이다.
//!
//! - **대상을 특정하지 않은 집행 서술** — 테스트 이름도 명령도 없이 "CI 가 잡아 준다"
//!   라고만 쓴 문장. 무엇을 가리키는지 텍스트만으로 결정할 수 없어 판정할 대상이 없다.
//!   이름을 요구하지 않고 표지만으로 짚으면 정확히 쓴 문장까지 함께 걸리고, 그 오탐을
//!   피하려 표지를 좁히면 결국 아무것도 안 잡는다. 그래서 **판정하지 않고 통과시킨다** —
//!   다만 그런 문장은 리뷰에서 "무엇이 그걸 돌리나" 를 되물어야 한다.
//! - **한 문장이 두 축을 묶은 형태** — "cognitive 복잡도와 파일 SLOC 의 신규분을 자동
//!   차단한다" 처럼 참인 축과 거짓인 축이 한 문장에 섞인 경우. 참/거짓이 문장 단위로
//!   갈리지 않아 짚을 좌표가 없고, 문장을 통째로 위반으로 부르면 참인 절반까지 지우게
//!   된다. **판정하지 않는다** — 대신 채널 정본이 축별 표를 갖고, 파생 문서는 채널을 다시
//!   서술하지 말고 정본을 링크한다(`docs/dev-guide/ci-gates.md`). 이 한계는 규칙으로
//!   메울 수 없어서 문서 관행으로 메운다.
//! - **강제 수단이 워크플로 밖에 있는 것** — clippy `deny`·`#[deny]`·pre-commit·타입
//!   시스템은 워크플로를 읽어서는 보이지 않는다. 그래서 이 가드의 "자동으로 돌지
//!   않는다" 는 **워크플로 채널에 한한 말**이고, "아무도 안 막는다" 는 뜻이 아니다.
//! - **문서 밖의 주장** — 커밋 메시지·PR 본문·티켓은 스캔 대상이 아니다.

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
    // 실행 축을 명시한 정밀한 형태 — 컴파일은 자동이라는 사실을 함께 남기려면 이렇게
    // 써야 하므로, 부재 표지도 이 형태를 알아야 한다.
    "실행 채널이 없",
    "실행 채널 없음",
    "실행은 수동",
];

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

/// 명령 문자열 없이 "CI 가 이 장치를 돌린다" 는 뜻으로 읽히는 표지.
///
/// [`CI_MARKERS`] 와 목록이 다르다. 저기엔 `test.yml` 같은 **참조**가 들어 있는데,
/// 워크플로를 가리키는 것 자체는 주장이 아니다 — 명령이 함께 있을 때만 주장이 된다.
/// 이 축은 명령이 없으므로 "강제한다/잡는다" 는 **집행 주장**만 표지로 삼는다.
const ENFORCE_MARKERS: &[&str] = &[
    "CI 강제",
    "CI 에서 강제",
    "CI 가 강제",
    "CI 로 강제",
    "CI 가 잡",
    "CI 에서 잡",
    "CI 가 막",
    "CI 에서 막",
    "CI 가 차단",
    "CI 에서 차단",
    "CI fail",
    "CI 가 fail",
    "CI 에서 fail",
    "(CI)",
];

/// 그 문장이 주장하는 것이 **실행이 아니라 컴파일/검사**인가.
///
/// 통합 테스트에 대해 자동 잡이 하는 일은 둘로 갈린다: **실행은 안 하지만 컴파일은
/// 한다**(Windows·headless 의 `clippy --all-targets` 가 `tests/*.rs` 를 타깃으로 잡는다).
/// 그래서 "이 테스트를 돌린다" 는 거짓이지만 "컴파일은 본다" 는 참이다. 이 구분을 빼면
/// 가드가 **참인 문장을 지우게 만든다** — 이 파일이 막으려는 실패의 거울상이다.
const COMPILE_CLAIM_MARKERS: &[&str] = &["컴파일", "clippy", "빌드", "compile"];

/// 그 자리가 **자동 채널을 이미 긍정하고 있는가** — 역방향 검사의 면제 조건.
///
/// 한 문장이 두 가드의 채널을 대비해 설명하면(이쪽은 자동으로 돈다, 저쪽은 아니다)
/// 창 안에 부재 표지와 lib 테스트 이름이 함께 놓인다. 그건 약하게 쓴 것이 아니라
/// **정확하게** 쓴 것이다.
const AUTOMATIC_CHANNEL_MARKERS: &[&str] = &[
    "--lib --bins",
    "자동으로 돈다",
    "자동으로 돌린다",
    "crossplatform-check",
];

/// lib 유닛 테스트 이름 추출의 하한 — 추출이 깨지면 역방향 검사가 통째로 잠잠해진다.
const MIN_LIB_TESTS: usize = 100;

/// 스캔 하한 — 수집이나 인용 추출이 조용히 줄어드는 것을 잡는다.
///
/// 가드가 "위반 0" 을 보고하는 이유는 두 가지다: 정말 없거나, **아무것도 안 봤거나.**
/// 둘을 구분하지 않으면 스캔이 깨진 날 초록이 뜬다.
const MIN_SCANNED_FILES: usize = 400;
/// 같은 이유의 하한 — 통합 테스트 파일 인용 지점 수.
const MIN_TEST_CITATIONS: usize = 40;

/// 인용된 명령이 **좁혀진 조합**인가 — `--lib`/`--bins`/`--test` 로 좁힌 형태는 실제로
/// 자동으로 돈다(`crossplatform-check.yml` 의 Windows·headless 잡, semver 가드).
fn is_narrowed(tail: &str) -> bool {
    logical_command(tail)
        .split_whitespace()
        .any(|w| w.starts_with("--lib") || w.starts_with("--bins") || w.starts_with("--test"))
}

/// 인용 지점부터 **그 명령이 끝나는 곳까지** — YAML 의 백슬래시 연속행을 이어 붙인다.
///
/// 판정 범위를 상수로 잡지 않는다. 이 함수의 앞선 두 형태가 각각 상수 하나씩을 들고
/// 있다가 밀렸다: 앞 N 단어만 보던 형태는 `--no-fail-fast` 가 추가된 날 좁힘을 놓쳤고,
/// 그것을 고치며 들어온 "물리적 한 줄" 형태는 연속행에 놓인 플래그를 못 본다. 두 번째
/// 형태의 주석은 "명령 끝까지 본다" 고 적고 있었는데 — **고정 개수를 지웠을 뿐 고정
/// 범위를 안 지웠다.** 끝나는 자리를 셸과 같은 규칙(줄 끝 `\` 가 없으면 거기서 끝)으로
/// 구조에서 끌어내면 남는 상수가 없다.
///
/// 다음 YAML 스텝까지 끌어오지는 않는다 — 연속 표시가 없는 줄에서 멈추므로, 아래에
/// 놓인 다른 `run:` 의 플래그가 이 명령의 판정에 섞이지 않는다.
fn logical_command(tail: &str) -> String {
    let mut out = String::new();
    for line in tail.lines() {
        let trimmed = line.trim_end();
        let continues = trimmed.ends_with('\\');
        out.push_str(trimmed.trim_end_matches('\\'));
        out.push(' ');
        if !continues {
            break;
        }
    }
    out
}

/// 텍스트에서 "전체 스위트를 CI 가 돌린다" 는 주장의 바이트 오프셋들.
fn claim_offsets(text: &str, path: &str) -> Vec<usize> {
    const NEEDLE: &str = "cargo test --workspace";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(NEEDLE) {
        let at = from + rel;
        from = at + NEEDLE.len();
        if is_narrowed(&text[from..]) {
            continue;
        }
        // 주장은 산문이다. Rust 소스의 문자열 리터럴·식별자는 검사 로직 자신일 뿐이라
        // 여기서 걸러진다 — 그래서 이 가드는 자기 자신을 위한 경로 면제가 필요 없다.
        if !is_prose_line(text, at, path) {
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
        // 같은 서술이 부재를 함께 말하면 정당하다.
        let scope = claim_scope(text, at);
        if ABSENCE_MARKERS.iter().any(|m| scope.contains(m)) {
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

/// 잡 본문에서 `cargo test` 호출 하나하나의 **인자 꼬리**를 뽑는다.
///
/// 스텝 경계(`- name:`)까지를 한 호출로 본다 — 워크플로가 `run: >` 접힘 문법으로 한
/// 명령을 여러 줄에 걸쳐 쓰기 때문에 줄 단위로 끊으면 `--test` 열거가 잘려 나간다.
fn cargo_test_tails(body: &str) -> Vec<String> {
    let mut tails = Vec::new();
    let mut from = 0;
    while let Some(rel) = body[from..].find("cargo test") {
        let at = from + rel + "cargo test".len();
        from = at;
        let end = body[at..]
            .find("- name:")
            .map_or(body.len(), |off| at + off);
        tails.push(body[at..end].to_string());
    }
    tails
}

/// 자동 잡이 **이름을 지목해** 돌리는 통합 테스트 이름들.
///
/// `None` 은 "이 축이 성립하지 않는다" 는 뜻이다 — 자동 잡 중 하나가 좁혀지지 않은
/// `cargo test` 를 돌리면 통합 테스트가 전부 자동으로 도는 것이므로 어떤 인용도 거짓이
/// 아니다. 그때 이 가드는 첫 번째 축과 같은 방식으로 스스로 잠잠해진다.
fn integration_tests_run_automatically(root: &Path) -> Option<std::collections::BTreeSet<String>> {
    let mut named = std::collections::BTreeSet::new();
    for body in automatic_job_bodies(&root.join(".github/workflows")) {
        for tail in cargo_test_tails(&body) {
            let words: Vec<&str> = tail.split_whitespace().collect();
            let narrowed = words
                .iter()
                .any(|w| w.starts_with("--lib") || w.starts_with("--bins") || *w == "--test");
            if !narrowed {
                return None;
            }
            for pair in words.windows(2) {
                if pair[0] == "--test" {
                    named.insert(pair[1].to_string());
                }
            }
        }
    }
    Some(named)
}

/// 경로가 통합 테스트면 그 테스트 이름 — `tests/X.rs` 와 `crates/<c>/tests/X.rs` 둘 다.
fn integration_test_name(rel: &str) -> Option<&str> {
    let after = rel.rsplit_once("tests/")?.1;
    if after.contains('/') {
        return None;
    }
    after.strip_suffix(".rs")
}

/// 텍스트가 지목하는 통합 테스트 인용 지점 — (오프셋, 이름).
fn cited_tests(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find("tests/") {
        let at = from + rel;
        from = at + "tests/".len();
        let rest = &text[from..];
        let end = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
            .map_or(rest.len(), |(i, _)| i);
        if rest[end..].starts_with(".rs") {
            found.push((at, rest[..end].to_string()));
        }
    }
    found
}

/// `src/` 안의 lib 유닛 테스트 이름(`#[test]` 가 붙은 함수).
///
/// 이 목록이 역방향 판정의 축이다 — **문자열이 아니라 대상의 위치**로 가른다. 똑같은
/// 집행 서술이라도 그 테스트가 여기 있으면 참이고 `tests/` 아래 있으면 거짓이다.
fn lib_test_names(root: &Path) -> std::collections::BTreeSet<String> {
    let mut files = Vec::new();
    collect_files(root, &mut files);
    let mut names = std::collections::BTreeSet::new();
    for file in files {
        let rel = file.strip_prefix(root).unwrap_or(&file);
        let rel = rel.to_string_lossy().replace('\\', "/");
        if !rel.ends_with(".rs") || !(rel.starts_with("src/") || rel.contains("/src/")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim() != "#[test]" {
                continue;
            }
            for next in lines.iter().skip(i + 1).take(4) {
                let t = next.trim_start();
                let t = t.strip_prefix("async ").unwrap_or(t);
                if let Some(rest) = t.strip_prefix("fn ")
                    && let Some(name) = rest.split(['(', '<']).next()
                    && !name.is_empty()
                {
                    names.insert(name.to_string());
                    break;
                }
            }
        }
    }
    names
}

/// 부재 표지가 놓인 오프셋들.
fn absence_offsets(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for marker in ABSENCE_MARKERS {
        let mut from = 0;
        while let Some(rel) = text[from..].find(marker) {
            let at = from + rel;
            from = at + marker.len();
            found.push(at);
        }
    }
    found.sort_unstable();
    found
}

/// `text` 안에서 `name` 이 **낱말로** 등장하는 오프셋들.
fn word_offsets(text: &str, name: &str) -> Vec<usize> {
    let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(name) {
        let at = from + rel;
        from = at + name.len();
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        if boundary(before) && boundary(after) {
            found.push(at);
        }
    }
    found
}

/// 주장이 놓인 **한 서술의 범위** — 표는 그 행, 산문은 그 문단, Rust 는 이어진 주석
/// 블록. 면제도 표지 탐색도 이 범위에서 한다.
///
/// **면제는 좁게, 검출은 넓게** 가 원칙이지만, 둘의 단위가 어긋나면 반대로 샌다.
/// 실측한 두 형태가 그것이다.
/// - 면제만 넓으면: 문서 끝에 붙인 실행 주장이, 앞 절에 있던 "컴파일" 한 단어로
///   면제됐다.
/// - 검출만 넓으면: 표의 **옆 행**에 있는 표지를 끌어와 정확히 쓴 행을 위반으로 짚었다.
///
/// 그래서 단위를 하나로 맞춘다. 셋으로 갈리는 이유:
/// - **표 행**(`|` 로 시작)과 **목록 항목**(`-`/`*`/`1.` 로 시작) — 한 행·한 항목이 곧
///   한 서술인데 사이에 빈 줄이 없어, 문단으로 묶으면 표나 목록 전체가 한 덩어리가
///   된다. 실측으로 옆 항목의 표지가 끌려와 오탐이 났다. 항목의 접힌 줄(들여쓴 이어짐)
///   은 그 항목에 붙인다.
/// - **문단**(빈 줄 사이) — 마크다운 산문은 한 문장을 예사로 줄바꿈한다. 줄로 끊으면
///   "그 잡은 수동 / 전용이라" 처럼 정확히 쓴 서술이 반토막 난다.
/// - **주석 블록** — Rust doc 도 같은 이유. 빈 주석 줄(`//!` 뿐)이 문단 경계다.
fn claim_scope(text: &str, at: usize) -> &str {
    let line_start = |i: usize| text[..i].rfind('\n').map_or(0, |j| j + 1);
    let line_end = |i: usize| text[i..].find('\n').map_or(text.len(), |j| i + j);
    let kind = |start: usize| -> u8 {
        let line = text[start..line_end(start)].trim_start();
        if line.starts_with('|') {
            0 // 표 행 — 혼자 선다
        } else if line.starts_with("//!") || line.starts_with("///") {
            if line.trim_end().len() <= 3 { 2 } else { 1 } // 주석 / 빈 주석(경계)
        } else if line.trim().is_empty() {
            2 // 빈 줄 — 경계
        } else {
            3 // 산문
        }
    };

    let starts_item = |start: usize| {
        let line = text[start..line_end(start)].trim_start();
        line.starts_with("- ")
            || line.starts_with("* ")
            || line.split_once(". ").is_some_and(|(head, _)| {
                !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit())
            })
    };

    let mut lo = line_start(at);
    let mut hi = line_end(at);
    let here = kind(lo);
    if here == 0 || here == 2 {
        return &text[lo..hi];
    }
    while lo > 0 && !starts_item(lo) {
        let prev = line_start(lo - 1);
        if kind(prev) != here {
            break;
        }
        lo = prev;
    }
    while hi < text.len() {
        let next = hi + 1;
        if next >= text.len() || kind(next) != here || starts_item(next) {
            break;
        }
        hi = line_end(next);
    }
    &text[lo..hi]
}

/// 그 오프셋이 **산문**에 있는가 — Rust 소스에서는 주석 줄만.
///
/// 주장은 사람이 읽는 문장이지 코드가 아니다. 이 구분이 없으면 표지 목록을 문자열
/// 리터럴로 들고 있는 파일(이 가드 자신이 그렇다)이 자기 목록에 걸린다.
fn is_prose_line(text: &str, at: usize, rel: &str) -> bool {
    if !rel.ends_with(".rs") {
        return true;
    }
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    text[start..end].trim_start().starts_with("//")
}

/// `at` 주변 창 — 첫 번째 축과 같은 폭으로 읽는다.
fn window_around(text: &str, at: usize) -> &str {
    let lo = text[..at]
        .char_indices()
        .rev()
        .nth(CLAIM_WINDOW)
        .map_or(0, |(i, _)| i);
    let hi = text[at..]
        .char_indices()
        .nth(CLAIM_WINDOW)
        .map_or(text.len(), |(i, _)| at + i);
    &text[lo..hi]
}

/// 한 파일의 **집행 주장 위반**과 인용 수 — 파일 순회·경로 처리와 분리된 판정기.
///
/// 순수 함수인 이유: 면제를 겨냥한 변이를 **합성 문자열**로 찌를 수 있어야 하기
/// 때문이다. 판정이 순회 루프 안에 있으면 변이가 "레포에 진짜 위반을 심는" 방식으로만
/// 가능해지고, 그건 느리고 트리를 더럽히며 되돌리다 사고가 난다.
fn enforcement_violations(
    text: &str,
    path: &str,
    automatic: &std::collections::BTreeSet<String>,
) -> (Vec<usize>, usize) {
    let mut candidates = cited_tests(text);
    let cited = candidates.len();
    // 자기 자신을 "이 테스트가 …" 로만 부르는 형태 — 파일 안에 자기 경로가 없어서 위
    // 인용 추출로는 잡히지 않는다. 통합 테스트 파일이면 집행 표지가 놓인 자리마다 자기
    // 이름을 인용한 것으로 본다.
    if let Some(own) = integration_test_name(path) {
        for marker in ENFORCE_MARKERS {
            let mut from = 0;
            while let Some(off) = text[from..].find(marker) {
                let at = from + off;
                from = at + marker.len();
                candidates.push((at, own.to_string()));
            }
        }
    }

    let mut found = Vec::new();
    for (at, name) in candidates {
        if automatic.contains(&name) {
            continue;
        }
        if !is_prose_line(text, at, path) {
            continue;
        }
        // 표지도 같은 범위에서 찾는다 — 옆 행·옆 항목의 표지를 끌어오면 정확히 쓴
        // 서술이 위반으로 걸린다.
        let scope = claim_scope(text, at);
        if !ENFORCE_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        if ABSENCE_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        // 실행이 아니라 컴파일을 주장하는 문장은 참이다 — 자동 잡의 `--all-targets`
        // clippy 가 통합 테스트 타깃을 컴파일한다.
        if COMPILE_CLAIM_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        found.push(at);
    }
    found.sort_unstable();
    found.dedup();
    (found, cited)
}

/// 한 파일에서 **lib 테스트를 두고 부재를 적은 자리** — 역방향 판정기.
fn weak_absence_offsets(
    text: &str,
    path: &str,
    lib_tests: &std::collections::BTreeSet<String>,
) -> Vec<usize> {
    // 정의 파일 자신은 대상이 아니다 — 거기 이름이 있는 것은 서술이 아니라 정의다.
    if path.starts_with("src/") || path.contains("/src/") {
        return Vec::new();
    }
    let mut found = Vec::new();
    // 부재를 적은 자리에서 출발한다 — lib 테스트 이름 전체를 파일마다 훑으면 이름 수 x
    // 파일 수가 되어 스캔이 느려지고, 얻는 것은 같다.
    for at in absence_offsets(text) {
        if !is_prose_line(text, at, path) {
            continue;
        }
        let scope = claim_scope(text, at);
        if AUTOMATIC_CHANNEL_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        if lib_tests.iter().any(|n| !word_offsets(scope, n).is_empty()) {
            found.push(at);
        }
    }
    found
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
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for at in claim_offsets(&text, &rel_str) {
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

/// 문서가 어떤 통합 테스트를 자동 집행 장치로 부르면, 자동 잡이 실제로 그 이름을
/// 돌리는지 워크플로에서 읽어 대조한다.
#[test]
fn no_file_claims_ci_enforces_an_integration_test_it_does_not_run() {
    let root = repo_root();
    let Some(automatic) = integration_tests_run_automatically(&root) else {
        // 자동 잡이 좁혀지지 않은 `cargo test` 를 돌린다 — 통합 테스트가 전부 자동으로
        // 도는 것이므로 이 축은 성립하지 않는다.
        return;
    };

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    assert!(
        files.len() >= MIN_SCANNED_FILES,
        "스캔한 파일이 {}개뿐이다(하한 {MIN_SCANNED_FILES}) — 수집이 줄었다",
        files.len()
    );

    let mut citations = 0usize;
    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };

        let (found, cited) = enforcement_violations(&text, &rel_str, &automatic);
        citations += cited;
        for at in found {
            violations.push(format!("{rel_str}:{}", line_of(&text, at)));
        }
    }

    assert!(
        citations >= MIN_TEST_CITATIONS,
        "통합 테스트 인용을 {citations}개밖에 못 찾았다(하한 {MIN_TEST_CITATIONS}) — 추출이 깨졌다"
    );

    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "자동 잡이 이름을 지목해 돌리는 통합 테스트는 {automatic:?} 뿐이다(나머지 자동 \
         테스트는 `--lib --bins` = 유닛 뿐). 아래는 그 밖의 통합 테스트를 CI 강제 장치로 \
         서술한 자리다. 문장을 지우지 말고, 그 문장이 전하려던 사실은 남긴 채 채널 주장만 \
         `docs/dev-guide/ci-gates.md` 에 맞춰라:\n  {}",
        violations.join("\n  ")
    );
}

/// 반대 방향 — **lib 유닛 테스트**를 두고 "자동 채널이 없다" 고 적은 자리.
///
/// `--lib --bins` 잡은 main push·PR 에서 자동으로 돈다. 그러므로 그 안에 있는 테스트를
/// 두고 부재를 적으면 사실보다 **약하다**. 강한 부정("없다")은 강한 긍정만큼 틀릴 수
/// 있고, 틀린 방향만 다를 뿐 다음 사람의 판단을 망치는 것은 같다.
#[test]
fn no_file_denies_the_automatic_channel_a_lib_test_actually_has() {
    let root = repo_root();
    let lib_tests = lib_test_names(&root);
    assert!(
        lib_tests.len() >= MIN_LIB_TESTS,
        "lib 테스트 이름을 {}개밖에 못 찾았다(하한 {MIN_LIB_TESTS}) — 추출이 깨졌다",
        lib_tests.len()
    );

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        // 부재를 적은 자리에서 출발한다 — lib 테스트 이름 전체를 파일마다 훑으면
        // 이름 수 x 파일 수가 되어 스캔이 느려지고, 얻는 것은 같다.
        for at in weak_absence_offsets(&text, &rel_str, &lib_tests) {
            violations.push(format!("{rel_str}:{}", line_of(&text, at)));
        }
    }

    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "아래는 **lib 유닛 테스트**를 두고 자동 채널의 부재를 적은 자리다. 그 테스트는 \
         `crossplatform-check.yml` 의 `cargo test --workspace --lib --bins`(main push · PR)로 \
         자동으로 돈다 — 서술이 사실보다 약하다. 채널 정본은 `docs/dev-guide/ci-gates.md`:\n  {}",
        violations.join("\n  ")
    );
}

/// 회귀 케이스 — **한 표 안에서 채널이 갈리는 행들.**
///
/// `docs/design/systems/theme.md` 의 토큰 규칙 표는 네 자리에서 가드를 인용하는데, 셋은
/// 통합 테스트(`tests/design_token_adherence.rs`)이고 하나는 lib 유닛 테스트다. 문자열만
/// 보고 일괄 처리하면 넷이 같아 보여서 **맞게 적힌 행까지 함께 지워진다.** 이 테스트는
/// 그 표가 대상별로 갈린 상태를 유지하는지 고정한다.
#[test]
fn the_theme_table_keeps_the_two_channels_apart() {
    let path = repo_root().join("docs/design/systems/theme.md");
    let text = std::fs::read_to_string(&path).expect("theme.md 를 읽지 못했다");

    let integration = word_offsets(&text, "design_token_adherence");
    assert!(
        !integration.is_empty(),
        "표가 통합 테스트 가드를 더 이상 인용하지 않는다 — 회귀 케이스가 사라졌다"
    );
    for at in integration {
        let window = window_around(&text, at);
        assert!(
            ABSENCE_MARKERS.iter().any(|m| window.contains(m)),
            "{}:{} — 통합 테스트인데 자동 채널의 부재가 함께 적혀 있지 않다",
            path.display(),
            line_of(&text, at)
        );
    }

    let lib = word_offsets(&text, "ui_font_size_tokens_are_integers_at_every_zoom");
    assert!(
        !lib.is_empty(),
        "표가 lib 유닛 테스트 행을 더 이상 인용하지 않는다 — 양방향성의 증거가 사라졌다"
    );
    for at in lib {
        let window = window_around(&text, at);
        assert!(
            AUTOMATIC_CHANNEL_MARKERS.iter().any(|m| window.contains(m)),
            "{}:{} — lib 유닛 테스트인데 자동 채널이 적혀 있지 않다(사실보다 약하다)",
            path.display(),
            line_of(&text, at)
        );
    }
}

// ─── 면제를 겨냥한 변이 (합성 입력) ───────────────────────────────────────────
//
// 면제를 하나 추가할 때마다 **그 면제를 겨냥한 변이**를 같이 넣는다. "면제 범위 안쪽에
// 진짜 위반을 심었을 때 잡히는가" 를 묻는 것이고, 검증하지 않은 면제는 그 면제만큼
// 구멍이다. 실측으로 두 번 샜다 — 한 번은 앞 절의 컴파일 언급이 면제로 작동해 위반을
// 가렸고, 한 번은 표의 옆 행 표지를 끌어와 정확히 쓴 행을 위반으로 짚었다.
//
// 판정기가 순수 함수라 합성 문자열로 찌른다. 레포에 진짜 위반을 심는 방식은 느리고
// 트리를 더럽히며 되돌리다 사고가 난다.
//
// **픽스처의 표지는 조각으로 조립한다.** 통째로 적으면 이 파일 자신이 위반으로 잡힌다 —
// 그것을 경로 면제로 덮으면 면제가 하나 늘고, 그 파일이 나중에 들이는 진짜 위반까지
// 함께 새어 나간다. 조립하면 면제 없이 닫힌다.
fn enforce() -> String {
    format!("CI 가 {}", "강제한다")
}

/// 자동 잡이 이름으로 지목하는 통합 테스트가 하나도 없는 상태.
fn no_named_tests() -> std::collections::BTreeSet<String> {
    std::collections::BTreeSet::new()
}

fn named(names: &[&str]) -> std::collections::BTreeSet<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn an_absence_marker_in_a_neighbouring_table_row_does_not_exempt() {
    let text = format!(
        "| 포맷 | `tests/a_guard.rs` 가 강제한다 — 자동 채널이 없다 |\n| 린트 | `tests/b_guard.rs` 를 {} |\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "옆 행의 부재 표지가 면제로 작동했다");
    assert_eq!(line_of(&text, found[0]), 2);
}

#[test]
fn an_absence_marker_in_a_neighbouring_list_item_does_not_exempt() {
    let text = format!(
        "- `tests/a_guard.rs` 가 강제한다 — 자동 채널이 없다.\n- `tests/b_guard.rs` 를 {}.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "옆 항목의 부재 표지가 면제로 작동했다");
    assert_eq!(line_of(&text, found[0]), 2);
}

#[test]
fn a_compile_marker_in_another_paragraph_does_not_exempt() {
    // 실측한 누수 그대로: 앞 문단이 컴파일을 언급하고, 뒤 문단이 실행을 주장한다.
    let text = format!(
        "자동 잡의 clippy 는 통합 테스트를 컴파일한다.\n\n`tests/b_guard.rs` 를 {}.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "앞 문단의 컴파일 언급이 면제로 작동했다");
}

#[test]
fn a_compile_claim_in_the_same_sentence_is_true_and_exempt() {
    let text = format!(
        "`tests/b_guard.rs` 를 {} — 다만 그것은 컴파일 검사다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert!(found.is_empty(), "참인 컴파일 주장을 위반으로 짚었다");
}

#[test]
fn a_wrapped_sentence_keeps_its_absence_marker_in_scope() {
    // 마크다운 산문은 한 문장을 예사로 줄바꿈한다 — 반토막 내면 정확히 쓴 서술이
    // 오탐이 된다.
    let text = format!(
        "`tests/b_guard.rs` 를 {} — 다만 그 잡은 수동\n전용이라 실행 채널이 없다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert!(found.is_empty(), "접힌 문장이 반토막 나 오탐이 났다");
}

#[test]
fn a_doc_comment_block_is_one_scope_but_a_blank_comment_line_splits_it() {
    let joined = format!(
        "//! `tests/b_guard.rs` 를 {} — 그 잡은 수동\n//! 전용이라 실행 채널이 없다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&joined, "src/some_module.rs", &no_named_tests());
    assert!(found.is_empty(), "이어진 주석 블록이 갈라졌다");

    let split = format!(
        "//! 그 잡은 수동 전용이라 실행 채널이 없다.\n//!\n//! `tests/b_guard.rs` 를 {}.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&split, "src/some_module.rs", &no_named_tests());
    assert_eq!(
        found.len(),
        1,
        "빈 주석 줄 너머의 부재 표지가 면제로 작동했다"
    );
}

#[test]
fn the_named_enumeration_exempts_only_the_names_the_workflow_lists() {
    let text = format!("`tests/api_baseline_0_7.rs` 를 {}.\n", enforce());
    let (found, _) = enforcement_violations(&text, "docs/x.md", &named(&["api_baseline_0_7"]));
    assert!(found.is_empty(), "열거된 이름을 위반으로 짚었다");

    // 워크플로에서 그 이름이 빠지면 같은 문장이 거짓이 된다 — 목록을 복사해 갖고 있지
    // 않다는 것이 이 대비로 드러난다.
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "열거가 사라졌는데 판정이 따라오지 않았다");
}

#[test]
fn a_code_literal_is_not_a_claim() {
    let text = format!(
        "    let needle = \"`tests/b_guard.rs` 를 {}\";\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "src/some_module.rs", &no_named_tests());
    assert!(found.is_empty(), "코드 리터럴을 서술로 읽었다");
}

#[test]
fn the_automatic_channel_marker_exempts_only_inside_the_same_row() {
    let libs = named(&["some_lib_test"]);

    let same_row = "| lib | 있다 — `--lib --bins` 가 돈다 | `some_lib_test` 자동 채널이 없다 |\n";
    assert!(
        weak_absence_offsets(same_row, "docs/x.md", &libs).is_empty(),
        "자동 채널을 함께 적은 행을 약한 서술로 짚었다"
    );

    let other_row =
        "| lib | 있다 — `--lib --bins` 가 돈다 |\n| 통합 | `some_lib_test` 는 자동 채널이 없다 |\n";
    assert_eq!(
        weak_absence_offsets(other_row, "docs/x.md", &libs).len(),
        1,
        "옆 행의 자동 채널 표지가 면제로 작동했다"
    );
}

// ─── 의도된 false negative (한계를 붙박는다) ─────────────────────────────────
//
// 못 잡는 것이 **의도**인 입력도 고정한다. 나중에 판정기를 넓힐 때 그 결정이 테스트
// 실패로 드러나야 하고, 안 적어 두면 의도된 한계와 버그가 구분되지 않는다.

#[test]
fn narrowing_is_seen_however_many_flags_precede_it() {
    // 좁힘 판정이 앞 몇 단어만 본다면, 플래그가 늘어난 날 조용히 밀린다.
    assert!(is_narrowed(" --workspace --lib --bins --locked\n"));
    assert!(is_narrowed(
        " --locked --no-fail-fast --frozen --offline --lib --bins\n"
    ));
    assert!(is_narrowed(
        " --locked --no-default-features --test api_baseline_0_7\n"
    ));
    // 좁혀지지 않은 전체 스위트는 그대로 잡힌다.
    assert!(!is_narrowed(" --locked --no-fail-fast\n"));
    // 다음 줄의 좁힘을 끌어오지 않는다.
    assert!(!is_narrowed(
        " --locked\n      - name: other\n        run: cargo test --lib\n"
    ));
}

/// 좁힘이 **백슬래시 연속행**에 놓여도 본다. YAML 의 `run:` 은 긴 명령을 줄로 나누는 것이
/// 관례이고, 물리적 한 줄만 읽으면 그 형태에서 좁혀진 스텝을 "안 좁혀졌다" 로 판정한다 —
/// 그러면 그 위에 선 채널 주장 판정이 통째로 뒤집힌다.
///
/// 이 테스트가 판정 범위를 붙박는 변이다. 범위를 다시 물리적 한 줄로 되돌리면 첫 두
/// 단언이 죽고, 반대로 줄 수 제한 없이 통째로 읽게 넓히면 마지막 단언이 죽는다.
#[test]
fn narrowing_is_seen_on_a_continuation_line() {
    assert!(is_narrowed(
        " --workspace --locked \\\n      --lib --bins\n"
    ));
    // 연속이 여러 번 이어져도 끝까지 따라간다.
    assert!(is_narrowed(
        " --workspace \\\n      --locked \\\n      --no-fail-fast \\\n      --test api_baseline_0_7\n"
    ));
    // 연속 표시가 끝난 **다음** 줄의 플래그는 이 명령의 것이 아니다.
    assert!(!is_narrowed(
        " --workspace \\\n      --locked\n      --lib --bins\n"
    ));
}

#[test]
fn a_claim_that_names_no_test_is_deliberately_not_judged() {
    let text = format!("이 규칙은 {}.\n", enforce());
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert!(
        found.is_empty(),
        "대상이 특정되지 않은 서술은 판정하지 않는다 — 짚을 좌표가 없다"
    );
}

#[test]
fn a_definition_file_under_src_is_deliberately_not_judged() {
    let libs = named(&["some_lib_test"]);
    let text = "//! `some_lib_test` 는 자동 채널이 없다.\n";
    assert!(
        weak_absence_offsets(text, "crates/x/src/lib.rs", &libs).is_empty(),
        "정의 파일의 이름 등장은 서술이 아니다"
    );
}
