//! egui-mesh 생성 요청이 첫 context 요청보다 먼저 같은 플러그인 채널로 전달돼야 한다.
//! 순서가 뒤집히면 플러그인이 생성 인자를 받기 전에 그릴 수 있다.
//!
//! create 호출이 있는 함수를 수집해 같은 함수의 첫 set_context와 텍스트 순서를 비교한다.
//! 첫 인자의 경로를 비교하고 두 호출 사이의 let·대입 형태도 찾지만, 같은 surface인지나
//! 조건 분기의 실행 순서·중복 호출·실제 값의 동일성을 증명하지는 않는다.
//! set_context가 여러 개면 생성과 무관한 재전송을 첫 짝으로 고를 수 있다.
//!
//! 두 호출을 함께 헬퍼로 옮기면 헬퍼가 검사 대상이 된다. 한쪽만 옮기면 짝을 찾지 못해 실패한다.
//! 동명 변수의 스코프와 참조를 통한 변경은 해석하지 않는다. 호출식·인덱싱은 안정적인 경로로
//! 비교할 수 없어 거절하며, 지역 변수로 한 번 받아 같은 이름을 전달하도록 요구한다.

use std::path::{Path, PathBuf};

use super::{enclosing_fn, first_arg, fn_spans, line_of, mask_non_code, rust_sources};

const CREATE: &str = ".send_egui_mesh_surface_create(";
const SET_CONTEXT: &str = ".send_surface_set_context(";
const SENDER_DEF: &str = "crates/tasty-host-plugin/src/manager/events.rs";
const GUARD_DIRS: &[&str] = &["src/source_guards/", "crates/tasty-doc-guards/"];

fn rel_str(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

fn bootstrap_files() -> Vec<(PathBuf, String)> {
    rust_sources()
        .into_iter()
        .filter(|(rel, _)| {
            let rel = rel_str(rel);
            rel != SENDER_DEF && !GUARD_DIRS.iter().any(|dir| rel.starts_with(dir))
        })
        .map(|(rel, src)| (rel, mask_non_code(&src)))
        .filter(|(_, masked)| masked.contains(CREATE))
        .collect()
}

/// 참조·역참조·mut 접두사를 제거해 경로 표기를 비교한다. 값의 동일성을 증명하는 것은 아니다.
fn binding_name(arg: &str) -> &str {
    let mut s = arg.trim();
    loop {
        let t = s.trim_start_matches(['&', '*']).trim_start();
        let t = t.strip_prefix("mut ").unwrap_or(t).trim_start();
        if t == s {
            return s;
        }
        s = t;
    }
}

/// 식별자·점·콜론만으로 된 인자를 허용한다. 호출식·인덱싱·연산식은 비교하지 못한다.
fn is_path(arg: &str) -> bool {
    let s = binding_name(arg);
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':')
}

#[test]
fn the_population_is_not_empty() {
    let files: Vec<String> = bootstrap_files().iter().map(|(r, _)| rel_str(r)).collect();
    assert!(
        !files.is_empty(),
        "`{CREATE}` 호출을 찾지 못했다. 함수 이름과 스캔 경로를 확인한다."
    );
}

#[test]
fn every_mesh_bootstrap_sends_create_before_the_first_set_context() {
    let mut checked = 0usize;
    for (rel, masked) in bootstrap_files() {
        let rel = rel_str(&rel);
        let spans = fn_spans(&masked);
        for (create, _) in masked.match_indices(CREATE) {
            let (name, open, close, _) = enclosing_fn(&spans, create).unwrap_or_else(|| {
                panic!(
                    "{rel}:{}의 `{CREATE}`가 속한 함수를 읽지 못했다. 함수 추출을 확인한다.",
                    line_of(&masked, create)
                )
            });
            let set = masked[*open..*close]
                .find(SET_CONTEXT)
                .map(|p| p + *open)
                .unwrap_or_else(|| {
                    panic!(
                        "{rel}의 fn {name}에서 `{CREATE}`는 있으나 `{SET_CONTEXT}`가 없다. 호출을 헬퍼로 나눴다면 실제 순서와 검사 범위를 함께 확인한다."
                    )
                });
            assert!(
                create < set,
                "{rel}의 fn {name}: `{CREATE}`(줄 {})가 `{SET_CONTEXT}`(줄 {})보다 뒤에 있다. 같은 채널에서는 전송 순서가 유지되므로 생성 인자보다 context가 먼저 전달될 수 있다.",
                line_of(&masked, create),
                line_of(&masked, set),
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "create 호출을 하나도 검사하지 못했다. 함수 본문 추출을 확인한다."
    );
}

/// 첫 인자의 경로가 같고 사이에 재바인딩으로 보이는 구문이 없는지 확인한다. 같은 채널인지의 완전한 증명은 아니다.
#[test]
fn the_two_sends_address_the_same_plugin() {
    let mut checked = 0usize;
    for (rel, masked) in bootstrap_files() {
        let rel = rel_str(&rel);
        let spans = fn_spans(&masked);
        for (create, _) in masked.match_indices(CREATE) {
            let Some((name, open, close, _)) = enclosing_fn(&spans, create) else {
                continue;
            };
            let Some(set) = masked[*open..*close].find(SET_CONTEXT).map(|p| p + *open) else {
                continue; // 짝이 없는 것은 위 시험이 이미 빨갛게 만든다.
            };
            let a = first_arg(&masked, create).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 create 인자를 못 읽었다",
                    line_of(&masked, create)
                )
            });
            let b = first_arg(&masked, set).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 set_context 인자를 못 읽었다",
                    line_of(&masked, set)
                )
            });
            for (which, arg, at) in [("create", &a, create), ("set_context", &b, set)] {
                assert!(
                    is_path(arg),
                    "{rel}의 fn {name}: {which} 첫 인자 `{arg}`가 경로가 아니다(줄 {}). 호출식·인덱싱은 두 번 계산한 값이 같은지 확인할 수 없으므로 지역 변수로 받아 같은 이름을 전달한다.",
                    line_of(&masked, at)
                );
            }
            let (a, b) = (binding_name(&a), binding_name(&b));
            assert_eq!(
                a, b,
                "{rel}의 fn {name}: create 인자 `{a}`와 set_context 인자 `{b}`가 다르다. 같은 플러그인 채널에 보내는지 확인한다."
            );
            let id = a;
            let span = &masked[create..set];
            let rebound = span.contains(&format!("let {id}"))
                || span.match_indices(id).any(|(i, _)| {
                    let rest = span[i + id.len()..].trim_start();
                    rest.starts_with('=') && !rest.starts_with("==")
                });
            assert!(
                !rebound,
                "{rel}의 fn {name}: 두 호출 사이에 `{id}` 재바인딩 또는 대입으로 보이는 구문이 있다. 같은 플러그인 ID를 전달하는지 확인한다.",
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "짝지어진 create/set_context 를 하나도 안 봤다");
}
