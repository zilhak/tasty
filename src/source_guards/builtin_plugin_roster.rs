//! 번들 plugin 명부가 적힌 다섯 자리가 같은 집합을 말하는지 못 박는다.
//!
//! 같은 사실("번들 plugin 이 무엇인가")이 다섯 곳에 열거되어 있다:
//!
//! 1. `crates/tasty-host-plugin/src/builtin.rs` 의 `BUILTINS` — `#[cfg(windows)]` 갈래
//! 2. 같은 파일의 `#[cfg(not(windows))]` 갈래
//! 3. `crates/tasty-plugin-*/tasty-plugin.toml` 의 `id` — 디스크 실물
//! 4. `docs/dev-guide/plugin-packaging.md` 의 "번들 plugin 목록" 표
//! 5. `docs/plugins/index.md` 의 카탈로그 표
//!
//! 자리가 여럿인 것 자체는 결함이 아니다 — 셋 이상이 일치하면 어긋난 하나를
//! 판정할 수 있다. 결함은 **자리가 여럿인데 잇는 것이 없는 상태**다. 실제로
//! `plugin-packaging.md` 는 "`BUILTINS` 가 단일 출처다. 아래 표는 그 복제" 라고
//! 스스로 선언하지만, 그 선언을 강제하는 것은 이 파일이 생기기 전까지 없었다.
//!
//! 두 `cfg` 갈래는 특히 위험하다 — 컴파일러가 한 번에 한쪽만 본다. Linux 에서
//! 빌드하는 한 `#[cfg(windows)]` 갈래의 오타는 어떤 빌드도 잡지 못한다.
//!
//! 판정 기준은 개수가 아니라 집합 동등이다. 개수만 맞추는 판정은 "하나를 다른
//! 것으로 바꾼" 변이를 통과시킨다 — 아래 변이 대조가 그것을 같은 테스트에서
//! 단언한다.

use std::collections::BTreeSet;

use super::{repo_root, strip_comments};

const BUILTIN_SRC: &str = "crates/tasty-host-plugin/src/builtin.rs";
const PACKAGING_DOC: &str = "docs/dev-guide/plugin-packaging.md";
const PACKAGING_TABLE_HEAD: &str = "| crate | plugin ID |";
const CATALOG_DOC: &str = "docs/plugins/index.md";
const CATALOG_TABLE_HEAD: &str = "| 플러그인 (id) | 무엇 | 주요 기여 |";
const PLUGIN_CRATE_PREFIX: &str = "tasty-plugin-";
/// 바늘을 쪼갠다 — 이 파일이 판정 대상 문자열을 그대로 담으면 자기 참조가 된다.
const ID_PREFIX: &str = concat!("com.", "tasty.");

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
}

/// `BuiltinSpec { id: "...", crate_dir: "...", .. }` 항목에서 (id, crate_dir) 를 뽑는다.
fn specs_in(block: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let mut id: Option<String> = None;
    for line in block.lines() {
        let t = line.trim();
        if let Some(v) = field(t, "id:") {
            id = Some(v);
        } else if let Some(v) = field(t, "crate_dir:")
            && let Some(i) = id.take()
        {
            // 단락 평가라야 한다 — 튜플로 묶으면 `crate_dir:` 가 아닌 줄에서도
            // `take()` 가 돌아 직전 `id` 를 조용히 버린다.
            out.insert((i, v));
        }
    }
    out
}

