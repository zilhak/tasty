//! builtin plugin 의 매니페스트(`tasty-plugin.toml`)와 그 크레이트의 코드가 **같은 값을
//! 말하는지** 검증한다. 물음이 그것이고, `version` 은 그 첫 칸이었다.
//!
//! 지금 두 칸이다.
//!
//! 1. 매니페스트 `version` ↔ `Cargo.toml` `version`
//! 2. 매니페스트 `id` ↔ 코드의 `const PLUGIN_ID`
//!
//! 두 칸을 한 파일에 두는 이유가 있다. 같은 물음에 답하는 시험이 둘로 갈리면 다음 사람이
//! 하나만 보고 "그 축은 이미 지켜진다" 고 읽는다 — 실제로 이 파일이 `version` 만 보는 동안
//! `id` 는 아무도 안 봤다.
//!
//! 배경: 매니페스트 version 은 `plugin.list` / 업그레이드 판정(`upgrade_builtins`)이
//! 노출·비교하는 값인데, 과거엔 Cargo.toml 만 patch 자동 +1 되고 매니페스트는 방치돼
//! 드리프트(예: markdown Cargo 0.1.11 vs manifest 0.1.1)가 쌓였다. 버전 정책이 이제
//! 둘의 lockstep 갱신을 요구하므로(§버전 정책 > Plugin), 이 테스트가 그 집행 채널이다.
//! 자동 실행 채널이 **둘**이다: `doc-guards.yml` 이 `cargo test -p tasty-doc-guards` 를
//! main push · PR 마다 **경로 필터 없이** 돌리고, `check-headless` 의 전체 스위트
//! (`cargo test --workspace --no-default-features`)도 이 타깃을 담는다. 그리고 Windows
//! 잡이 `-p tasty-doc-guards` 로 이 크레이트의 통합 타깃을 지목해 돌린다 — 기본 조합의
//! `--lib --bins` 스텝은 여전히 통합 타깃을 안 보지만, 그 잡에 그 스텝만 있는 것은
//! 아니다. 채널 정본은 `docs/dev-guide/ci-gates.md`.
//! 앞엣것은 이 타깃이 이 크레이트로 옮겨오면서 생긴 채널이다(ADR-0138).
//!
//! 참고: 매니페스트가 없는 라이브러리/인프라 크레이트(sdk, protocol, manifest 등)는
//! 번들 대상이 아니므로 스캔에서 자동 제외된다(파일 부재 = skip).

use std::path::{Path, PathBuf};

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 이 값을 **소유한 표**의 줄만 낸다 — 섹션 헤더 앞(최상위 표)이거나 `[package]` 안.
///
/// "최상위" 하나로는 안 된다. 이 함수를 쓰는 파일이 둘이고 두 파일에서 그 값이 사는 표가
/// 다르기 때문이다: `tasty-plugin.toml` 은 섹션 헤더 앞에 두고, `Cargo.toml` 은
/// `[package]` 안에 둔다. 표를 아예 안 보면 `[dependencies.어떤것]` 아래의 `version` 이
/// 먼저 걸려 **남의 버전을 이 크레이트의 버전으로 읽는다**(실측: 그 형태를 지어 주면
/// 9.9.9 를 냈다). 지금 실물 `Cargo.toml` 전부가 `[package]` 를 먼저 두어 잠복이었을 뿐,
/// 판정이 그것을 막고 있던 것이 아니다.
fn owning_table_lines(contents: &str) -> impl Iterator<Item = &str> {
    let mut table: Option<String> = None;
    contents.lines().filter(move |line| {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            table = Some(trimmed.to_string());
            return false;
        }
        match &table {
            None => true,
            Some(name) => name == "[package]",
        }
    })
}

