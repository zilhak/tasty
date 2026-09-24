//! 이 크레이트의 각 bin에 대응하는 통합 테스트가 있는지 소스에서 확인한다.
//! 테스트 파일에서 Cargo의 바이너리 경로 환경 변수 이름과 출력·종료 상태 표지를 찾는다.
//! 파일 이름 규칙에는 의존하지 않지만, 문자열의 존재가 실제 실행이나 결과 검증을 보장하지는 않는다.
//! 주석·무관한 사용도 판독에 섞일 수 있고, 바이너리 경로를 직접 쓴 테스트는 찾지 못한다.
//! 합성 입력은 표지 판독만 검증하며 실제 저장소 검사 전체를 대체하지 않는다.

// 이유: 테스트의 반환값 무시는 제품 코드의 lint 예외 명부에 포함하지 않는다.
#![allow(clippy::let_underscore_must_use)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const CRATE_DIR: &str = "crates/tasty-doc-guards";

/// 2026-09-23의 bin 소스 파일과 Cargo metadata 모두4개였던 측정을 기준으로 둔다.
/// 자동 발견 bin이 삭제되면 검사 대상도 함께 줄어드므로 별도로 하한을 둔다.
/// 의도적인 삭제 때만 두 출처를 다시 확인해 하한을 갱신한다.
const MIN_BINS: usize = 4;

/// 출력·종료 상태를 읽는 것으로 분류할 문자열 표지.
const READS_OUTPUT: &[&str] = &["stdout", "status", "code()"];

fn reads_output(text: &str) -> bool {
    READS_OUTPUT.iter().any(|m| text.contains(m))
}

fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while !dir.join(".git").exists() {
        if !dir.pop() {
            panic!("레포 루트를 못 찾았다");
        }
    }
    dir
}

/// `src/bin/` 의 바이너리 이름들 — 파일 stem 이 곧 cargo 가 쓰는 이름이다.
fn bin_names(crate_dir: &Path) -> BTreeSet<String> {
    let dir = crate_dir.join("src/bin");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("`{}` 를 읽지 못했다", dir.display());
    };
    let mut names = BTreeSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rs")
            && let Some(stem) = path.file_stem()
        {
            names.insert(stem.to_string_lossy().to_string());
        }
    }
    names
}

/// 자기 자신의 검색 문자열을 바이너리 참조로 오해하지 않도록 접두어를 조각으로 조립한다.
fn handles_in(source: &str) -> BTreeSet<String> {
    let needle = concat!("CARGO_BIN_", "EXE_");
    let mut found = BTreeSet::new();
    let mut from = 0;
    while let Some(rel) = source[from..].find(needle) {
        let at = from + rel + needle.len();
        from = at;
        let name: String = source[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !name.is_empty() {
            found.insert(name);
        }
    }
    found
}

/// bin 이름별로 해당 경로 환경 변수 이름이 나오는 테스트 파일을 모은다.
fn executors(crate_dir: &Path) -> BTreeMap<String, Vec<String>> {
    let dir = crate_dir.join("tests");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("`{}` 를 읽지 못했다", dir.display());
    };
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        for name in handles_in(&text) {
            map.entry(name).or_default().push(file.to_string());
        }
    }
    map
}

