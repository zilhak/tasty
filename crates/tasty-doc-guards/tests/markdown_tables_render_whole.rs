//! Markdown 표의 셀 수가 머리글과 다른 행, 본문에 끼어든 구분행을 찾는다.
//! GFM은 남는 셀을 버리며, 코드 스팬 안의 |도 이스케이프하지 않으면 셀을 나눈다.
//! 같은 열 수의 표를 빈 줄 없이 붙이면 셀 수 검사만으로 찾을 수 없어 구분행도 검사한다.
//!
//! 렌더러 없이 표 모양을 읽으므로 표로 인식하지 못한 구문, 코드펜스 오류,
//! 들여쓰기·HTML·링크 때문에 렌더가 깨지는 문제까지 보장하지 않는다.
use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

const PRUNE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// 로컬 경로를 문서 인용으로 오인하지 않도록 순회용 이름을 조립한다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

fn is_pruned(name: &str) -> bool {
    PRUNE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
}

fn gather(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // worktree의 .git은 파일일 수 있으므로 종류를 확인하기 전에 이름으로 제외한다.
        // 다른 이름을 쓴 CARGO_TARGET_DIR도 캐시 표식으로 제외한다.
        if is_pruned(name) || tasty_doc_guards::is_build_cache_dir(&p) {
            continue;
        }
        gather(&p, root, out);
    }
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 이스케이프한 파이프는 셀을 나누지 않는다. 코드 스팬 안의 파이프도 이스케이프해야 한다.
fn cells(line: &str) -> usize {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            chars.next();
            cur.push('|');
            continue;
        }
        if c == '|' {
            parts.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push(c);
    }
    parts.push(cur);
    if parts.first().is_some_and(|p| p.trim().is_empty()) {
        parts.remove(0);
    }
    if parts.last().is_some_and(|p| p.trim().is_empty()) {
        parts.pop();
    }
    parts.len()
}

fn is_delimiter_row(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('|') || !t.contains('-') {
        return false;
    }
    t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

struct Finding {
    at: String,
    what: String,
}

fn scan(rel: &str, src: &str) -> (Vec<Finding>, usize) {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut tables = 0usize;
    let mut fence: Option<String> = None;
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim();
        if let Some(f) = &fence {
            if t.starts_with(f.as_str()) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(t[..3].to_string());
            i += 1;
            continue;
        }
        let opens_table = lines[i].contains('|')
            && i + 1 < lines.len()
            && is_delimiter_row(lines[i + 1])
            && lines[i + 1].contains('|');
        if !opens_table {
            i += 1;
            continue;
        }
        tables += 1;
        let header = cells(lines[i]);
        let mut j = i + 2;
        while j < lines.len() && !lines[j].trim().is_empty() && lines[j].contains('|') {
            if is_delimiter_row(lines[j]) {
                out.push(Finding {
                    at: format!("{rel}:{}", j + 1),
                    what: "표 본문에 구분행이 있다. 두 표 사이에 빈 줄을 넣어 분리한다."
                        .to_string(),
                });
            }
            let row = cells(lines[j]);
            if row != header {
                out.push(Finding {
                    at: format!("{rel}:{}", j + 1),
                    what: format!(
                        "셀 {row} 개인데 헤더는 {header} 개다 — GFM 은 넘친 셀을 버리고 \
                         모자란 칸은 비운다. 셀 안의 `|` 는 백슬래시로 이스케이프한다 \
                         (코드 스팬 안이라도 쪼개진다)"
                    ),
                });
            }
            j += 1;
        }
        i = j;
    }
    (out, tables)
}

/// 빈 순회가 통과하지 않게 하는 하한. 2026-09-05에 표 519개를 측정했다.
const MIN_TABLES: usize = 300;

