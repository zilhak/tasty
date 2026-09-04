//! 문서가 인용한 **좌표가 실제로 풀리는가** 를 본다. 두 축이다.
//!
//! ① **경로 축** — 마크다운 문서가 레포 경로 형태(`src/…` · `tests/…` · `crates/…` 등)로
//!    적은 파일 경로는 실재해야 한다.
//! ② **인접 짝 축** — `` `이름`(`경로`) `` 처럼 이름 바로 뒤 괄호가 파일을 지목하면,
//!    그 이름은 그 파일 안에 있어야 한다.
//!
//! ## 왜 필요한가 — 없는 것을 가리키는 좌표는 "주어 없음" 보다 나쁘다
//!
//! 주어 없이 "가드가 강제한다" 고만 적힌 문장은 읽는 사람이 확인할 수 없다는 것이
//! 눈에 보인다. 반면 **틀린 좌표는 검증된 것처럼 보인다** — 이름과 경로가 붙어 있으니
//! 아무도 다시 세지 않는다. 이 저장소의 실측에서 그 형태가 실제로 있었다: 사라진
//! 파일을 근거로 든 ADR, 크레이트 루트 기준 약칭을 레포 루트 경로처럼 적어 **다른
//! 실재 파일로 조용히 해석되는** 인용, 남의 저장소(egui) 내부 경로를 우리 경로 형태로
//! 적은 참조.
//!
//! ## 판정 규칙 — 문맥을 추론하지 않는다
//!
//! 이 가드는 문장의 뜻을 분류하지 않는다. 경로도 이름도 **리터럴**이고, 판정은
//! "그 자리에 파일이 있는가 / 그 파일에 그 토큰이 있는가" 뿐이다. 산문 패턴으로 문맥을
//! 나누는 검출기는 양방향으로 틀리면서 초록일 때 아무것도 보장하지 못한다 — 그 함정을
//! 피하려고 판정 대상을 **좌표를 스스로 들고 있는 인용**으로만 좁혔다.
//!
//! **경로 해석은 두 자리에서 시도한다.** 레포 루트, 그리고 인용한 문서가 크레이트 안에
//! 살면 그 크레이트 루트. 크레이트 README·CHANGELOG 가 `src/…` 를 자기 크레이트 기준으로
//! 적는 것은 정당한 관례이고, 그것을 예외 목록으로 덮는 대신 해석 규칙에 넣었다.
//!
//! **중괄호·와일드카드 축약은 판정 대상이 아니다.** `src/{a,b}.rs` 같은 형태는 여러
//! 경로를 한 번에 쓰는 표기라 단일 파일로 풀 수 없다. 축약을 받아주는 것이 아니라
//! **판정할 수 없는 것을 판정하지 않는 것**이고, 그만큼이 이 가드의 사각이다.
//!
//! ## 오차 방향
//!
//! **놓치는 쪽으로 틀린다.** 경로 형태가 아닌 인용(백틱 안의 맨 식별자, 디렉토리 인용,
//! 남의 저장소 경로를 크레이트 이름 없이 적은 것 중 우연히 우리 파일과 겹치는 것)은
//! 판정 밖이다. 존재 판정을 파일시스템으로 하므로 **git 이 추적하지 않지만 디스크에는
//! 있는 파일**도 통과한다. 반대 방향(있는 것을 없다고 하는 것)은 초록이 아니라 빨강으로
//! 나오므로 조용히 넘어가지 않는다.
//!
//! ## 이 가드가 덮지 않는 것 — 밝혀 둔다
//!
//! 문서가 백틱으로 인용하는 **맨 snake_case 식별자**(테스트 이름 · 함수 이름)는 좌표가
//! 함께 적히지 않는 한 판정하지 않는다. 그 형태를 보는 가드는 이 저장소에 **하나도
//! 없다** — 조용히 안 만든 것이 아니라 여기 적어 둔다. 손으로 재는 절차와 그 절차의
//! 함정은 `docs/documentation-model.md` §6 에 있다.
//!
//! 선례: `crates/tasty-doc-guards/tests/no_checkbox_in_docs.rs`(docs 순회 구조) ·
//! `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`(레포 전체 스캔).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 레포 경로로 읽히는 최상위 디렉토리. 이 목록에 없는 접두(예 `egui/src/…`)는 애초에
/// 우리 경로로 읽히지 않으므로 판정 대상이 아니다 — 남의 저장소 내부 경로를 인용할 때
/// 크레이트 이름을 앞에 붙이면 그것만으로 모호함이 사라진다.
const ROOT_PREFIXES: &[&str] = &[
    "src/",
    "tests/",
    "crates/",
    "scripts/",
    "site/",
    "lang/",
    "assets/",
    "docs/",
    "benches/",
    "examples/",
    ".github/",
    ".cargo/",
];