fn field(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 두 `cfg` 갈래를 각각 (라벨, 명세 집합) 으로 돌려준다.
///
/// 배열 경계는 `const BUILTINS` 부터 다음 `];` 까지다. 주석을 먼저 지워 주석
/// 안의 예시가 항목으로 읽히지 않게 한다.
fn builtin_arms() -> Vec<(String, BTreeSet<(String, String)>)> {
    let src = strip_comments(&read(BUILTIN_SRC));
    let mut arms = Vec::new();
    let rest = src.as_str();
    let mut cursor = 0usize;
    while let Some(i) = rest[cursor..].find("const BUILTINS") {
        let start = cursor + i;
        let label = src[..start]
            .rsplit('\n')
            .find(|l| l.trim_start().starts_with("#[cfg("))
            .unwrap_or("(cfg 없음)")
            .trim()
            .to_string();
        let end = rest[start..]
            .find("\n];")
            .map(|e| start + e)
            .unwrap_or(rest.len());
        arms.push((label, specs_in(&rest[start..end])));
        cursor = end;
    }
    arms
}

/// 디스크의 매니페스트 — 파서가 아니라 실물이라 독립 오라클이다.
fn manifest_specs() -> BTreeSet<(String, String)> {
    let crates = repo_root().join("crates");
    let mut out = BTreeSet::new();
    let entries = std::fs::read_dir(&crates).expect("crates/ 를 읽지 못했다");
    for e in entries.flatten() {
        let dir = e.file_name().to_string_lossy().to_string();
        if !dir.starts_with(PLUGIN_CRATE_PREFIX) {
            continue;
        }
        let manifest = e.path().join("tasty-plugin.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue; // 매니페스트가 없는 라이브러리 크레이트는 번들 대상이 아니다
        };
        for line in text.lines() {
            if let Some(v) = field(line.trim().replace(' ', "").as_str(), "id=") {
                out.insert((v, dir.clone()));
                break;
            }
        }
    }
    out
}

/// 헤더로 표 하나를 고른다 — 같은 문서의 다른 표를 긁지 않기 위해서다.
fn table_rows(doc: &str, head: &str) -> Vec<Vec<String>> {
    let text = read(doc);
    let mut rows = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if !inside {
            if t == head {
                inside = true;
            }
            continue;
        }
        if !t.starts_with('|') {
            break;
        }
        if t.chars().all(|c| "|-: ".contains(c)) {
            continue; // 구분선
        }
        rows.push(
            t.trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect(),
        );
    }
    assert!(
        !rows.is_empty(),
        "{doc} 에서 헤더 {head:?} 로 시작하는 표를 못 찾았다 — 표가 옮겨졌거나 헤더가 바뀌었다"
    );
    rows
}

fn backticked(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cell;
    while let Some(i) = rest.find('`') {
        let after = &rest[i + 1..];
        match after.find('`') {
            Some(j) => {
                out.push(after[..j].to_string());
                rest = &after[j + 1..];
            }
            None => break,
        }
    }
    out
}

/// `plugin-packaging.md` 표: 두 열이 각각 crate 와 id 다 → (id, crate_dir).
fn packaging_specs() -> BTreeSet<(String, String)> {
    table_rows(PACKAGING_DOC, PACKAGING_TABLE_HEAD)
        .iter()
        .filter_map(|r| {
            let krate = backticked(r.first()?).into_iter().next()?;
            let id = backticked(r.get(1)?).into_iter().next()?;
            Some((id, krate))
        })
        .collect()
}

/// `plugins/index.md` 카탈로그: 첫 열에 링크와 id 가 함께 있어 id 만 뽑는다.
fn catalog_ids() -> BTreeSet<String> {
    table_rows(CATALOG_DOC, CATALOG_TABLE_HEAD)
        .iter()
        .filter_map(|r| {
            backticked(r.first()?)
                .into_iter()
                .find(|t| t.starts_with(ID_PREFIX))
        })
        .collect()
}

