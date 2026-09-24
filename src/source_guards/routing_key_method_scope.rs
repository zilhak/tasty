//! 메서드별로 제한한 라우팅 키를 다른 메서드도 읽고 있는지 (메서드, 키) 쌍으로 비교한다.
//! 키 이름만 대조하면 pty.read에서 쓰는 id가 다른 메서드에서도 인식된 것으로 오인될 수 있다.
//!
//! handler.rs의 등록된 match 형태에서 호출하는 함수·재수출을 제한된 깊이까지 따라간다.
//! 플러그인·host-call에만 있는 경로는 이 범위에 없으므로 핸들러 파일 전체를 읽는
//! routing_key_coverage와 함께 사용한다. 타입 해석과 임의의 간접 호출은 지원하지 않는다.
//! 범용 키·메서드 한정 키·비대상 키·PAIR_EXEMPT 중 어디에 속하는지와 오래된 예외를 확인한다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::repo_root;

const HANDLER_DIR: &str = "src/adapters/ipc/handler";
pub(super) const HANDLER_ROOT: &str = "src/adapters/ipc/handler.rs";
const ROUTING_SOURCE: &str = "src/core/request_target.rs";

/// 호출을 따라갈 깊이 제한. 기존 측정에서 깊이 4 이후 5·7도 같은 결과여서 고정점보다 한 단계 크게 뒀다.
pub(super) const RESOLVE_DEPTH: u32 = 5;

/// 2026-09-05 dispatch 메서드 259개를 측정한 뒤 빈 파싱을 찾도록 둔 하한이다.
const MIN_METHODS: usize = 200;

/// 한정 범위 밖에서 읽지만 라우팅이 필요 없는 (메서드, 키)와 근거. 전 창 목록 집계 여부와는 별개의 분류다.
const PAIR_EXEMPT: &[(&str, &str, &str)] = &[(
    "pty.attach_surface",
    "id",
    "같은 요청의 `pane_id` 가 범용 키라 이미 주인을 짚는다 — `request_target.rs` 가 \
         이 메서드를 pty 한정에서 뺀 이유가 그것이다",
)];

/// 키 호출 뒤의 메서드 체인까지만 읽어 다른 수신자의 as_str을 이 키의 문자열 읽기로 오인하지 않도록 한다.
fn call_chain_after(after: &str, end: usize) -> &str {
    let b = after.as_bytes();
    // 이미 읽은 닫는 따옴표를 문자열 시작으로 오인하지 않도록 건너뛴다.
    let Some(mut i) = close_of_group(b, end + 1, 1) else {
        return &after[end..];
    };
    while i + 1 < b.len() && b[i + 1] == b'.' {
        let mut j = i + 2;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
            j += 1;
        }
        if j >= b.len() || b[j] != b'(' {
            break;
        }
        let Some(close) = close_of_group(b, j, 0) else {
            break;
        };
        i = close;
    }
    &after[end..=i.min(b.len() - 1)]
}