/// `version = "..."` 의 따옴표 안 값을 추출한다. 값을 소유한 표 안에서만 찾는다.
/// `manifest_version` / `api_version` 처럼 접미 `version` 은 `^version = ` 앵커로 배제.
fn extract_version(contents: &str, file: &Path) -> String {
    for line in owning_table_lines(contents) {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("version") {
            // `version` 뒤 첫 비공백이 `=` 여야 최상위 필드 (manifest_version 등 배제)
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let after_eq = after_eq.trim();
                let value = after_eq
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .to_string();
                assert!(!value.is_empty(), "빈 version 필드: {}", file.display());
                return value;
            }
        }
    }
    panic!("version 필드를 찾지 못함: {}", file.display());
}

/// 매니페스트를 가진 번들 plugin 디렉토리를 모은다.
///
/// 두 시험이 같은 모수를 봐야 하므로 여기 한 번만 적는다. 그리고 `read_dir` 을 두 번
/// 부르지 않기 위해서이기도 하다 — 통합 타깃의 직접 `read_dir` 은 여유 0 래칫이라
/// 사본을 하나 더 만들면 그 자리에서 빨개진다(`scripts/check-shared-walk-ratchet.sh`).
fn bundled_plugin_dirs() -> Vec<(String, PathBuf)> {
    // `CARGO_MANIFEST_DIR` 이 곧 레포 루트가 아니다(여기서는 크레이트 디렉토리다).
    // 공용 `repo_root()` 는 표지 파일 넷으로 자기가 잡은 경로를 검증한다.
    let crates_dir = tasty_doc_guards::repo_root().join("crates");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&crates_dir).expect("crates 디렉토리 read_dir 실패") {
        let entry = entry.expect("crates dir entry 읽기 실패");
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if !name.starts_with("tasty-plugin-") {
            continue;
        }
        // 번들 매니페스트 없는 라이브러리/인프라 크레이트 → 대상 아님.
        if !dir.join("tasty-plugin.toml").exists() {
            continue;
        }
        out.push((name, dir));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 매니페스트에서 값을 소유한 표의 `id = "..."` 값.
///
/// 들여쓰기로 거르지 않는다. 옛 형태는 `version` 쪽이 `trim_start` 를 하고 이쪽은 안 해서,
/// 같은 물음에 답하는 두 함수가 **들여쓴 줄에 대해 반대로** 답했다 — 그리고 어느 쪽도
/// 표를 안 봤다. 거르는 축은 들여쓰기가 아니라 표다.
fn extract_id(contents: &str, file: &Path) -> String {
    for line in owning_table_lines(contents) {
        if let Some(rest) = line.trim_start().strip_prefix("id") {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let value = after_eq.trim().trim_matches('"').to_string();
                assert!(!value.is_empty(), "빈 id 필드: {}", file.display());
                return value;
            }
        }
    }
    panic!("id 필드를 찾지 못함: {}", file.display());
}

/// 판정에 들어가는 한 파일 — repo-relative 경로와 그 **원문**.
///
/// 디스크에서 읽어도 되고 손으로 지어도 된다. 아래 두 판정은 파일이 실재하는지 안 묻는다.
type Source = (String, String);

/// 이름과 두 원문의 짝. `(크레이트 이름, 매니페스트 원문, Cargo.toml 원문)`.
type Manifested = (String, String, String);

/// 매니페스트/Cargo.toml 짝을 디스크에서 읽는다. 디스크 접촉을 이 한 함수로 모은다.
fn read_manifested(dirs: &[(String, PathBuf)]) -> Vec<Manifested> {
    dirs.iter()
        .map(|(name, dir)| {
            let manifest = dir.join("tasty-plugin.toml");
            let cargo = dir.join("Cargo.toml");
            (
                name.clone(),
                std::fs::read_to_string(&manifest).expect("매니페스트 read 실패"),
                std::fs::read_to_string(&cargo).expect("Cargo.toml read 실패"),
            )
        })
        .collect()
}

