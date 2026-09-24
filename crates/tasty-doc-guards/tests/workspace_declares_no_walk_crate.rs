//! 워크스페이스에 파일 순회 크레이트 의존성이 추가되면 기존 순회 검사 범위를 검토하게 한다.
//! 현재 순회 검사는 std::fs::read_dir 중심이라 다른 라이브러리의 호출은 놓칠 수 있다.
//! 루트와 crates의 매니페스트를 읽고 workspace exclude는 검사에서 제외하되 진단에 표시한다.
//! 실제 호출 여부를 보는 file_walks_declare_their_mechanism과는 검사 대상이 다르다.
//!
//! 라이브러리 이름 목록은 해당 검사 소스에서 읽어 두 목록이 따로 바뀌지 않게 한다.
//! 이 파일의 문자열에 실제 이름을 쓰면 그 검사가 순회 호출로 오인하므로 합성 입력에는 다른 이름을 쓴다.
//! 캐시 제외 규칙은 docs/dev-guide/guard-population.md에 있다.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tasty_doc_guards::cargo_manifest;
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};
use tasty_doc_guards::repo_root;
use tasty_doc_guards::source_text::repo_relative;

/// 공용 순회로 crates의 매니페스트를 수집하고 빈 결과가 통과하지 않게 하한을 적용한다.
const CRATE_MANIFEST_FLOOR: Floor = Floor {
    min: 40,
    measured: 52,
    measured_on: "2026-09-08",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::NEVER_COUNTED,
    why_this_gap: "crates 바로 아래 Cargo.toml 수의 하한이다. 크레이트 정리를 허용할 여유를 두되 큰 수집 누락이 있으면 확인한다.",
};

/// 매니페스트 수가 맞아도 의존 항목을 읽지 못할 수 있어 별도 하한을 둔다.
const MIN_DEP_ENTRIES: usize = 200;

const MECHANISM_SOURCE: &str =
    "crates/tasty-doc-guards/tests/file_walks_declare_their_mechanism.rs";

/// workspace의 exclude를 읽어 대상에서 제외한다.
fn excluded_dirs(root_manifest: &str) -> Vec<String> {
    let out: Vec<String> = root_manifest
        .lines()
        .find(|l| l.trim_start().starts_with("exclude"))
        .map(|l| l.split('"').skip(1).step_by(2).map(str::to_owned).collect())
        .unwrap_or_default();
    assert!(
        !out.is_empty(),
        "workspace exclude를 읽지 못했다. 제외한 크레이트가 검사 대상에 섞이지 않도록 파서를 확인한다."
    );
    out
}

/// 표준 라이브러리를 제외한 순회 크레이트 이름을 읽는다.
fn walk_crate_names() -> BTreeSet<String> {
    let path = repo_root().join(MECHANISM_SOURCE);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|why| panic!("순회 수단 목록을 읽지 못했다: {} — {why}", path.display()));
    walk_crate_names_in(&src, MECHANISM_SOURCE)
}

/// 실패 메시지가 실제 입력을 가리키도록 원문과 출처를 함께 받는다.
fn walk_crate_names_in(src: &str, whence: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for marker in ["const KNOWN_MECHANISMS", "const UNDECLARED_MECHANISMS"] {
        let Some(at) = src.find(marker) else { continue };
        let rest = &src[at..];
        let end = rest.find("];").map_or(rest.len(), |i| i + 2);
        for raw in rest[..end].split('"').skip(1).step_by(2) {
            // 설명 문자열(한글 산문)이 아니라 심볼 형태만 취한다.
            if raw.is_empty() || !raw.is_ascii() {
                continue;
            }
            let krate = raw.split("::").next().unwrap_or(raw);
            if krate == "std" || krate.contains(' ') {
                continue;
            }
            names.insert(krate.to_owned());
        }
    }
    // 실제 순회 크레이트 이름을 리터럴로 쓰지 않고 추출 개수와 이름 형태를 확인한다.
    let shaped = names.iter().all(|n| {
        n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    });
    assert!(
        names.len() >= 4 && shaped,
        "순회 크레이트 이름 목록을 읽지 못했다: {names:?}. {whence}의 상수 이름과 형식을 확인한다."
    );
    names
}

