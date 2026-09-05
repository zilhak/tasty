//! `PluginManager.packages` 를 바꾸는 자리가 **유도표를 같이 다시 만드는가.**
//!
//! ## 무엇이 실제로 났나
//!
//! `ipc_namespaces`(어느 prefix 를 어느 plugin 이 갖는가)는 [ADR-0173] 이후
//! **설치된 매니페스트에서 유도되는 표**다. 유도는 `PluginManager::refresh_packages`
//! 안에서만 돈다. 그런데 `plugin.remove` 는 그 함수를 안 거치고 `packages` 를 손으로
//! `retain` 했다. 그래서 **지운 plugin 의 prefix 가 표에 남았고**, 그 이름의 호출이
//! `-32002 plugin '<id>' is not running` 으로 거절됐다 — 설치조차 안 돼 있는데.
//! 호스트가 같은 이름에 구현을 갖고 있으면(`image.list`·`image.open`·`markdown.navigate`)
//! 그 구현이 그 상태에서 통째로 가려진다.
//!
//! 실측(2026-09-05, gui 격리 홈): `plugin.remove com.tasty.image` 뒤 `plugin.list` 는
//! 8 개(image 없음)인데 `image.list` 는 `-32002` 를 답했다. 고친 뒤 같은 자리에서
//! `image.open {}` 이 `-32602 missing 'surface_id'` 를 **plugin SDK 의 `host call …
//! failed:` 래퍼 없이** 답한다 — 래퍼의 유무가 "누가 답했나" 를 가른다.
//!
//! ## 왜 테스트가 아니라 텍스트인가
//!
//! 두 표의 정합은 값으로 물을 수 있지만, **물으려면 그 상태를 만들어야 한다** — 설치된
//! plugin 이 있는 매니저에서 제거를 태워야 하고, 그건 디스크와 프로세스를 요구한다.
//! 반면 "유도를 안 거치고 원본을 바꾼 자리가 있는가" 는 소스로 답이 난다. 실제로 이
//! 결함은 두 조합의 유닛 스위트를 통과했고 실행 확인에서만 드러났다.
//!
//! [ADR-0173]: ../../docs/adr/0173-namespace-resolution-reads-the-manifest-not-the-process-table.md

use std::path::{Path, PathBuf};

use super::{repo_root, strip_comments};

/// 유도표의 원본. 이 필드를 바꾸면 `ipc_namespaces` 를 다시 만들어야 한다.
const SOURCE_FIELD: &str = ".packages";

/// 원본을 바꾸는 형태. `=` 는 대입, 나머지는 `Vec` 의 변경 메서드다.
const MUTATIONS: &[&str] = &[
    ".retain(", ".push(", ".clear(", ".remove(", ".insert(", " =",
];

/// 유도가 사는 크레이트 — 여기 안에서는 유도 함수와 원본이 같은 자리에 있다.
const OWNING_CRATE: &str = "crates/tasty-host-plugin";

/// 스캔 대상. 본체와 다른 크레이트가 이 필드를 만지는지 본다.
const SCAN_ROOTS: &[&str] = &["src", "crates"];

/// 훑는 `.rs` 파일 수의 하한 — **연기 검사**. 값의 근거: 2026-09-05 실측 1400 이상.
const MIN_SCANNED_FILES: usize = 800;

fn rust_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("디렉터리 항목을 읽을 수 없다");
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 그 줄이 원본을 바꾸는가.
fn mutates_source(line: &str) -> bool {
    let Some(at) = line.find(SOURCE_FIELD) else {
        return false;
    };
    let after = &line[at + SOURCE_FIELD.len()..];
    // `.packages_of()` 같은 더 긴 이름을 배제한다.
    if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return false;
    }
    MUTATIONS.iter().any(|m| after.starts_with(m))
}

