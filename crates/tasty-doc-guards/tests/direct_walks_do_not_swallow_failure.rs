//! 통합 테스트의 직접 디렉터리 순회에서 오류를 무시할 가능성이 있는 자리를 센다.
//! 공유 순회 사용 여부 검사와 달리 오류 처리 형태의 변화에 초점을 둔다.
//! 주석·리터럴을 마스킹한 뒤 read_dir 주변의 panic·unwrap·expect·?를 판독한다.
//! 오류 전파는 별도로 구분하며 호출자가 다시 무시하는지는 추적하지 않는다.
//! 텍스트 주변을 보는 분류이므로 무관한 문장의 오류 처리가 섞일 수 있다.
//! 파일 수 감소와 같은 부분 누락을 다른 검사가 잡는지는 이 수만으로 알 수 없다.

use std::collections::BTreeMap;

use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

/// 오류를 무시한다고 분류한 자리의 기준값. 증가뿐 아니라 감소도 확인해 개선 후 기준을 낮춘다.
/// 2026-09-08 실측29를 기준으로 뒀다. 빈 수집도 기준값과 달라 실패한다.
const SWALLOW_CAP: usize = 29;

#[derive(PartialEq, Eq, Debug)]
enum Handling {
    Swallow,
    Loud,
    Propagate,
}

fn is_target(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        ["tests", f] => f.ends_with(".rs"),
        ["crates", _, "tests", f] => f.ends_with(".rs"),
        _ => false,
    }
}

/// 앞 한 줄부터 뒤 여섯 줄까지 오류 처리 표지를 찾는다. 실제 제어 흐름은 분석하지 않는다.
fn handling_at(lines: &[&str], i: usize) -> Handling {
    let lo = i.saturating_sub(1);
    let hi = (i + 7).min(lines.len());
    let ctx = lines[lo..hi].join(" ");
    let loud = ["panic!", ".unwrap()", ".expect("];
    if ctx.contains(").unwrap_or_else(") && ctx.contains("panic!") {
        return Handling::Loud;
    }
    if let Some(rest) = ctx.split_once("read_dir").map(|(_, r)| r) {
        let head: String = rest.chars().take(60).collect();
        if head.contains('?') && !head.contains("else") {
            return Handling::Propagate;
        }
        if loud.iter().any(|m| head.contains(m)) {
            return Handling::Loud;
        }
    }
    Handling::Swallow
}

#[test]
fn direct_walks_do_not_swallow_failure() {
    let root = tasty_doc_guards::repo_root();
    let mut per_file: BTreeMap<String, usize> = BTreeMap::new();
    let (mut sites, mut swallow) = (0usize, 0usize);
    for (path, text) in rust_sources(&root, &["tests", "crates"]) {
        let rel = path.to_string_lossy().replace('\\', "/");
        if !is_target(&rel) {
            continue;
        }
        let masked = mask_non_code(&text);
        let lines: Vec<&str> = masked.split('\n').collect();
        for (i, l) in lines.iter().enumerate() {
            if !l.contains("read_dir") {
                continue;
            }
            sites += 1;
            if handling_at(&lines, i) == Handling::Swallow {
                swallow += 1;
                *per_file.entry(rel.clone()).or_default() += 1;
            }
        }
    }

    let listing: Vec<String> = per_file
        .iter()
        .map(|(f, n)| format!("  {n}  {f}"))
        .collect();
    assert_eq!(
        swallow,
        SWALLOW_CAP,
        "읽기 실패를 무시한다고 분류한 자리가 {swallow}개다(기준 {SWALLOW_CAP}, 전체 {sites}).\n늘었다면 실제 오류 처리와 주변 문맥을 확인하고 필요한 오류를 보고하도록 고친다. 줄었다면 개선인지 수집 누락인지 확인한 뒤 기준을 낮춘다.\n현재 위치:\n{}",
        listing.join("\n")
    );
}