/// 공용 cargo_manifest 파서가 읽은 의존 이름을 사용한다.
fn declared_deps(manifest: &str) -> Vec<String> {
    cargo_manifest::declared_deps(manifest)
        .into_iter()
        .map(|(_, name)| name)
        .collect()
}

/// 크레이트 **루트**의 매니페스트만 — `crates/<이름>/Cargo.toml` 은 슬래시가 정확히 둘이다.
fn is_crate_root_manifest(w: &Walked) -> bool {
    w.rel.starts_with("crates/")
        && w.rel.ends_with("/Cargo.toml")
        && w.rel.matches('/').count() == 2
}

fn member_manifests() -> Vec<PathBuf> {
    let root = repo_root();
    let root_manifest = fs::read_to_string(root.join("Cargo.toml")).expect("루트 Cargo.toml");
    let excluded = excluded_dirs(&root_manifest);

    let found = walk_with_floor(
        &root.join("crates"),
        &root,
        &CRATE_MANIFEST_FLOOR,
        Descend::SkipBuildCachesAndDotDirs,
        &is_crate_root_manifest,
    )
    .unwrap_or_else(|why| panic!("crates/ 순회가 실패했다: {why}"));

    members_of(root.join("Cargo.toml"), found, &excluded)
}

/// 순회 결과에서 workspace exclude를 제외하고 루트 매니페스트를 포함한다.
fn members_of(root_manifest: PathBuf, found: Vec<Walked>, excluded: &[String]) -> Vec<PathBuf> {
    let mut out = vec![root_manifest];
    for w in found {
        let dir = w.rel.trim_end_matches("/Cargo.toml");
        if excluded.iter().any(|e| e == dir) {
            continue;
        }
        out.push(w.path);
    }
    out.sort();
    out
}

/// 위반 없음과 수집 실패를 구별할 수 있도록 전체 의존 수와 발견 항목을 함께 반환한다.
struct Scanned {
    entries: usize,
    hits: Vec<String>,
}

/// 보고용 경로는 호출자가 repo_relative로 변환해 전달한다.
fn scan(manifests: &[(String, String)], walk_crates: &BTreeSet<String>) -> Scanned {
    let mut out = Scanned {
        entries: 0,
        hits: Vec::new(),
    };
    for (rel, text) in manifests {
        for name in declared_deps(text) {
            out.entries += 1;
            if walk_crates.contains(&name) {
                out.hits.push(format!("  {rel} — `{name}`"));
            }
        }
    }
    out
}

#[test]
fn the_workspace_still_depends_on_no_walk_crate() {
    let root = repo_root();
    let walk_crates = walk_crate_names();
    let manifests = member_manifests();

    // Windows에서도 같은 형식으로 보고하도록 공용 상대 경로 변환을 사용한다.
    let named: Vec<(String, String)> = manifests
        .iter()
        .map(|m| {
            let rel = repo_relative(m.strip_prefix(&root).unwrap_or(m));
            let text = fs::read_to_string(m).unwrap_or_else(|why| panic!("{}: {why}", m.display()));
            (rel.display().to_string(), text)
        })
        .collect();
    let Scanned { entries, hits } = scan(&named, &walk_crates);
    assert!(
        entries >= MIN_DEP_ENTRIES,
        "의존 항목을 {entries}개만 읽었다(하한 {MIN_DEP_ENTRIES}). 매니페스트 수와 별개로 의존 절 파싱을 확인한다."
    );

    println!(
        "[순회 크레이트 검사] 멤버 매니페스트 {} 개 · 의존 항목 {entries} 개 · 순회 크레이트 이름 {:?}",
        manifests.len(),
        walk_crates
    );

    assert!(
        hits.is_empty(),
        "워크스페이스가 순회 크레이트를 직접 의존한다:\n{}\n새 라이브러리의 필터·캐시 제외가 기존 순회 검사와 같은 범위를 보는지 검토한다(docs/dev-guide/guard-population.md). 필요하면 검사 범위와 {MECHANISM_SOURCE}의 등록 목록을 함께 갱신한다. 이름을 목록에서 제거해 이 검사와 호출 검사 모두를 우회하지 않는다.",
        hits.join("\n")
    );
}

