//! `lang/*.toml` 의 `cli.help.*` 가 **실재하는 자리를 정확히** 가리키는가.
//!
//! Every compiled help slot must have a catalog entry; English stays source-exact.

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
fn every_help_slot_has_a_translation_key() {
    let all: BTreeSet<String> = slots().into_iter().map(|s| s.key).collect();
    assert!(!all.is_empty());
    let catalog: BTreeSet<String> = english_catalog().into_keys().collect();
    assert_eq!(
        all, catalog,
        "compiled help slots and catalog must match exactly"
    );
}