fn ids(specs: &BTreeSet<(String, String)>) -> BTreeSet<String> {
    specs.iter().map(|(i, _)| i.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 컴파일러가 한 번에 한쪽만 보는 두 갈래를 여기서 함께 본다.
    #[test]
    fn both_cfg_arms_of_builtins_declare_the_same_plugins() {
        let arms = builtin_arms();
        assert_eq!(
            arms.len(),
            2,
            "{BUILTIN_SRC} 의 BUILTINS 갈래가 2 개가 아니다: {:?}",
            arms.iter().map(|(l, _)| l).collect::<Vec<_>>()
        );
        assert!(
            arms[0].1.len() >= 5,
            "갈래 하나가 비었거나 너무 작다 — 파싱 실패다: {:?}",
            arms[0]
        );
        assert_eq!(
            arms[0].1,
            arms[1].1,
            "{BUILTIN_SRC} 의 두 cfg 갈래가 다른 plugin 을 선언한다.\n  {} 쪽에만: {:?}\n  {} 쪽에만: {:?}\n\
             이 어긋남은 한 플랫폼에서 빌드하는 한 컴파일러가 못 잡는다.",
            arms[0].0,
            arms[0].1.difference(&arms[1].1).collect::<Vec<_>>(),
            arms[1].0,
            arms[1].1.difference(&arms[0].1).collect::<Vec<_>>(),
        );
    }

    /// 디스크 실물과의 대조 — 이쪽은 파서가 아니라 오라클이다.
    #[test]
    fn the_manifests_on_disk_and_the_builtin_table_name_the_same_plugins() {
        let code = builtin_arms().into_iter().next().expect("갈래 없음").1;
        let disk = manifest_specs();
        assert!(
            disk.len() >= 5,
            "매니페스트를 {} 개밖에 못 찾았다 — 측정 실패다",
            disk.len()
        );
        assert_eq!(
            code,
            disk,
            "BUILTINS 와 디스크의 tasty-plugin.toml 이 어긋난다.\n  코드에만: {:?}\n  디스크에만: {:?}",
            code.difference(&disk).collect::<Vec<_>>(),
            disk.difference(&code).collect::<Vec<_>>(),
        );
    }

    /// `plugin-packaging.md` 는 자기 표를 "복제" 라고 선언한다 — 그 선언을 강제한다.
    #[test]
    fn both_docs_that_copy_the_builtin_table_still_match_it() {
        let code = builtin_arms().into_iter().next().expect("갈래 없음").1;
        let packaging = packaging_specs();
        assert_eq!(
            code,
            packaging,
            "{PACKAGING_DOC} 의 번들 목록 표가 BUILTINS 와 어긋난다.\n  코드에만: {:?}\n  문서에만: {:?}",
            code.difference(&packaging).collect::<Vec<_>>(),
            packaging.difference(&code).collect::<Vec<_>>(),
        );
        let catalog = catalog_ids();
        assert_eq!(
            ids(&code),
            catalog,
            "{CATALOG_DOC} 의 카탈로그가 BUILTINS 와 어긋난다.\n  코드에만: {:?}\n  문서에만: {:?}",
            ids(&code).difference(&catalog).collect::<Vec<_>>(),
            catalog.difference(&ids(&code)).collect::<Vec<_>>(),
        );
    }

    /// 변이 대조 — 개수를 보존하는 치환은 "건수 고정" 강도를 통과하고
    /// "집합 동등" 만 잡는다. 두 강도의 차이를 같은 테스트에서 단언한다.
    #[test]
    fn swapping_one_entry_is_caught_although_the_count_is_unchanged() {
        let code = builtin_arms().into_iter().next().expect("갈래 없음").1;
        let mut mutated = code.clone();
        let victim = code.iter().next().expect("빈 집합").clone();
        mutated.remove(&victim);
        mutated.insert((format!("{ID_PREFIX}not-a-real-plugin"), victim.1.clone()));

        assert_eq!(
            mutated.len(),
            code.len(),
            "변이가 개수를 바꿨다 — 이 대조는 강도 차이를 못 보인다"
        );
        assert_ne!(
            mutated,
            manifest_specs(),
            "치환 변이가 집합 동등 판정을 통과했다 — 이 가드는 어긋남을 못 잡는다"
        );
    }

    /// 내가 실제로 빠진 함정 — 한 문서 안의 다른 표를 긁으면 자리가 뒤바뀐다.
    /// `plugin-packaging.md` 에는 표가 넷 있고, 그중 하나만 번들 목록이다.
    #[test]
    fn the_table_parser_reads_only_the_table_it_was_pointed_at() {
        let text = read(PACKAGING_DOC);
        let heads: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('|') && !l.chars().all(|c| "|-: ".contains(c)))
            .collect();
        let other_heads = heads
            .iter()
            .filter(|l| **l != PACKAGING_TABLE_HEAD && l.matches('|').count() >= 3)
            .count();
        assert!(
            other_heads > 0,
            "{PACKAGING_DOC} 에 다른 표가 없다 — 이 대조가 무의미해졌다"
        );
        let rows = table_rows(PACKAGING_DOC, PACKAGING_TABLE_HEAD);
        assert!(
            rows.iter().all(|r| r.len() == 2),
            "번들 목록 표는 2 열인데 다른 열 수의 행이 섞였다 — 표 경계를 넘어 읽었다: {rows:?}"
        );
        assert!(
            rows.iter().all(|r| backticked(&r[1])
                .first()
                .is_some_and(|t| t.starts_with(ID_PREFIX))),
            "표 밖의 행을 읽었다: {rows:?}"
        );
    }
}
