//! 플랫폼 조건이 있는 IPC 분기에는 반대 조건의 분기도 있어야 한다.
//! CLI·메서드 표에 있는 기능을 지원하지 않는 플랫폼에서는 메서드 없음 대신 미지원 이유를 응답한다(ADR-0004).
//!
//! 소스의 조건 문자열에서 공백을 제거한 뒤 cond와 not(cond)를 비교한다.
//! 동등한 다른 논리식을 모두 인정하거나 응답의 오류 코드·실제 도달 여부를 검증하는 것은 아니다.
//! 플랫폼 조건이 없는 다른 분기가 있어도 정확한 반대 조건의 짝을 요구한다.

use std::collections::{BTreeMap, BTreeSet};

use super::repo_root;

const DISPATCH_SOURCES: &[&str] = &["src/adapters/ipc/handler.rs"];

/// 2026-09-05 플랫폼 조건의 메서드 2개를 측정했다. 빈 수집을 찾기 위한 하한이다.
const MIN_PLATFORM_ARMS: usize = 2;

/// cfg 뒤의 패턴 길이와 ;·{를 검사해 const·use 선언을 dispatch 분기로 오인하지 않도록 한다.
const MAX_ARM_PATTERN: usize = 200;

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

fn normalize(cond: &str) -> String {
    cond.chars().filter(|c| !c.is_whitespace()).collect()
}

fn cfg_condition(src: &str, open_paren: usize) -> Option<(String, usize)> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    for (i, _) in src[open_paren..].char_indices() {
        match bytes[open_paren + i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((
                        src[open_paren + 1..open_paren + i].to_string(),
                        open_paren + i,
                    ));
                }
            }
            _ => {}
        }
    }
    None
}

fn literals(seg: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = seg;
    while let Some(at) = rest.find('"') {
        let after = &rest[at + 1..];
        let Some(end) = after.find('"') else { break };
        out.insert(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

pub(super) struct Arms {
    by_method: BTreeMap<String, BTreeSet<String>>,
    platform: Vec<(String, String)>,
}

/// 구조 구분자는 마스킹한 소스에서 찾고 조건·메서드 이름은 같은 바이트 구간의 원문에서 읽는다.
pub(super) fn scan(src: &str) -> Arms {
    let code = tasty_doc_guards::source_text::mask_non_code_aligned(src);
    let mut by_method: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut platform: Vec<(String, String)> = Vec::new();
    let mut from = 0usize;
    while let Some(at) = code[from..].find("#[cfg(") {
        let at = from + at;
        let open = at + "#[cfg".len();
        let Some((_, close)) = cfg_condition(&code, open) else {
            break;
        };
        let cond = src[open + 1..close].to_string();
        from = close + 1;
        let Some(arrow) = code[from..].find("=>") else {
            continue;
        };
        let seg_code = &code[from..from + arrow];
        let seg = &src[from..from + arrow];
        let looks_like_arm = arrow <= MAX_ARM_PATTERN
            && !seg_code.contains(';')
            && !seg_code.contains('{')
            && seg.contains('"');
        if !looks_like_arm {
            continue;
        }
        let cond_n = normalize(&cond);
        for m in literals(seg) {
            by_method
                .entry(m.clone())
                .or_default()
                .insert(cond_n.clone());
            if cond.contains("target_os") {
                platform.push((m, cond_n.clone()));
            }
        }
    }
    Arms {
        by_method,
        platform,
    }
}

fn complement_of(cond: &str) -> String {
    if let Some(inner) = cond.strip_prefix("not(").and_then(|s| s.strip_suffix(')')) {
        inner.to_string()
    } else {
        format!("not({cond})")
    }
}

#[test]
fn a_platform_gated_method_still_answers_elsewhere() {
    let mut arms = 0usize;
    let mut orphan: Vec<String> = Vec::new();
    for rel in DISPATCH_SOURCES {
        let found = scan(&read(rel));
        arms += found.platform.len();
        for (method, cond) in &found.platform {
            let want = complement_of(cond);
            let has = found
                .by_method
                .get(method)
                .is_some_and(|conds| conds.contains(&want));
            if !has {
                orphan.push(format!(
                    "{rel}: `{method}` — `{cond}` 만 있고 `{want}` 가 없다"
                ));
            }
        }
    }
    assert!(
        arms >= MIN_PLATFORM_ARMS,
        "플랫폼 조건의 메서드를 {arms}개만 찾았다(하한 {MIN_PLATFORM_ARMS}, 2026-09-05 측정 2개). 추출기와 DISPATCH_SOURCES를 확인한다."
    );
    assert!(
        orphan.is_empty(),
        "플랫폼 조건의 dispatch 분기에 반대 조건의 짝이 없다. 다른 플랫폼에서도 미지원 이유를 응답하거나 CLI·메서드 표의 제공 범위를 함께 검토한다:\n  {}",
        orphan.join("\n  ")
    );
}

#[cfg(test)]
mod exemption_mutations {
    use super::*;

    #[test]
    fn a_lone_platform_arm_is_caught() {
        let src = "\
match m {
    #[cfg(all(target_os = \"macos\", feature = \"gui\"))]
    \"surface.raw_key\" => go(),
    _ => not_found(),
}
";
        let found = scan(src);
        assert_eq!(found.platform.len(), 1, "게이트 걸린 arm 을 못 찾았다");
        let (m, cond) = &found.platform[0];
        assert!(
            !found.by_method[m].contains(&complement_of(cond)),
            "짝이 없는데 있다고 판정했다"
        );
    }

    #[test]
    fn the_complement_closes_it() {
        let src = "\
match m {
    #[cfg(all(target_os = \"macos\", feature = \"gui\"))]
    \"surface.raw_key\" => go(),
    #[cfg(not(all(target_os = \"macos\", feature = \"gui\")))]
    \"surface.raw_key\" => why_not(),
    _ => not_found(),
}
";
        let found = scan(src);
        let (m, cond) = &found.platform[0];
        assert!(found.by_method[m].contains(&complement_of(cond)));

        let wrong = "\
match m {
    #[cfg(all(target_os = \"macos\", feature = \"gui\"))]
    \"surface.raw_key\" => go(),
    #[cfg(not(feature = \"gui\"))]
    \"surface.raw_key\" => something_else(),
}
";
        let found = scan(wrong);
        let (m, cond) = &found.platform[0];
        assert!(
            !found.by_method[m].contains(&complement_of(cond)),
            "다른 조건을 정확한 반대 조건의 짝으로 인정했다"
        );
    }

    #[test]
    fn a_gated_item_is_not_mistaken_for_an_arm() {
        let src = "\
#[cfg(not(all(target_os = \"macos\", feature = \"gui\")))]
const WHY: &str = \"platform\";
fn f() { let x = a => b; }
";
        let found = scan(src);
        assert!(
            !found.by_method.contains_key("platform"),
            "게이트된 항목의 문자열을 arm 패턴으로 집었다: {:?}",
            found.by_method
        );
    }

    #[test]
    fn an_or_pattern_counts_each_name() {
        let src = "\
match m {
    #[cfg(all(target_os = \"macos\", feature = \"gui\"))]
    \"a.one\" | \"a.two\" => go(),
    _ => x(),
}
";
        let found = scan(src);
        assert_eq!(found.platform.len(), 2, "`|` 로 묶인 이름을 하나로 셌다");
    }
}

