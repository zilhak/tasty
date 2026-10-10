//! site/vendor 의 파일마다 MANIFEST 에 기록한 해시와 내용이 같은지 확인한다.
//! 기본 줄은 로컬 해시가 원격 해시와 같아야 하고, `variant` 줄은 기록한 로컬 해시와 같고 이유가 있어야 한다.
//! 파일 집합도 비교한다. README.md 와 MANIFEST 자신은 사본이 아니므로 제외한다.
//!
//! 원격은 조회하지 않는다. 통과는 사본이 마지막 재-vendor 때 기록한 내용 그대로라는 뜻이며,
//! 원격이 그 뒤에 바뀌었는지는 알 수 없다. 원격과의 대조는 site/vendor/README.md 의 갱신 절차를 따른다.
//! MANIFEST 는 site/scripts/vendor-manifest.mjs 가 쓴다.

use std::collections::BTreeMap;

use tasty_doc_guards::floored_walk::{CountedOn, Descend, Floor, Walked, walk_with_floor};
use tasty_doc_guards::sha256::sha256_hex;

const VENDOR_DIR: &str = "site/vendor";
const MANIFEST: &str = "site/vendor/MANIFEST";
/// 사본을 설명하는 파일. 원격 원본이 없다.
const OWN_FILES: [&str; 2] = ["site/vendor/README.md", "site/vendor/MANIFEST"];

/// 정확한 파일 집합은 MANIFEST 와 비교하므로, 이 하한은 수집 실패를 찾는 데만 쓴다.
const VENDOR_FLOOR: Floor = Floor {
    min: 200,
    measured: 236,
    measured_on: "2026-10-10",
    counted_on: CountedOn::Tree(
        "8380e467c — site/vendor 의 파일 237개에서 README.md 를 뺀 236개다. MANIFEST 를 더하기 전에 셌다.",
    ),
    why_this_gap: "파일 집합은 MANIFEST 와 정확히 비교한다. 하한은 빈 수집끼리 통과하지 않게 할 뿐이며, 원격에서 갤러리 페이지 몇 개가 지워지는 정도의 정상 감소에는 여유를 둔다.",
};

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Plain {
        remote: String,
    },
    Variant {
        remote: String,
        local: String,
        reason: String,
    },
}

