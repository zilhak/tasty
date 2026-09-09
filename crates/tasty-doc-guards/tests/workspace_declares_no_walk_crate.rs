//! ADR-0146 의 재검토 조건 하나에 **발화 자리**를 준다 —
//! "워크스페이스에 `walkdir` 류 순회 크레이트가 들어온다".
//!
//! 그 ADR 은 "순회 수단이 `std::fs::read_dir` 뿐" 이라는 실측 위에 서 있다. 그 전제가
//! 깨지는 날을 사람이 알아채야 발동하는 상태였다.
//!
//! # 좌변
//!
//! 워크스페이스 멤버의 **매니페스트**다 — 루트 `Cargo.toml` 과 `crates/*/Cargo.toml`
//! 중 `[workspace] exclude` 를 뺀 것. 조건의 주어가 "크레이트가 워크스페이스에
//! 들어온다" 이므로 좌변도 그것을 선언하는 자리여야 한다. 순회 호출 **건수**처럼
//! 주어와 함께 움직이는 모수를 좌변으로 삼지 않는다.
//!
//! # 이미 있는 판사와 무엇이 다른가
//!
//! [`file_walks_declare_their_mechanism`] 은 **통합 테스트 타깃의 소스**에서 그 수단을
//! 실제로 쓰는지 본다. 이쪽은 **의존 선언**을 본다. 의존이 먼저 들어오고 사용은
//! 나중이라 저쪽은 이쪽보다 늦게 울고, 프로덕션(`src/`)에서만 쓰이면 저쪽 모수 밖이라
//! 아예 안 운다. 같은 물음에 판사를 둘 만든 것이 아니다.
//!
//! [`file_walks_declare_their_mechanism`]: ../file_walks_declare_their_mechanism.rs
//!
//! # 이름 목록을 복제하지 않는다
//!
//! 수단 이름은 그 파일의 두 상수에서 **읽는다**. 손으로 두 번 쓰면 한쪽만 늘어난 날
//! 갈리고, 갈린 쪽이 조용한 쪽이다.
//!
//! # 이 파일은 수단 이름을 코드에 리터럴로 쓰지 않는다
//!
//! `file_walks_declare_their_mechanism` 의 좌변은 "테스트 타깃의 **코드**에 그 낱말이
//! 있는가" 이고, 자기 파일만 이름으로 면제한다. 그래서 여기에 이름을 리터럴로 적으면
//! 그쪽이 빨개진다 — 실측 2026-09-08 에 실제로 그렇게 됐다. 이름은 소스에서 읽고,
//! 사람이 읽을 이름은 주석에만 둔다(그쪽 판정은 줄 주석을 지우고 센다).
//! 여기서 말하는 수단은 `walk`+`dir` · `jwalk` · `glob` · `ignore` 계열이다.
//!
//! # `[workspace] exclude` 밖은 판정하지 않고 진단으로 적는다
//!
//! `site/` 는 그 첫째 크레이트를 직접 의존한다. ADR 의 문구가 "워크스페이스에" 라 그것은
//! 발화가 **아니다** — 그러나 경계가 어디에 있는지는 값으로 남긴다. 경계를 옮기는 것은
//! 이 시험이 할 일이 아니라 그 ADR 을 다시 쓰는 일이다.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tasty_doc_guards::cargo_manifest;
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};
use tasty_doc_guards::repo_root;
use tasty_doc_guards::source_text::repo_relative;

