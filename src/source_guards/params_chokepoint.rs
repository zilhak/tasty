//! 핸들러가 IPC params 를 숫자로 읽는 자리는 **관문 하나**를 통과한다.
//!
//! `handler/params.rs` 가 생기기 전에는 `params.get(k).and_then(|v| v.as_u64())` 뒤에
//! `as u32` 를 붙이는 형태가 계층 전체에 흩어져 있었다. 자르기는 값을 거절하지 않고
//! **다른 값으로 바꾼다** — `<실재 id> + 2^32` 가 그 실재 surface 를 가리키고,
//! `4_294_967_296` 이 종료 코드 자리에서 `0`(정상 종료) 이 된다. 흩어져 있는 동안은
//! 하나를 고쳐도 나머지가 안 고쳐졌다.
//!
//! 전수를 옮기고 나면 이 명제는 **문법적**이 된다: "핸들러는 관문 밖에서 params 를
//! 숫자로 읽지 않는다." 값의 출처를 따지지 않고 소스 모양만으로 판정된다.
//!
//! ## 술어의 범위 — 면제가 아니라 정의
//!
//! 대상은 **핸들러 계층**(`src/adapters/ipc/handler/`, 짝인 `handler.rs`)이고
//! 관문 자신은 뺀다. 계층 안에도 숫자 읽기가 남아 있지만 그것들은 params 가 아닌 값을
//! 읽는다(레이아웃 트리, 저장된 audit·approval 레코드, 호스트 응답, anomaly detail).
//! 그것들을 면제 목록에 적는 대신 **술어를 "params 에서 파생된 값" 으로 좁혔다** —
//! 목록은 늙지만 정의는 안 늙는다.
//!
//! ## 초록이 뜻하는 것과 안 뜻하는 것
//!
//! 초록은 "이 계층에서 이름이 `params`/`_params` 인 값과, 그 값에서 `let` 으로 갈라져
//! 나온 지역 바인딩이 숫자로 안 읽힌다" 는 뜻이다. 다음은 **안 뜻한다**:
//!
//! - 핸들러가 params 인자의 이름을 다르게 지으면 술어 밖이 된다. 계층 전체가 `params`
//!   또는 `_params` 를 쓰고 있어서 지금은 성립하지만, 이름이 규약인 이상 규약이 깨지면
//!   가드도 같이 조용해진다. (`req.params` 처럼 **필드**로 접근하는 것은 대상이 아니다 —
//!   그건 지역에서 만든 요청이지 핸들러가 받은 인자가 아니다.)
//! - 바인딩을 두 번 이상 거치거나 함수로 넘긴 뒤 읽는 것은 못 잡는다. 한 홉만 본다.
//! - 관문 **안**의 판정이 옳은지는 안 본다. 그건 `params.rs` 의 단위 테스트가 든다.
//! - 핸들러 계층 **밖**(plugin 크레이트 등)은 대상이 아니다.
//!
//! 그래서 여기서 0 은 "이 축이 지켜진다" 가 아니라 "이 모양으로는 안 새고 있다" 다.

use std::path::{Path, PathBuf};

use super::{mask_non_code, repo_root};

/// IPC 요청의 params 를 읽는 계층 전부. 개별 파일이 아니라 디렉터리다(ADR-0133 ①).
///
/// 둘인 이유: 대부분의 메서드는 `adapters/ipc/handler` 에서 처리되지만, 창을 소유해야
/// 하는 것과 App 상태를 만지는 것은 `app/ipc` 에서 처리된다. **두 계층이 같은 명제를
/// 각자 판정한다** — 한쪽만 관문에 걸면 다른 쪽이 조용히 자르고 버린다(실측: 확장 전
/// `app/ipc` 에 16 곳이 있었고 그중 `remote_workspace` 는 `as u32` 로 잘랐다).
const SCAN_DIRS: &[&str] = &["src/adapters/ipc/handler", "src/app/ipc"];

/// `handler` 디렉터리와 짝인 모듈 루트.
const HANDLER_ROOT: &str = "src/adapters/ipc/handler.rs";

/// 관문 자신. 여기서는 `as_u64()` 를 부르는 것이 **일**이다.
const CHOKEPOINT: &str = "src/adapters/ipc/handler/params.rs";

/// 스캔한 핸들러 `.rs` 파일 수의 하한 — **연기 검사**다. 경로가 틀리면 예외가 아니라
/// 조용한 0 이 되고, 0 인 모수는 "위반 없음" 을 공짜로 만든다.
/// 값의 근거: 2026-09-05 실측 **81 개**(관문 제외).
const MIN_HANDLER_FILES: usize = 55;

