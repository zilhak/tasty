//! 모듈 선언과 Cargo 타깃 위치로 테스트 전용 파일을 찾는다.
//! 파일 SLOC·플러그인 버전 등 여러 검사가 같은 판정을 사용한다.
//! SLOC 스크립트의 파일명 패턴은 별도 정합 검사에서 이 판정과 대조한다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::cfg_predicate::implies;
use crate::source_text::mask_non_code;

/// 제품 코드에서 제외되는 파일의 저장소 상대 경로를 반환한다.
/// cfg(test)로만 연결된 파일과 그 자식, 패키지 루트의 Cargo 통합 테스트 파일을 포함한다.
/// 일반 모듈 디렉터리의 tests라는 이름만으로 제외하지 않으며 bench/example은 별도 분류하지 않는다.
/// 인라인 테스트 블록은 파일 전체를 제외하지 않고 cfg_gated_lines에서 줄별로 처리한다.
/// 입력의 파일 범위는 소비자가 정한다.
pub fn test_only_files(root: &Path, sources: &[(PathBuf, String)]) -> BTreeSet<PathBuf> {
    let edges = declaration_edges(root, sources);
    let shipping = shipping_closure(sources, &edges);
    sources
        .iter()
        .map(|(p, _)| p.clone())
        .filter(|p| is_cargo_test_target(root, p) || !shipping.contains(p))
        .collect()
}

/// 제품 코드에서 도달 가능한 파일을 구한다.
/// 들어오는 선언이 없는 파일을 시작점으로, 테스트 조건 없는 선언을 따라 더 이상 늘지 않을 때까지 찾는다.
/// 파일을 여러 곳에서 선언했다면 하나라도 제품 경로에서 도달할 수 있으면 포함한다.
fn shipping_closure(
    sources: &[(PathBuf, String)],
    edges: &BTreeMap<PathBuf, Vec<(PathBuf, bool)>>,
) -> BTreeSet<PathBuf> {
    let all: BTreeSet<PathBuf> = sources
        .iter()
        .map(|(p, _)| p.clone())
        .chain(edges.values().flatten().map(|(parent, _)| parent.clone()))
        .collect();
    let mut shipping: BTreeSet<PathBuf> =
        all.into_iter().filter(|p| !edges.contains_key(p)).collect();
    loop {
        let mut grew = false;
        for (child, parents) in edges {
            if shipping.contains(child) {
                continue;
            }
            if parents
                .iter()
                .any(|(parent, gated)| !gated && shipping.contains(parent))
            {
                shipping.insert(child.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    shipping
}

/// 가장 가까운 Cargo.toml의 패키지 루트에서 tests/ 아래인 파일인지 확인한다.
/// src 내부의 tests 모듈은 여기에 해당하지 않는다.
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

/// mod 선언마다 (부모 파일, test 조건 여부)를 기록한다. 같은 자식을 선언하는 부모를 모두 보존한다.
/// 구조는 마스킹한 코드에서, path 속성값은 같은 위치의 원문에서 읽는다.
fn declaration_edges(
    root: &Path,
    sources: &[(PathBuf, String)],
) -> BTreeMap<PathBuf, Vec<(PathBuf, bool)>> {
    let mut edges: BTreeMap<PathBuf, Vec<(PathBuf, bool)>> = BTreeMap::new();
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
                    edges.entry(cand).or_default().push((path.clone(), gated));
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

    #[test]
    fn a_gate_on_the_parent_reaches_the_grandchild() {
        let root = crate::repo_root();
        // 후보 자식 파일의 존재를 확인하므로 실제 파일을 입력으로 사용한다.
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

    /// 하나라도 제품 경로의 선언이 있으면 입력 순서와 관계없이 제품 파일이다.
    #[test]
    fn a_file_declared_both_ways_ships() {
        let root = crate::repo_root();
        let child = (
            PathBuf::from("src/source_guards/sloc_gate_skip_proxy.rs"),
            String::new(),
        );
        let gated = (
            PathBuf::from("src/source_guards/mod.rs"),
            "#[cfg(test)]\nmod sloc_gate_skip_proxy;\n".to_string(),
        );
        // 부모 경로는 실재하지 않아도 된다 — `declaration_edges` 는 후보 **자식**의
        // 실재만 확인하고 부모는 인자로 받은 것을 그대로 믿는다.
        let plain = (
            PathBuf::from("src/source_guards/mod2.rs"),
            "#[path = \"sloc_gate_skip_proxy.rs\"]\nmod reused;\n".to_string(),
        );
        for sources in [
            vec![child.clone(), gated.clone(), plain.clone()],
            vec![child.clone(), plain.clone(), gated.clone()],
        ] {
            assert!(
                test_only_files(&root, &sources).is_empty(),
                "test 조건 없는 선언이 하나라도 있으면 파일 전체를 test 전용으로 분류하지 않는다 — \
                 선언 하나만 보지 않고 모든 모듈 연결을 확인해야 한다"
            );
        }
    }

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
