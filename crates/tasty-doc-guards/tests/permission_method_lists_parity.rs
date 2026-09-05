//! 권한 문서 표의 **메서드 목록**이 `METHOD_TABLE` 과 맞는지 검증한다.
//!
//! 형제 가드([`permission_token_docs_parity`])는 **토큰**이 문서에 등장하는지만 본다 —
//! 자기 doc 에 그렇게 적혀 있다. 그런데 `docs/dev-guide/plugin-permissions.md` 의 표는
//! 토큰마다 **그 토큰이 여는 메서드 목록**을 함께 싣고, 그 목록은 지금까지 어떤 가드도
//! 보지 않았다. 매니페스트 작성자는 이 표를 읽고 권한을 고르므로, 어긋나면 필요한
//! 권한을 안 붙이거나 필요 없는 권한을 붙인다.
//!
//! ## 단일 출처는 있다
//!
//! `crates/tasty-ipc/src/method_meta.rs` 의 `METHOD_TABLE` 이 메서드 → 필요 권한을
//! 싣고 스스로 "단일 진실 원천" 이라고 적어 두었다. 즉 이 축의 답은 "출처를 만든다" 가
//! 아니라 "있는 출처와 문서를 잇는다" 다.
//!
//! ## 어디까지 가를 수 있는가 — 행마다 다르다
//!
//! 문서의 가운데 열은 산문이라 한 가지 강도로 검사할 수 없다. 표기 자체가 네 부류를
//! 구분하고 있어서, 그 구분을 그대로 검사 강도로 쓴다.
//!
//! | 표기 | 행 | 검사 |
//! |---|---|---|
//! | 메서드만 나열 | 20 | **집합 동등** — 나열이 곧 전부라는 주장이다 |
//! | `` `x.*` 전부 `` | 4 | **접두 집합 동등** — 더 강한 주장이라 양방향으로 검사된다 |
//! | `` `memory.bb_*` 조회 `` | 2 | 정방향 + **쌍 단위 완전성**(아래) |
//! | `… 등` | 4 | **정방향 포함만** — `등` 이 불완전을 명시적으로 선언한다 |
//!
//! `등` 행의 역방향은 검사하지 않는다. 그 표기가 "여기 적힌 것이 전부는 아니다" 라는
//! 뜻이므로, 역방향 누락은 **결함이 아니라 그 행의 설계**다. 대신 그런 행이 몇 개인지를
//! 고정한다 — 면제 목록이 조용히 자라면 검사가 껍데기가 되고, 그 목록이 곧 이 가드가
//! 답하지 못하는 질문의 집합이기 때문이다. 닫힌 행에 `등` 을 붙여 검사를 낮추는 변경은
//! 그 고정에서 걸린다.
//!
//! `memory.read` / `memory.write` 는 `` `memory.bb_*` 조회 `` / `` … 변경 `` 처럼 한국어
//! 한정어로 글롭을 가른다 — 어느 `bb_*` 가 조회인지는 기계가 못 정한다. 그래서 그 둘은
//! **쌍으로** 검사한다: 두 토큰이 합쳐 `memory.*`(secret 제외) 전부를 덮고 서로 겹치지
//! 않으며, 접두 밖의 예외는 문서가 이름으로 적은 것뿐이라는 주장은 기계가 가른다.
//!
//! ## 백틱 안에 토큰과 메서드가 섞여 있다
//!
//! 형제 가드가 겪은 벽이다(그 doc 의 "역방향은 검사하지 않는다" 문단). 여기서는 벽을
//! 피할 수 있다 — 이 가드는 `METHOD_TABLE` 을 갖고 있으므로, 백틱 안 문자열이 토큰 집합에
//! 있고 메서드 표에 없으면 **토큰 참조**로 읽는다. 두 집합이 겹치는 이름은 지금 없고,
//! 생기면 그 이름은 메서드로 해석돼 정방향 검사를 받는다(안전한 방향이다).
//!
//! 백틱에는 산문도 들어온다 — `process.spawn` 행의 "`method_meta` 어느 메서드도 요구하지
//! 않는다" 같은 모듈 이름이다. **표에 있으면 메서드, 표에 없고 점도 없으면 산문**으로
//! 가른다. 점을 근거로 삼는 쪽이 단순하지만 그러면 `split` · `tree` 처럼 점 없는 실제
//! 메서드가 조용히 빠지므로, 표 조회를 먼저 둔다. 남는 구멍은 "점 없는 이름을 오타 낸
//! 경우" 하나이고, 그 둘이 그대로 있는지는 파서 테스트가 붙든다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tasty_doc_guards::{KNOWN_CTORS, method_table};