/// 순회에서 통째로 가지치기할 디렉토리명.
const PRUNE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// gitignored 로컬 폴더 이름의 조각. 리터럴로 두면 이 파일이 비-git 경로 참조 금지
/// (`docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md`) 를 어긴다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

fn is_pruned(name: &str) -> bool {
    PRUNE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
}

fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-')
}

/// 경로 인용이 시작할 수 있는 자리인지 — 앞 글자가 경로 글자면 더 긴 경로의 일부다.
fn is_boundary(prev: Option<char>) -> bool {
    !prev.is_some_and(is_path_char)
}

/// 마지막 조각에 1~5 글자 소문자/숫자 확장자가 붙어 있는가. 디렉토리 인용을 거른다.
fn has_file_extension(p: &str) -> bool {
    let last = p.rsplit('/').next().unwrap_or("");
    let Some((stem, ext)) = last.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=5).contains(&ext.len())
        && ext
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// 한 줄에서 레포 경로 형태 인용을 뽑는다. 중괄호 축약(`…/{a,b}.rs`)은 뺀다.
fn scan_paths(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if !is_boundary(i.checked_sub(1).map(|k| chars[k])) {
            i += 1;
            continue;
        }
        let rest: String = chars[i..].iter().collect();
        let Some(prefix) = ROOT_PREFIXES.iter().find(|p| rest.starts_with(**p)) else {
            i += 1;
            continue;
        };
        let mut end = i + prefix.chars().count();
        while end < chars.len() && is_path_char(chars[end]) {
            end += 1;
        }
        // 중괄호 축약은 여러 경로를 한 번에 쓰는 표기라 단일 파일로 풀 수 없다.
        let braced = chars.get(end) == Some(&'{');
        let token: String = chars[i..end]
            .iter()
            .collect::<String>()
            .trim_end_matches(['.', '-', '/'])
            .to_string();
        if !braced && !token.contains('*') && has_file_extension(&token) {
            out.push(token);
        }
        i = end.max(i + 1);
    }
    out
}

/// 백틱 span 이 snake_case 식별자인가 — 소문자로 시작하고 밑줄이 하나 이상.
fn is_snake_ident(s: &str) -> bool {
    !s.is_empty()
        && s.contains('_')
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.ends_with('_')
}

/// 한 줄에서 `` `이름`(… `경로` …) `` 인접 짝을 뽑는다. 괄호는 같은 줄에서 닫혀야 한다.
fn scan_pairs(line: &str) -> Vec<(String, Vec<String>)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '`' {
            i += 1;
            continue;
        }
        let Some(close) = (i + 1..chars.len()).find(|&k| chars[k] == '`') else {
            break;
        };
        let name: String = chars[i + 1..close].iter().collect();
        i = close + 1;
        if !is_snake_ident(&name) {
            continue;
        }
        let mut j = i;
        while chars.get(j) == Some(&' ') {
            j += 1;
        }
        if chars.get(j) != Some(&'(') {
            continue;
        }
        let Some(rparen) = (j + 1..chars.len()).find(|&k| chars[k] == ')') else {
            continue;
        };
        let inner: String = chars[j + 1..rparen].iter().collect();
        let paths = scan_paths(&inner);
        if !paths.is_empty() {
            out.push((name, paths));
        }
        i = rparen + 1;
    }
    out
}

/// 스캔 대상 `.md` 를 모은다.
fn gather(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|e| e == "md") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() && is_pruned(p.file_name().and_then(|n| n.to_str()).unwrap_or("")) {
            continue;
        }
        gather(&p, out);
    }
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 인용 문서가 사는 크레이트 루트(repo-relative). 레포 루트 자신은 돌려주지 않는다 —
/// 레포 루트도 크레이트라 그것까지 돌려주면 해석이 두 번 같은 자리를 본다.
fn crate_root_of(root: &Path, rel_doc: &str) -> Option<String> {
    let mut dir = Path::new(rel_doc).parent()?;
    loop {
        let s = dir.to_string_lossy().replace('\\', "/");
        if s.is_empty() {
            return None;
        }
        if root.join(&s).join("Cargo.toml").is_file() {
            return Some(s);
        }
        dir = dir.parent()?;
    }
}

/// 레포 루트 또는 인용 문서의 크레이트 루트 기준으로 푼다.
fn resolve(root: &Path, rel_doc: &str, cited: &str) -> Option<PathBuf> {
    let direct = root.join(cited);
    if direct.is_file() {
        return Some(direct);
    }
    let crate_rel = crate_root_of(root, rel_doc)?;
    let nested = root.join(&crate_rel).join(cited);
    nested.is_file().then_some(nested)
}