/// 계층 안에서 발견되는 숫자 읽기 자리 수의 하한 — **검출기 생존 검사**다.
/// 위반이 0 인 것과 마커가 하나도 안 잡히는 것이 구분돼야 한다.
/// 값의 근거: 2026-09-05 실측 **27 곳**(전부 params 파생이 아니다).
const MIN_NUMERIC_READS: usize = 14;

/// 숫자로 읽는 형태. `.as_bool()`/`.as_str()` 은 자르기가 없어 대상이 아니다.
const NUMERIC_READS: &[&str] = &[".as_u64()", ".as_i64()", ".as_f64()", ".as_number()"];

/// params 를 담는 이름. 두 계층이 이 둘만 쓴다(실측: `params` 287 · `_params` 4).
///
/// 한 글자 이름(`p` 등)은 **일부러 안 받는다.** app 계층이 `let p = &cmd.request.params`
/// 를 쓰고 있었지만, `p` 는 클로저 인자로도 흔해서 이름으로 받으면 관계없는 자리를
/// 위반으로 센다(실측: `pty.rs` 의 `|p| p["pty_id"].as_u64()` 넷). 이름을 넓히는 대신
/// **그 바인딩들을 `params` 로 통일했다** — 술어가 이름에 매여 있으니 이름이 규약이다.
const PARAMS_NAMES: &[&str] = &["params", "_params"];

/// 살아 있는 요청에서 params 를 꺼내는 필드 경로. `app/ipc` 는 인자가 아니라
/// `IpcCommand` 를 통째로 받아 `cmd.request.params` 로 읽는다.
///
/// `.request.` 라는 마디가 있어야 한다 — 지역에서 **만든** 요청의 `req.params` 와
/// 갈리는 자리가 거기다(핸들러 계층의 CLI 진입점 테스트가 그 모양을 쓴다).
const REQUEST_PARAMS: &str = ".request.params";

/// 공백을 줄인 사본과 각 글자의 원래 줄 번호.
///
/// 실제 코드는 `params\n    .get("x")` 처럼 줄을 나눠 쓰기도 하고 붙여 쓰기도 한다.
/// 두 형태가 같은 문자열이 돼야 마커 하나로 잡힌다 — 못 본 형태는 위반이 아니라
/// **침묵**이라, 하나를 놓치면 가드가 초록인 채로 비어 간다.
///
/// 다만 **식별자와 식별자 사이의 공백은 한 칸으로 남긴다.** 전부 지우면 `let ttl` 이
/// `letttl` 이 되어 키워드 경계가 사라지고, 아래 바인딩 추출이 통째로 눈이 먼다.
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

/// `at` 의 토큰이 **홀로 선 이름**인가 — 앞이 식별자 글자도 `.` 도 아니어야 한다.
///
/// `.` 을 막는 것이 요지다. `req.params` 는 지역에서 만든 요청의 필드이지 핸들러가
/// 받은 인자가 아니다.
fn stands_alone(text: &str, at: usize, name: &str) -> bool {
    let b = text.as_bytes();
    let before_ok = at == 0 || (!is_ident_char(b[at - 1]) && b[at - 1] != b'.');
    let end = at + name.len();
    let after_ok = end >= b.len() || !is_ident_char(b[end]);
    before_ok && after_ok
}

/// `from` 부터 **하나의 식 범위**를 잡는다 — 깊이 0 에서 `,` `;` 를 만나거나
/// 괄호가 닫혀 밖으로 나가면 끝. 클로저(`|v| v.as_u64()`)는 여는 괄호 안에 있으므로
/// 범위에 포함된다.
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

/// 한 소스에서 params 파생 값을 숫자로 읽는 자리.
///
/// 두 형태를 본다.
/// - **직접**: `params` 로 시작하는 식 안에 숫자 읽기가 있다.
/// - **한 홉 경유**: `let X = <params 로 시작하는 식>` 으로 갈라 둔 뒤 `X.as_u64()`.
///   webhook 의 `let ttl = params.get("ttl_secs"); … ttl.as_u64()` 가 이 모양이었고,
///   체인 머리만 보는 매처에는 안 걸렸다.
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
            // 바인딩을 만든 `let` 자신은 위반이 아니다 — 그건 params 로 시작하는 식이라
            // 위 갈래가 이미 본다.
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

/// `let <패턴> = <params 로 시작하는 식>` 으로 갈라 둔 이름들.
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
        // `==` 는 대입이 아니다.
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

/// 패턴에서 소문자로 시작하는 이름만 — `Some` · `Ok` 같은 생성자는 뺀다.
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