/// 시작 깊이에서 ()·[]가 닫히는 위치. 일반 문자열 안의 괄호는 제외한다.
fn close_of_group(b: &[u8], from: usize, depth: i32) -> Option<usize> {
    let (mut depth, mut i) = (depth, from);
    let (mut in_str, mut esc) = (false, false);
    while i < b.len() {
        let c = b[i] as char;
        if in_str {
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn is_id_shaped(key: &str) -> bool {
    key == "id" || key.ends_with("_id")
}

/// 공용 마스킹으로 주석·리터럴을 제외한 뒤 중괄호 짝을 찾는다.
pub(super) fn balanced(src: &str, open_at: usize) -> &str {
    let code = tasty_doc_guards::source_text::mask_non_code_aligned(&src[open_at..]);
    match tasty_doc_guards::match_arms::matching_close(&code, 0) {
        Some(close) => &src[open_at..=open_at + close],
        None => &src[open_at..],
    }
}

/// 공용 match 파서로 분기를 읽고 해석하지 못한 블록은 실패시킨다.
pub(super) fn dispatch_arms(src: &str) -> Vec<(Vec<String>, String)> {
    use tasty_doc_guards::match_arms::{Source, matching_close};
    const HEAD: &str = "Some(match request.method.as_str() {";
    let source = Source::new(src);
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = source.code[from..].find(HEAD) {
        let open = from + at + HEAD.len() - 1;
        let close = matching_close(&source.code, open).unwrap_or_else(|| {
            panic!(
                "dispatch `match` 가 닫히지 않는다({}행)",
                source.line_of(open)
            )
        });
        let arms = source
            .match_arms(open..close + 1)
            .unwrap_or_else(|e| panic!("dispatch 팔을 못 읽었다 — {e}"));
        for arm in arms {
            let names: Option<Vec<String>> = source
                .alternatives(&arm.pattern)
                .iter()
                .map(|alt| {
                    source
                        .plain_string(alt)
                        .filter(|name| {
                            !name.is_empty()
                                && name.chars().all(|c| {
                                    c.is_ascii_lowercase()
                                        || c.is_ascii_digit()
                                        || c == '.'
                                        || c == '_'
                                })
                        })
                        .map(str::to_string)
                })
                .collect();
            if let Some(names) = names.filter(|n| !n.is_empty()) {
                // guard에서 읽는 params도 해당 메서드의 읽기에 포함한다.
                let guard = arm.guard.as_ref().map_or("", |g| source.slice(g));
                out.push((names, format!("{guard}\n{}", source.slice(&arm.body))));
            }
        }
        from = close + 1;
    }
    out
}

fn module_of(rel: &str) -> Vec<String> {
    if rel == HANDLER_ROOT {
        return Vec::new();
    }
    let stem = rel
        .strip_prefix(&format!("{HANDLER_DIR}/"))
        .unwrap_or(rel)
        .trim_end_matches(".rs");
    let mut parts: Vec<String> = stem.split('/').map(str::to_string).collect();
    if parts.last().is_some_and(|p| p == "mod") {
        parts.pop();
    }
    parts
}

pub(super) fn handler_sources() -> Vec<(String, String)> {
    let root = repo_root();
    let mut paths = Vec::new();
    gather_rs(&root.join(HANDLER_DIR), &mut paths);
    paths.push(root.join(HANDLER_ROOT));
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let src = std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("{} 읽기 실패: {e}", p.display()));
            (rel, super::strip_comments(&src.replace("\r\n", "\n")))
        })
        .collect()
}

fn gather_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
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

pub(super) type FnKey = (Vec<String>, String);

/// 모듈·함수 이름으로 본문을 색인한다. 같은 이름이 중복되면 먼저 수집한 정의를 사용한다.
pub(super) fn fn_index(files: &[(String, String)]) -> BTreeMap<FnKey, String> {
    let mut out = BTreeMap::new();
    for (rel, src) in files {
        let module = module_of(rel);
        for (name, body) in fn_bodies(src) {
            out.entry((module.clone(), name)).or_insert(body);
        }
    }
    out
}