#[test]
fn markdown_tables_do_not_lose_cells_when_rendered() {
    let root = &tasty_doc_guards::repo_root();
    let mut files = Vec::new();
    gather(root, root, &mut files);
    files.sort();

    let mut findings: Vec<Finding> = Vec::new();
    let mut tables = 0usize;
    for path in &files {
        let rel = rel_of(path, root);
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let (mut f, t) = scan(&rel, &src);
        findings.append(&mut f);
        tables += t;
    }

    assert!(
        tables >= MIN_TABLES,
        "표를 {tables}개만 수집했다(하한 {MIN_TABLES}, 2026-09-05 실측 519). 문서 순회와 표 인식을 확인한다."
    );

    assert!(
        findings.is_empty(),
        "표의 셀 수나 구분행이 잘못됐다. 표 {tables}개 중 {}건:\n  {}",
        findings.len(),
        findings
            .iter()
            .map(|f| format!("{} — {}", f.at, f.what))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn both_predicates_fire_on_the_shapes_that_once_survived() {
    let split_cell = "| 이름 | 뜻 |\n|---|---|\n| `a|b` | 쪼개진다 |\n";
    let (f, t) = scan("x.md", split_cell);
    assert_eq!(t, 1, "합성 표를 인식하지 못했다");
    assert_eq!(f.len(), 1, "이스케이프 안 된 `|` 를 못 잡는다");

    // 두 표의 열 수가 같으므로 셀 수 검사로는 구별하지 못한다.
    let glued = "| A | B |\n|---|---|\n| 1 | 2 |\n| C | D |\n|---|---|\n| 3 | 4 |\n";
    let (f, t) = scan("y.md", glued);
    assert_eq!(t, 1, "빈 줄 없이 붙은 뒤 표는 별도 표로 세지 않아야 한다");
    assert_eq!(
        f.len(),
        1,
        "같은 열 수의 표가 붙었을 때 본문의 구분행을 찾지 못했다"
    );

    let clean = "| A | B |\n|---|---|\n| 1 | 2 |\n\n| C | D |\n|---|---|\n| `a\\|b` | 4 |\n";
    let (f, t) = scan("z.md", clean);
    assert_eq!(t, 2, "빈 줄로 떨어진 표 둘을 하나로 셌다");
    assert!(f.is_empty(), "멀쩡한 표를 잡는다: {:?}", f[0].at);

    let fenced = "```\n| A | B |\n|---|---|\n| 1 |\n```\n";
    let (f, t) = scan("w.md", fenced);
    assert_eq!(t, 0, "코드 펜스 안을 표로 셌다");
    assert!(f.is_empty());
}

/// 로컬 폴더가 실물인지 심볼릭 링크인지에 따라 실제 순회 범위가 달라질 수 있다.
/// 합성 트리로 포함·제외할 경로를 고정해 검사한다.
#[test]
fn the_walk_prunes_by_name_and_by_marker_and_takes_only_markdown() {
    let probe = Scratch::new("md-tables");
    let dir = probe.path();

    let table = "| A | B |\n|---|---|\n| 1 | 2 |\n";
    let local_dot = format!(".{LOCAL_HEAD}{LOCAL_TAIL}");
    let dot_only = format!(".{LOCAL_HEAD}");
    // 등록되지 않은 숨김 디렉터리는 계속 수집해야 한다.
    for sub in [
        "sub",
        "target",
        ".git",
        &local_dot,
        &dot_only,
        "opaque",
        ".unrelated",
    ] {
        std::fs::create_dir_all(dir.join(sub)).expect("합성 트리를 만들지 못했다");
        std::fs::write(dir.join(sub).join("inner.md"), table).expect("합성 문서를 쓰지 못했다");
    }
    std::fs::write(dir.join("top.md"), table).expect("합성 문서를 쓰지 못했다");
    std::fs::write(dir.join("notes.txt"), table).expect("합성 문서를 쓰지 못했다");
    std::fs::write(
        dir.join("opaque").join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .expect("표식을 쓰지 못했다");

    let mut files = Vec::new();
    gather(dir, dir, &mut files);
    let mut got: Vec<String> = files.iter().map(|f| rel_of(f, dir)).collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            ".unrelated/inner.md".to_string(),
            "sub/inner.md".to_string(),
            "top.md".to_string()
        ],
        "포함·제외한 경로가 예상과 다르다. 하한 검사만으로는 수집 범위가 늘어난 오류를 찾지 못한다."
    );

    let mut none = Vec::new();
    gather(&dir.join("does-not-exist"), dir, &mut none);
    assert!(
        none.is_empty(),
        "없는 경로에서 파일을 수집했다: {none:?}. read_dir 실패 시 빈 결과가 나오는지 확인한다."
    );
}