const TOKEN_SOURCE: &str = "crates/tasty-plugin-manifest/src/types.rs";
const METHOD_SOURCE: &str = "crates/tasty-ipc/src/method_meta.rs";
const DOC: &str = "docs/dev-guide/plugin-permissions.md";

/// `등` 으로 불완전을 선언한 행의 수. 이 가드가 역방향을 답하지 못하는 행의 개수이기도
/// 하다 — 늘면 검사가 조용히 약해지므로 고정한다.
const OPEN_ROWS: usize = 4;

fn root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// `Permission::as_token` 의 팔에서 variant → 토큰을 뽑는다.
fn variant_tokens(src: &str) -> BTreeMap<String, String> {
    let start = src
        .find("pub fn as_token")
        .expect("as_token 을 못 찾았다 — 파서가 낡았다");
    let body = &src[start..];
    let end = body
        .find("\n    }\n")
        .expect("as_token 본문의 끝을 못 찾았다");
    let mut out = BTreeMap::new();
    for line in body[..end].lines() {
        let Some(rest) = line.trim().strip_prefix("Self::") else {
            continue;
        };
        let variant: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if variant.is_empty() {
            continue;
        }
        let Some(q0) = line.find('"') else { continue };
        let Some(q1) = line[q0 + 1..].find('"') else {
            continue;
        };
        out.insert(variant, line[q0 + 1..q0 + 1 + q1].to_string());
    }
    out
}

/// `method_meta.rs` 가 실제로 정의한 `MethodMeta` 생성자 이름.
///
/// 손으로 유지하는 목록 대신 **소스에서 도출**한다. 도출한 것과 [`KNOWN_CTORS`] 가
/// 갈라지면 그 자리에서 실패한다 — 파서가 모르는 생성자를 조용히 건너뛰면 그 항목들이
/// 표에서 사라지고, 그때 이 가드는 "문서가 없는 메서드를 적었다" 는 **거짓 결함**을
/// 보고한다(2026-09-05 실제로 그렇게 났다: `plugin_only` 를 추가하자 넷이 사라졌다).
fn constructors_in_source(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("const fn ") else {
            continue;
        };
        if !t.contains("-> MethodMeta") {
            continue;
        }
        if let Some((name, _)) = rest.split_once('(') {
            out.insert(name.trim().to_string());
        }
    }
    out
}

/// 문서 표의 `| \`토큰\` | 메서드 열 | …` 행.
fn doc_rows(md: &str, tokens: &BTreeSet<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in md.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.split('|').collect();
        if cells.len() < 4 {
            continue;
        }
        let head = cells[1].trim();
        let Some(tok) = head.strip_prefix('`').and_then(|s| s.strip_suffix('`')) else {
            continue;
        };
        // scoped 토큰은 문서에 `ipc.invoke:<prefix>` 처럼 자리표시자와 함께 실린다.
        let base = tok.split(':').next().unwrap_or(tok);
        let scoped = format!("{base}:");
        if tokens.contains(tok) || tokens.contains(&scoped) {
            out.push((tok.to_string(), cells[2].to_string()));
        }
    }
    out
}

/// 열에서 백틱으로 감싼 항목을 뽑는다.
fn backticked(col: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = col;
    while let Some(a) = rest.find('`') {
        let after = &rest[a + 1..];
        let Some(b) = after.find('`') else { break };
        out.push(after[..b].to_string());
        rest = &after[b + 1..];
    }
    out
}

/// `a.b.c/d/e` 축약을 편다 — 슬래시 뒤는 **마지막 점 뒤 세그먼트**를 갈아 끼운다.
fn expand(lit: &str) -> Vec<String> {
    if !lit.contains('/') {
        return vec![lit.to_string()];
    }
    let mut parts = lit.split('/');
    let first = parts.next().unwrap_or_default().to_string();
    let base = match first.rfind('.') {
        Some(i) => first[..i].to_string(),
        None => return vec![first],
    };
    let mut out = vec![first];
    for p in parts {
        out.push(format!("{base}.{p}"));
    }
    out
}

#[derive(PartialEq, Debug)]
enum Kind {
    /// 나열이 곧 전부라는 주장.
    Closed,
    /// `` `x.*` 전부 `` — 접두로 양방향 검사.
    GlobTotal,
    /// 한국어 한정어가 붙은 글롭(`memory.bb_*` 조회) — 쌍 단위로만 검사.
    GlobQualified,
    /// `등` — 불완전을 명시적으로 선언한 행.
    Open,
}

