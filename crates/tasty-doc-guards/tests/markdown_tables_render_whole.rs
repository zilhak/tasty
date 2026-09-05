//! 마크다운 표가 **렌더에서** 내용을 잃지 않는지 보는 가드.
//!
//! 추적 `.md` 를 보는 가드는 여럿이지만 전부 **소스 텍스트**를 본다. 그래서 소스로는
//! 아무 규칙도 안 어기는데 렌더에서만 깨지는 부류가 통째로 안 잡혔다. 사람이 소스를
//! 읽으면 내용이 다 보이므로 리뷰로도 안 잡힌다 — **읽는 형태와 배포되는 형태가 다른
//! 것 자체가 구멍**이다. 실제로 그 부류가 오래 살아남은 자리가 셋 있었다(`debug-ipc.md`
//! 의 붙은 표 둘 · `theme.md` 와 `action-dispatch.md` 의 이스케이프 안 된 `|`).
//!
//! ## 술어를 "렌더가 안 깨진다" 로 세우지 않는다
//!
//! GFM 은 깨진 표도 **무언가를** 렌더한다. "깨졌다" 는 판정 불가이므로 판정 가능한 둘로
//! 좁혔다. 둘 다 렌더러가 필요 없다.
//!
//! - **(a) 행마다 셀 수가 헤더와 같다.** GFM 은 헤더가 열 수를 정하고 **넘친 셀을 버린다.**
//!   이스케이프 안 된 `|` 가 셀을 쪼개면 여기 걸린다 — 코드 스팬 안이라도 쪼개진다.
//! - **(b) 표 본문에 구분행(`|---|`)이 없다.** 표 둘이 빈 줄 없이 붙으면 뒤 표의 헤더와
//!   구분행이 **앞 표의 본문 행으로 삼켜진다.** 그 형태가 이 가드의 자리다.
//!
//! **(b) 가 (a) 의 부분집합이 아니다.** 붙은 두 표의 열 수가 같으면 셀 수는 전부 맞아
//! (a) 는 침묵한다 — `debug-ipc.md` 가 겪은 것이 열 수가 다른 쪽이었을 뿐이다.
//! 처음에 (b) 를 "표 앞에 빈 줄이 있는가" 로 세웠다가 버렸다: 붙은 표는 **표로 인식되지
//! 않으므로** 그 술어는 발화할 기회 자체가 없다.
//!
//! ## 이 가드가 못 잡는 것 (사전 등록)
//!
//! 약한 술어라는 것을 인정하고 무엇이 밖인지 적어 둔다. 나중에 "이건 왜 안 잡혔나" 가
//! 나왔을 때 범위 밖인지 결함인지 그 자리에서 갈리게 하려는 것이다.
//!
//! - 코드펜스 짝이 안 맞아 이후 문서 전체가 코드로 렌더되는 것.
//! - 헤더와 구분행의 열 수가 달라 **표로 인식되지 않는 것**(GFM 은 그때 표를 안 만든다).
//!   이 가드는 그 둘이 맞는 표만 대상으로 삼는다.
//! - 4 칸 이상 들여쓴 표(GFM 은 코드 블록으로 본다).
//! - 링크·이미지 참조가 렌더에서 깨지는 것(경로 실재는 `cited_coordinates_exist` 가 본다).
//! - 셀 안 HTML 이 표 구조를 무너뜨리는 것.
//!
//! 선례: `crates/tasty-doc-guards/tests/no_checkbox_in_docs.rs`(docs 스캔 구조).

use std::path::{Path, PathBuf};

/// 순회에서 통째로 가지치기할 디렉토리명.
const PRUNE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// gitignored 로컬 폴더 이름의 조각. 조립해서 쓰면 비-git 경로 참조 금지
/// (`docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md`)를 어기지 않는다.
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
        // ★ **이름으로 하는 가지치기는 종류를 묻지 않는다.** worktree 에서 `.git` 은
        // 디렉토리가 아니라 `gitdir:` 한 줄이 든 **파일**이다 — 종류를 먼저 물으면
        // 그 파일이 가지치기를 빠져나가 모집단에 들고, 같은 커밋이 worktree 와 메인
        // 체크아웃에서 서로 다른 파일을 보게 된다. 모집단이 환경을 읽으면 답도
        // 언젠가 환경을 읽는다.
        if is_pruned(name) {
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

/// GFM 셀 분해. `\|` 는 셀을 쪼개지 않고, **코드 스팬 안의 `|` 는 쪼갠다** — GFM 의
/// 실제 동작이고, 그것이 `theme.md` 가 내용을 잃은 방식이다.
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
    // 바깥 파이프가 만든 앞뒤 빈 칸은 셀이 아니다.
    if parts.first().is_some_and(|p| p.trim().is_empty()) {
        parts.remove(0);
    }
    if parts.last().is_some_and(|p| p.trim().is_empty()) {
        parts.pop();
    }
    parts.len()
}