/// 두 원문에서 뽑은 `version` 이 갈라진 자리. 값을 어디서 읽었는지는 안 묻는다.
fn version_mismatches(pairs: &[Manifested]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, manifest_src, cargo_src) in pairs {
        let manifest_v = extract_version(manifest_src, Path::new(name));
        let cargo_v = extract_version(cargo_src, Path::new(name));
        if manifest_v != cargo_v {
            out.push(format!(
                "  {name}: manifest={manifest_v} vs Cargo={cargo_v}"
            ));
        }
    }
    out
}

#[test]
fn manifest_version_matches_cargo_version() {
    let pairs = read_manifested(&bundled_plugin_dirs());
    let checked = pairs.len();
    let mismatches = version_mismatches(&pairs);

    assert!(
        checked > 0,
        "스캔된 builtin plugin 이 0 개 — 경로/네이밍이 바뀌었는지 확인"
    );
    assert!(
        mismatches.is_empty(),
        "매니페스트 version 이 Cargo.toml 과 드리프트됨 (버전 정책 §Plugin: lockstep 갱신 + 재서명 필요):\n{}",
        mismatches.join("\n")
    );
}

/// `crates/<크레이트>/src/` 아래 `.rs` 수의 하한.
///
/// 하한이 겨냥하는 것은 순회의 죽음이다. 다만 "크게 움직이므로 여유를 넓게" 라는 옛
/// 논지는 실측으로 안 섰다 — 3 일 1215 커밋에서 이 모수는 단조 증가했고 한 커밋 최대
/// 이동이 3 이었다. 넓은 여유는 움직임을 견디는 것이 아니라 **안 보는 구간**이었다.
const CRATES_FLOOR: Floor = Floor {
    min: 450,
    measured: 576,
    measured_on: "2026-09-08",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree("b134d28e3"),
    why_this_gap: "이 모수는 `crates/<크레이트>/src/` 아래 `.rs` 파일 수다(`/src/` 를 안 \
                   지나는 것은 안 센다 — 그 술어를 안 적으면 다음 사람이 `crates/` 아래 \
                   `.rs` 전부를 세고 다른 수를 얻는다). 2026-09-08 에 `b134d28e3` 에서 \
                   576 이었다. 옛 값 650 은 낡은 것이 아니라 **안 잰 값**이었다 — 그것을 \
                   적은 커밋의 트리에서 이 좌변은 574 였고, 그날 하루 어느 시점에도 650 이 \
                   아니었다. 여유를 126 으로 좁힌 근거: 1215 커밋(2026-09-05~09-08)에서 이 \
                   모수는 550 에서 576 으로 **단조 증가**했고 한 커밋 최대 이동이 3 이었다. \
                   옛 문장이 근거로 든 '크레이트가 한 번에 수십 개를 옮긴다' 는 이 창에서 \
                   관측되지 않았다. 그래도 여유를 0 으로 안 붙이는 것은 크레이트 하나를 \
                   통째로 접는 변경이 아직 안 났을 뿐 날 수 있기 때문이고, 126 은 가장 큰 \
                   크레이트 하나가 통째로 빠지는 폭이다.",
};