/// workspace exclude의 의존은 실패시키지 않고 검사 밖의 상태로 보고한다.
#[test]
fn the_excluded_tree_is_reported_but_not_judged() {
    let root = repo_root();
    let walk_crates = walk_crate_names();
    let root_manifest = fs::read_to_string(root.join("Cargo.toml")).expect("루트 Cargo.toml");

    let mut looked = 0usize;
    let mut outside: Vec<String> = Vec::new();
    for rel in excluded_dirs(&root_manifest) {
        let m = root.join(&rel).join("Cargo.toml");
        let Ok(text) = fs::read_to_string(&m) else {
            continue;
        };
        looked += 1;
        for name in declared_deps(&text) {
            if walk_crates.contains(&name) {
                outside.push(format!("{rel}/Cargo.toml — `{name}`"));
            }
        }
    }
    assert!(
        looked > 0,
        "workspace exclude의 매니페스트를 하나도 읽지 못했다. 제외 목록과 경로를 확인한다."
    );
    println!(
        "[순회 크레이트 제외 범위] exclude 매니페스트 {looked} 개 · 순회 크레이트 {outside:?}"
    );
}

/// 실제 순회 라이브러리와 무관한 이름으로 TOML 의존 선언의 형식을 검사한다.
#[test]
fn dependency_sections_are_read_in_every_toml_shape() {
    let manifest = "\
[package]
name = \"x\"

[dependencies]
alpha = \"1\"
bravo = { version = \"2.5.0\" }

[dependencies.charlie]
version = \"0.8\"

[target.'cfg(unix)'.dev-dependencies]
delta = \"0.3\"

[target.'cfg(windows)'.build-dependencies.echo]
version = \"0.7\"

[lints]
workspace = true
";
    let mut got = declared_deps(manifest);
    got.sort();
    assert_eq!(
        got,
        vec!["alpha", "bravo", "charlie", "delta", "echo"],
        "의존 선언의 형식별 추출 결과가 다르다. 일반/인라인/하위 표/target 조건의 의존은 읽고 lints 항목은 제외해야 한다."
    );
}

/// 파일을 만들지 않고 경로 모양과 exclude 적용을 검사한다.
fn walked(rels: &[&str]) -> Vec<Walked> {
    rels.iter()
        .map(|r| Walked {
            path: PathBuf::from(*r),
            rel: (*r).to_string(),
        })
        .collect()
}

/// 합성 이름 4개 이상을 사용해 크레이트 추출과 파싱 하한을 함께 확인한다.
#[test]
fn the_name_parser_keeps_symbols_and_drops_prose_and_the_standard_library() {
    // 공백 있는 산문과 비ASCII 이름은 서로 다른 필터로 제외되므로 둘 다 넣는다.
    let src = "\
const KNOWN_MECHANISMS: &[&str] = &[
    \"zonecrawl::ZoneCrawl\",
    \"std::fs::read_dir\",
    \"이 목록은 수단 이름을 담는다\",
    \"순회수단\",
    \"\",
];
const UNDECLARED_MECHANISMS: &[&str] = &[
    \"zonesweep\",
    \"zone_probe::Builder\",
    \"zonetrail\",
];
";
    let names = walk_crate_names_in(src, "zone/mechanisms.rs");
    let got: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        got,
        vec!["zone_probe", "zonecrawl", "zonesweep", "zonetrail"],
        "경로는 첫 마디만 · 표준 라이브러리는 버리고 · 산문과 빈 칸도 버려야 한다"
    );
}

