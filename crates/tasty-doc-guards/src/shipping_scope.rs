//! **이 파일은 출하되는가** — 한 물음, 한 판정기.
//!
//! 이 물음에 답하는 자리가 셋이었다: 파일 SLOC 게이트의 `skip()` 글롭(이름),
//! `sloc_gate_skip_proxy` 의 선언 파서, 그리고 `tests/cli_method_table_parity.rs` 의
//! 자체 사본. 셋의 답이 갈리면 갈린 쪽은 조용하다 — 면제하는 방향으로 틀리면 그 파일은
//! 검사에서 사라지고, 사라진 것은 위반으로 안 보인다.
//!
//! 여기 사는 것이 **선언 기반 판정** 하나다. 이름은 성질이 아니라서 개명 한 번으로
//! 따라오지만, `#[cfg(test)] mod x;` 는 파일 안에 있고 diff 에 남는다.
//!
//! 게이트 스크립트의 이름 글롭은 **대리인으로 남아 있고**, `sloc_gate_skip_proxy` 가
//! 그것을 이 판정에 양방향으로 못박는다(그 모듈의 (가)·(나)). 대리인을 없애는 것과
//! 판정기를 하나로 모으는 것은 다른 물음이다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::cfg_predicate::implies;
use crate::source_text::mask_non_code;

/// 선언상 **출하되지 않는** 파일 집합(레포 상대 경로).
///
/// **성질은 "이 파일이 출하 산출물(라이브러리/바이너리)에 들어가는가" 다** — 형태(디렉토리
/// 이름)가 아니라(R426). 출하 안 되는 형태를 전부 세고 각각 넣을지·이유를 밝힌다:
///
/// - **`#[cfg(test)] mod x;` 로 (전이적으로) 선언된 파일** — 넣는다. 부모가 test 게이트면
///   자식도 안 나간다(전이 폐쇄). diff 에 남는 선언 기반 판정이라 개명에 안 흔들린다.
/// - **cargo 예약 통합테스트 타깃**(패키지 루트 바로 아래 `tests/` — [`is_cargo_test_target`])
///   — 넣는다. cargo 가 별도 타깃으로 빌드해 lib/bin 산출물에 안 들어간다. 이들은 **루트
///   파일이라 들어오는 `mod` 간선이 없어** 위 전이 폐쇄로는 절대 안 잡힌다(그게 통합테스트
///   env/cwd 변형을 조용히 놓치던 사각이었다). `src/**/tests/` 같은 일반 모듈 디렉토리는
///   패키지 루트 바로 밑이 아니라 제외된다 — 그건 출하되는 모듈일 수 있다.
/// - **bench·example 디렉토리** — 성질상(bench 하네스·example 바이너리는 앱 산출물 밖)
///   후보지만 **이 레포에 그 디렉토리가 없다** — 양성 대조가 불가능한 절은 두지 않는다
///   (R415). 생기면 위 통합테스트와 같은 구조 규칙을 확장한다.
/// - **인라인 `#[cfg(test)]` 블록** — 파일 단위 물음이 아니라 줄 단위라 여기 물음이 아니다.
///   [`crate::cfg_predicate::cfg_gated_lines`] 가 답한다.
///
/// 입력은 `(레포 상대 경로, 원문)` 목록이다. 파일 순회를 인자로 받는 이유는 소비자마다
/// 스캔 범위가 다르기 때문이다 — 게이트 대리인은 `src`·`crates` 전체를 보고, CLI 메서드
/// 대조는 자기 스캔 루트만 본다. **판정은 같고 모수만 다르다.**
pub fn test_only_files(root: &Path, sources: &[(PathBuf, String)]) -> BTreeSet<PathBuf> {
    let edges = declaration_edges(root, sources);
    fn walk(
        p: &PathBuf,
        edges: &BTreeMap<PathBuf, (PathBuf, bool)>,
        seen: &mut BTreeSet<PathBuf>,
    ) -> bool {
        if !seen.insert(p.clone()) {
            return false;
        }
        match edges.get(p) {
            None => false,
            Some((_, true)) => true,
            Some((parent, false)) => walk(parent, edges, seen),
        }
    }
    sources
        .iter()
        .map(|(p, _)| p.clone())
        .filter(|p| is_cargo_test_target(root, p) || walk(p, &edges, &mut BTreeSet::new()))
        .collect()
}

/// cargo 예약 통합테스트 타깃인가 — **패키지 루트(`Cargo.toml` 이 있는 디렉토리) 바로 아래의
/// `tests/`** 다. cargo 가 이들을 별도 타깃으로 빌드해 lib/bin 산출물에 안 넣으므로, 선언이
/// 없어도 출하되지 않는다. `src/**/tests/` 같은 일반 모듈 디렉토리는 패키지 루트 바로 밑이
/// 아니라 제외된다(그건 출하될 수 있다) — 경로에 `/tests/` 가 있는지만 봐서는 안 갈린다.
///
/// **이 판정기는 하나다.** 같은 물음("cargo 타깃이라 안 나가는가")을 본체 SLOC 게이트 대리인
/// (`sloc_gate_skip_proxy`)도 갖는데, 사본을 두면 답이 갈린다(R414) — 그쪽이 여기로 위임한다.
pub fn is_cargo_test_target(root: &Path, rel: &Path) -> bool {
    let full = root.join(rel);
    let mut dir = full.parent();
    while let Some(d) = dir {
        if d.join("Cargo.toml").is_file() {
            return full
                .strip_prefix(d)
                .is_ok_and(|rest| rest.starts_with("tests"));
        }
        dir = d.parent();
    }
    false
}