/// 매니페스트 `id` 와 코드의 `const PLUGIN_ID` 가 같은 값을 말하는지 본다.
///
/// ## 왜 이 값에 채널이 필요한가
///
/// plugin 은 hello 로 자기 id 를 보내고 host 는 **그 값 그대로** registry 에 등록한다 —
/// 매니페스트의 `id` 와 대조하지 않는다. 그래서 둘이 어긋나도 부팅은 조용히 성공하고,
/// 매니페스트를 근거로 도는 것(IPC 네임스페이스 · 권한 · `plugin.list`)과 hello 를 근거로
/// 도는 것이 **서로 다른 plugin 을 가리키게 된다.** 컴파일도 테스트도 그것을 안 본다.
///
/// ## 그리고 바로 옆줄이 옳은 형태다
///
/// 같은 파일에서 `PLUGIN_VERSION` 은 `env!("CARGO_PKG_VERSION")` 으로 **파생**된다 —
/// 어긋날 수가 없다. `PLUGIN_ID` 만 손으로 적은 두 번째 사본이고, 그 사실이 어디에도
/// 적혀 있지 않다. 파생으로 바꾸는 편이 이 시험보다 강하지만(매니페스트를 `include_str!`
/// 로 읽어 파싱하는 build.rs 가 필요하다) 그 비용이 이 값 하나에 비해 크다. 그래서
/// 여기서는 **두 사본이 갈라지는 것을 잡는 것**까지 한다.
/// 한 줄에서 `const PLUGIN_ID: ... = "값"` 의 값을 뽑는다.
///
/// 이름 뒤에 `:` 가 와야 한다 — 부분 문자열로 찾으면 `PLUGIN_IDENT` 같은 다른 상수까지
/// 잡히고, 그러면 이 시험이 "그 상수를 찾았다" 고 잘못 센다. (변이로 밟았다: 상수 이름을
/// 바꿔도 접두가 매치돼 하한이 발화하지 않았다.)
fn plugin_id_in_line(line: &str) -> Option<&str> {
    let (_, rest) = line.trim_start().split_once("const PLUGIN_ID:")?;
    rest.split('"').nth(1)
}

/// 훑은 결과. plugin 마다의 눈먼 목록을 따로 내는 것이 이 구조체의 이유다 — 합으로
/// 세면 한 crate 의 여분 사본이 다른 crate 의 소실을 가린다.
struct IdScan {
    blind: Vec<String>,
    mismatches: Vec<String>,
}

/// `sources` 는 `(repo-relative 경로, 원문)` 짝, `plugins` 는 `(크레이트 이름,
/// 매니페스트 id)` 명부다. **둘 다 인자다** — 이 판정이 지키는 성질은 지금 저장소에
/// 어떤 plugin 이 있는가가 아니라 "plugin 마다 상수를 찾았고 값이 같은가" 라는 모양이고,
/// 그 모양은 명부와 무관하게 성립해야 한다(R1072).
fn id_scan(sources: &[Source], plugins: &[(String, String)]) -> IdScan {
    let mut blind = Vec::new();
    let mut mismatches = Vec::new();

    for (name, manifest_id) in plugins {
        let mut found_here = 0usize;
        let prefix = format!("crates/{name}/src/");
        for (rel, text) in sources.iter().filter(|(rel, _)| rel.starts_with(&prefix)) {
            for line in text.lines() {
                let Some(value) = plugin_id_in_line(line) else {
                    continue;
                };
                found_here += 1;
                if value != manifest_id {
                    mismatches.push(format!(
                        "  {rel}: manifest id={manifest_id} vs const PLUGIN_ID={value}"
                    ));
                }
            }
        }
        if found_here == 0 {
            blind.push(format!("  {name}"));
        }
    }

    IdScan { blind, mismatches }
}

