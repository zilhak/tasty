//! 저장소 상대 경로를 문자열로 비교할 때 구분자를 /로 맞추는지 확인한다.
//! Windows 경로를 그대로 쓰면 파일 목록·예외 명부의 / 경로와 일치하지 않을 수 있다.
//!
//! 등록한 루트 인자의 strip_prefix 형태를 찾아 공용 helper·직접 replace·정규화 없음으로 분류한다.
//! 호출 앞 두 줄과 뒤 일곱 줄 안의 원문을 보며 ;나 다음 블록에서 멈춘다.
//! 실제 값의 흐름과 치환 결과를 검증하지 않으므로 주변 주석·다른 식의 정규화를 근거로 오인할 수 있다.
//! 등록한 인자 이름 밖의 경로 변환도 수집하지 못한다.

use tasty_doc_guards::source_text::mask_non_code;

/// 문자열로 합치지 않거나 성분을 /로 직접 합치는 예외 파일과 사유.
const HANDLES_COMPONENTS: &[(&str, &str)] = &[
    (
        "crates/tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs",
        "성분을 뽑아 자기가 `/` 로 잇는다",
    ),
    (
        "src/dpi_conversion_guard.rs",
        "성분 목록으로만 쓰고 문자열로 잇지 않는다",
    ),
    (
        "crates/tasty-doc-guards/src/source_text.rs",
        "`repo_relative` 자신 — 규칙이 사는 자리다",
    ),
];

const ROOT_ARGS: &[&str] = &[
    "&root",
    "root)",
    "&repo_root()",
    "repo_root()",
    "&base",
    "base)",
    "&dir",
    "dir)",
    "&self.root",
];

struct Site {
    rel: String,
    line: usize,
    kind: Kind,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Kind {
    Helper,
    Hand,
    None,
}

fn classify(masked: &str, raw: &str) -> Vec<(usize, Kind)> {
    let mut out = Vec::new();
    let bytes: Vec<&str> = masked.lines().collect();
    let raws: Vec<&str> = raw.lines().collect();
    for (i, line) in bytes.iter().enumerate() {
        if !line.contains("strip_prefix(") {
            continue;
        }
        let head = format!("{} {}", line, bytes.get(i + 1).unwrap_or(&""));
        let after = head.split("strip_prefix(").nth(1).unwrap_or("");
        if !ROOT_ARGS.iter().any(|a| {
            after.trim_start().starts_with(a.trim_start_matches('&'))
                || after.trim_start().starts_with(a)
        }) {
            continue;
        }
        // replace 인자의 리터럴이 필요해 분류는 원문에서 한다. 위치 검색은 마스킹한 코드에서 한다.
        let mut stmt = String::new();
        for k in i.saturating_sub(2)..(i + 8).min(raws.len()) {
            // 뒤의 else 블록을 같은 변환식으로 읽지 않도록 다음 블록에서 멈춘다.
            if k > i && raws[k].contains('{') {
                break;
            }
            stmt.push_str(raws[k]);
            stmt.push(' ');
            if raws[k].contains(';') && stmt.contains("strip_prefix(") {
                break;
            }
        }
        let flattened = stmt.contains("to_string_lossy")
            || stmt.contains(".display()")
            || stmt.contains(".to_str()");
        let kind = if stmt.contains("repo_relative(") {
            Kind::Helper
        } else if stmt.contains("replace('\\\\', \"/\")") || stmt.contains(".replace('\\\\'") {
            Kind::Hand
        } else if flattened {
            Kind::None
        } else {
            continue;
        };
        out.push((i + 1, kind));
    }
    out
}

fn scan() -> Vec<Site> {
    let mut out = Vec::new();
    for (rel, src) in super::rust_sources_with_integration_tests() {
        let rel = rel.to_string_lossy().to_string();
        if HANDLES_COMPONENTS.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        for (line, kind) in classify(&mask_non_code(&src), &src) {
            out.push(Site {
                rel: rel.clone(),
                line,
                kind,
            });
        }
    }
    out
}

#[test]
fn no_repo_relative_path_is_flattened_without_normalizing_the_separator() {
    let sites = scan();
    assert!(
        sites.len() >= 20,
        "루트 제거 호출을 {}개만 찾았다(하한 20). 검색 형태와 경로를 확인한다.",
        sites.len()
    );
    assert!(
        !HANDLES_COMPONENTS.is_empty(),
        "경로 성분으로 처리하는 예외 명부가 비었다"
    );

    let bad: Vec<String> = sites
        .iter()
        .filter(|s| s.kind == Kind::None)
        .map(|s| format!("  {}:{}", s.rel, s.line))
        .collect();
    assert!(
        bad.is_empty(),
        "상대 경로를 문자열로 바꾸면서 구분자 정규화를 찾지 못한 곳이 {}개다:\n{}\ntasty_doc_guards::source_text::repo_relative를 사용한다. 보고용 경로도 다른 검사의 입력으로 인용될 수 있으므로 정규화한다.",
        bad.len(),
        bad.join("\n")
    );

    let hand = sites.iter().filter(|s| s.kind == Kind::Hand).count();
    let helper = sites.iter().filter(|s| s.kind == Kind::Helper).count();
    assert_eq!(
        hand, 25,
        "직접 replace로 구분자를 바꾼 곳이 {hand}개다(기록 25). 새 변환은 공용 helper를 사용하고 이전한 만큼 기록을 낮춘다."
    );
    println!("[레포 상대 경로] helper {helper} · hand {hand} · 미정규화 0");
}

#[cfg(test)]
mod detector {
    use super::*;

    #[test]
    fn a_bare_strip_prefix_that_is_flattened_is_reported() {
        let src = "let rel = f.strip_prefix(&root).unwrap_or(&f).to_string_lossy().to_string();";
        assert_eq!(classify(src, src), vec![(1, Kind::None)]);
    }

    #[test]
    fn a_hand_rolled_replace_is_its_own_kind() {
        let src = "let rel = f.strip_prefix(&root).unwrap_or(&f).to_string_lossy().replace('\\\\', \"/\");";
        assert_eq!(classify(src, src), vec![(1, Kind::Hand)]);
    }

    #[test]
    fn going_through_the_helper_is_the_good_kind() {
        let src = "let rel = repo_relative(f.strip_prefix(&root).unwrap_or(&f)).to_string_lossy().to_string();";
        assert_eq!(classify(src, src), vec![(1, Kind::Helper)]);
    }

    #[test]
    fn a_path_that_never_becomes_a_string_is_not_a_site() {
        let src = "let rel = f.strip_prefix(&root).unwrap_or(&f).to_path_buf();";
        assert!(classify(src, src).is_empty());
    }

    #[test]
    fn stripping_a_string_prefix_is_not_a_site() {
        let src = "let s = p.strip_prefix(\"http://\").unwrap_or(p).to_string();";
        assert!(classify(src, src).is_empty());
    }
}
