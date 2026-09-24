//! 아키텍처 문서의 계층 목록과 매니페스트의 내부 의존성을 읽는다.
//! 크레이트 목록은 공통 함수로 수집하며, 문서에 빠진 크레이트는 소비자 검사가 거부한다.

use std::path::Path;

/// crates/ 바로 아래에서 Cargo.toml이 있는 디렉터리를 정렬해 반환한다.
/// workspace exclude는 포함하고 저장소 루트 패키지는 제외한다. 문서의 크레이트 수 기준이다.
/// 결과가 비면 panic한다. 개수 자체를 수집하는 함수라 고정된 측정값을 다시 선언하지 않는다.
pub fn crate_dir_names(repo_root: &Path) -> Vec<String> {
    let dir = repo_root.join("crates");
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().join("Cargo.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !names.is_empty(),
        "{} 아래에 크레이트 디렉토리가 하나도 없다 — 순회가 죽었거나 레포 루트 해석이 \
         틀렸다. 이 값을 그대로 쓰면 소비자가 '위반 0' 으로 읽는다",
        dir.display()
    );
    names.sort();
    names
}

/// 문서의 계층 절 하나 — 이름과 그 절이 **열거한** 크레이트들.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub crates: Vec<String>,
}

/// 각 ### 절의 첫 문단에서 열거한 크레이트를 읽는다.
/// 항목은 ` · `로 구분하고 백틱 이름으로 시작해야 한다. 항목 설명 속 이름은 제외한다.
/// known에 없는 이름은 반환하지 않는다.
pub fn sections(doc: &str, known: &[String]) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut cur: Option<(String, Vec<String>, bool, bool)> = None; // 이름, 항목, 문단 시작, 문단 끝
    for line in doc.lines() {
        if let Some(name) = line.strip_prefix("### ") {
            if let Some((n, c, _, _)) = cur.take() {
                out.push(Section { name: n, crates: c });
            }
            cur = Some((name.replace('\\', ""), Vec::new(), false, false));
            continue;
        }
        if line.starts_with("## ") {
            if let Some((n, c, _, _)) = cur.take() {
                out.push(Section { name: n, crates: c });
            }
            continue;
        }
        let Some((_, items, started, done)) = cur.as_mut() else {
            continue;
        };
        if *done {
            continue;
        }
        if line.trim().is_empty() {
            // 첫 문단이 시작도 안 했으면 절 제목 바로 아래의 빈 줄이다.
            *done = *started;
            continue;
        }
        *started = true;
        for item in line.split(" · ") {
            let t = item.trim_start();
            let Some(rest) = t.strip_prefix('`') else {
                continue;
            };
            let Some(name) = rest.split('`').next() else {
                continue;
            };
            if known.iter().any(|k| k == name) && !items.iter().any(|i| i == name) {
                items.push(name.to_string());
            }
        }
    }
    if let Some((n, c, _, _)) = cur.take() {
        out.push(Section { name: n, crates: c });
    }
    out
}