fn fn_bodies(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = src[from..].find("fn ") {
        let start = from + at;
        let prev_ok = start == 0
            || !src.as_bytes()[start - 1].is_ascii_alphanumeric()
                && src.as_bytes()[start - 1] != b'_';
        let after = &src[start + 3..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        from = start + 3;
        if !prev_ok || name.is_empty() {
            continue;
        }
        let Some(brace) = src[start..].find('{') else {
            break;
        };
        let body = balanced(src, start + brace);
        out.push((name, body.to_string()));
    }
    out
}

/// 접두사·명시 모듈·현재 모듈·루트·유일한 이름 순으로 찾는다. 수신자 타입을 해석하는 것은 아니다.
pub(super) fn resolve(
    index: &BTreeMap<FnKey, String>,
    caller: &[String],
    path: &str,
) -> Option<FnKey> {
    let mut parts: Vec<&str> = path.split("::").collect();
    let name = parts.pop()?.to_string();
    if parts.is_empty() {
        for cand in [caller.to_vec(), Vec::new()] {
            let key = (cand, name.clone());
            if index.contains_key(&key) {
                return Some(key);
            }
        }
        let mut hits = index.keys().filter(|(_, n)| *n == name);
        let only = hits.next()?;
        return hits.next().is_none().then(|| only.clone());
    }
    let base: Vec<String> = match parts[0] {
        "self" => caller.to_vec(),
        "super" => caller
            .split_last()
            .map_or_else(Vec::new, |(_, r)| r.to_vec()),
        "crate" => Vec::new(),
        _ => caller.to_vec(),
    };
    let tail: Vec<String> = parts
        .iter()
        .skip(usize::from(matches!(parts[0], "self" | "super" | "crate")))
        .filter(|p| !matches!(**p, "adapters" | "ipc" | "handler"))
        .map(|p| (*p).to_string())
        .collect();
    let prefixes = [
        [base.clone(), tail.clone()].concat(),
        tail.clone(),
        [caller.to_vec(), tail.clone()].concat(),
    ];
    for cand in &prefixes {
        let key = (cand.clone(), name.clone());
        if index.contains_key(&key) {
            return Some(key);
        }
    }
    // 자식 모듈의 함수를 재수출하는 형태도 따라간다.
    for pre in &prefixes {
        let mut hits = index
            .keys()
            .filter(|(m, n)| *n == name && m.len() > pre.len() && m.starts_with(pre));
        // 첫 후보에 재수출이 없어도 뒤 후보를 계속 확인해야 한다.
        let Some(only) = hits.next() else { continue };
        if hits.next().is_none() {
            return Some(only.clone());
        }
    }
    None
}

fn flatten(src: &str) -> String {
    src.chars().filter(|c| !c.is_whitespace()).collect()
}

fn id_keys_in(fragment: &str) -> BTreeSet<String> {
    params_keys_in(fragment)
        .into_iter()
        .filter(|k| is_id_shaped(k))
        .collect()
}

pub(super) fn params_keys_in(fragment: &str) -> BTreeSet<String> {
    const MARKERS: &[&str] = &[
        "params.get(\"",
        "(params,\"",
        "(&params,\"",
        "(&request.params,\"",
        "(request.params,\"",
    ];
    let flat = flatten(fragment);
    let mut out = BTreeSet::new();
    for marker in MARKERS {
        let mut rest = flat.as_str();
        while let Some(at) = rest.find(marker) {
            let after = &rest[at + marker.len()..];
            let Some(end) = after.find('"') else { break };
            let key = &after[..end];
            // 같은 키도 문자열 ID일 수 있어 이 호출 체인의 as_str 여부로 제외한다. 반환 타입 전체를 분석하지는 않는다.
            let tail = call_chain_after(after, end);
            if !key.is_empty()
                && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                && !tail.contains("as_str()")
            {
                out.insert(key.to_string());
            }
            rest = &after[end..];
        }
    }
    out
}

/// 키워드와 함수 이름이 붙지 않도록 공백을 보존한 소스에서 호출 경로를 읽는다.
pub(super) fn called_paths(fragment: &str) -> Vec<String> {
    let b = fragment.as_bytes();
    let mut out = Vec::new();
    for (i, c) in fragment.char_indices() {
        if c != '(' {
            continue;
        }
        let mut s = i;
        while s > 0 {
            let p = b[s - 1];
            if p.is_ascii_alphanumeric() || p == b'_' || p == b':' {
                s -= 1;
            } else {
                break;
            }
        }
        let path = &fragment[s..i];
        if path.is_empty() || path.ends_with(':') {
            continue;
        }
        if path
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            || path.contains("::")
        {
            out.push(path.trim_start_matches(':').to_string());
        }
    }
    out
}

fn reachable_keys(
    index: &BTreeMap<FnKey, String>,
    caller: &[String],
    fragment: &str,
    depth: u32,
    seen: &mut BTreeSet<FnKey>,
) -> BTreeSet<String> {
    reachable_keys_with(index, caller, fragment, depth, seen, id_keys_in)
}

/// 다른 검사도 같은 깊이·재수출 해석을 쓰도록 키 추출 함수만 교체할 수 있게 한다.
pub(super) fn reachable_keys_with(
    index: &BTreeMap<FnKey, String>,
    caller: &[String],
    fragment: &str,
    depth: u32,
    seen: &mut BTreeSet<FnKey>,
    extract: fn(&str) -> BTreeSet<String>,
) -> BTreeSet<String> {
    let mut keys = extract(fragment);
    if depth == 0 {
        return keys;
    }
    for path in called_paths(fragment) {
        let Some(key) = resolve(index, caller, &path) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            continue;
        }
        let body = index[&key].clone();
        keys.extend(reachable_keys_with(
            index,
            &key.0,
            &body,
            depth - 1,
            seen,
            extract,
        ));
    }
    keys
}

