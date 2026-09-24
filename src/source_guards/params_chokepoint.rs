//! IPC 숫자 인자는 공용 params 읽기 함수로 처리해야 한다.
//! as u32 등으로 직접 줄이면 범위를 벗어난 ID가 다른 유효한 ID로 바뀔 수 있다.
//!
//! handler 디렉터리·handler.rs·app/ipc에서 params와 _params, .request.params의 숫자 읽기를 찾는다.
//! 이 값에서 let으로 받은 지역 이름도 한 단계 추적한다. 다른 이름의 인자·두 단계 전달·함수 호출을
//! 통한 전달은 놓칠 수 있고 바인딩의 스코프·shadowing도 해석하지 않는다.
//! params 구현 자체와 그 밖의 크레이트는 제외하며 공용 읽기 함수의 동작은 해당 단위 시험에서 검증한다.

use std::path::{Path, PathBuf};

use super::{mask_non_code, repo_root};

/// 일반 IPC 핸들러와 창·App 상태를 처리하는 계층을 함께 검사한다.
const SCAN_DIRS: &[&str] = &["src/adapters/ipc/handler", "src/app/ipc"];

const HANDLER_ROOT: &str = "src/adapters/ipc/handler.rs";

/// 공용 숫자 판정을 재수출하고 JSON-RPC 오류로 변환하는 파일은 제외한다. 판정 구현은 core/param_bag.rs에 있다.
const CHOKEPOINT: &str = "src/adapters/ipc/handler/params.rs";

/// 수집 누락을 찾는 파일 수 하한. 2026-09-05 공용 params 파일을 제외하고 81개를 측정했다.
const MIN_HANDLER_FILES: usize = 55;

/// 숫자 읽기 검색이 비지 않았는지 확인한다. 2026-09-05 params 외의 읽기 27곳을 측정했다.
const MIN_NUMERIC_READS: usize = 14;

const NUMERIC_READS: &[&str] = &[".as_u64()", ".as_i64()", ".as_f64()", ".as_number()"];

/// 한 글자 p 등은 무관한 클로저 인자와 충돌하므로 검사하지 않는다. 인자 이름이 이 목록과 달라지면 놓칠 수 있다.
const PARAMS_NAMES: &[&str] = &["params", "_params"];

/// App이 받는 명령의 params 경로. .request. 없는 req.params는 이 조건으로 수집하지 않는다.
const REQUEST_PARAMS: &str = ".request.params";

/// 줄을 합치되 let ttl 같은 식별자 경계의 공백은 남긴다. 진단용으로 원래 줄 번호도 보관한다.
struct Flat {
    text: String,
    line: Vec<usize>,
}

fn flatten(src: &str) -> Flat {
    let chars: Vec<char> = src.chars().collect();
    let mut text = String::with_capacity(src.len());
    let mut line = Vec::with_capacity(src.len());
    let mut at = 1usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if !c.is_whitespace() {
            text.push(c);
            line.push(at);
            i += 1;
            continue;
        }
        let start_line = at;
        while i < chars.len() && chars[i].is_whitespace() {
            if chars[i] == '\n' {
                at += 1;
            }
            i += 1;
        }
        let joins_two_words = text
            .chars()
            .next_back()
            .is_some_and(|p| p.is_ascii_alphanumeric() || p == '_')
            && chars
                .get(i)
                .is_some_and(|n| n.is_ascii_alphanumeric() || *n == '_');
        if joins_two_words {
            text.push(' ');
            line.push(start_line);
        }
    }
    Flat { text, line }
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// 앞의 점과 식별자 문자를 제외해 req.params를 params 인자로 세지 않는다.
fn stands_alone(text: &str, at: usize, name: &str) -> bool {
    let b = text.as_bytes();
    let before_ok = at == 0 || (!is_ident_char(b[at - 1]) && b[at - 1] != b'.');
    let end = at + name.len();
    let after_ok = end >= b.len() || !is_ident_char(b[end]);
    before_ok && after_ok
}

/// 최상위 쉼표·세미콜론·닫는 괄호까지 식을 읽는다. 괄호 안 클로저는 포함한다.
fn expression_extent(text: &str, from: usize) -> &str {
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            b',' | b';' if depth == 0 => break,
            _ => {}
        }
        i += 1;
    }
    &text[from..i]
}