/// `Cargo.toml` 텍스트에서 **워크스페이스 내부** 의존 이름을 읽는다.
///
/// `[dev-dependencies]` 는 뺀다 — 계층 주장은 산출물의 의존 방향에 대한 것이고, 테스트가
/// 위쪽 크레이트를 쓰는 것은 방향 역전이 아니다.
///
/// 대상 절은 `[dependencies]` 와 `[target.'...'.dependencies]` 다. 이름이 **줄 머리**에
/// 오는 형태(`tasty-x = ...`)만 읽는다 — `{ version = ... }` 안쪽은 내부 크레이트가 아니다.
pub fn internal_deps(manifest: &str, known: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_dev = false;
    for line in manifest.lines() {
        if line.starts_with('[') {
            in_dev = line.contains("dev-dependencies");
            continue;
        }
        if in_dev {
            continue;
        }
        // 들여쓴 줄은 인라인 테이블 안쪽이다 — `features = ["tasty-a"]` 의 항목이
        // 의존으로 세어지면 안 된다.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((lhs, _)) = line.split_once('=') else {
            continue;
        };
        let name = lhs.trim();
        if known.iter().any(|k| k == name) && !out.iter().any(|o| o == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// 문서의 계층 순서를 거스르는 의존을 (소비자, 의존)으로 반환한다.
/// 목록에 없는 크레이트는 검사할 수 없으므로 호출자가 누락·중복 소속을 먼저 검사해야 한다.
pub fn inversions(
    sections: &[Section],
    deps: &std::collections::BTreeMap<String, Vec<String>>,
) -> Vec<(String, String)> {
    let rank = |c: &str| {
        sections
            .iter()
            .position(|s| s.crates.iter().any(|x| x == c))
    };
    let mut out = Vec::new();
    for (consumer, ds) in deps {
        let Some(rc) = rank(consumer) else { continue };
        for d in ds {
            let Some(rd) = rank(d) else { continue };
            if rd > rc {
                out.push((consumer.clone(), d.clone()));
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Vec<String> {
        ["tasty-a", "tasty-b", "tasty-c"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn an_item_body_does_not_claim_a_crate_for_this_section() {
        // `tasty-a` 항목의 설명이 `tasty-b` 를 부른다 — b 는 이 절 소속이 아니다.
        let doc = "### 하나\n`tasty-a`(설명에서 `tasty-b` 를 부른다)\n\n규칙 문단\n\
                   \n### 둘\n`tasty-b`(진짜 여기)\n";
        let s = sections(doc, &known());
        assert_eq!(s[0].crates, vec!["tasty-a"], "설명 속 이름을 소속으로 셌다");
        assert_eq!(s[1].crates, vec!["tasty-b"]);
    }

    #[test]
    fn the_rule_paragraph_after_the_list_is_not_an_enumeration() {
        let doc = "### 하나\n`tasty-a`\n\n이 절은 `tasty-c` 에 의존할 수 있다.\n";
        let s = sections(doc, &known());
        assert_eq!(s[0].crates, vec!["tasty-a"], "규칙 문단을 열거로 셌다");
    }

    #[test]
    fn prose_before_a_later_crate_example_is_not_an_enumeration() {
        let doc = "### 기준\n\n\n분리할 때 의존 방향을 확인한다.\n설명은 여러 줄일 수 있다.\n\n`tasty-a`는 이 기준의 예다.\n\n### 실제 계층\n\n`tasty-a` · `tasty-b`\n`tasty-c`\n";
        let s = sections(doc, &known());
        assert!(
            s[0].crates.is_empty(),
            "첫 설명 문단 뒤의 예시를 계층 소속으로 셌다"
        );
        assert_eq!(s[1].crates, vec!["tasty-a", "tasty-b", "tasty-c"]);
    }

    #[test]
    fn dev_dependencies_are_not_part_of_the_direction_claim() {
        let m = "[dependencies]\ntasty-a = { path = \"../a\" }\n\
                 [dev-dependencies]\ntasty-b = { path = \"../b\" }\n";
        assert_eq!(internal_deps(m, &known()), vec!["tasty-a"]);
    }

    #[test]
    fn a_target_specific_dependency_still_counts() {
        let m = "[target.'cfg(unix)'.dependencies]\ntasty-c = { path = \"../c\" }\n";
        assert_eq!(internal_deps(m, &known()), vec!["tasty-c"]);
    }

    fn secs(spec: &[(&str, &[&str])]) -> Vec<Section> {
        spec.iter()
            .map(|(n, cs)| Section {
                name: (*n).to_string(),
                crates: cs.iter().map(|c| (*c).to_string()).collect(),
            })
            .collect()
    }

    fn deps(spec: &[(&str, &[&str])]) -> std::collections::BTreeMap<String, Vec<String>> {
        spec.iter()
            .map(|(c, ds)| {
                (
                    (*c).to_string(),
                    ds.iter().map(|d| (*d).to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn a_dependency_on_a_higher_section_is_an_inversion() {
        let s = secs(&[("아래", &["tasty-a"]), ("위", &["tasty-b"])]);
        let d = deps(&[("tasty-a", &["tasty-b"])]);
        assert_eq!(
            inversions(&s, &d),
            vec![("tasty-a".to_string(), "tasty-b".to_string())]
        );
    }

    #[test]
    fn the_same_edge_the_other_way_is_not() {
        let s = secs(&[("아래", &["tasty-a"]), ("위", &["tasty-b"])]);
        let d = deps(&[("tasty-b", &["tasty-a"])]);
        assert!(inversions(&s, &d).is_empty());
    }

    #[test]
    fn an_edge_inside_one_section_is_not_an_inversion() {
        let s = secs(&[("하나", &["tasty-a", "tasty-b"])]);
        let d = deps(&[("tasty-a", &["tasty-b"])]);
        assert!(inversions(&s, &d).is_empty());
    }

    #[test]
    fn an_indented_key_inside_an_inline_table_is_not_a_dependency() {
        let m = concat!(
            "[dependencies]\n",
            "windows-sys = { version = \"0.59\", features = [\n",
            "    \"tasty-a\",\n",
            "] }\n",
        );
        assert!(internal_deps(m, &known()).is_empty());
    }
}