/// 유도표의 원본은 **유도가 사는 크레이트 밖에서** 바뀌지 않는다.
#[test]
fn plugin_packages_are_only_mutated_where_the_derivation_lives() {
    let root = repo_root();
    let mut files = Vec::new();
    for r in SCAN_ROOTS {
        rust_files(&root.join(r), &mut files);
    }
    assert!(
        files.len() >= MIN_SCANNED_FILES,
        "훑은 .rs 가 {} 개뿐이다(하한 {MIN_SCANNED_FILES}) — 스캔이 죽으면 아래 판정은 \
         볼 것이 없어 그냥 통과한다",
        files.len()
    );

    let mut offenders: Vec<String> = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with(OWNING_CRATE) {
            continue;
        }
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
            .replace("\r\n", "\n");
        for (i, line) in strip_comments(&src).lines().enumerate() {
            if mutates_source(line) {
                offenders.push(format!("{rel}:{} — {}", i + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "`{SOURCE_FIELD}` 를 유도가 사는 곳 밖에서 바꾼다. `ipc_namespaces` 는 이 값에서 \
         유도되므로(ADR-0173) 여기서 바꾸면 표가 낡는다 — 지운 plugin 의 prefix 가 남아 \
         그 이름의 호출이 `-32002 … is not running` 으로 거절되고, 호스트가 같은 이름에 \
         가진 구현이 가려진다. `PluginManager::refresh_packages()` 를 불러라.\n  {}",
        offenders.join("\n  ")
    );
}

/// 판정이 실제로 그 형태를 집는다 — 대조군.
///
/// 검사 대상 줄을 **문자열로 조립한다.** 리터럴로 적으면 이 파일이 자기 스캔에 걸려
/// 자기 대조군을 위반으로 센다(R80). 같은 이유로 다른 가드도 needle 을 통째로 안 적는다.
#[test]
fn the_mutation_shapes_are_recognised_and_reads_are_not() {
    let f = SOURCE_FIELD;
    assert!(mutates_source(&format!(
        "        mgr{f}.retain(|p| p.id != x);"
    )));
    assert!(mutates_source(&format!("    self{f} = packages;")));
    assert!(mutates_source(&format!("    m{f}.push(pkg);")));
    assert!(
        !mutates_source(&format!("    for pkg in &mgr{f} {{")),
        "읽기를 쓰기로 셌다"
    );
    assert!(
        !mutates_source(&format!("    let n = mgr{f}.len();")),
        "읽기를 쓰기로 셌다"
    );
    assert!(
        !mutates_source(&format!("    mgr{f}_of(id).clear();")),
        "더 긴 이름을 이 필드로 셌다"
    );
}

/// 주석 안의 같은 형태는 위반이 아니다.
#[test]
fn a_mutation_inside_a_comment_is_not_counted() {
    let f = SOURCE_FIELD;
    let src = format!("fn f() {{\n    // mgr{f}.retain(|p| true);\n    ok();\n}}\n");
    let stripped = strip_comments(&src);
    assert!(
        !stripped.lines().any(mutates_source),
        "주석 안의 형태를 위반으로 셌다 — 결함을 설명할수록 나빠지는 판정이다"
    );
}

/// 이 파일 자신이 스캔에 잡히면 안 되는데, **면제가 아니라 조립으로** 그렇게 한다.
///
/// 면제 목록으로 빼면 이 파일이 나중에 진짜 위반을 들여도 안 보인다. 그래서 여기서는
/// 리터럴을 안 쓰는 쪽을 택했고, 그 사실이 유지되는지를 못 박는다.
#[test]
fn this_file_carries_no_whole_mutation_literal() {
    let me = repo_root().join("src/source_guards/derived_plugin_tables_are_not_bypassed.rs");
    let src = std::fs::read_to_string(&me).expect("이 파일을 읽어야 한다");
    let hits: Vec<&str> = strip_comments(&src)
        .lines()
        .filter(|l| mutates_source(l))
        .map(|_| "hit")
        .collect();
    assert!(
        hits.is_empty(),
        "이 파일이 자기 판정에 걸린다({} 줄) — 대조군을 리터럴로 적었다면 조립으로 바꿔라",
        hits.len()
    );
}