#[test]
fn every_bin_is_run_by_some_test() {
    let crate_dir = repo_root().join(CRATE_DIR);
    let bins = bin_names(&crate_dir);
    assert!(
        bins.len() >= MIN_BINS,
        "src/bin에서 바이너리를 {}개만 찾았다(하한 {MIN_BINS}, 크레이트 {CRATE_DIR}). 실제 삭제인지 수집 오류인지 확인한다.",
        bins.len()
    );

    let runs = executors(&crate_dir);
    let orphans: Vec<&String> = bins.iter().filter(|b| !runs.contains_key(*b)).collect();
    assert!(
        orphans.is_empty(),
        "통합 테스트에서 대응하는 Cargo 바이너리 경로 환경 변수 참조를 찾지 못했다:\n  {}\n바이너리를 실행해 결과를 검증하는 테스트를 추가한다. 이미 테스트가 있다면 경로를 직접 지정했는지 확인한다. 이 검사는 환경 변수 이름의 존재만 확인하며 실제 실행과 판정의 타당성은 보장하지 않는다.",
        orphans
            .iter()
            .map(|b| format!("src/bin/{b}.rs"))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn no_executor_runs_the_bin_and_looks_away() {
    let crate_dir = repo_root().join(CRATE_DIR);
    let runs = executors(&crate_dir);
    assert!(
        !runs.is_empty(),
        "바이너리 경로 환경 변수 참조를 찾지 못했다. 테스트 소스와 판독을 확인한다."
    );

    let mut blind = Vec::new();
    for (bin, files) in &runs {
        for file in files {
            let path = crate_dir.join("tests").join(file);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if !reads_output(&text) {
                blind.push(format!(
                    "tests/{file} — {bin} 참조 파일에 출력·종료 상태 표지가 없다"
                ));
            }
        }
    }
    assert!(
        blind.is_empty(),
        "바이너리를 참조한 테스트 파일에 출력·종료 상태를 읽는 표지가 없다:\n  {}\n실행 결과를 검증하는지 확인한다. 표지의 존재만으로 올바른 검증이 보장되는 것은 아니다.",
        blind.join("\n  ")
    );
}

fn handle(name: &str) -> String {
    format!("env!(\"{}{name}\")", concat!("CARGO_BIN_", "EXE_"))
}

#[test]
fn the_reader_answers_both_yes_and_no() {
    let covered = format!("const BIN: &str = {};", handle("mask-source"));
    assert!(
        handles_in(&covered).contains("mask-source"),
        "바이너리 경로 환경 변수 이름을 추출하지 못했다"
    );

    let uncovered = "let path = \"target/debug/mask-source\"; // 경로 하드코딩";
    assert!(
        handles_in(uncovered).is_empty(),
        "직접 지정한 경로를 Cargo 환경 변수 참조로 오해했다"
    );
}

#[test]
fn a_second_bin_in_the_same_file_is_not_swallowed() {
    let both = format!("{}\n{}", handle("alpha"), handle("beta-two"));
    let found = handles_in(&both);
    assert!(
        found.contains("alpha") && found.contains("beta-two"),
        "한 파일의 바이너리 참조 두 개를 모두 읽지 못했다: {found:?}"
    );
}

#[test]
fn a_bare_prefix_is_not_a_bin_name() {
    let bare = format!(
        "const NEEDLE: &str = \"{}\";",
        concat!("CARGO_BIN_", "EXE_")
    );
    assert!(
        handles_in(&bare).is_empty(),
        "이름 없는 접두사를 bin 이름으로 주웠다"
    );
}

/// 기준 목록에서 합성 입력을 만들면 항목 삭제를 놓치므로 표지를 독립적으로 하나씩 작성한다.
#[test]
fn every_output_marker_is_actually_read_as_reading_output() {
    let cases: [(&str, &str); 3] = [
        (
            "stdout",
            r#"let text = String::from_utf8_lossy(&out.stdout);"#,
        ),
        (
            "status",
            r#"assert!(out.status.success(), "판정기가 죽었다");"#,
        ),
        (
            "code()",
            r#"assert_eq!(exit.code(), Some(0), "종료 코드");"#,
        ),
    ];
    for (marker, snippet) in cases {
        assert!(
            reads_output(snippet),
            "출력 표지 {marker:?}를 인식하지 못했다. 목록과 판독을 확인한다."
        );
    }
}

#[test]
fn running_a_bin_without_looking_is_not_reading_output() {
    assert!(
        !reads_output(r#"Command::new(BIN).arg("--help").output().unwrap();"#),
        "결과 표지가 없는 소스를 결과를 읽는 것으로 분류했다"
    );
}

/// 같은 파일의 여러 임시 경로를 구별하도록 PID와 호출 줄 번호를 함께 쓴다.
fn probe_root(tag: &str, line: u32) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tasty-binfloor-{tag}-{}-{line}",
        std::process::id()
    ));
    // 앞선 실행의 잔여를 치운다 — 없는 것이 정상이라 실패가 정보가 아니다.
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn with_bins(tag: &str, line: u32, names: &[&str]) -> PathBuf {
    let root = probe_root(tag, line);
    let bin = root.join("src/bin");
    std::fs::create_dir_all(&bin).expect("임시 디렉토리를 못 만들었다");
    for n in names {
        std::fs::write(bin.join(format!("{n}.rs")), "fn main() {}\n").expect("쓰기 실패");
    }
    root
}

#[test]
fn the_bin_floor_sees_a_collapsed_collection() {
    let empty = with_bins("empty", line!(), &[]);
    assert_eq!(
        bin_names(&empty).len(),
        0,
        "빈 src/bin에서 바이너리를 수집했다"
    );

    let one = with_bins("one", line!(), &["only"]);
    assert!(
        bin_names(&one).len() < MIN_BINS,
        "바이너리 하나만 있는 합성 트리가 하한 미달로 분류되지 않았다"
    );

    let many = with_bins("many", line!(), &["a", "b", "c", "d"]);
    assert!(
        bin_names(&many).len() >= MIN_BINS,
        "충분한 바이너리가 있는 합성 트리도 하한을 넘지 못했다"
    );

    for d in [empty, one, many] {
        // 뒷정리 실패는 무시한다 — 임시 디렉토리라 남아도 다음 실행이 먼저 지우고,
        // 여기서 죽으면 위 단정의 결과가 정리 오류에 가린다.
        let _ = std::fs::remove_dir_all(d);
    }
}