fn kind(col: &str) -> Kind {
    if col.contains("전부") {
        return Kind::GlobTotal;
    }
    if col.contains('등') {
        return Kind::Open;
    }
    if backticked(col).iter().any(|b| b.contains('*')) {
        return Kind::GlobQualified;
    }
    Kind::Closed
}

struct Model {
    tok: BTreeMap<String, String>,
    meth: BTreeMap<String, Option<Vec<String>>>,
    rows: Vec<(String, String)>,
    by_tok: BTreeMap<String, BTreeSet<String>>,
}

fn model() -> Model {
    let tok = variant_tokens(&read(TOKEN_SOURCE));
    let meth = method_table(&read(METHOD_SOURCE));
    let tokens: BTreeSet<String> = tok.values().cloned().collect();
    let rows = doc_rows(&read(DOC), &tokens);
    let mut by_tok: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, req) in &meth {
        let Some(vs) = req else { continue };
        for v in vs {
            let t = tok
                .get(v)
                .unwrap_or_else(|| panic!("METHOD_TABLE 의 variant `{v}` 가 as_token 에 없다"));
            by_tok.entry(t.clone()).or_default().insert(name.clone());
        }
    }
    Model {
        tok,
        meth,
        rows,
        by_tok,
    }
}

/// 소스가 정의한 생성자를 이 파서가 **전부** 해석할 줄 안다.
///
/// 이 단정이 이 파일에서 제일 오래 살 부류를 막는다 — 스캐너가 **자기가 아는 모양만**
/// 보는 형태다. 생성자를 하나 더 만드는 커밋은 이 파서를 조용히 눈멀게 하는데, 그때
/// 나오는 것은 "파서가 못 읽었다" 가 아니라 "문서가 없는 메서드를 적었다" 라는 **엉뚱한
/// 방향의 결함**이라 읽는 사람이 문서를 고치러 간다. 그래서 목록을 손으로 두지 않고
/// 소스에서 도출해 대조한다.
#[test]
fn the_known_constructors_cover_what_the_source_defines() {
    let src = read(METHOD_SOURCE);
    let defined = constructors_in_source(&src);
    assert!(
        defined.len() >= 3,
        "생성자를 {}개밖에 못 뽑았다 — 도출이 죽었다(2026-09-05 실측 3: plugin · \
         plugin_only · local_only). `const fn …() -> MethodMeta` 형태가 바뀌었는지 봐라",
        defined.len()
    );
    let known: BTreeSet<String> = KNOWN_CTORS.iter().map(|(n, _)| (*n).to_string()).collect();
    let unhandled: Vec<&String> = defined.difference(&known).collect();
    assert!(
        unhandled.is_empty(),
        "`method_meta.rs` 가 정의한 생성자를 이 파서가 해석할 줄 모른다 — 그 생성자로 적힌 \
         항목은 표에서 조용히 사라진다. `KNOWN_CTORS` 에 (이름, plugin 이 부를 수 있는가) \
         를 더해라: {unhandled:?}"
    );
    let stale: Vec<&String> = known.difference(&defined).collect();
    assert!(
        stale.is_empty(),
        "`KNOWN_CTORS` 에 있는데 소스에 없는 생성자다 — 지워진 것을 계속 해석하고 있다: \
         {stale:?}"
    );
}

/// 파서가 조용히 빈 결과를 내면 아래 전부가 무력해진다. 특히 **여러 줄 항목**은
/// 한 줄 파서에서 소리 없이 빠지므로 대표를 하나 박아 둔다.
#[test]
fn the_parsers_are_alive() {
    let m = model();
    assert!(
        m.tok.len() >= 25,
        "as_token 파싱이 깨졌다 — variant {}개",
        m.tok.len()
    );
    assert!(
        m.meth.len() >= 200,
        "METHOD_TABLE 파싱이 깨졌다 — 항목 {}개",
        m.meth.len()
    );
    assert!(
        m.rows.len() >= 25,
        "문서 표 파싱이 깨졌다 — 행 {}개",
        m.rows.len()
    );
    assert_eq!(
        m.meth.get("terminal.tell").cloned().flatten(),
        Some(vec!["TerminalWrite".to_string()]),
        "한 줄 항목을 못 읽는다"
    );
    assert!(
        m.meth
            .get("terminal.spawn")
            .cloned()
            .flatten()
            .is_some_and(|v| v.contains(&"TerminalSpawn".to_string())),
        "여러 줄 항목(`terminal.spawn`)을 못 읽는다 — 한 줄 파서로 되돌아갔다"
    );
    assert_eq!(
        m.meth.get("timer.list").cloned(),
        Some(None),
        "local_only 항목을 못 읽는다"
    );
    assert!(
        m.meth
            .get("banner.open")
            .cloned()
            .flatten()
            .is_some_and(|v| v.contains(&"UiBanner".to_string())),
        "`plugin_only(&[..])` 항목을 못 읽는다 — 이 갈래가 빠지면 그 메서드들이 조용히 \
         사라져 '문서가 없는 메서드를 적었다' 로 오보된다"
    );
    assert_eq!(
        expand("approval.summary.get/set"),
        vec![
            "approval.summary.get".to_string(),
            "approval.summary.set".to_string()
        ]
    );
}