#[test]
fn manifest_id_matches_the_plugin_id_constant() {
    let root = tasty_doc_guards::repo_root();
    let walked = walk_with_floor(
        &root.join("crates"),
        &root,
        &CRATES_FLOOR,
        Descend::SkipBuildCaches,
        &|w: &Walked| w.rel.ends_with(".rs") && w.rel.contains("/src/"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    let sources: Vec<Source> = walked
        .iter()
        .map(|w| {
            (
                w.rel.clone(),
                std::fs::read_to_string(&w.path).unwrap_or_default(),
            )
        })
        .collect();

    let dirs = bundled_plugin_dirs();
    let plugins: Vec<(String, String)> = dirs
        .iter()
        .map(|(name, dir)| {
            let manifest = dir.join("tasty-plugin.toml");
            let id = extract_id(
                &std::fs::read_to_string(&manifest).expect("매니페스트 read 실패"),
                &manifest,
            );
            (name.clone(), id)
        })
        .collect();

    let checked = plugins.len();
    let IdScan { blind, mismatches } = id_scan(&sources, &plugins);

    assert!(
        checked > 0,
        "스캔된 builtin plugin 이 0 개 — 경로/네이밍이 바뀌었는지 확인"
    );
    // 합이 아니라 **plugin 마다** 센다. 합으로 세면 한 crate 의 여분 사본이 다른 crate 의
    // 소실을 가린다 — 실제로 `tasty-plugin-claude` 는 `lib.rs` 와 `main.rs` 두 곳에 그
    // 상수를 두고 있어, 합 판정에서는 어느 한 plugin 이 통째로 안 보여도 수가 맞는다.
    // (변이로 확인했다: 상수 이름 하나를 바꿔도 합 단정은 통과했다.)
    assert!(
        blind.is_empty(),
        "이 plugin 들에서 `const PLUGIN_ID` 를 하나도 못 찾았다 — 상수를 다른 이름으로 \
         적었거나 순회가 그 파일을 못 봤다는 뜻이다. 이 상태에서 뒤따르는 \"불일치 0\" 은 \
         일치한다는 뜻이 아니라 그 plugin 을 아예 안 봤다는 뜻이다:\n{}",
        blind.join("\n")
    );
    assert!(
        mismatches.is_empty(),
        "매니페스트 `id` 와 코드의 `const PLUGIN_ID` 가 갈라졌다.\n\
         host 는 hello 로 받은 id 를 그대로 등록하고 매니페스트와 대조하지 않으므로, \
         이 어긋남은 부팅을 막지 않고 IPC 네임스페이스·권한·`plugin.list` 가 서로 다른 \
         plugin 을 가리키게 만든다:\n{}",
        mismatches.join("\n")
    );
}

/// 합성 말뭉치. 디스크를 안 탄다.
///
/// 경로는 실재하지 않는 이름을 쓰고, 문자열 안에만 둔다 — 주석에 두면 좌표 가드가
/// 없는 파일을 가리키는 인용으로 읽는다(그 가드는 비-`.md` 에서 주석 줄만 본다).
fn corpus(pairs: &[(&str, &str)]) -> Vec<Source> {
    pairs
        .iter()
        .map(|(rel, src)| ((*rel).to_string(), (*src).to_string()))
        .collect()
}

/// 합성 명부 — `(크레이트 이름, 매니페스트 id)`.
fn roster(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, id)| ((*name).to_string(), (*id).to_string()))
        .collect()
}

/// 접미가 `version` 인 이름은 최상위 `version` 이 아니다.
///
/// 이 갈래는 실물 매니페스트로 못 태운다 — 지금 어느 매니페스트도 `api_version` 을
/// `version` 앞줄에 두고 있지 않아서, 앵커가 없어도 답이 같다.
#[test]
fn a_suffixed_name_is_not_the_version_field() {
    let src =
        "manifest_version = 1\napi_version = \"3\"\nversions = [\"9\"]\nversion = \"0.1.2\"\n";
    assert_eq!(extract_version(src, Path::new("zone")), "0.1.2");

    // 반대 방향 — 앵커가 값을 못 찾게 만들지는 않는다.
    assert_eq!(
        extract_version("version=\"7.7.7\"\n", Path::new("zone")),
        "7.7.7",
        "공백 없는 형태를 못 읽는다"
    );
}

/// 갈라진 짝은 이름과 두 값으로 실리고, 맞는 짝은 안 실린다.
#[test]
fn a_drifted_pair_is_named_and_a_matching_pair_is_not() {
    let same: Manifested = (
        "zonealpha".to_string(),
        "version = \"0.2.0\"\n".to_string(),
        "[package]\nversion = \"0.2.0\"\n".to_string(),
    );
    let drifted: Manifested = (
        "zonebeta".to_string(),
        "version = \"0.1.1\"\n".to_string(),
        "[package]\nversion = \"0.1.9\"\n".to_string(),
    );

    assert_eq!(version_mismatches(&[same.clone()]), Vec::<String>::new());
    assert_eq!(
        version_mismatches(&[same, drifted]),
        vec!["  zonebeta: manifest=0.1.1 vs Cargo=0.1.9".to_string()]
    );
}