/// 구분행(`|---|:--:|`)인가. 표의 헤더 바로 아래에 오면 표를 열고, 본문에 나타나면
/// 앞 표가 뒤 표를 삼켰다는 신호다.
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

/// 한 파일의 표를 훑어 (a)·(b) 위반을 모은다. 펜스 안은 표가 아니다.
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
                    what: "표 본문에 구분행이 있다 — 빈 줄 없이 붙은 뒤 표가 앞 표에 \
                           삼켜졌다. 뒤 표의 헤더는 본문 행이 되고 그 표는 렌더되지 않는다"
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

/// 이 가드의 모수 하한 — **연기 검사**다. 표가 0 개로 세어지면 아래 판정은 빈 집합을
/// 훑고 조용히 통과한다. 값의 근거: 2026-09-05 실측 519 개.
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
        "표를 {tables} 개만 세었다(하한 {MIN_TABLES}, 2026-09-05 실측 519). 스캔이 \
         죽으면 아래 판정은 빈 집합을 훑고 통과한다 — 0 은 통과가 아니라 측정 실패다"
    );

    assert!(
        findings.is_empty(),
        "렌더에서 내용을 잃는 표가 있다. 소스로는 아무 규칙도 안 어기므로 리뷰로는 \
         안 잡힌다. 표 {tables} 개 중 {} 건:\n  {}",
        findings.len(),
        findings
            .iter()
            .map(|f| format!("{} — {}", f.at, f.what))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// 두 술어가 **정말 발화하는가** — 과거에 실재했던 두 형태를 그대로 만들어 먹인다.
///
/// 초록이 "깨진 표가 없다" 의 증거가 되려면 탐지기가 살아 있어야 한다. 위 테스트만
/// 두면 파서가 죽어도 초록이다(모수 하한이 그 일부를 막지만, 표를 세면서 검사만
/// 빠지는 형태는 못 막는다).
#[test]
fn both_predicates_fire_on_the_shapes_that_once_survived() {
    // (a) 이스케이프 안 된 `|` 가 셀을 쪼갠다 — `theme.md` 가 겪은 것.
    let split_cell = "| 이름 | 뜻 |\n|---|---|\n| `a|b` | 쪼개진다 |\n";
    let (f, t) = scan("x.md", split_cell);
    assert_eq!(t, 1, "표를 못 열었다 — 픽스처가 아니라 파서가 문제다");
    assert_eq!(f.len(), 1, "이스케이프 안 된 `|` 를 못 잡는다");

    // (b) 열 수가 **같은** 표 둘이 빈 줄 없이 붙는다 — (a) 로는 못 잡는 형태.
    let glued = "| A | B |\n|---|---|\n| 1 | 2 |\n| C | D |\n|---|---|\n| 3 | 4 |\n";
    let (f, t) = scan("y.md", glued);
    assert_eq!(
        t, 1,
        "붙은 뒤 표는 표로 인식되지 않는다 — 그것이 이 결함의 정체다"
    );
    assert_eq!(
        f.len(),
        1,
        "삼켜진 표를 못 잡는다. 열 수가 같아 (a) 는 침묵하므로 (b) 가 유일한 채널이다"
    );

    // 음성 대조 — 멀쩡한 표와 이스케이프한 `|` 는 조용하다.
    let clean = "| A | B |\n|---|---|\n| 1 | 2 |\n\n| C | D |\n|---|---|\n| `a\\|b` | 4 |\n";
    let (f, t) = scan("z.md", clean);
    assert_eq!(t, 2, "빈 줄로 떨어진 표 둘을 하나로 셌다");
    assert!(f.is_empty(), "멀쩡한 표를 잡는다: {:?}", f[0].at);

    // 펜스 안의 표 모양은 표가 아니다.
    let fenced = "```\n| A | B |\n|---|---|\n| 1 |\n```\n";
    let (f, t) = scan("w.md", fenced);
    assert_eq!(t, 0, "코드 펜스 안을 표로 셌다");
    assert!(f.is_empty());
}
