//! debug-ipc.md의 † 표시·각주 목록·require_input_simulation 호출 핸들러의 메서드 목록을 대조한다.
//! 입력 게이트가 필요한 기준은 [ADR-0012](../../docs/adr/0012-request-admission-and-isolation.md)를 따른다.
//! 게이트 호출의 존재만 확인하며, 실제 거절 동작이나 모든 실행 경로의 도달 여부는 검증하지 않는다.
//!
//! 문자열 리터럴이 아닌 dispatch 이름은 수집하지 못한다. 메서드 수 하한도 이런 누락을 보장하지 못하므로
//! 이름의 형식은 super::dispatch_name_literals에서 따로 확인한다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::{mask_non_code, repo_root, strip_comments};

const DOC: &str = "docs/dev-guide/debug-ipc.md";
const DISPATCH: &str = "src/adapters/ipc/handler.rs";
const HANDLER_DIR: &str = "src/adapters/ipc/handler";

/// 스캔을 핸들러 트리로 제한해 이 검사 자체의 문자열은 수집하지 않는다.
const GATE: &str = "require_input_simulation";

/// 2026-09-05 dispatch 메서드 214개를 측정했다. 빈 파싱을 찾기 위한 하한이다.
const MIN_DISPATCH_ARMS: usize = 150;

fn read(rel: &str) -> String {
    let path: PathBuf = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 읽을 수 없다: {e}", path.display()))
}

fn backticked_methods(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('`') {
        let after = &rest[i + 1..];
        match after.find('`') {
            None => break,
            Some(j) => {
                let inner = &after[..j];
                let ok = inner.contains('.')
                    && inner
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
                if ok {
                    out.push(inner.to_owned());
                }
                rest = &after[j + 1..];
            }
        }
    }
    out
}

fn dagger_marked(doc: &str) -> BTreeSet<String> {
    doc.lines()
        .filter(|l| l.trim_start().starts_with('|') && l.contains('†'))
        .filter_map(|l| backticked_methods(l).into_iter().next())
        .collect()
}

/// 각주의 예시·반례를 섞지 않도록 —와 첫 조사 는 사이의 목록만 읽는다.
fn footnote_enumerated(doc: &str) -> BTreeSet<String> {
    let line = doc
        .lines()
        .find(|l| l.trim_start().starts_with('†'))
        .expect("각주 본문(† 로 시작하는 줄)을 못 찾았다 — 각주가 사라졌거나 형태가 바뀌었다");
    let after_dash = line
        .split_once('—')
        .map(|(_, r)| r)
        .expect("각주에서 `—` 를 못 찾았다 — 열거 구간을 자를 수 없다");
    let span = after_dash
        .split_once('는')
        .map(|(l, _)| l)
        .expect("각주에서 열거 뒤의 조사를 못 찾았다 — 열거 구간을 자를 수 없다");
    let out: BTreeSet<String> = backticked_methods(span).into_iter().collect();
    assert!(
        !out.is_empty(),
        "각주 목록에서 메서드 이름을 읽지 못했다. 목록 구간의 형식을 확인한다: {span:?}"
    );
    out
}

/// 오류 응답만 반환하는 분기는 실행 핸들러가 아니므로 제외한다.
const REFUSAL_CALLEES: &[&str] = &["error", "invalid_params"];

/// 같은 메서드가 cfg별로 다른 핸들러를 부를 수 있어 호출 대상을 집합으로 보존한다.
fn dispatch_map() -> BTreeMap<String, BTreeSet<String>> {
    dispatch_map_of(&strip_comments(&read(DISPATCH)))
}

/// 리터럴 속 구분자로 분기를 잘못 나누지 않도록 마스킹한 소스에서 경계를 찾고 원문에서 이름을 읽는다.
fn dispatch_map_of(src: &str) -> BTreeMap<String, BTreeSet<String>> {
    let code = tasty_doc_guards::source_text::mask_non_code_aligned(src);
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (i, _) in code.match_indices("=>") {
        let rhs = &code[i + 2..];
        let Some(paren) = rhs.find('(') else { continue };
        let callee: String = rhs[..paren]
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default()
            .to_owned();
        if callee.is_empty() || REFUSAL_CALLEES.contains(&callee.as_str()) {
            continue;
        }
        let lhs_start = code[..i]
            .rfind("=>")
            .map(|p| p + 2)
            .into_iter()
            .chain(code[..i].rfind('{').map(|p| p + 1))
            .chain(code[..i].rfind(',').map(|p| p + 1))
            .max()
            .unwrap_or(0);
        for m in backticked_or_quoted(&src[lhs_start..i]) {
            out.entry(m).or_default().insert(callee.clone());
        }
    }
    out
}