/// 상수 이름 뒤의 `:` 가 없으면 그 줄이 아니다 — 그리고 있으면 그 줄이 맞다.
///
/// 한 방향만 재면 "못 찾았다" 와 "안 봤다" 가 안 갈린다. 접두 매치로 잡히던 시절
/// `PLUGIN_IDENT` 가 상수 하나를 대신 세서 하한이 발화하지 않았다.
#[test]
fn the_constant_needle_needs_the_colon_and_the_quotes() {
    assert_eq!(
        plugin_id_in_line("    pub const PLUGIN_ID: &str = \"zone.alpha\";"),
        Some("zone.alpha")
    );
    assert_eq!(
        plugin_id_in_line("const PLUGIN_IDENT: &str = \"zone.alpha\";"),
        None,
        "다른 상수를 이 상수로 센다"
    );
    assert_eq!(
        plugin_id_in_line("    let sent = PLUGIN_ID;"),
        None,
        "사용처를 선언으로 센다"
    );
    assert_eq!(
        plugin_id_in_line("const PLUGIN_ID: &str = OTHER;"),
        None,
        "따옴표 없는 값에서 값을 지어낸다"
    );
}

/// 상수를 하나도 못 찾은 plugin 은 "일치" 가 아니라 **눈먼** 자리다.
#[test]
fn a_plugin_with_no_constant_is_blind_not_matching() {
    let sources = corpus(&[(
        "crates/tasty-plugin-zonealpha/src/lib.rs",
        "pub const PLUGIN_ID: &str = \"zone.alpha\";",
    )]);
    let plugins = roster(&[
        ("tasty-plugin-zonealpha", "zone.alpha"),
        ("tasty-plugin-zonebeta", "zone.beta"),
    ]);

    let scan = id_scan(&sources, &plugins);
    assert_eq!(scan.blind, vec!["  tasty-plugin-zonebeta".to_string()]);
    assert_eq!(
        scan.mismatches,
        Vec::<String>::new(),
        "눈먼 자리를 불일치로도 센다"
    );
}

/// 눈먼 판정은 **plugin 마다**다 — 합으로 세면 한쪽의 여분 사본이 다른 쪽의 소실을 가린다.
///
/// 실물이 그 형태다: 한 plugin 이 `lib.rs` 와 `main.rs` 두 곳에 상수를 둔다. 합 판정은
/// 그 둘로 다른 plugin 하나가 통째로 안 보이는 것을 덮는다 — 그리고 그 상태에서
/// 뒤따르는 "불일치 0" 은 일치한다는 뜻이 아니다.
#[test]
fn the_blind_check_is_per_plugin_not_a_sum() {
    let sources = corpus(&[
        (
            "crates/tasty-plugin-zonealpha/src/lib.rs",
            "pub const PLUGIN_ID: &str = \"zone.alpha\";",
        ),
        (
            "crates/tasty-plugin-zonealpha/src/main.rs",
            "const PLUGIN_ID: &str = \"zone.alpha\";",
        ),
    ]);
    let plugins = roster(&[
        ("tasty-plugin-zonealpha", "zone.alpha"),
        ("tasty-plugin-zonebeta", "zone.beta"),
    ]);

    // 합은 2 라 0 보다 크다. plugin 마다 세야만 zonebeta 가 보인다.
    assert_eq!(
        id_scan(&sources, &plugins).blind,
        vec!["  tasty-plugin-zonebeta".to_string()]
    );
}

