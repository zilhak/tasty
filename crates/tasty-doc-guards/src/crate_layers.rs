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
//!
//! 여기에 **좌변을 만드는 함수도 함께 산다**([`crate_dir_names`]). 아래 함수들이 전부
//! `known: &[String]` 을 요구하는데 그것을 만드는 방법이 소비자마다 다르면 같은 물음
//! ("워크스페이스 크레이트가 무엇인가")에 답이 여럿 생긴다. 실측으로 밟은 형태다 —
//! `README` 배지가 그 답을 손으로 적어 두고 낡았다.

use std::path::Path;

/// `crates/` **바로 아래**에서 `Cargo.toml` 을 가진 디렉토리 이름을 정렬해 돌려준다.
/// 이것이 이 레포에서 "워크스페이스 크레이트 수" 의 **정의**다.
///
/// workspace `exclude` 여부와 무관하게 **디렉토리 기준**이다 — `crates/tasty-plugin-sdk-wasm`
/// 은 루트 `Cargo.toml` 의 `exclude` 라 `cargo metadata` 에 안 나오지만 여기서는 센다.
/// 반대로 레포 루트의 본 바이너리 크레이트(`tasty`)는 `crates/` 아래가 아니라 **안 센다**.
/// 이 좌변을 고른 이유는 이것이 **문서가 가리키는 자리에서 그대로 셀 수 있는 수**이기
/// 때문이다 — `README` 배지는 `crates/` 로 링크하고, `docs/architecture/index.md` 는
/// 그 디렉토리를 전수 열거한다. `cargo metadata` 의 member 수는 그 자리에서 셀 수 없다.
///
/// 빈 결과는 돌려주지 않고 panic 한다. 순회가 죽어 아무것도 못 찾은 상태를 소비자가
/// "위반 0" 으로 읽으면 그 가드는 지키려던 것이 깨진 순간에 초록이 된다
/// ([`crate::floored_walk`] 와 같은 이유. 하한을 소비자마다 다시 쓰면 언젠가 빠뜨린다).
///
/// ★ **그러면서 그 공용 순회를 안 쓴다 — 왜인지를 적어 둔다.** 그 모듈의 doc 이 존재
/// 이유로 든 것이 정확히 "각자 하한을 박는 것이고 형태가 제각각" 이라, 여기 `assert!`
/// 한 줄은 안 적으면 그 문장이 가리키는 미착륙 자리로 읽힌다.
///
/// 못 쓰는 것은 아니다. [`crate::floored_walk::walk_dirs_with_floor`] 에 "`Cargo.toml`
/// 이 있으면 [`Pick::TakeAndStop`], 아니면 [`Pick::Skip`]" 을 주면 **오늘 이 트리에서는
/// 같은 목록이 나온다**(실측 2026-09-09: `crates/` 한 겹 디렉토리는 전부 `Cargo.toml`
/// 을 갖고 있고, 그보다 깊은 곳의 `Cargo.toml` 은 0 개다 — 그래서 내려갈 일이 없다).
///
/// 안 쓰는 이유는 **하한의 모양**이다. [`crate::floored_walk::Floor`] 는 `min` 만이
/// 아니라 `measured`(마지막으로 실제로 센 값)를 함께 요구하는데, 여기서 그 값은
/// **크레이트 수**다 — 이 함수가 존재하는 이유가 바로 그 수를 손으로 적어 둔 자리를
/// 없애는 것이다(`README` 배지가 그렇게 낡았다). 하한을 공용으로 올리면서 그 수를 코드에
/// 다시 심으면 지키려던 것이 그 자리에서 깨진다. 여기서 필요한 하한은 재야 할 값이 아니라
/// **불변식**(`crates/` 는 빌 수 없다)이고, 그것은 `assert!` 한 줄이 정확히 표현한다.
/// 그래서 이 자리는 "형태가 제각각" 의 예가 아니라 **다른 물음**이다.
///
/// ☆ 그리고 이 순회가 여기 온 것은 공용 순회 래칫의 처방을 따른 결과가 아니다. 그
/// 래칫의 좌변은 통합 테스트 타깃 한 겹(`tests/*.rs` · `crates/*/tests/*.rs`)이라
/// `src/` 를 안 본다 — 순회가 테스트에서 lib 으로 오면서 좌변 **밖으로** 나갔고, 래칫이
/// 센 −1 은 그 이동의 값이지 공용 순회 채택의 값이 아니다.
///
/// [`Pick::TakeAndStop`]: crate::floored_walk::Pick::TakeAndStop
/// [`Pick::Skip`]: crate::floored_walk::Pick::Skip
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