/// `crates/` 아래 매니페스트 수집의 하한. 수집이 죽으면 위반 0 이 언제나 참이 된다.
/// 공용 순회를 쓴다 — 직접 `read_dir` 는 `scripts/check-shared-walk-ratchet.sh` 의
/// 상한에 앉고, 그 래칫은 **여유를 0 으로 유지한다**(건수가 줄면 상한도 같이 내린다).
/// 값을 여기 안 적는 것은 그 수가 커밋마다 움직여 사본이 낡기 때문이다(ADR-0139) —
/// 지금 값은 그 스크립트를 돌리면 마지막 줄에 나온다.
const CRATE_MANIFEST_FLOOR: Floor = Floor {
    min: 40,
    measured: 52,
    measured_on: "2026-09-08",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::NEVER_COUNTED,
    why_this_gap: "이 모수는 `crates/*/Cargo.toml` 의 수다. 크레이트는 늘어 왔고 한 번에 \
                   열 개가 사라지는 변경은 없었으므로 여유를 좁게 둔다. 넓게 두면 순회가 \
                   절반 죽어도 통과하는데, 이 가드가 겨냥하는 사고가 바로 그것이다.",
};

/// 훑은 의존 항목 수의 하한. 절 파서가 죽어도 매니페스트 수는 그대로라
/// 모수를 하나 더 본다. 실측 2026-09-08: 아래 census 줄이 찍는다.
const MIN_DEP_ENTRIES: usize = 200;

/// 이 목록의 출처. 사본을 만들지 않으려고 소스에서 읽는다.
const MECHANISM_SOURCE: &str =
    "crates/tasty-doc-guards/tests/file_walks_declare_their_mechanism.rs";

/// `[workspace] exclude` 를 **읽어서** 뺀다. 박아 두면 `Cargo.toml` 이 바뀌는 날
/// 모수가 조용히 어긋난다.
fn excluded_dirs(root_manifest: &str) -> Vec<String> {
    let out: Vec<String> = root_manifest
        .lines()
        .find(|l| l.trim_start().starts_with("exclude"))
        .map(|l| l.split('"').skip(1).step_by(2).map(str::to_owned).collect())
        .unwrap_or_default();
    assert!(
        !out.is_empty(),
        "루트 `Cargo.toml` 의 `[workspace] exclude` 를 한 항목도 못 읽었다 — 파서가 죽으면 \
         제외 대상이 멤버로 섞여 이 시험이 **워크스페이스 밖의 사실로** 빨개진다"
    );
    out
}

/// 순회 크레이트 이름. `std::fs::read_dir` 처럼 표준 라이브러리인 것은 뺀다 —
/// 그것은 "들어온다" 의 대상이 아니다.
fn walk_crate_names() -> BTreeSet<String> {
    let path = repo_root().join(MECHANISM_SOURCE);
    let src = fs::read_to_string(&path).unwrap_or_else(|why| {
        panic!(
            "순회 수단 목록의 원문을 못 읽었다 ({}): {why} — 못 읽은 채로 통과하면 이 \
             시험은 \"순회 크레이트가 없다\" 가 아니라 \"안 봤다\" 가 된다",
            path.display()
        )
    });
    walk_crate_names_in(&src, MECHANISM_SOURCE)
}

/// 원문에서 수단 이름을 뽑는다 — **디스크를 안 본다.** 원문과 그 출처가 인자다.
///
/// 출처를 인자로 받는 것은 실패문 때문이다. 상수를 직접 읽으면 이 파서를 다른 원문에
/// 태울 때 실패문이 엉뚱한 파일을 지목한다.
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
    // 앵커에 수단 **이름을 리터럴로 쓰지 않는다.** 쓰면 이 파일이
    // `file_walks_declare_their_mechanism` 의 좌변(코드에 그 낱말이 있는가)에 앉아
    // 그쪽을 빨갛게 만든다 — 실측 2026-09-08 에 그렇게 됐다. 그래서 앵커는 개수와
    // 형태로만 잡는다: 크레이트 이름은 소문자 ascii + `_`/`-` 다.
    let shaped = names.iter().all(|n| {
        n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    });
    assert!(
        names.len() >= 4 && shaped,
        "순회 수단 목록을 못 뽑았다(뽑힌 것: {names:?}). `{whence}` 의 두 상수 \
         형태가 바뀌었다는 뜻이다 — 목록이 비면 이 시험은 아무것도 안 보면서 초록이 된다.\n\
         ☞ 이 단언을 지워서 통과시키지 마라. 그 파일의 상수 이름·형태를 보고 위 파서를 고쳐라."
    );
    names
}