/// `mod X;` 선언에서 모듈 파일로 가는 간선. 값은 `(부모 파일, cfg 가 test 를 함의하는가)`.
///
/// **판정은 선언 지점에서 한다** — 파일 안을 grep 하면 문서주석과 문자열이 섞인다.
/// 마스킹한 사본으로 줄을 읽되 `#[path = "..."]` 의 값은 문자열이라 마스킹에 지워지므로
/// **같은 줄 번호의 원문**에서 꺼낸다(`mask_non_code` 가 줄 구조를 보존한다).
fn declaration_edges(
    root: &Path,
    sources: &[(PathBuf, String)],
) -> BTreeMap<PathBuf, (PathBuf, bool)> {
    let mut edges = BTreeMap::new();
    for (path, raw) in sources {
        let masked = mask_non_code(raw);
        let mlines: Vec<&str> = masked.lines().collect();
        let rlines: Vec<&str> = raw.lines().collect();
        for (i, mline) in mlines.iter().enumerate() {
            let Some(name) = mod_decl_name(mline) else {
                continue;
            };
            let mut gated = false;
            let mut explicit: Option<String> = None;
            let mut j = i;
            while j > 0 {
                j -= 1;
                let t = mlines[j].trim();
                if t.is_empty() || rlines[j].trim_start().starts_with("//") {
                    continue;
                }
                if !t.starts_with("#[") {
                    break;
                }
                if let Some(pred) = t.strip_prefix("#[cfg(").and_then(|s| s.strip_suffix(")]"))
                    && implies(pred, "test")
                {
                    gated = true;
                }
                if rlines[j].contains("path")
                    && let Some(v) = path_attr_value(rlines[j])
                {
                    explicit = Some(v);
                }
            }
            let base = match path.file_name().and_then(|n| n.to_str()) {
                Some("mod.rs") | Some("lib.rs") | Some("main.rs") => {
                    path.parent().map(Path::to_path_buf)
                }
                _ => path
                    .parent()
                    .map(|p| p.join(path.file_stem().unwrap_or_default())),
            }
            .unwrap_or_default();
            let candidates: Vec<PathBuf> = match &explicit {
                Some(rel) => vec![path.parent().unwrap_or(Path::new("")).join(rel)],
                None => vec![
                    base.join(format!("{name}.rs")),
                    base.join(&name).join("mod.rs"),
                ],
            };
            for cand in candidates {
                if root.join(&cand).is_file() {
                    edges.insert(cand, (path.clone(), gated));
                    break;
                }
            }
        }
    }
    edges
}

fn mod_decl_name(line: &str) -> Option<String> {
    let t = line.trim();
    let t = t.strip_prefix("pub ").unwrap_or(t);
    let t = match t.find("mod ") {
        Some(0) => t,
        _ if t.starts_with("pub(") => t.split_once(") ").map(|(_, r)| r)?,
        _ => return None,
    };
    let rest = t.strip_prefix("mod ")?;
    let name = rest.strip_suffix(';')?.trim();
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .then(|| name.to_string())
}

fn path_attr_value(raw_line: &str) -> Option<String> {
    let at = raw_line.find("path")?;
    let rest = &raw_line[at..];
    let open = rest.find('"')?;
    let after = &rest[open + 1..];
    let close = after.find('"')?;
    Some(after[..close].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 전이 폐쇄 — 부모가 test 게이트면 자식 선언에 게이트가 없어도 안 나간다.
    /// 합성 입력으로 본다: 실재 트리로만 보면 트리가 바뀔 때 조용히 빈 모수가 된다.
    #[test]
    fn a_gate_on_the_parent_reaches_the_grandchild() {
        let root = crate::repo_root();
        // 실재하는 파일 하나를 부모로 삼는다 — `declaration_edges` 가 후보의 실재를
        // 확인하므로 존재하지 않는 이름으로는 간선이 생기지 않는다.
        let sources = vec![
            (
                PathBuf::from("src/source_guards/mod.rs"),
                "#[cfg(test)]\nmod sloc_gate_skip_proxy;\n".to_string(),
            ),
            (
                PathBuf::from("src/source_guards/sloc_gate_skip_proxy.rs"),
                String::new(),
            ),
        ];
        let found = test_only_files(&root, &sources);
        assert!(found.contains(&PathBuf::from("src/source_guards/sloc_gate_skip_proxy.rs")));
    }

    /// 게이트가 없으면 자식은 출하된다 — 위 테스트의 대조군.
    #[test]
    fn a_plain_declaration_ships() {
        let root = crate::repo_root();
        let sources = vec![
            (
                PathBuf::from("src/source_guards/mod.rs"),
                "mod sloc_gate_skip_proxy;\n".to_string(),
            ),
            (
                PathBuf::from("src/source_guards/sloc_gate_skip_proxy.rs"),
                String::new(),
            ),
        ];
        assert!(test_only_files(&root, &sources).is_empty());
    }
}
