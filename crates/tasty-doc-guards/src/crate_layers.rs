//! `docs/architecture/index.md` 의 **계층 절**과 각 크레이트의 **워크스페이스 내부 의존**을
//! 텍스트로 읽는다.
//!
//! 그 문서는 절을 순서대로 늘어놓고 "의존은 아래 계층 순서로만 흐른다" 고 주장한다.
//! 그 주장에 못이 없으면 순서가 조용히 낡는다 — 실측으로 밟았다: 절 넷이 잘못된 자리에
//! 있어 **정상 의존 넷이 위반처럼 보였고**, 문서에 예외로 적힌 것은 하나뿐이었다.
//!
//! 왜 `cargo metadata` 가 아니라 텍스트인가: 이 크레이트는 의존이 0 이라 콜드 빌드가 1 초
//! 미만이고, 그래서 `doc-guards.yml` 이 경로 필터 없이 매 push 돌 수 있다(ADR-0138).
//! cargo 를 부르면 그 전제가 무너진다.
//!
//! **모르는 형태는 조용히 건너뛰지 않는다.** 절에 안 잡힌 크레이트는 소비자가 실패로
//! 보고한다 — 건너뛰면 그 크레이트의 의존이 아예 안 세어져 통과한다.

/// 문서의 계층 절 하나 — 이름과 그 절이 **열거한** 크레이트들.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub crates: Vec<String>,
}

/// `### ` 절을 문서 순서대로 읽고, 각 절의 **첫 문단**에서 열거 항목을 뽑는다.
///
/// 열거의 형태는 문서의 규약이다: 항목은 ` · ` 로 갈리고 **각 항목이 백틱 이름으로
/// 시작**한다. 항목 안의 설명에도 다른 크레이트 이름이 백틱으로 나오므로(예: `tasty-ansi`
/// 항목이 `tasty-terminal`·`tasty-output` 을 부른다) **항목 머리만** 본다 — 문단 전체에서
/// 이름을 긁으면 남의 절 크레이트가 이 절 소속으로 잡힌다(실측으로 밟았다).
///
/// `known` 은 실재하는 크레이트 디렉토리 이름이다. 그 밖의 이름(바이너리 이름 등)은 뺀다.
pub fn sections(doc: &str, known: &[String]) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut cur: Option<(String, Vec<String>, bool)> = None; // 이름, 항목, 첫 문단 끝났나
    for line in doc.lines() {
        if let Some(name) = line.strip_prefix("### ") {
            if let Some((n, c, _)) = cur.take() {
                out.push(Section { name: n, crates: c });
            }
            cur = Some((name.replace('\\', ""), Vec::new(), false));
            continue;
        }
        if line.starts_with("## ") {
            if let Some((n, c, _)) = cur.take() {
                out.push(Section { name: n, crates: c });
            }
            continue;
        }
        let Some((_, items, done)) = cur.as_mut() else {
            continue;
        };
        if *done {
            continue;
        }
        if line.trim().is_empty() {
            // 첫 문단이 시작도 안 했으면 절 제목 바로 아래의 빈 줄이다.
            *done = !items.is_empty();
            continue;
        }
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
    if let Some((n, c, _)) = cur.take() {
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

/// 절 순서를 어기는 간선 — `(소비자, 의존)`. 문서 순서에서 **의존이 소비자보다 위**면 위반.
///
/// 절에 안 잡힌 크레이트는 여기서 조용히 빠진다. 그래서 소비자가 "전부 정확히 한 절에
/// 잡혔는가" 를 **먼저** 단언해야 한다 — 안 그러면 열거에서 빠진 크레이트의 간선이
/// 통째로 안 세어지고 게이트가 초록이 된다.
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
        // 판별력 — 위→아래는 정상이다. 이걸 안 고정하면 "전부 위반" 판정기도 통과한다.
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