/// 핸들러 계층은 관문 밖에서 params 를 숫자로 읽지 않는다.
#[test]
fn no_handler_reads_a_param_as_a_number_outside_the_chokepoint() {
    let files = scanned_files();
    assert!(
        files.len() >= MIN_HANDLER_FILES,
        "핸들러 `.rs` 를 {} 개만 걷었다(하한 {MIN_HANDLER_FILES}, 2026-09-05 실측 81). \
         경로가 틀리면 예외가 아니라 조용한 0 이 되고, 모수가 비면 아래 판정은 그냥 통과한다",
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
        "계층 전체에서 숫자 읽기 마커를 {numeric_reads} 개만 봤다(하한 \
         {MIN_NUMERIC_READS}, 2026-09-05 실측 27). 마커 문자열이 낡았으면 위반 0 은 \
         '안 샌다' 가 아니라 '아무것도 안 봤다' 다"
    );

    assert!(
        violations.is_empty(),
        "핸들러가 관문 밖에서 params 를 숫자로 읽는다:\n{}\n\
         `handler/params.rs` 의 `read_int` · `read_i64` · `read_f64` · `read_id_or_name` \
         (또는 `opt_*` / `require_u32`) 을 써라. 직접 읽으면 `as u32` 로 자르기 쉽고, \
         잘린 id 는 **실재하는 다른 대상**을 가리킨다.",
        violations.join("\n")
    );
}

/// 검출기의 극성 — 무엇을 잡고 무엇을 안 잡는지.
///
/// 이것이 없으면 위 테스트는 "검출기가 아무것도 안 잡는다" 여도 통과한다. 마커 생존
/// 하한이 절반을 막지만, 그건 마커 문자열이 살아 있다는 것까지만 말한다.
#[test]
fn the_detector_sees_params_derived_reads_and_not_lookalikes() {
    // ── 잡아야 하는 것 ──────────────────────────────────────────────────────
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
        "지역 바인딩 한 홉을 놓쳤다 — 실제로 두 자리가 이 모양이었다"
    );

    let hop_pattern = "let Some(v) = params.get(\"category\") else { return };\n\
                       let t = v.as_u64();";
    assert_eq!(scan(hop_pattern).len(), 1, "패턴 바인딩 한 홉을 놓쳤다");

    // app 계층은 인자가 아니라 `IpcCommand` 를 통째로 받는다.
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

    // ── 안 잡아야 하는 것 ───────────────────────────────────────────────────
    let via_gate = "let n = params::read_int::<u32>(params, \"surface\")?;";
    assert!(scan(via_gate).is_empty(), "관문 경유가 위반으로 잡힌다");

    let other_value = "let n = obj.get(\"id\").and_then(|v| v.as_u64());";
    assert!(scan(other_value).is_empty(), "params 가 아닌 값이 잡힌다");

    // 지역에서 **만든** 요청. `.request.` 마디가 없는 것이 살아 있는 요청과 갈리는 자리다.
    let field = "assert_eq!(req.params.get(\"id\").and_then(|v| v.as_u64()), Some(3));";
    assert!(scan(field).is_empty(), "`.params` 필드 접근이 잡힌다");

    // 한 글자 이름은 술어 밖이다 — 클로저 인자로 흔해서 이름으로 받으면 관계없는
    // 자리를 센다.
    let short_name = "assert!(arr.iter().any(|p| p[\"pty_id\"].as_u64() == Some(3)));";
    assert!(scan(short_name).is_empty(), "클로저 인자 `p` 가 잡힌다");

    let not_numeric = "let s = params.get(\"kind\").and_then(|v| v.as_str());";
    assert!(scan(not_numeric).is_empty(), "숫자가 아닌 읽기가 잡힌다");

    let commented = "// let n = params.get(\"x\").and_then(|v| v.as_u64());";
    assert!(scan(commented).is_empty(), "주석이 코드로 읽힌다");

    let in_string = "let doc = \"params.get(k).and_then(|v| v.as_u64())\";";
    assert!(scan(in_string).is_empty(), "문자열 리터럴이 코드로 읽힌다");
}

/// 스캔 루트가 이 가드 자신을 포함하지 않는다.
///
/// 이 파일은 자기가 찾는 형태를 **픽스처로 담고 있다.** 루트가 넓어져 이 파일을 삼키면
/// 자기 픽스처를 위반으로 세고 영구히 빨개진다 — 그때 고치는 방법이 면제 추가가 되면
/// 그 면제가 다음 진짜 위반도 덮는다. 루트를 좁게 유지하는 것으로 막는다.
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