/// 다른 plugin 디렉토리의 상수는 이 plugin 것으로 안 센다.
///
/// 접두가 헐거우면 이름이 접두인 crate(`…-zone` 과 `…-zonebeta`)가 서로의 상수를
/// 빌려 눈먼 자리를 덮는다.
#[test]
fn a_constant_under_another_prefix_does_not_cover_this_plugin() {
    let sources = corpus(&[(
        "crates/tasty-plugin-zonealphax/src/lib.rs",
        "pub const PLUGIN_ID: &str = \"zone.alpha\";",
    )]);
    let plugins = roster(&[("tasty-plugin-zonealpha", "zone.alpha")]);

    assert_eq!(
        id_scan(&sources, &plugins).blind,
        vec!["  tasty-plugin-zonealpha".to_string()],
        "접두가 겹치는 다른 crate 의 상수를 이 plugin 것으로 센다"
    );
}

/// 갈라진 사본은 **자기 파일 이름과 함께** 실리고, 같은 plugin 의 맞는 사본은 안 실린다.
#[test]
fn a_drifted_constant_is_named_with_the_file_that_holds_it() {
    let sources = corpus(&[
        (
            "crates/tasty-plugin-zonealpha/src/lib.rs",
            "pub const PLUGIN_ID: &str = \"zone.alpha\";",
        ),
        (
            "crates/tasty-plugin-zonealpha/src/main.rs",
            "const PLUGIN_ID: &str = \"zone.alpha.old\";",
        ),
    ]);
    let plugins = roster(&[("tasty-plugin-zonealpha", "zone.alpha")]);

    let scan = id_scan(&sources, &plugins);
    assert_eq!(scan.blind, Vec::<String>::new());
    assert_eq!(
        scan.mismatches,
        vec![
            "  crates/tasty-plugin-zonealpha/src/main.rs: manifest id=zone.alpha vs const PLUGIN_ID=zone.alpha.old"
                .to_string()
        ]
    );
}

/// 남의 표에 있는 `version` 은 이 크레이트의 버전이 아니다.
///
/// 이 갈래는 실물로 못 태운다 — 지금 모든 `Cargo.toml` 이 `[package]` 를 먼저 둬서,
/// 표를 봐도 안 봐도 답이 같다. 그 상태에서 초록은 판정이 막고 있다는 뜻이 아니었다.
#[test]
fn a_version_in_another_table_is_not_the_packages_version() {
    let cargo = "[dependencies.zonebox]\nversion = \"9.9.9\"\n\n[package]\nversion = \"0.1.2\"\n";
    assert_eq!(extract_version(cargo, Path::new("zone")), "0.1.2");

    // 반대 방향 — 표를 보는 것이 정상 배치를 막지는 않는다. 두 형태 다 읽어야 한다:
    // 매니페스트는 섹션 헤더 앞, `Cargo.toml` 은 `[package]` 안이다.
    assert_eq!(
        extract_version("version = \"0.3.0\"\n[permissions]\n", Path::new("zone")),
        "0.3.0",
        "섹션 앞의 최상위 값을 못 읽는다"
    );
    assert_eq!(
        extract_version(
            "[package]\nname = \"zone\"\nversion = \"0.4.0\"\n",
            Path::new("zone")
        ),
        "0.4.0",
        "`[package]` 안의 값을 못 읽는다"
    );
}

/// 같은 규칙이 `id` 에도 걸린다 — 그리고 거르는 축은 들여쓰기가 아니다.
#[test]
fn the_owning_tables_id_is_found_even_when_indented() {
    assert_eq!(
        extract_id(
            "  id = \"zone.right\"\n[meta]\nid = \"zone.wrong\"\n",
            Path::new("zone")
        ),
        "zone.right"
    );
}

/// 남의 표에만 있는 `id` 는 **찾은 것이 아니다.**
///
/// 위 시험과 짝이다. 이쪽만 있으면 규칙이 너무 좁아 정상 매니페스트도 못 읽는 상태와
/// 구별되지 않고, 저쪽만 있으면 남의 표를 읽는 상태와 구별되지 않는다.
#[test]
#[should_panic(expected = "id 필드를 찾지 못함")]
fn an_id_that_lives_only_in_another_table_is_not_found() {
    extract_id("[meta]\nid = \"zone.wrong\"\n", Path::new("zone"));
}
