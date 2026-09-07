//! `lang/*.toml` 의 `cli.help.*` 가 **실재하는 자리를 정확히** 가리키는가.
//!
//! # 왜 "전부 있어야 한다" 가 아닌가
//!
//! 도움말 자리는 천 개가 넘고, 언어 카탈로그는 세 파일의 키 집합이 **정확히 같아야**
//! 한다(`tests/i18n_key_parity.rs`). 그래서 커버리지는 한 번에 못 채우고 **묶음으로**
//! 자란다. 안 채운 자리는 컴파일된 영어가 그대로 보이므로 부분 커버리지가 깨진 상태가
//! 아니다 — 섞여서 읽힐 뿐이다.
//!
//! 그러니 이 가드가 묻는 것은 둘이다:
//!
//! 1. **있는 키가 옳은가** — `lang/en.toml` 의 `cli.help.*` 키가 전부 실재하는 자리를
//!    가리키고, 그 값이 컴파일된 영어와 **글자 그대로** 같은가. 이것이 이름 변경·오타·
//!    죽은 키를 잡는다. 어긋나면 그 키의 번역이 **엉뚱한 자리에 붙거나 사라진다**.
//! 2. **커버리지가 줄지 않는가** — 래칫이다. 늘면 하한을 같이 올리라고 말하고, 줄면
//!    실패한다. 남는 여유가 곧 안 보는 구간이다.
//!
//! ★ 1 번이 없으면 2 번은 무의미하다. 키를 아무렇게나 늘려도 수는 오르기 때문이다.

use clap::CommandFactory;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // `crates/tasty-cli` 에서 두 층 위.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<크레이트> 아래에 있어야 한다")
        .to_path_buf()
}

/// `lang/en.toml` 에서 `cli.help.` 로 시작하는 키만 평평하게 꺼낸다.
fn english_catalog() -> BTreeMap<String, String> {
    let path = repo_root().join("lang/en.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 읽지 못했다 — {e}", path.display()));
    let value: toml::Value = text
        .parse()
        .unwrap_or_else(|e| panic!("{} 를 파싱하지 못했다 — {e}", path.display()));
    let mut out = BTreeMap::new();
    flatten(&value, String::new(), &mut out);
    out.retain(|k, _| k.starts_with(&format!("{}.", tasty_cli::help_i18n::PREFIX)));
    out
}

fn flatten(value: &toml::Value, prefix: String, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let next = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(v, next, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix, s.clone());
        }
        _ => {}
    }
}

fn slots() -> Vec<tasty_cli::help_i18n::Slot> {
    tasty_cli::help_i18n::slots(&tasty_cli::Cli::command())
}

/// 커버된 도움말 자리의 하한. **줄어도 실패한다** — 상한 래칫과 같은 이유다. 번역을
/// 늘렸으면 이 수를 같이 올려라. 그 한 줄이 리뷰에 보이는 것이 이 래칫의 값이다.
const COVERAGE_FLOOR: usize = 61;

#[test]
fn every_english_help_key_names_a_real_slot() {
    let by_key: BTreeMap<String, String> =
        slots().into_iter().map(|s| (s.key, s.english)).collect();
    let catalog = english_catalog();
    let orphans: Vec<&String> = catalog
        .keys()
        .filter(|k| !by_key.contains_key(*k))
        .collect();
    assert!(
        orphans.is_empty(),
        "`lang/en.toml` 에 실재하지 않는 도움말 키가 있다: {orphans:#?}\n\
         명령이나 인자의 이름이 바뀌면 키가 그 자리를 잃는다. 그때 번역은 **사라지지 않고 \
         조용히 안 쓰인다** — 영어가 그대로 보이므로 en 에서는 아무 증상이 없다."
    );
}

#[test]
fn every_english_help_value_matches_the_compiled_text() {
    let by_key: BTreeMap<String, String> =
        slots().into_iter().map(|s| (s.key, s.english)).collect();
    let mut drift = Vec::new();
    for (key, value) in english_catalog() {
        let Some(compiled) = by_key.get(&key) else {
            continue; // 위 시험이 보고한다
        };
        if compiled != &value {
            drift.push(format!(
                "  {key}\n    소스: {compiled:?}\n    en.toml: {value:?}"
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "`lang/en.toml` 의 영어가 소스의 doc comment 와 다르다:\n{}\n\
         영어 원본은 소스다(`clap_help_text_is_english_only` 가 그것을 강제한다). \
         en.toml 은 **자유도 없는 복제**이고, 그 복제가 번역자에게 키 목록이 된다. \
         갈라지면 번역자는 지금 화면에 없는 문장을 번역한다.",
        drift.join("\n")
    );
}

#[test]
fn help_translation_coverage_does_not_shrink() {
    let all: BTreeSet<String> = slots().into_iter().map(|s| s.key).collect();
    let covered = english_catalog()
        .keys()
        .filter(|k| all.contains(*k))
        .count();
    assert!(
        covered >= COVERAGE_FLOOR,
        "도움말 번역 커버리지가 줄었다: {covered} / {} 자리 < 하한 {COVERAGE_FLOOR}. \
         키를 지웠으면 이 하한도 같이 내려라 — 남는 여유가 곧 안 보는 구간이다.",
        all.len()
    );
    assert!(
        !all.is_empty(),
        "도움말 자리가 0 이다 — 순회가 깨졌다. 이 상태의 커버리지는 아무것도 안 본 값이다"
    );
}
