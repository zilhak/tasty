//! **레포 상대 경로를 문자열로 펴는 자리는 구분자를 정규화한다.**
//!
//! `Path::strip_prefix` 는 **그 플랫폼의 구분자**를 그대로 남긴다. 그 결과를
//! `to_string_lossy()` 로 펴서 소스에 박힌 `/` 리터럴(명부의 좌표·접두사)과 비교하면
//! Windows 에서는 `crates\x\y.rs` 가 되어 **어떤 리터럴과도 안 맞는다.** 그리고 그
//! 어긋남은 예외가 아니라 **조용한 0** 이다 — 조회가 전부 빗나가고 가드는 "명부에 없다"
//! 또는 "위반 0" 을 보고한다. 두 방향 다 사람이 안 본다.
//!
//! 실측 둘(2026-09-06):
//! - 갤러리 사본 판정이 Windows 에서만 11 건을 미등록으로 잡았다 — 같은 트리, 같은 커밋,
//!   Linux 는 초록. 판정의 입력이 트리뿐인데 플랫폼이 답을 갈랐다.
//! - `plugin_locale_specific_literals` 는 자기 순회로 만든 경로를 공용 스캐너가 낸 집합과
//!   맞췄다. Windows 에서는 한 건도 안 맞아 `#[cfg(test)]` 필터가 **통째로 꺼진 채**
//!   초록이 된다.
//!
//! # 왜 Linux 에서 도는 이 가드가 그 성질을 잡는가
//!
//! 잡지 않는다 — **잡을 수 없다.** 고치기 전에도 Linux 는 `/` 를 내므로, 구분자를 재는
//! 단정은 여기서 언제나 참이고 그건 공허한 초록이 하나 느는 것이다. 그래서 이 가드는
//! 성질이 아니라 **형태**를 본다: 경로를 펴는 자리가 정규화를 거치는가. 형태는 소스에
//! 있으니 어느 플랫폼에서든 같은 답이 나온다.
//!
//! # 세 갈래
//!
//! - **helper** — `source_text::repo_relative` 를 지난다. 규칙이 한 벌인 형태다.
//! - **hand** — 그 자리에서 `replace('\\', "/")` 를 손으로 붙인다. 동작은 맞지만 규칙이
//!   자리 수만큼 복제된다. 수를 못 박아 두고 helper 로 옮길 때마다 내린다.
//! - **none** — 아무것도 안 한다. **이 수는 0 이어야 한다.**

use tasty_doc_guards::source_text::mask_non_code;

/// 경로를 성분으로 직접 다뤄 구분자가 애초에 안 생기는 자리 — **자리로** 적는다.
///
/// `components()` 로 쪼갠 뒤 자기가 `/` 로 잇거나, 성분 하나만 쓰는 형태다. 부류로
/// 면제하지 않는 이유는 다음 사람이 아무 자리나 "성분으로 다룬다" 고 부르지 않게 하려는
/// 것이다.
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

/// 루트를 벗기는 호출인가 — 문자열 접두사 제거(`strip_prefix(r"\\\\?\\")` 등)와 가른다.
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