fn method_id_keys() -> BTreeMap<String, BTreeSet<String>> {
    let files = handler_sources();
    let index = fn_index(&files);
    let root_src = files
        .iter()
        .find(|(rel, _)| rel == HANDLER_ROOT)
        .map(|(_, s)| s.clone())
        .unwrap_or_default();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (names, expr) in dispatch_arms(&root_src) {
        let mut seen = BTreeSet::new();
        let keys = reachable_keys(&index, &[], &expr, RESOLVE_DEPTH, &mut seen);
        for name in names {
            out.entry(name).or_default().extend(keys.iter().cloned());
        }
    }
    assert!(
        out.len() >= MIN_METHODS,
        "dispatch 메서드를 {}개만 읽었다(하한 {MIN_METHODS}, 2026-09-05 측정 259개). 추출 범위를 확인한다.",
        out.len()
    );
    out
}

fn generic_keys(routing: &str) -> BTreeSet<String> {
    generic_keys_all(routing)
        .into_iter()
        .filter(|k| is_id_shaped(k))
        .collect()
}

/// surface·parent·target·pane도 라우팅 대상이므로 id 형태로 제한하지 않은 범용 키를 제공한다.
pub(super) fn generic_keys_all(routing: &str) -> BTreeSet<String> {
    let at = routing
        .find("fn params_resource_id")
        .expect("라우팅의 params_resource_id 함수를 찾지 못했다");
    let brace = routing[at..].find('{').expect("본문이 없다") + at;
    let body = balanced(routing, brace);
    let list_at = body.find("for key in [").expect("범용 키 배열을 못 찾았다");
    let list_end = body[list_at..].find(']').expect("배열이 안 닫힌다") + list_at;
    literals(&body[list_at..list_end])
}

/// 긍정형 if 블록에서 메서드와 키를 연결한다. 앞에서 부정 조건으로 반환하는 형태는 이 파서가 해석하지 못한다.
pub(super) fn scoped_pairs(routing: &str) -> BTreeSet<(String, String)> {
    let at = routing
        .find("fn method_scoped_resource_id")
        .expect("라우팅의 method_scoped_resource_id 함수를 찾지 못했다");
    let brace = routing[at..].find('{').expect("본문이 없다") + at;
    let body = balanced(routing, brace);
    let mut out = BTreeSet::new();
    let mut from = 1usize;
    while let Some(rel) = body[from..].find("if ") {
        let if_at = from + rel;
        let Some(open_rel) = body[if_at..].find('{') else {
            break;
        };
        let open = if_at + open_rel;
        let block = balanced(body, open);
        let methods: BTreeSet<String> = literals(&body[if_at..open])
            .into_iter()
            .filter(|s| s.contains('.'))
            .collect();
        let keys: BTreeSet<String> = literals(block)
            .into_iter()
            .filter(|k| is_id_shaped(k))
            .collect();
        for m in &methods {
            for k in &keys {
                out.insert((m.clone(), k.clone()));
            }
        }
        from = open + block.len();
    }
    out
}

pub(super) fn literals(fragment: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = fragment;
    while let Some(at) = rest.find('"') {
        let after = &rest[at + 1..];
        let Some(end) = after.find('"') else { break };
        let lit = &after[..end];
        if !lit.is_empty()
            && lit
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c == '.')
        {
            out.insert(lit.to_string());
        }
        rest = &after[end + 1..];
    }
    out
}