#[test]
fn the_parser_survives_one_missing_constant_and_the_anchor_fires_on_an_empty_list() {
    let only_second = "\
const UNDECLARED_MECHANISMS: &[&str] = &[
    \"zonecrawl\", \"zonesweep\", \"zone_probe\", \"zonetrail\",
];
";
    assert_eq!(
        walk_crate_names_in(only_second, "zone/mechanisms.rs").len(),
        4,
        "상수 하나가 없어도 나머지 상수는 읽어야 한다"
    );

    let renamed = "const ZONE_RENAMED: &[&str] = &[\"zonecrawl\", \"zonesweep\"];\n";
    let why = std::panic::catch_unwind(|| walk_crate_names_in(renamed, "zone/mechanisms.rs"))
        .expect_err("목록 상수 이름을 찾지 못하면 실패해야 한다");
    let msg = why
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        msg.contains("zone/mechanisms.rs"),
        "실패 메시지가 인자로 받은 출처를 가리키지 않는다: {msg}"
    );
}

#[test]
fn the_member_filter_takes_crate_roots_only_and_honours_the_exclude_list() {
    let found = walked(&[
        "crates/zone-alpha/Cargo.toml",
        "crates/zone-bravo/Cargo.toml",
        "crates/zone-alpha/vendor/nested/Cargo.toml",
    ]);
    let kept: Vec<&str> = found
        .iter()
        .filter(|w| is_crate_root_manifest(w))
        .map(|w| w.rel.as_str())
        .collect();
    assert_eq!(
        kept,
        vec![
            "crates/zone-alpha/Cargo.toml",
            "crates/zone-bravo/Cargo.toml"
        ],
        "크레이트 루트만 — 슬래시가 정확히 둘인 것만 센다"
    );

    let excluded = vec!["crates/zone-bravo".to_string()];
    let members = members_of(PathBuf::from("Cargo.toml"), walked(&kept), &excluded);
    // 경로 문자열의 구분자를 직접 변환하지 않고 PathBuf로 비교한다.
    assert_eq!(
        members,
        vec![
            PathBuf::from("Cargo.toml"),
            PathBuf::from("crates/zone-alpha/Cargo.toml")
        ],
        "제외 목록에 있는 크레이트는 멤버가 아니다. 루트 매니페스트는 언제나 들어간다"
    );
}

#[test]
fn the_exclude_reader_takes_what_is_between_the_quotes() {
    let root = "\
[workspace]
members = [\"crates/*\"]
exclude = [\"zone-outside\", \"crates/zone-charlie\"]
resolver = \"2\"
";
    assert_eq!(
        excluded_dirs(root),
        vec!["zone-outside", "crates/zone-charlie"],
        "따옴표 사이만, 그리고 구분자는 빼야 한다"
    );
}

#[test]
fn the_scan_names_the_offending_manifest_and_still_counts_every_entry() {
    let mut mechanisms = BTreeSet::new();
    mechanisms.insert("zonecrawl".to_string());
    mechanisms.insert("zonesweep".to_string());

    let manifests = vec![
        (
            "crates/zone-alpha/Cargo.toml".to_string(),
            "[dependencies]\nserde = \"1\"\nzonecrawl = \"2\"\n".to_string(),
        ),
        (
            "crates/zone-bravo/Cargo.toml".to_string(),
            "[dependencies]\nserde = \"1\"\n".to_string(),
        ),
        (
            "Cargo.toml".to_string(),
            "[dependencies.zonesweep]\nversion = \"0.3\"\n".to_string(),
        ),
    ];
    let got = scan(&manifests, &mechanisms);
    assert_eq!(
        got.entries, 4,
        "발견한 순회 크레이트뿐 아니라 모든 의존 항목을 세어야 한다"
    );
    assert_eq!(
        got.hits,
        vec![
            "  crates/zone-alpha/Cargo.toml — `zonecrawl`".to_string(),
            "  Cargo.toml — `zonesweep`".to_string(),
        ],
        "걸린 자리는 매니페스트 이름과 크레이트 이름을 함께 대야 한다. 안 걸린 것은 안 섞인다"
    );

    let clean = vec![(
        "crates/zone-bravo/Cargo.toml".to_string(),
        "[dependencies]\nserde = \"1\"\n".to_string(),
    )];
    assert!(scan(&clean, &mechanisms).hits.is_empty(), "없는데 걸었다");
}