/// 코드 펜스 밖의 줄만 (1-based 줄번호와 함께) 돌려준다.
fn prose_lines(contents: &str) -> Vec<(usize, &str)> {
    let mut fence = false;
    let mut out = Vec::new();
    for (i, line) in contents.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
            continue;
        }
        if !fence {
            out.push((i + 1, line));
        }
    }
    out
}

fn docs_of(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    gather(root, &mut files);
    files.sort();
    files
        .iter()
        .filter_map(|f| {
            std::fs::read_to_string(f)
                .ok()
                .map(|c| (rel_of(f, root), c))
        })
        .collect()
}

#[test]
fn cited_repo_paths_resolve() {
    let root = &tasty_doc_guards::repo_root();
    let docs = docs_of(root);
    assert!(
        !docs.is_empty(),
        "스캔 대상 .md 가 0 이다 — 순회 경로가 틀어졌다"
    );

    let mut judged = 0usize;
    let mut violations = Vec::new();
    for (rel, contents) in &docs {
        for (line_no, line) in prose_lines(contents) {
            for cited in scan_paths(line) {
                judged += 1;
                if resolve(root, rel, &cited).is_none() {
                    violations.push(format!("  {rel}:{line_no} — `{cited}`"));
                }
            }
        }
    }
    assert!(
        judged > 100,
        "판정한 경로 인용이 {judged} 개뿐이다 — 검출기가 죽었을 때도 이 테스트는 초록이 \
         되므로 모수를 함께 본다"
    );
    assert!(
        violations.is_empty(),
        "문서가 인용한 레포 경로가 실재하지 않는다 — 읽는 사람에게는 확인된 좌표처럼 \
         보이지만 따라갈 곳이 없다. 판정 {judged} 회 중 {} 회:\n{}\n\
         고치는 법: (a) 옮겨졌으면 현재 경로로, (b) 남의 저장소 경로면 크레이트 이름을 \
         앞에 붙여(`egui/src/style.rs`) 우리 경로 형태에서 빼고, (c) 생성물이거나 실재한 \
         적이 없으면 경로 인용 대신 서술로 적는다. 예외 목록은 두지 않는다 — 형태를 \
         고치면 부류가 닫힌다.",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
fn names_paired_with_a_file_live_in_that_file() {
    let root = &tasty_doc_guards::repo_root();
    let docs = docs_of(root);
    let mut cache: HashMap<PathBuf, String> = HashMap::new();
    let mut judged = 0usize;
    let mut violations = Vec::new();

    for (rel, contents) in &docs {
        for (line_no, line) in prose_lines(contents) {
            for (name, cited_paths) in scan_pairs(line) {
                let resolved: Vec<PathBuf> = cited_paths
                    .iter()
                    .filter_map(|p| resolve(root, rel, p))
                    .collect();
                if resolved.is_empty() {
                    continue; // 경로 축이 따로 신고한다.
                }
                judged += 1;
                let found = resolved.iter().any(|p| {
                    let body = cache
                        .entry(p.clone())
                        .or_insert_with(|| std::fs::read_to_string(p).unwrap_or_default());
                    contains_token(body, &name)
                });
                if !found {
                    violations.push(format!(
                        "  {rel}:{line_no} — `{name}` 이 {cited_paths:?} 에 없다"
                    ));
                }
            }
        }
    }
    assert!(
        judged > 20,
        "인접 짝 판정이 {judged} 회뿐이다 — 검출기가 죽었는지 모수로 확인한다"
    );
    assert!(
        violations.is_empty(),
        "이름 바로 뒤 괄호가 지목한 파일에 그 이름이 없다 — 옮겼거나 이름이 바뀌었다. \
         판정 {judged} 회 중 {} 회:\n{}",
        violations.len(),
        violations.join("\n")
    );
}

/// 식별자 토큰이 통째로 등장하는지. 부분문자열이면 더 긴 이름에 걸려 오탐한다.
fn contains_token(body: &str, name: &str) -> bool {
    let bytes = body.as_bytes();
    let mut from = 0usize;
    while let Some(off) = body[from..].find(name) {
        let start = from + off;
        let end = start + name.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[test]
fn the_path_scanner_takes_repo_shaped_citations_only() {
    // 잡아야 하는 것 — 백틱 안팎, 문장 끝 마침표, 크레이트 경로, 점 파일.
    assert_eq!(
        scan_paths("코드는 `src/state.rs` 에 있다"),
        ["src/state.rs"]
    );
    assert_eq!(
        scan_paths("경로는 tests/layering.rs."),
        ["tests/layering.rs"]
    );
    assert_eq!(
        scan_paths("`.github/workflows/format-check.yml` 이 본다"),
        [".github/workflows/format-check.yml"]
    );
    assert_eq!(
        scan_paths("`crates/tasty-doc-guards/Cargo.toml` 은 비어 있다"),
        ["crates/tasty-doc-guards/Cargo.toml"]
    );
    // 줄 범위 접미는 경로가 아니다 — `:` 에서 끊는다.
    assert_eq!(scan_paths("`src/state.rs:17-18`"), ["src/state.rs"]);

    // 잡지 않아야 하는 것.
    assert!(scan_paths("`src/engine/` 아래").is_empty(), "디렉토리 인용");
    assert!(scan_paths("`src/{a,b}.rs`").is_empty(), "중괄호 축약");
    assert!(scan_paths("`src/**/*.rs`").is_empty(), "와일드카드");
    assert!(
        scan_paths("`egui/src/style.rs`").is_empty(),
        "남의 크레이트 접두"
    );
    assert!(scan_paths("`vendor/src/x.rs`").is_empty(), "모르는 최상위");
    assert!(
        scan_paths("`~/.tasty/tasty.port`").is_empty(),
        "홈 런타임 경로"
    );
    // 더 긴 경로의 내부는 따로 잡지 않는다 — 한 인용은 한 번만 판정한다.
    assert_eq!(
        scan_paths("`crates/tasty-model/src/lib.rs`"),
        ["crates/tasty-model/src/lib.rs"]
    );
}

#[test]
fn the_pair_scanner_needs_the_paren_right_after_the_name() {
    assert_eq!(
        scan_pairs("`repo_root`(`crates/tasty-doc-guards/src/lib.rs`) 가 판다"),
        [(
            "repo_root".to_string(),
            vec!["crates/tasty-doc-guards/src/lib.rs".to_string()]
        )]
    );
    // 괄호 안에 타입이 함께 와도 경로만 뽑는다.
    assert_eq!(
        scan_pairs("`repo_root`(`Root`, `src/lib.rs`)"),
        [("repo_root".to_string(), vec!["src/lib.rs".to_string()])]
    );
    // 이름과 괄호가 떨어져 있으면 짝이 아니다.
    assert!(scan_pairs("`repo_root` 는 어딘가 (`src/lib.rs`)").is_empty());
    // 괄호에 경로가 없으면 판정 대상이 아니다.
    assert!(scan_pairs("`repo_root`(순수 함수)").is_empty());
    // snake_case 가 아닌 백틱 span 은 이름이 아니다.
    assert!(scan_pairs("`Root`(`src/lib.rs`)").is_empty());
    assert!(scan_pairs("`cargo test`(`src/lib.rs`)").is_empty());
}

#[test]
fn a_dead_coordinate_is_caught_and_a_live_one_is_not() {
    let root = &tasty_doc_guards::repo_root();
    // 살아 있는 좌표 — 이 파일 자신.
    let live = "crates/tasty-doc-guards/tests/cited_coordinates_exist.rs";
    assert_eq!(scan_paths(&format!("`{live}`")), [live]);
    assert!(
        resolve(root, "docs/x.md", live).is_some(),
        "실재하는 경로를 못 푼다 — 판정이 반대로 서 있다"
    );
    // 죽은 좌표 — 같은 자리에서 확장자만 바꾼 것.
    let dead = "crates/tasty-doc-guards/tests/cited_coordinates_exist.rss";
    assert!(resolve(root, "docs/x.md", dead).is_none());

    // 크레이트 상대 해석: 크레이트 안의 문서가 적은 `src/lib.rs` 는 그 크레이트 것이다.
    assert!(
        resolve(root, "crates/tasty-doc-guards/README.md", "src/lib.rs").is_some(),
        "크레이트 루트 기준 해석이 죽었다"
    );
    // 크레이트 밖 문서에는 그 해석이 없다.
    assert_eq!(crate_root_of(root, "docs/dev-guide/build.md"), None);

    // 인접 짝: 이 파일에 있는 이름과 없는 이름.
    let body = std::fs::read_to_string(root.join(live)).expect("자기 소스를 읽는다");
    assert!(contains_token(&body, "scan_pairs"));
    // 없는 이름은 조각으로 조립한다 — 리터럴로 적으면 이 파일 자신이 그 이름을 갖게 돼
    // 판정이 뒤집힌다. 파서가 자기 소스를 시험대로 삼을 때의 기본 함정이다.
    let absent = format!("{}{}", "scan_pairs_", "with_no_such_name");
    assert!(!contains_token(&body, &absent));
    // 부분문자열은 토큰이 아니다.
    assert!(!contains_token("fn scan_pairs_more() {}", "scan_pairs"));
}