fn backticked_or_quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('"') {
        let after = &rest[i + 1..];
        match after.find('"') {
            None => break,
            Some(j) => {
                let inner = &after[..j];
                if inner.contains('.')
                    && inner
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                {
                    out.push(inner.to_owned());
                }
                rest = &after[j + 1..];
            }
        }
    }
    out
}

/// 게이트 이름 앞의 마지막 fn 이름을 수집한다. 함수 정의 자체는 제외하며 실제 호출 관계까지 분석하지 않는다.
fn gated_handler_fns() -> BTreeSet<String> {
    let dir = repo_root().join(HANDLER_DIR);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} 를 읽을 수 없다: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.push(repo_root().join(DISPATCH));
    files.sort();

    let mut out = BTreeSet::new();
    for f in files {
        let Ok(raw) = std::fs::read_to_string(&f) else {
            continue;
        };
        let masked = mask_non_code(&raw);
        for (pos, _) in masked.match_indices(GATE) {
            let before = &masked[..pos];
            if before.trim_end().ends_with("fn") {
                continue;
            }
            let Some(fi) = before.rfind("fn ") else {
                continue;
            };
            let name: String = masked[fi + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() && name != GATE {
                out.insert(name);
            }
        }
    }
    out
}

/// 오류 응답 분기를 제외한 모든 호출 대상에서 게이트 이름을 찾은 메서드만 인정한다.
fn gated_methods(map: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    gated_methods_with(map, &gated_handler_fns())
}

fn gated_methods_with(
    map: &BTreeMap<String, BTreeSet<String>>,
    fns: &BTreeSet<String>,
) -> BTreeSet<String> {
    map.iter()
        .filter(|(_, callees)| !callees.is_empty() && callees.iter().all(|c| fns.contains(c)))
        .map(|(method, _)| method.clone())
        .collect()
}

#[test]
fn the_dagger_the_footnote_and_the_gate_name_the_same_methods() {
    let doc = read(DOC);
    let map = dispatch_map();

    assert!(
        map.len() >= MIN_DISPATCH_ARMS,
        "dispatch 메서드를 {}개만 읽었다(하한 {MIN_DISPATCH_ARMS}). 수집 경로와 파서를 확인한다.",
        map.len()
    );

    let marked = dagger_marked(&doc);
    let listed = footnote_enumerated(&doc);
    let gated = gated_methods(&map);

    assert!(
        !gated.is_empty(),
        "`{GATE}`가 있는 핸들러를 찾지 못했다. 게이트가 없어졌는지, 스캔 경로가 맞는지 확인한다."
    );

    assert_eq!(
        marked,
        gated,
        "† 목록과 게이트 호출을 찾은 메서드 목록이 다르다.\n  †만 있음: {:?}\n  게이트만 있음: {:?}\nADR-0012의 입력 게이트 기준에 따라 문서와 구현 중 바꿀 대상을 판단한다.",
        marked.difference(&gated).collect::<Vec<_>>(),
        gated.difference(&marked).collect::<Vec<_>>()
    );
    assert_eq!(
        listed,
        gated,
        "각주 본문이 열거하는 메서드와 실제 게이트 대상이 다르다.\n  \
         열거만 됨: {:?}\n  게이트만 됨: {:?}",
        listed.difference(&gated).collect::<Vec<_>>(),
        gated.difference(&listed).collect::<Vec<_>>()
    );
}

