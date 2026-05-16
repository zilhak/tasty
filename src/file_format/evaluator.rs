//! detector rule 평가자. Phase A 는 cheap path (확장자/glob/is-directory) 만.
//! magic bytes / MIME / Lua / structure-check 는 Phase B-D 에서 본격 구현.

use super::types::{DetectorRuleKind, FileTarget};

/// 단일 rule 이 target 에 매칭되는지 평가. cheap path 만.
///
/// 디렉토리 분기는 호출자(registry::identify)가 pre-filter 로 처리하므로 여기서는
/// rule kind 별로 매칭 여부만 판단.
pub fn evaluate_cheap(rule: &DetectorRuleKind, target: &FileTarget) -> bool {
    let path = target.as_path();
    match rule {
        DetectorRuleKind::Extension { values } => path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| {
                let lower = ext.to_ascii_lowercase();
                values.iter().any(|v| v.eq_ignore_ascii_case(&lower))
            })
            .unwrap_or(false),

        DetectorRuleKind::PathGlob { pattern } => {
            // 1단계: 파일명만 비교 (간단한 wildcard). 본격 glob 매칭은 globset crate 도입
            // 시점에 교체. `Dockerfile`, `*.config.json` 같은 패턴 최소 지원.
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => return false,
            };
            simple_glob_match(pattern, name)
        }

        DetectorRuleKind::IsDirectory => target.is_directory(),

        // 다음 항목들은 Phase A cheap path 에서 false (deep 단계에서 평가).
        DetectorRuleKind::Mime { .. }
        | DetectorRuleKind::Magic { .. }
        | DetectorRuleKind::Lua { .. }
        | DetectorRuleKind::StructureCheck { .. }
        | DetectorRuleKind::Unknown { .. } => false,
    }
}

/// 매우 단순한 glob 매처 — `*` 와 정확 일치만 지원. Phase B 에서 `globset` 도입.
fn simple_glob_match(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut cursor = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !name[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
        } else if i == parts.len() - 1 {
            if !name.ends_with(part) {
                return false;
            }
        } else {
            match name[cursor..].find(part) {
                Some(pos) => cursor += pos + part.len(),
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn target(p: &str) -> FileTarget {
        FileTarget::new(PathBuf::from(p))
    }

    #[test]
    fn extension_lowercase_match() {
        let rule = DetectorRuleKind::Extension {
            values: vec!["md".into(), "markdown".into()],
        };
        assert!(evaluate_cheap(&rule, &target("a/b.md")));
        assert!(evaluate_cheap(&rule, &target("a/b.MD")));
        assert!(evaluate_cheap(&rule, &target("c.markdown")));
        assert!(!evaluate_cheap(&rule, &target("c.txt")));
        assert!(!evaluate_cheap(&rule, &target("noext")));
    }

    #[test]
    fn path_glob_exact_and_wildcard() {
        let exact = DetectorRuleKind::PathGlob {
            pattern: "Dockerfile".into(),
        };
        assert!(evaluate_cheap(&exact, &target("repo/Dockerfile")));
        assert!(!evaluate_cheap(&exact, &target("repo/Dockerfile.txt")));

        let wild = DetectorRuleKind::PathGlob {
            pattern: "*.config.json".into(),
        };
        assert!(evaluate_cheap(&wild, &target("foo/bar.config.json")));
        assert!(!evaluate_cheap(&wild, &target("foo/bar.json")));
    }

    #[test]
    fn deep_kinds_return_false_in_cheap() {
        let mime = DetectorRuleKind::Mime {
            types: vec!["application/pdf".into()],
        };
        assert!(!evaluate_cheap(&mime, &target("x.pdf")));

        let magic = DetectorRuleKind::Magic {
            offset: 0,
            bytes: vec![0x25, 0x50, 0x44, 0x46],
        };
        assert!(!evaluate_cheap(&magic, &target("x.pdf")));
    }
}