/// 의존 절 판정은 `tasty_doc_guards::cargo_manifest` 하나가 한다 — 이 물음에 판사를
/// 둘 만들면 한쪽만 고쳐진 날 답이 갈린다. 여기서는 이름만 쓴다.
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

/// 걸어 낸 매니페스트에서 **멤버만** 남긴다 — 디스크를 안 본다.
///
/// 루트 매니페스트와 제외 목록이 인자다. 제외 목록을 여기서 다시 읽지 않는 것이
/// 요점이다 — 읽는 자리가 둘이 되면 한쪽만 고쳐진 날 두 답이 갈린다.
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

/// 매니페스트 묶음을 훑은 결과. 훑은 의존 항목 수와 걸린 자리를 **함께** 낸다 —
/// 걸린 자리가 0 인 것과 아무것도 안 훑은 것을 부르는 쪽이 갈라야 하기 때문이다.
struct Scanned {
    entries: usize,
    hits: Vec<String>,
}

/// 이름 붙은 매니페스트 원문들에서 순회 크레이트 의존을 찾는다 — **디스크를 안 본다.**
///
/// 첫 칸은 보고에 쓸 이름이다. 부르는 쪽이 `repo_relative` 를 지난 값을 넣는다 —
/// 여기서 경로를 문자열로 펴면 Windows 에서 구분자가 어긋나고, 그 어긋남은 예외가
/// 아니라 조용한 0 이다.
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

    // 루트를 벗긴 경로는 **반드시** `repo_relative` 를 지난다 — 안 지나면 Windows 에서
    // 구분자가 `\\` 로 나오고, 그 어긋남은 예외가 아니라 조용한 0 이다. 보고 문자열도
    // 다른 가드의 좌표로 인용된다.
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
        "의존 항목을 {entries} 개밖에 못 읽었다(하한 {MIN_DEP_ENTRIES}) — 절 파서가 죽으면 \
         매니페스트 수가 맞아도 아무것도 안 본다"
    );

    println!(
        "[ADR-0146 좌변] 멤버 매니페스트 {} 개 · 의존 항목 {entries} 개 · 순회 크레이트 이름 {:?}",
        manifests.len(),
        walk_crates
    );

    assert!(
        hits.is_empty(),
        "워크스페이스가 순회 크레이트를 직접 의존한다:\n{}\n\
         ★ 이것은 회귀가 아니라 **ADR-0146 의 재검토 조건이 발동한 것**이다 \
         (docs/adr/0146-build-dirs-are-pruned-by-their-tag-not-by-their-name.md).\n\
         그 ADR 은 순회 수단이 `std::fs::read_dir` 뿐이라는 실측 위에 서 있다. 순서가 있다. \
         (1) 그 크레이트의 **자체 필터**가 그 ADR 의 가지치기 판정과 어긋나는지 본다 — \
         `ignore` 는 gitignore 를 읽으므로 표식 기반 판정과 다른 답을 낸다. \
         (2) 어긋나면 그 ADR 의 판정 범위를 다시 쓴다. (3) 안 어긋나면 그 수단을 \
         `{MECHANISM_SOURCE}` 의 `KNOWN_MECHANISMS` 에 선언하고 이 시험의 통과 조건을 \
         그 ADR 과 함께 다시 정한다.\n\
         ☞ 이 목록에서 이름을 빼서 통과시키지 마라 — 그 목록은 저 파일에서 읽어 오는 \
         것이라, 빼면 저쪽 가드도 같이 눈이 먼다.",
        hits.join("\n")
    );
}

/// `[workspace] exclude` 밖에도 순회 크레이트가 있다 — 그것은 발화가 아니다.
/// 경계가 어디인지를 값으로 남긴다: 이 시험이 **안 보는** 자리가 비어 있지 않다는 사실.
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
        "`exclude` 로 지목된 디렉토리에서 매니페스트를 하나도 못 읽었다 — 경계 진단이 \
         빈 채로 통과하면 \"밖에도 없다\" 와 \"안 봤다\" 가 같은 모양이 된다"
    );
    println!("[ADR-0146 경계] exclude 매니페스트 {looked} 개 · 순회 크레이트 {outside:?}");
}