fn has_numeric_read(s: &str) -> bool {
    NUMERIC_READS.iter().any(|m| s.contains(m))
}

/// 직접 숫자 읽기와 params 식에서 let으로 받은 이름의 숫자 읽기를 찾는다.
pub(super) fn scan(src: &str) -> Vec<(usize, String)> {
    let flat = flatten(&mask_non_code(src));
    let mut out: Vec<(usize, String)> = Vec::new();

    for name in PARAMS_NAMES {
        for at in find_all(&flat.text, name) {
            if !stands_alone(&flat.text, at, name) {
                continue;
            }
            let extent = expression_extent(&flat.text, at);
            if has_numeric_read(extent) {
                out.push((flat.line[at], snippet(extent)));
            }
        }
    }

    for at in find_all(&flat.text, REQUEST_PARAMS) {
        let extent = expression_extent(&flat.text, at);
        if has_numeric_read(extent) {
            out.push((flat.line[at], snippet(extent)));
        }
    }

    for bound in params_derived_bindings(&flat) {
        for at in find_all(&flat.text, &bound) {
            if !stands_alone(&flat.text, at, &bound) {
                continue;
            }
            let extent = expression_extent(&flat.text, at);
            // let 대입은 직접 읽기 검사에서 이미 다뤘으므로 중복 신고하지 않는다.
            if has_numeric_read(extent) && !extent.contains('=') {
                out.push((flat.line[at], snippet(extent)));
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

fn snippet(extent: &str) -> String {
    extent.chars().take(90).collect()
}

fn find_all(hay: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = hay[from..].find(needle) {
        out.push(from + at);
        from += at + 1;
    }
    out
}

fn params_derived_bindings(flat: &Flat) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for at in find_all(&flat.text, "let") {
        if !stands_alone(&flat.text, at, "let") {
            continue;
        }
        let rest = &flat.text[at + 3..];
        let Some(eq) = rest.find('=') else {
            continue;
        };
        let (pattern, rhs) = (&rest[..eq], &rest[eq + 1..]);
        if rhs.starts_with('=') {
            continue;
        }
        let starts_with_params = PARAMS_NAMES
            .iter()
            .any(|n| rhs.starts_with(n) && stands_alone(rhs, 0, n))
            || rhs.starts_with('&').then(|| &rhs[1..]).is_some_and(|r| {
                PARAMS_NAMES
                    .iter()
                    .any(|n| r.starts_with(n) && stands_alone(r, 0, n))
            })
            || expression_extent(rhs, 0).contains(REQUEST_PARAMS);
        if !starts_with_params {
            continue;
        }
        out.extend(identifiers(pattern));
    }
    out.sort();
    out.dedup();
    out
}

/// 패턴의 소문자 이름을 수집하고 Some·Ok 같은 생성자는 제외한다.
fn identifiers(pattern: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in pattern.chars().chain(std::iter::once('\0')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if !cur.is_empty() && cur != "mut" && cur.starts_with(|c: char| c.is_ascii_lowercase())
            {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
        }
    }
    out
}

fn scanned_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut out = Vec::new();
    for dir in SCAN_DIRS {
        gather_rs(&root.join(dir), &mut out);
    }
    out.push(root.join(HANDLER_ROOT));
    let skip = root.join(CHOKEPOINT);
    out.retain(|p| *p != skip);
    out.sort();
    out
}

fn gather_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            gather_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} 읽기 실패: {e}", path.display()))
}

#[test]
fn no_handler_reads_a_param_as_a_number_outside_the_chokepoint() {
    let files = scanned_files();
    assert!(
        files.len() >= MIN_HANDLER_FILES,
        "핸들러 파일을 {}개만 수집했다(하한 {MIN_HANDLER_FILES}, 2026-09-05 측정 81개). 경로와 순회 범위를 확인한다.",
        files.len()
    );

    let root = repo_root();
    let mut violations: Vec<String> = Vec::new();
    let mut numeric_reads = 0usize;
    for path in &files {
        let src = read(path);
        let masked = mask_non_code(&src);
        numeric_reads += NUMERIC_READS
            .iter()
            .map(|m| masked.matches(m).count())
            .sum::<usize>();
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        for (line, what) in scan(&src) {
            violations.push(format!("  {rel}:{line}  {what}"));
        }
    }

    assert!(
        numeric_reads >= MIN_NUMERIC_READS,
        "숫자 읽기 형태를 {numeric_reads}개만 찾았다(하한 {MIN_NUMERIC_READS}, 2026-09-05 측정 27개). 검색 형태와 실제 읽기 코드를 대조한다."
    );

    assert!(
        violations.is_empty(),
        "공용 params 함수 밖에서 숫자를 직접 읽는다:\n{}\nhandler/params.rs의 read_int·read_i64·read_f64·read_id_or_name 또는 opt_*·require_u32를 사용한다. as u32로 줄이면 범위 밖 ID가 다른 유효한 대상으로 바뀔 수 있다.",
        violations.join("\n")
    );
}