/// 문서가 이름으로 적은 메서드는 실재하고, plugin 이 부를 수 있고, 그 권한을 요구한다.
#[test]
fn every_method_the_doc_names_really_needs_that_permission() {
    let m = model();
    let tokens: BTreeSet<String> = m.tok.values().cloned().collect();
    let mut bad = Vec::new();
    for (t, col) in &m.rows {
        for lit in backticked(col) {
            if lit.contains('*') {
                continue;
            }
            for name in expand(&lit) {
                if !m.meth.contains_key(&name) {
                    // 백틱 안의 토큰 참조와 산문(모듈 이름 등)은 메서드가 아니다.
                    if tokens.contains(&name) || !name.contains('.') {
                        continue;
                    }
                }
                match m.meth.get(&name) {
                    None => bad.push(format!(
                        "  [{t}] `{lit}` → `{name}` 이 METHOD_TABLE 에 없다"
                    )),
                    Some(None) => bad.push(format!(
                        "  [{t}] `{lit}` → `{name}` 은 local_only 라 plugin 이 못 부른다"
                    )),
                    Some(Some(vs)) => {
                        let have: BTreeSet<&String> =
                            vs.iter().filter_map(|v| m.tok.get(v)).collect();
                        if !have.contains(t) {
                            bad.push(format!(
                                "  [{t}] `{name}` 은 그 권한을 요구하지 않는다 (실제 {have:?})"
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "문서가 적은 메서드가 코드와 어긋난다 ({}건).\n{}\n\n\
         단일 출처는 {METHOD_SOURCE} 의 METHOD_TABLE 이다 — 문서를 그쪽에 맞춘다.",
        bad.len(),
        bad.join("\n")
    );
}

/// 나열이 전부라고 주장하는 행은 코드와 **집합이 같아야** 한다.
#[test]
fn closed_rows_list_exactly_what_the_code_requires() {
    let m = model();
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for (t, col) in &m.rows {
        if kind(col) != Kind::Closed {
            continue;
        }
        checked += 1;
        let listed: BTreeSet<String> = backticked(col)
            .iter()
            .filter(|l| !l.contains('*'))
            .flat_map(|l| expand(l))
            .filter(|n| m.meth.contains_key(n))
            .collect();
        let code = m.by_tok.get(t).cloned().unwrap_or_default();
        let missing: Vec<&String> = code.difference(&listed).collect();
        let extra: Vec<&String> = listed.difference(&code).collect();
        if !missing.is_empty() {
            bad.push(format!("  [{t}] 문서에 없다: {missing:?}"));
        }
        if !extra.is_empty() {
            bad.push(format!("  [{t}] 코드가 요구하지 않는다: {extra:?}"));
        }
    }
    assert!(
        checked >= 15,
        "닫힌 행이 {checked}개뿐이다 — 분류가 깨졌거나 표가 통째로 `등` 이 됐다"
    );
    assert!(
        bad.is_empty(),
        "메서드만 나열한 행은 그 나열이 전부라는 주장이다 ({}건 어긋남).\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// `` `x.*` 전부 `` 는 더 강한 주장이라 양방향으로 검사한다.
#[test]
fn total_glob_claims_hold_in_both_directions() {
    let m = model();
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for (t, col) in &m.rows {
        if kind(col) != Kind::GlobTotal {
            continue;
        }
        checked += 1;
        let lits = backticked(col);
        let prefixes: Vec<String> = lits
            .iter()
            .filter(|l| l.ends_with('*'))
            .map(|l| l.trim_end_matches('*').to_string())
            .collect();
        let extras: BTreeSet<String> = lits
            .iter()
            .filter(|l| !l.contains('*'))
            .flat_map(|l| expand(l))
            .filter(|n| m.meth.contains_key(n))
            .collect();
        assert!(!prefixes.is_empty(), "[{t}] `전부` 인데 글롭이 없다");
        let code = m.by_tok.get(t).cloned().unwrap_or_default();
        for name in &code {
            if !prefixes.iter().any(|p| name.starts_with(p)) && !extras.contains(name) {
                bad.push(format!(
                    "  [{t}] `{name}` 이 그 권한을 요구하는데 문서의 `{prefixes:?}` 밖이고 이름으로도 안 적혔다"
                ));
            }
        }
        for (name, req) in &m.meth {
            let Some(vs) = req else { continue };
            if !prefixes.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            let have: BTreeSet<&String> = vs.iter().filter_map(|v| m.tok.get(v)).collect();
            if !have.contains(t) {
                bad.push(format!(
                    "  [{t}] `{name}` 이 문서의 `{prefixes:?}` 안인데 그 권한을 요구하지 않는다 (실제 {have:?})"
                ));
            }
        }
    }
    assert!(
        checked >= 3,
        "`전부` 행이 {checked}개뿐이다 — 분류가 깨졌다"
    );
    assert!(
        bad.is_empty(),
        "`전부` 는 접두 전체를 덮는다는 주장이다 ({}건 어긋남).\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// `memory.read` / `memory.write` 는 어느 글롭이 어느 쪽인지 기계가 못 가른다
/// (`` `memory.bb_*` 조회 `` / `` … 변경 ``). 대신 **쌍으로** 검사한다.
#[test]
fn the_memory_pair_covers_every_memory_method_exactly_once() {
    let m = model();
    let read_set = m.by_tok.get("memory.read").cloned().unwrap_or_default();
    let write_set = m.by_tok.get("memory.write").cloned().unwrap_or_default();
    let union: BTreeSet<String> = read_set.union(&write_set).cloned().collect();

    let expected: BTreeSet<String> = m
        .meth
        .iter()
        .filter(|(n, req)| {
            req.is_some() && n.starts_with("memory.") && !n.starts_with("memory.secret.")
        })
        .map(|(n, _)| n.clone())
        .collect();
    assert!(
        expected.len() >= 30,
        "memory.* 가 {}개뿐이다 — 파싱이 깨졌다",
        expected.len()
    );

    let uncovered: Vec<&String> = expected.difference(&union).collect();
    assert!(
        uncovered.is_empty(),
        "`memory.read` 와 `memory.write` 가 합쳐도 안 덮는 memory 메서드가 있다: {uncovered:?}\n\
         문서 두 행은 `memory.*` 전체를 조회/변경으로 가른다고 말한다."
    );

    let both: Vec<&String> = read_set.intersection(&write_set).collect();
    assert!(
        both.is_empty(),
        "한 메서드가 `memory.read` 와 `memory.write` 를 함께 요구한다: {both:?}\n\
         문서는 둘을 배타적인 조회/변경으로 서술한다 — 서술이나 코드 한쪽을 고쳐야 한다."
    );

    // 접두 밖 예외는 문서가 이름으로 적은 것뿐이어야 한다.
    let outside: BTreeSet<String> = union.difference(&expected).cloned().collect();
    let named: BTreeSet<String> = m
        .rows
        .iter()
        .flat_map(|(_, col)| backticked(col))
        .filter(|l| !l.contains('*'))
        .flat_map(|l| expand(&l))
        .collect();
    let unnamed: Vec<&String> = outside.difference(&named).collect();
    assert!(
        unnamed.is_empty(),
        "`memory.*` 밖인데 memory 권한을 요구하고 문서 어디에도 이름이 없다: {unnamed:?}"
    );
}

/// `등` 행의 수를 고정한다 — 이 가드가 역방향을 **답하지 못하는** 행의 집합이다.
#[test]
fn the_number_of_rows_that_declare_themselves_incomplete_is_pinned() {
    let m = model();
    let open: Vec<&String> = m
        .rows
        .iter()
        .filter(|(_, col)| kind(col) == Kind::Open)
        .map(|(t, _)| t)
        .collect();
    let found = open.len();
    assert_eq!(
        found, OPEN_ROWS,
        "`등` 으로 불완전을 선언한 행이 {found}개다(고정값 {OPEN_ROWS}): {open:?}\n\n\
         이 행들은 역방향(코드에 있는데 문서에 없는 메서드)이 검사되지 않는다 — `등` 이 곧 \
         '전부는 아니다' 라는 선언이라 역방향 누락이 결함이 아니기 때문이다.\n\
         늘었다면: 닫힌 행에 `등` 을 붙여 검사를 낮춘 것은 아닌지 본다. 낮춘 것이 맞고 \
         의도한 것이라면 이 상수를 올린다.\n\
         줄었다면: 좋은 방향이다. 상수를 내린다."
    );
}