#[test]
fn a_dagger_without_a_gate_is_caught() {
    let doc = read(DOC);
    let real = dagger_marked(&doc);
    assert!(
        real.len() >= 4,
        "† 가 붙은 행이 {} 개뿐이다 — 변이 대조가 약해진다",
        real.len()
    );

    let victim = "debug.switch_workspace";
    assert!(
        !real.contains(victim),
        "{victim} 에 이미 † 가 붙어 있다 — 이 변이는 아무것도 안 바꾼다"
    );
    let mutated: String = doc
        .lines()
        .map(|l| {
            if l.trim_start().starts_with('|') && l.contains(&format!("`{victim}`")) {
                format!("{} †|", l.trim_end().trim_end_matches('|'))
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let after = dagger_marked(&mutated);
    assert!(
        after.contains(victim),
        "게이트 없는 메서드에 추가한 †를 읽지 못했다"
    );
    assert_eq!(
        after.len(),
        real.len() + 1,
        "† 하나를 더했는데 집합 크기가 1 만큼 안 늘었다 — 파서가 행을 잘못 세고 있다"
    );
    let gated = gated_methods(&dispatch_map());
    assert_eq!(real, gated, "변이 전에는 두 집합이 같아야 한다");
    assert_ne!(
        after, gated,
        "게이트 없는 메서드에 †를 추가했는데 목록 비교가 실패하지 않았다"
    );

    let listed = footnote_enumerated(&doc);
    let dropped = listed
        .iter()
        .next()
        .expect("열거가 비어 있을 리 없다")
        .clone();
    let shrunk = doc.replace(&format!("`{dropped}` · "), "");
    let after_listed = footnote_enumerated(&shrunk);
    assert_eq!(
        after_listed.len(),
        listed.len() - 1,
        "각주 열거에서 `{dropped}` 를 뺐는데 파서가 여전히 같은 수를 센다"
    );
    assert!(!after_listed.contains(&dropped));
}

#[test]
fn the_footnote_parser_reads_only_the_enumeration() {
    let doc = read(DOC);
    let listed = footnote_enumerated(&doc);
    let whole_line = doc
        .lines()
        .find(|l| l.trim_start().starts_with('†'))
        .expect("각주 줄");
    let everything: BTreeSet<String> = backticked_methods(whole_line).into_iter().collect();

    assert!(
        everything.len() > listed.len(),
        "각주 전체의 이름 {}개가 목록 안의 이름 {}개보다 많지 않아 제외 범위를 확인할 수 없다: {everything:?}",
        everything.len(),
        listed.len()
    );
    for extra in everything.difference(&listed) {
        assert!(
            !listed.contains(extra),
            "열거 밖의 이름 `{extra}` 이 열거로 읽혔다"
        );
    }
}

#[cfg(test)]
mod exemption_mutations {
    use super::*;

    fn fns(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_refusal_arm_does_not_hide_the_real_handler() {
        let src = r#"
match m {
    "surface.raw_key" => input_source::handle_raw_key(state, engine, id, p),
    "surface.raw_key" => JsonRpcResponse::error(id.clone(), -32015, WHY),
}
"#;
        let map = dispatch_map_of(src);
        assert_eq!(
            map.get("surface.raw_key").map(BTreeSet::len),
            Some(1),
            "거절 팔이 지도에 들어왔거나 실제 핸들러가 사라졌다: {map:?}"
        );
        let gated = gated_methods_with(&map, &fns(&["handle_raw_key"]));
        assert!(
            gated.contains("surface.raw_key"),
            "실제 핸들러가 게이트를 부르는데 거절 팔 때문에 게이트 없음으로 읽혔다"
        );
    }

    #[test]
    fn an_ungated_execution_path_is_still_caught() {
        let src = r#"
match m {
    "surface.raw_key" => input_source::handle_raw_key(state, engine, id, p),
    "surface.raw_key" => other::handle_raw_key_fallback(state, engine, id, p),
    "surface.raw_key" => JsonRpcResponse::error(id.clone(), -32015, WHY),
}
"#;
        let map = dispatch_map_of(src);
        let gated = gated_methods_with(&map, &fns(&["handle_raw_key"]));
        assert!(
            !gated.contains("surface.raw_key"),
            "게이트 없이 실행되는 분기가 있는데 게이트가 있는 메서드로 분류했다"
        );
    }

    #[test]
    fn a_method_with_only_refusals_is_not_gated() {
        let src = r#"
match m {
    "ns.only_refused" => JsonRpcResponse::error(id.clone(), -32015, WHY),
}
"#;
        let map = dispatch_map_of(src);
        let gated = gated_methods_with(&map, &fns(&["handle_raw_key"]));
        assert!(
            gated.is_empty(),
            "빈 호출 집합을 게이트됨으로 읽었다: {gated:?}"
        );
    }
}