/// 의존 절 파서가 **형태**를 읽는지. 이 픽스처는 이 파일의 상수에서 파생하지 않는다 —
/// 자기가 재려는 값으로 자기를 지으면 그 값에 대해 항진명제가 된다(R1078).
///
/// 이름은 일부러 **아무 이름**이다. 이 시험이 재는 것은 TOML 절의 네 형태를 읽느냐이지
/// 어떤 크레이트냐가 아니고, 진짜 수단 이름을 리터럴로 쓰면 이 파일이
/// `file_walks_declare_their_mechanism` 의 좌변에 앉는다.
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
        "의존 절의 네 형태(평서 · 인라인 테이블 · `[dependencies.foo]` · target 조건) 중 \
         안 읽히는 것이 있다. 안 읽히는 형태로 순회 크레이트가 들어오면 이 가드는 \
         조용히 통과한다.\n\
         ☞ `[lints] workspace = true` 가 결과에 섞이면 절 판정이 헐거워진 것이다."
    );
}

/// 합성 순회 명부 — 디스크를 안 만진다.
///
/// `Walked` 는 순회가 낸 것이든 손으로 지은 것이든 같은 두 칸이다. 이 축이 묻는 것은
/// 파일이 실재하는가가 아니라 **경로 모양을 어떻게 가르는가**라, 트리를 팔 이유가 없다.
/// 이름은 실재하지 않는 `zone-*` 으로 짓는다 — 레포 경로처럼 생긴 합성 좌표는 주석에
/// 적히는 순간 좌표 가드를 깨운다.
fn walked(rels: &[&str]) -> Vec<Walked> {
    rels.iter()
        .map(|r| Walked {
            path: PathBuf::from(*r),
            rel: (*r).to_string(),
        })
        .collect()
}

/// 수단 이름 파서의 세 갈래 — 무엇을 이름으로 세고 무엇을 버리는가.
///
/// 레포의 원문에는 셋 다 섞여 있는데, 버리는 두 갈래가 **버려진 결과로만** 관측된다.
/// 여기서 그 셋을 한 원문에 넣어 태운다.
///
/// ★ 진짜 수단 이름을 여기 리터럴로 쓰지 않는다. 이 파일이
/// `file_walks_declare_their_mechanism` 의 좌변("테스트 타깃의 코드에 그 낱말이
/// 있는가")에 앉아 그쪽을 빨갛게 만든다 — 이 파일 머리 주석이 적어 둔 사고다.
/// 이 시험이 재는 것은 **형태를 어떻게 가르느냐**이지 어떤 크레이트냐가 아니다.
///
/// 합성 원문이 이름 **넷 이상**을 내야 한다 — 파서 안의 앵커가 그 하한을 걸고 있다.
/// 그 제약은 이 시험의 불편이 아니라 지켜지는 성질이고, 아래
/// [`the_parser_survives_one_missing_constant_and_the_anchor_fires_on_an_empty_list`]
/// 가 그 앵커 자체를 태운다.
#[test]
fn the_name_parser_keeps_symbols_and_drops_prose_and_the_standard_library() {
    // 산문이 둘인 것은 거르개가 둘이기 때문이다 — 공백 있는 것은 공백 거르개가,
    // 공백 없는 것은 비-ascii 거르개가 잡는다. 하나만 넣으면 다른 하나를 꺼도
    // 이 시험이 안 죽는다(실측: 공백 있는 것만 넣었을 때 비-ascii 변이가 살아남았다).
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

/// 두 상수 중 **하나만** 있어도 뽑히는가, 그리고 목록이 비면 앵커가 우는가.
///
/// 앞쪽은 그 파일이 상수 하나를 지우는 날 이 파서가 조용히 반쯤 눈멀지 않게 한다.
/// 뒤쪽은 앵커가 실제로 발화하는지를 본다 — 앵커가 안 울면 빈 목록으로 초록이 되고,
/// 그 초록은 "순회 크레이트가 없다" 가 아니라 "안 봤다" 다.
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
        "상수 하나가 없으면 나머지 하나도 안 읽는다 — 그러면 절반만 보고 초록이 된다"
    );

    let renamed = "const ZONE_RENAMED: &[&str] = &[\"zonecrawl\", \"zonesweep\"];\n";
    let why = std::panic::catch_unwind(|| walk_crate_names_in(renamed, "zone/mechanisms.rs"))
        .expect_err("상수 이름이 바뀌면 앵커가 울어야 한다");
    let msg = why
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        msg.contains("zone/mechanisms.rs"),
        "실패문이 **인자로 받은 출처**를 지목해야 한다. 상수를 직접 읽으면 엉뚱한 파일을 \
         지목하고, 그 실패를 읽은 사람이 멀쩡한 파일을 고치러 간다. 실제 문구: {msg}"
    );
}