/// CLI·메서드 표에 target_os가 없어 플랫폼 차이를 dispatch에서 처리한다는 전제를 확인한다. 바뀌면 ADR-0004의 선택을 다시 검토한다.
mod platform_uniform_layers {
    use super::*;

    const UNIFORM_TREES: &[&str] = &["crates/tasty-cli/src"];
    const UNIFORM_FILES: &[&str] = &["crates/tasty-ipc/src/method_meta.rs"];

    /// 2026-09-05 Rust 파일 84개를 측정한 뒤 수집 누락을 찾도록 둔 하한이다.
    const MIN_SCANNED: usize = 40;

    fn rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                rs_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    #[test]
    fn registration_and_cli_do_not_branch_on_the_platform() {
        let root = repo_root();
        let mut files = Vec::new();
        for t in UNIFORM_TREES {
            rs_files(&root.join(t), &mut files);
        }
        for f in UNIFORM_FILES {
            files.push(root.join(f));
        }
        files.sort();
        assert!(
            files.len() >= MIN_SCANNED,
            "Rust 파일을 {}개만 읽었다(하한 {MIN_SCANNED}, 2026-09-05 측정 84개). 수집 범위를 확인한다.",
            files.len()
        );

        let mut hits: Vec<String> = Vec::new();
        for f in &files {
            let Ok(raw) = std::fs::read_to_string(f) else {
                continue;
            };
            let masked = crate::source_guards::mask_non_code(&raw);
            for (i, line) in masked.lines().enumerate() {
                if line.contains("target_os") {
                    let rel = tasty_doc_guards::source_text::repo_relative(
                        f.strip_prefix(&root).unwrap_or(f),
                    );
                    let rel = rel.display();
                    hits.push(format!("{rel}:{}", i + 1));
                }
            }
        }
        assert!(
            hits.is_empty(),
            "CLI 또는 메서드 표에서 target_os를 찾았다. 플랫폼 차이는 dispatch에서 처리한다는 ADR-0004의 전제가 바뀌었는지 검토한다:\n  {}",
            hits.join("\n  ")
        );
    }

    #[test]
    fn the_scan_would_see_a_platform_branch() {
        let masked = crate::source_guards::mask_non_code(
            "#[cfg(target_os = \"macos\")]\nfn only_here() {}\n",
        );
        assert!(
            masked.contains("target_os"),
            "마스킹 과정에서 cfg 속성이 사라졌다"
        );
        let commented = crate::source_guards::mask_non_code("// target_os 는 여기서 안 쓴다\n");
        assert!(
            !commented.contains("target_os"),
            "주석의 target_os 언급을 코드로 수집했다"
        );
    }
}