#[test]
fn the_detector_sees_params_derived_reads_and_not_lookalikes() {
    let direct = "let n = params.get(\"surface\").and_then(|v| v.as_u64());";
    assert_eq!(scan(direct).len(), 1, "직접 읽기를 놓쳤다");

    let wrapped = "let n = params\n    .get(\"surface\")\n    .and_then(|v| v.as_u64());";
    assert_eq!(scan(wrapped).len(), 1, "줄바꿈된 체인을 놓쳤다");

    let indexed = "let n = params[\"id\"].as_u64();";
    assert_eq!(scan(indexed).len(), 1, "인덱스 읽기를 놓쳤다");

    let hop = "let ttl = params.get(\"ttl_secs\");\nlet secs = ttl.as_u64();";
    assert_eq!(
        scan(hop).len(),
        1,
        "params에서 let으로 받은 이름의 숫자 읽기를 놓쳤다"
    );

    let hop_pattern = "let Some(v) = params.get(\"category\") else { return };\n\
                       let t = v.as_u64();";
    assert_eq!(scan(hop_pattern).len(), 1, "패턴 바인딩 한 홉을 놓쳤다");

    let via_request = "let n = cmd.request.params.get(\"id\").and_then(|v| v.as_u64());";
    assert_eq!(
        scan(via_request).len(),
        1,
        "`cmd.request.params` 읽기를 놓쳤다"
    );

    let via_request_hop = "let params = &cmd.request.params;\n\
                           let n = params.get(\"id\").and_then(|v| v.as_u64());";
    assert_eq!(
        scan(via_request_hop).len(),
        1,
        "`&cmd.request.params` 를 받아 둔 바인딩을 놓쳤다"
    );

    let via_gate = "let n = params::read_int::<u32>(params, \"surface\")?;";
    assert!(scan(via_gate).is_empty(), "관문 경유가 위반으로 잡힌다");

    let other_value = "let n = obj.get(\"id\").and_then(|v| v.as_u64());";
    assert!(scan(other_value).is_empty(), "params 가 아닌 값이 잡힌다");

    let field = "assert_eq!(req.params.get(\"id\").and_then(|v| v.as_u64()), Some(3));";
    assert!(scan(field).is_empty(), "`.params` 필드 접근이 잡힌다");

    // 짧은 이름은 다른 값과 구별할 수 없어 제외한다.
    let short_name = "assert!(arr.iter().any(|p| p[\"pty_id\"].as_u64() == Some(3)));";
    assert!(scan(short_name).is_empty(), "클로저 인자 `p` 가 잡힌다");

    let not_numeric = "let s = params.get(\"kind\").and_then(|v| v.as_str());";
    assert!(scan(not_numeric).is_empty(), "숫자가 아닌 읽기가 잡힌다");

    let commented = "// let n = params.get(\"x\").and_then(|v| v.as_u64());";
    assert!(scan(commented).is_empty(), "주석이 코드로 읽힌다");

    let in_string = "let doc = \"params.get(k).and_then(|v| v.as_u64())\";";
    assert!(scan(in_string).is_empty(), "문자열 리터럴이 코드로 읽힌다");
}

#[test]
fn the_scan_root_does_not_contain_this_guard() {
    let me = Path::new(file!());
    for dir in SCAN_DIRS {
        assert!(
            !me.starts_with(dir),
            "이 가드({}) 가 스캔 루트({dir}) 안에 있다 — 자기 픽스처를 위반으로 센다",
            me.display()
        );
    }
}