/// `strip_prefix(<루트>)` 자리를 찾아 갈래를 정한다. 순수 함수 — 합성 입력을 먹인다.
fn classify(masked: &str, raw: &str) -> Vec<(usize, Kind)> {
    let mut out = Vec::new();
    let bytes: Vec<&str> = masked.lines().collect();
    let raws: Vec<&str> = raw.lines().collect();
    for (i, line) in bytes.iter().enumerate() {
        if !line.contains("strip_prefix(") {
            continue;
        }
        // 인자가 루트인 호출만. 인자가 다음 줄에 오는 형태까지 한 줄 더 본다.
        let head = format!("{} {}", line, bytes.get(i + 1).unwrap_or(&""));
        let after = head.split("strip_prefix(").nth(1).unwrap_or("");
        if !ROOT_ARGS.iter().any(|a| {
            after.trim_start().starts_with(a.trim_start_matches('&'))
                || after.trim_start().starts_with(a)
        }) {
            continue;
        }
        // 이 자리부터 문장 끝(;)까지, 그리고 바로 앞 두 줄(helper 로 감싼 형태).
        // 갈래 판정은 **원문**에서 한다. `mask_non_code` 는 리터럴 속을 지우므로
        // `replace('\\', "/")` 의 인자가 통째로 사라져 손세공이 미정규화로 보인다 —
        // 실측으로 34 자리가 그렇게 잘못 분류됐다. 자리를 *찾는* 것은 여전히 마스크에서
        // 한다(주석·문자열 속 언급을 안 세려고).
        let mut stmt = String::new();
        for k in i.saturating_sub(2)..(i + 8).min(raws.len()) {
            // 뒤쪽에서 블록이 열리면 거기서 끊는다 — `let Ok(rel) = p.strip_prefix(&root)
            // else { ... p.display() ... }` 의 else 몸통은 **다른 문장**인데, 안 끊으면
            // 그 `.display()` 를 이 사슬의 평탄화로 오독한다(실측 1 건).
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
            // 문자열로 안 펴면 이 결함의 대상이 아니다(PathBuf 로만 다룬다).
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
        "루트를 벗기는 자리를 {} 개밖에 못 찾았다(하한 20) — 수집이 깨지면 아래 판정이 \
         전부 공허하다",
        sites.len()
    );
    assert!(
        !HANDLES_COMPONENTS.is_empty(),
        "성분으로 다루는 자리 명부가 비었다 — 비면 그 자리들이 갈래 판정에 섞여 든다"
    );

    let bad: Vec<String> = sites
        .iter()
        .filter(|s| s.kind == Kind::None)
        .map(|s| format!("  {}:{}", s.rel, s.line))
        .collect();
    assert!(
        bad.is_empty(),
        "레포 상대 경로를 문자열로 펴면서 구분자를 정규화하지 않는 자리가 {} 개다:\n{}\n\n\
         `tasty_doc_guards::source_text::repo_relative` 를 지나게 해라. Windows 에서만 \
         어긋나고 **그 어긋남은 예외가 아니라 조용한 0** 이라, 여기서 안 막으면 아무도 \
         못 본다.\n\
         ★ 이 목록에서 자리를 빼는 방법은 하나뿐이다 — 정규화를 붙이는 것. 보고용으로만 \
         쓴다고 넘기지 마라: 보고 문자열도 다른 가드의 좌표로 인용되고, 그때 다시 리터럴과 \
         비교된다.",
        bad.len(),
        bad.join("\n")
    );

    let hand = sites.iter().filter(|s| s.kind == Kind::Hand).count();
    let helper = sites.iter().filter(|s| s.kind == Kind::Helper).count();
    assert_eq!(
        hand, 29,
        "손으로 `replace('\\\\', \"/\")` 를 붙인 자리가 {hand} 개다(기록 29). 늘었으면 \
         규칙이 한 벌 더 복제된 것이고, 줄었으면 helper 로 옮긴 만큼 이 수를 내려라 — \
         남는 여유가 곧 안 보는 구간이다"
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

    /// 문자열로 안 펴면 이 결함의 대상이 아니다 — `PathBuf` 끼리는 구분자가 양쪽 다 같다.
    #[test]
    fn a_path_that_never_becomes_a_string_is_not_a_site() {
        let src = "let rel = f.strip_prefix(&root).unwrap_or(&f).to_path_buf();";
        assert!(classify(src, src).is_empty());
    }

    /// 문자열 접두사 제거는 경로 루트 벗기기가 아니다 — 인자로 가른다.
    #[test]
    fn stripping_a_string_prefix_is_not_a_site() {
        let src = "let s = p.strip_prefix(\"http://\").unwrap_or(p).to_string();";
        assert!(classify(src, src).is_empty());
    }
}
