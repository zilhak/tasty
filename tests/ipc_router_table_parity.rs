//! 수집한 IPC 라우터의 메서드 후보가 권한 표에 등록됐는지 확인한다.
//! 로컬 호출자 전용이라면 미등록 상태 대신 local_only로 명시한다. 새 라우터 파일은 수집 목록에 추가해야 한다.
//! DEBUG_METHODS와 비교하므로 debug 빌드에서만 검사한다.
//! 자동 실행은 헤드리스 조합에서만 일어난다(check-headless). 기본 조합의 --lib --bins 호출은 이 통합 타깃을 실행하지 않는다.
#![cfg(debug_assertions)]

use std::collections::BTreeSet;
use std::path::Path;

use tasty_doc_guards::match_arms::{Source, matching_close};
use tasty_ipc::method_meta::method_meta;

const ROUTER_SOURCES: &[&str] = &[
    "src/adapters/ipc/handler.rs",
    "src/adapters/ipc/handler/ime.rs",
    "src/adapters/ipc/handler/debug_plugin.rs",
    "src/app/dispatch/list_global.rs",
    "src/boot/headless_dispatch.rs",
];

/// 이 디렉터리의 Rust 파일은 새 라우터도 포함하도록 비재귀 수집한다.
const ROUTER_DIRS: &[&str] = &["src/app/ipc"];

/// 줄 앞의 일반 문자열 바로 뒤에 =>가 오는 형태만 읽는다.
fn arm_method(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t.strip_prefix('"')?;
    let (name, after) = rest.split_once('"')?;
    if !after.trim_start().starts_with("=>") {
        return None;
    }
    if !is_method_name(name) {
        return None;
    }
    Some(name)
}

/// 한 줄에서 == 앞이 method로 끝나는 비교의 문자열을 읽는다. 실제 수신자 타입은 확인하지 않는다.
fn eq_methods(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(pos) = rest.find("== \"") {
        let before = rest[..pos].trim_end();
        let after = &rest[pos + 4..]; // `== "` 다음
        let Some((name, tail)) = after.split_once('"') else {
            break;
        };
        if before.ends_with("method") && is_method_name(name) {
            out.push(name);
        }
        rest = tail;
    }
    out
}

/// match 조건에 method 문자열이 있는 블록에서 대안 패턴과 guard를 구별해 읽는다. 실제 메서드 변수인지는 분석하지 않는다.
fn match_arm_methods(src: &str) -> Vec<String> {
    let source = Source::new(src);
    let code = source.code.as_str();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = code[from..].find("match ") {
        let at = from + at;
        from = at + "match ".len();
        let word_start = !code[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let Some(open) = code[from..].find('{').map(|k| from + k) else {
            break;
        };
        let scrutinee = &code[from..open];
        if !word_start || !scrutinee.contains("method") || scrutinee.contains(';') {
            continue;
        }
        let close = matching_close(code, open)
            .unwrap_or_else(|| panic!("{}행의 `match` 가 닫히지 않는다", source.line_of(open)));
        let arms = source
            .match_arms(open..close + 1)
            .unwrap_or_else(|e| panic!("dispatch 팔을 못 읽었다 — {e}"));
        for arm in arms {
            for alt in source.alternatives(&arm.pattern) {
                if let Some(name) = source.plain_string(&alt)
                    && is_method_name(name)
                {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

fn is_method_name(name: &str) -> bool {
    name.contains('.')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c == '.')
}

#[test]
fn every_router_arm_is_registered_in_method_table() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut missing: Vec<String> = Vec::new();
    let mut arm_scanned = 0usize;
    let mut eq_scanned = 0usize;
    let mut judge_scanned = 0usize;
    let mut line_only: Vec<String> = Vec::new();

    let mut sources: Vec<String> = ROUTER_SOURCES.iter().map(|s| s.to_string()).collect();
    for dir in ROUTER_DIRS {
        let abs = root.join(dir);
        let entries = std::fs::read_dir(&abs)
            .unwrap_or_else(|e| panic!("라우터 디렉토리를 읽을 수 없다: {dir}: {e}"));
        for entry in entries {
            let entry = entry.expect("디렉토리 엔트리");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let name = path.file_name().and_then(|n| n.to_str()).expect("파일명");
                sources.push(format!("{dir}/{name}"));
            }
        }
    }
    sources.sort();
    sources.dedup();

    for rel in &sources {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("라우터 소스를 읽을 수 없다: {rel}: {e}"));
        let judged_arms = match_arm_methods(&src);
        for name in &judged_arms {
            judge_scanned += 1;
            if method_meta(name).is_none() {
                missing.push(format!("{rel}: {name}"));
            }
        }
        let judged: BTreeSet<&str> = judged_arms.iter().map(String::as_str).collect();
        for line in src.lines() {
            if let Some(name) = arm_method(line) {
                arm_scanned += 1;
                if !judged.contains(name) {
                    line_only.push(format!("{rel}: {name}"));
                }
                if method_meta(name).is_none() {
                    missing.push(format!("{rel}: {name}"));
                }
            }
            for name in eq_methods(line) {
                eq_scanned += 1;
                if method_meta(name).is_none() {
                    missing.push(format!("{rel}: {name}"));
                }
            }
        }
    }

    // 판독별로 수를 확인해 한쪽 누락이 다른 쪽에 가려지지 않게 한다. 세 결과는 중복되므로 합이 메서드 수는 아니다.
    // 2026-09-23 측정: 줄 패턴311, method 비교46, match 판독321. 하한은 각각 측정값의 약80%다.
    assert!(
        arm_scanned >= 250,
        "`\"…\" =>` 줄 팔을 {arm_scanned} 개밖에 못 찾았다(실측 311) — 줄 판독이 깨졌을 가능성이 크다"
    );
    assert!(
        eq_scanned >= 36,
        "`… .method == \"…\"` 비교를 {eq_scanned} 개밖에 못 찾았다(실측 46) — 비교 판독이 깨졌을 가능성이 크다"
    );
    assert!(
        judge_scanned >= 250,
        "판정기로 읽은 dispatch 팔을 {judge_scanned} 개밖에 못 찾았다(실측 321) — 판정기 판독이 깨졌을 가능성이 크다"
    );
    // 줄 패턴 판독의 이름이 같은 파일의 match 판독 결과에도 있는지 비교한다.
    // 두 방법은 입력 형식이 다르며 같은 이름이 다른 위치에 있어도 통과하므로 위치별 일치를 증명하지는 않는다.
    assert!(
        line_only.is_empty(),
        "줄 패턴에서 읽은 이름이 같은 파일의 match 판독 결과에 없다. 함수별 입력 형식과 누락 위치를 확인한다:\n  {}",
        line_only.join("\n  ")
    );
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "라우터에 분기가 있는데 METHOD_TABLE/DEBUG_METHODS/PREFIX_RULES 어디에도 \
         등재되지 않은 메서드가 있다. plugin 에 열 것이면 plugin(&[..]) 으로, \
         local caller 전용으로 둘 것이면 local_only() 로 **명시 등재**하라 \
         (미등재는 UnknownMethod 거부라 의도와 구분되지 않는다):\n  {}",
        missing.join("\n  ")
    );
}