pub(super) fn routing_source() -> String {
    let path = repo_root().join(ROUTING_SOURCE);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{ROUTING_SOURCE}: {e}"));
    let production = src.split("#[cfg(test)]").next().unwrap_or(&src).to_string();
    super::strip_comments(&production.replace("\r\n", "\n"))
}

#[test]
fn every_method_scoped_key_is_read_only_where_it_routes() {
    let routing = routing_source();
    let generic = generic_keys(&routing);
    let scoped = scoped_pairs(&routing);
    assert!(!generic.is_empty(), "범용 라우팅 키를 추출하지 못했다");
    assert!(
        !scoped.is_empty(),
        "메서드 한정 라우팅 쌍을 추출하지 못했다"
    );
    let key_exempt: BTreeSet<&str> = super::routing_key_coverage::NOT_A_ROUTING_TARGET
        .iter()
        .map(|(k, _)| *k)
        .collect();
    let pair_exempt: BTreeSet<(String, String)> = PAIR_EXEMPT
        .iter()
        .map(|(m, k, _)| ((*m).to_string(), (*k).to_string()))
        .collect();

    let mut unscoped: BTreeSet<(String, String)> = BTreeSet::new();
    for (method, keys) in method_id_keys() {
        for key in keys {
            if generic.contains(&key) || key_exempt.contains(key.as_str()) {
                continue;
            }
            if scoped.contains(&(method.clone(), key.clone())) {
                continue;
            }
            unscoped.insert((method.clone(), key));
        }
    }
    let missing: Vec<_> = unscoped.difference(&pair_exempt).collect();
    let stale: Vec<_> = pair_exempt.difference(&unscoped).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "메서드별 키 분류가 읽기 목록과 다르다.\n  미등록 쌍: {missing:?}\n  오래된 예외: {stale:?}\n창의 리소스를 가리키면 method_scoped_resource_id에 메서드를 등록하고, 아니라면 PAIR_EXEMPT에 근거를 적는다."
    );
}

/// if !matches!(method...)와 if !method 형태가 생기면 긍정형 블록 추출을 다시 검토해야 한다.
#[test]
fn the_scoped_side_has_no_inverted_guard() {
    let routing = routing_source();
    let at = routing
        .find("fn method_scoped_resource_id")
        .expect("함수가 없다");
    let brace = routing[at..].find('{').unwrap() + at;
    let body = flatten(balanced(&routing, brace));
    assert!(
        !body.contains("if!matches!(method") && !body.contains("if!method"),
        "`method_scoped_resource_id` 에 뒤집힌 가드가 있다 — 긍정형 `if` 로 써라"
    );
}

#[test]
fn the_extractor_reads_arms_calls_and_keys() {
    let arms = dispatch_arms(concat!(
        "Some(match request.method.as_str() {\n",
        "    \"a.one\" => f(state, id, &request.params),\n",
        "    \"a.two\" | \"a_b.three2\" => { g(params) }\n",
        "    other => fallback(other),\n",
        "})"
    ));
    let names: Vec<Vec<String>> = arms.iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(
        names,
        vec![
            vec!["a.one".to_string()],
            vec!["a.two".to_string(), "a_b.three2".to_string()]
        ],
        "|로 연결한 메서드 이름을 모두 수집하고 이름이 아닌 패턴은 제외해야 한다"
    );
    assert_eq!(
        id_keys_in("let a = params.get(\"surface_id\");\nrequire_u32(params, \"tab_id\", &id);\nresp.get(\"other_id\");\nparams.get(\"kind\");")
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["surface_id".to_string(), "tab_id".to_string()],
        "키 추출 결과가 예상한 목록과 다르다"
    );
    assert!(
        id_keys_in("params.get(\"id\").and_then(|v| v.as_str())").is_empty(),
        "문자열로 꺼내는 읽기가 대상 지목으로 잡혔다 — 라우팅은 숫자만 본다"
    );
    // 같은 키의 체인 밖에 있는 as_str은 문자열 키라는 근거가 아니다.
    assert_eq!(
        id_keys_in("params.get(\"observer_id\");x.as_str()")
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["observer_id".to_string()],
        "다른 수신자의 as_str() 이 이 키의 읽기로 세어졌다"
    );
    assert!(called_paths("a::b::f(x) + g(y)").contains(&"a::b::f".to_string()));
    assert!(
        called_paths("match require_surface_id(params, &id) {")
            .contains(&"require_surface_id".to_string()),
        "키워드가 호출 이름에 붙어 헬퍼를 찾지 못했다. 공백 처리 방식을 확인한다."
    );
}