fn is_sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 형식 오류는 모아서 돌려준다. 경로는 site/vendor 기준이다.
fn parse(text: &str) -> Result<BTreeMap<String, Entry>, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut errors = Vec::new();
    let mut prev: Option<String> = None;
    for (i, line) in text.lines().enumerate() {
        let at = i + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let entry = match f.as_slice() {
            [_, remote] if is_sha(remote) => Entry::Plain {
                remote: remote.to_string(),
            },
            [_, remote, "variant", local, reason] if is_sha(remote) && is_sha(local) => {
                if reason.trim().is_empty() {
                    errors.push(format!("{at}행: variant 줄에 이유가 없다"));
                    continue;
                }
                if remote == local {
                    errors.push(format!(
                        "{at}행: variant 인데 원격과 로컬 해시가 같다. 기본 줄로 바꾼다"
                    ));
                    continue;
                }
                Entry::Variant {
                    remote: remote.to_string(),
                    local: local.to_string(),
                    reason: reason.to_string(),
                }
            }
            _ => {
                errors.push(format!(
                    "{at}행: `<경로> TAB <sha256>` 또는 `<경로> TAB <sha256> TAB variant TAB <sha256> TAB <이유>` 형식이 아니다: {line:?}"
                ));
                continue;
            }
        };
        let path = f[0].to_string();
        if let Some(p) = &prev
            && *p >= path
        {
            errors.push(format!(
                "{at}행: 경로가 정렬되지 않았거나 중복이다: {p} 다음 {path}"
            ));
        }
        prev = Some(path.clone());
        out.insert(path, entry);
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// 기록과 실제 해시를 비교해 어긋난 항목을 설명한다.
fn judge(recorded: &BTreeMap<String, Entry>, actual: &BTreeMap<String, String>) -> Vec<String> {
    let mut bad = Vec::new();
    for path in actual.keys().filter(|p| !recorded.contains_key(*p)) {
        bad.push(format!(
            "{path}: MANIFEST 에 없다. 원격에서 받은 파일이면 vendor-manifest.mjs 로 다시 쓴다"
        ));
    }
    for (path, entry) in recorded {
        let Some(got) = actual.get(path) else {
            bad.push(format!("{path}: MANIFEST 에 있지만 사본에 없다"));
            continue;
        };
        match entry {
            Entry::Plain { remote } if got != remote => bad.push(format!(
                "{path}: 원격 해시 {remote} 와 다르다(로컬 {got}). 옛 판이 남았거나 직접 고친 것이다"
            )),
            Entry::Variant { local, reason, .. } if got != local => bad.push(format!(
                "{path}: 기록한 변형 해시 {local} 와 다르다(로컬 {got}). 변형 이유: {reason}"
            )),
            _ => {}
        }
    }
    bad
}

fn actual_hashes() -> BTreeMap<String, String> {
    let root = tasty_doc_guards::repo_root();
    let walked = walk_with_floor(
        &root.join(VENDOR_DIR),
        &root,
        &VENDOR_FLOOR,
        Descend::Everything,
        &|w: &Walked| !OWN_FILES.contains(&w.rel.as_str()),
    )
    .unwrap_or_else(|why| panic!("{VENDOR_DIR} 순회: {why}"));
    walked
        .into_iter()
        .map(|w| {
            let bytes = std::fs::read(&w.path)
                .unwrap_or_else(|e| panic!("{} 를 못 읽었다: {e}", w.path.display()));
            let rel = w.rel[VENDOR_DIR.len() + 1..].to_string();
            (rel, sha256_hex(&bytes))
        })
        .collect()
}

fn recorded() -> BTreeMap<String, Entry> {
    let path = tasty_doc_guards::repo_root().join(MANIFEST);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 못 읽었다: {e}", path.display()));
    parse(&text).unwrap_or_else(|errors| panic!("{MANIFEST} 형식 오류:\n{}", errors.join("\n")))
}

#[test]
fn every_vendored_file_matches_its_recorded_hash() {
    let recorded = recorded();
    let actual = actual_hashes();
    let bad = judge(&recorded, &actual);
    assert!(
        bad.is_empty(),
        "site/vendor 가 MANIFEST 와 {} 곳 어긋난다:\n{}\n\
         재-vendor 했다면 site/scripts/vendor-manifest.mjs 로 MANIFEST 를 다시 쓴다(site/vendor/README.md 의 갱신 절차).",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn the_manifest_parser_reads_both_line_kinds() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let text = format!("# 머리\n\nx.css\t{a}\ny.svg\t{a}\tvariant\t{b}\t메타데이터를 지웠다\n");
    let got = parse(&text).expect("정상 입력");
    assert_eq!(got.len(), 2);
    assert_eq!(got["x.css"], Entry::Plain { remote: a.clone() });
    assert_eq!(
        got["y.svg"],
        Entry::Variant {
            remote: a.clone(),
            local: b.clone(),
            reason: "메타데이터를 지웠다".to_string()
        }
    );
}

#[test]
fn the_manifest_parser_rejects_malformed_lines() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    for bad in [
        format!("x.css\t{}", "A".repeat(64)),
        format!("x.css\t{a}\tvariant\t{b}\t "),
        format!("x.css\t{a}\tvariant\t{a}\t이유"),
        format!("x.css\t{a}\tvariant\t{b}"),
        format!("y.css\t{a}\nx.css\t{a}"),
        format!("x.css\t{a}\nx.css\t{a}"),
    ] {
        assert!(parse(&bad).is_err(), "거부해야 한다: {bad:?}");
    }
}

#[test]
fn the_judge_reports_each_kind_of_drift() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let c = "c".repeat(64);
    let recorded: BTreeMap<String, Entry> = [
        ("plain.css".to_string(), Entry::Plain { remote: a.clone() }),
        (
            "variant.svg".to_string(),
            Entry::Variant {
                remote: a.clone(),
                local: b.clone(),
                reason: "이유".to_string(),
            },
        ),
        ("gone.jsx".to_string(), Entry::Plain { remote: a.clone() }),
    ]
    .into_iter()
    .collect();
    let same: BTreeMap<String, String> = [
        ("plain.css".to_string(), a.clone()),
        ("variant.svg".to_string(), b.clone()),
        ("gone.jsx".to_string(), a.clone()),
    ]
    .into_iter()
    .collect();
    assert!(judge(&recorded, &same).is_empty());

    let drifted: BTreeMap<String, String> = [
        ("plain.css".to_string(), c.clone()),
        ("variant.svg".to_string(), a.clone()),
        ("extra.html".to_string(), a.clone()),
    ]
    .into_iter()
    .collect();
    let bad = judge(&recorded, &drifted);
    assert_eq!(bad.len(), 4, "{bad:#?}");
    for needle in ["extra.html", "gone.jsx", "plain.css", "variant.svg"] {
        assert!(
            bad.iter().any(|l| l.starts_with(needle)),
            "{needle} 를 놓쳤다: {bad:#?}"
        );
    }
}