/// 멤버 명부의 두 거르개 — 깊이와 제외 목록.
///
/// 레포에는 `crates/<이름>/Cargo.toml` 보다 깊은 매니페스트가 없고, 제외된 크레이트가
/// 순회 크레이트를 의존하지도 않는다. 그래서 두 거르개 다 **꺼도 레포는 초록이다.**
#[test]
fn the_member_filter_takes_crate_roots_only_and_honours_the_exclude_list() {
    let found = walked(&[
        "crates/zone-alpha/Cargo.toml",
        "crates/zone-bravo/Cargo.toml",
        // 더 깊다 — 크레이트 루트가 아니라 그 안의 무엇이다.
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
    // 경로를 문자열로 펴지 않는다. 이 자리는 공용 순회를 쓰는 파일이라 손 정규화를
    // 하나 더 만들면 `floored_walk_consumers_do_not_renormalize` 가 운다 — 실제로
    // 울렸다. `PathBuf` 끼리 견주면 정규화 자체가 필요 없다.
    assert_eq!(
        members,
        vec![
            PathBuf::from("Cargo.toml"),
            PathBuf::from("crates/zone-alpha/Cargo.toml")
        ],
        "제외 목록에 있는 크레이트는 멤버가 아니다. 루트 매니페스트는 언제나 들어간다"
    );
}

/// 제외 목록 판독 — 따옴표 **사이**를 집는가.
///
/// 이 판독이 어긋나면 제외 대상이 멤버로 섞이고, 그 어긋남은 레포에서 조용하다
/// (섞여 들어온 크레이트가 마침 순회 크레이트를 안 쓰면 초록이다).
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

/// 훑기의 두 값 — 걸린 자리와 훑은 항목 수.
///
/// 레포에는 걸리는 자리가 없어서 `hits` 가 채워지는 갈래에 **입력이 한 번도 안 들어갔다.**
/// 여기서 하나를 심고, 걸리지 않는 것들이 안 섞이는지도 함께 본다.
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
        "훑은 항목 수는 걸린 것과 무관하게 전부 세야 한다 — 그 수가 \"안 봤다\" 를 가른다"
    );
    assert_eq!(
        got.hits,
        vec![
            "  crates/zone-alpha/Cargo.toml — `zonecrawl`".to_string(),
            "  Cargo.toml — `zonesweep`".to_string(),
        ],
        "걸린 자리는 매니페스트 이름과 크레이트 이름을 함께 대야 한다. 안 걸린 것은 안 섞인다"
    );

    // 걸리는 것이 없으면 비어야 한다 — 늘 차 있으면 위 단정이 공허하게 참이다.
    let clean = vec![(
        "crates/zone-bravo/Cargo.toml".to_string(),
        "[dependencies]\nserde = \"1\"\n".to_string(),
    )];
    assert!(scan(&clean, &mechanisms).hits.is_empty(), "없는데 걸었다");
}