#[test]
fn generic_and_scoped_are_read_as_two_layers() {
    let fake = concat!(
        "fn params_resource_id(p: &V) -> Option<R> {\n",
        "    for key in [\"surface_id\", \"pane_id\"] { use_key(key); }\n",
        "    None\n}\n",
        "fn method_scoped_resource_id(method: &str, p: &V) -> Option<R> {\n",
        "    if method == \"pty.read\" { return numeric(params, \"id\"); }\n",
        "    None\n}\n",
    );
    let generic = generic_keys(fake);
    let scoped = scoped_pairs(fake);
    assert_eq!(
        generic,
        ["pane_id".to_string(), "surface_id".to_string()]
            .into_iter()
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(
        scoped,
        [("pty.read".to_string(), "id".to_string())]
            .into_iter()
            .collect::<BTreeSet<_>>()
    );
    assert!(
        !generic.contains("id"),
        "메서드 한정 키를 범용 키로 수집했다"
    );
}

#[cfg(test)]
mod exemption_mutations {
    use super::*;

    #[test]
    fn a_new_method_reading_an_exempt_key_is_not_covered() {
        let pair_exempt: BTreeSet<(String, String)> = PAIR_EXEMPT
            .iter()
            .map(|(m, k, _)| ((*m).to_string(), (*k).to_string()))
            .collect();
        let invented = ("invented.method".to_string(), "id".to_string());
        assert!(
            !pair_exempt.contains(&invented),
            "한 메서드의 예외가 같은 키를 쓰는 다른 메서드까지 제외했다"
        );
    }

    #[test]
    fn every_pair_exemption_states_a_reason() {
        for (m, k, why) in PAIR_EXEMPT {
            assert!(
                why.len() > 20,
                "({m}, {k}) 의 면제 사유가 너무 짧다 — 왜 라우팅이 필요 없는지를 적어라"
            );
        }
    }
}

/// 각 쌍이 네 분류 중 정확히 하나에 속하는지 확인한다.
#[test]
fn the_four_layers_partition_every_pair() {
    let routing = routing_source();
    let generic = generic_keys(&routing);
    let scoped = scoped_pairs(&routing);
    let key_exempt: BTreeSet<&str> = super::routing_key_coverage::NOT_A_ROUTING_TARGET
        .iter()
        .map(|(k, _)| *k)
        .collect();
    let pair_exempt: BTreeSet<(String, String)> = PAIR_EXEMPT
        .iter()
        .map(|(m, k, _)| ((*m).to_string(), (*k).to_string()))
        .collect();
    let mut uncovered = Vec::new();
    let mut doubled = Vec::new();
    for (method, keys) in method_id_keys() {
        for key in keys {
            let pair = (method.clone(), key.clone());
            let layers = u8::from(generic.contains(&key))
                + u8::from(key_exempt.contains(key.as_str()))
                + u8::from(scoped.contains(&pair))
                + u8::from(pair_exempt.contains(&pair));
            if layers == 0 {
                uncovered.push(pair);
            } else if layers > 1 {
                doubled.push(pair);
            }
        }
    }
    assert!(
        uncovered.is_empty() && doubled.is_empty(),
        "층이 분할이 아니다.\n 어느 층에도 안 드는 쌍: {uncovered:?}\n 두 층에 걸친 쌍: {doubled:?}"
    );
}
