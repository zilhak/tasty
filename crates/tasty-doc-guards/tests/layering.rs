//! 본체 src의 tasty_cli:: 참조를 제한한다. 공통 기능은 CLI와 GUI가 함께 쓰는 크레이트에 둔다.
//! 코드뿐 아니라 주석의 해당 문자열도 검사하며 별칭 참조를 모두 해석하지는 않는다.
//! 정당한 진입·조립 경로(ALLOWED_PATHS), 제거해야 할 기존 참조(BASELINE_FILES),
//! 테스트 전용 파일(TEST_ONLY_FILES)은 이유가 달라 분리한다.
//! 한시 참조는 제거되면 명부도 줄이고, 테스트 예외는 부모의 test 조건을 확인한다.
//! 정책은 [ADR-0048](../../../docs/adr/0048-source-guards-and-exemptions.md)을 따른다.

// 이유: 테스트의 반환값 무시는 제품 코드의 lint 예외 명부에 포함하지 않는다.
#![allow(clippy::let_underscore_must_use)]

use std::path::Path;
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, normalized_rel, walk_with_floor};

/// 주석·코드에서 모두 찾을 직접 크레이트 경로.
const FORBIDDEN: &str = "tasty_cli::";

/// CLI 파서를 소유하는 진입점·boot 경로·재노출 파일은 정책상 허용한다. 빈 목록으로 만드는 대상이 아니다.
const ALLOWED_PATHS: &[&str] = &[
    "src/main.rs",
    "src/boot.rs",
    "src/boot/",
    "src/adapters/cli.rs",
];

/// 제거할 기존 참조의 한시 목록. 새 위반을 추가해 허용하지 않는다.
const BASELINE_FILES: &[&str] = &[];

/// 테스트 전용 파일과 근거. 부모의 test 조건과 참조가 여전히 남아 있는지 확인한다.
const TEST_ONLY_FILES: &[(&str, &str)] = &[
    (
        "src/adapters/ipc/handler/cli_entry_tests.rs",
        "CLI가 만든 요청을 제품 핸들러에 전달해 검증한다. 사용하는 핸들러·AppState 픽스처가 cfg(test) 전용이라 외부 통합 테스트에서 사용할 수 없다.",
    ),
    (
        "src/adapters/ipc/handler/cli_entry_debug_tests.rs",
        "동일한 요청 검증 중 debug 전용 항목을 분리한 파일이다. 부모에서 all(test, debug_assertions)로 선언해 제품 빌드에서 제외한다.",
    ),
];

/// cfg(test) 또는 all(test, ...) 형태만 인정한다. 모든 cfg 논리식을 계산하지는 않는다.
fn implies_test(gate: &str) -> bool {
    let g: String = gate.chars().filter(|c| !c.is_whitespace()).collect();
    g == "#[cfg(test)]" || (g.starts_with("#[cfg(all(test,") && g.ends_with(")]"))
}

/// 부모 모듈의 test 조건이 제거되면 예외로 제품 참조를 허용하게 되므로 선언을 함께 확인한다.
fn declared_under_cfg_test(rel: &str, root: &Path) -> Result<(), String> {
    let path = Path::new(rel);
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Err(format!("{rel}: 모듈 이름을 뽑을 수 없다"));
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    let candidates = [
        root.join(dir).with_extension("rs"),
        root.join(dir).join("mod.rs"),
    ];
    let Some((parent_rel, src)) = candidates.iter().find_map(|c| {
        std::fs::read_to_string(c)
            .ok()
            .map(|s| (normalized_rel(c, root), s))
    }) else {
        return Err(format!("{rel}: 부모 모듈 파일을 찾지 못했다"));
    };

    let lines: Vec<&str> = src.lines().collect();
    let decl = format!("mod {stem};");
    let Some(i) = lines
        .iter()
        .position(|l| l.trim() == decl || l.trim().ends_with(&format!(" {decl}")))
    else {
        return Err(format!(
            "{rel}: 부모 모듈 `{parent_rel}` 에 `{decl}` 선언이 없다"
        ));
    };
    // 속성과 선언 사이의 빈 줄·줄 주석은 건너뛰고 직전 조건을 확인한다.
    let gate = lines[..i]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty() && !l.trim().starts_with("//"));
    match gate {
        Some(g) if implies_test(g) => Ok(()),
        Some(g) => Err(format!(
            "{rel}: 부모 모듈 `{parent_rel}` 의 `{decl}` 앞이 test 게이트가 아니라 \
             `{}` 다 — 면제의 전제(프로덕션 빌드에 안 들어간다)를 이 가드가 확인할 수 \
             있어야 하므로 `#[cfg(test)]` 또는 `#[cfg(all(test, ...))]` 로 적는다",
            g.trim()
        )),
        None => Err(format!(
            "{rel}: 부모 모듈 `{parent_rel}` 의 `{decl}` 앞에 게이트가 없다 — 면제의 \
             전제(프로덕션 빌드에 안 들어간다)가 깨졌다"
        )),
    }
}

/// src의 대량 수집 누락을 찾는 하한.
const SRC_FLOOR: Floor = Floor {
    min: 420,
    // 같은 수집 대상의 실측값은 공용 SRC_RS에서 가져온다.
    measured: tasty_doc_guards::floored_walk::populations::SRC_RS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::SRC_RS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::SRC_RS.counted_on,
    why_this_gap: "src Rust 파일 수의 대량 누락을 검사한다. 2026-09-25 16c4fdd1c에서 655개 중 깊이 4 이하가 429개였다. 하한 420은 그 이하이므로 얕은 파일만 수집한 경우에는 별도 깊이 검사가 실패한다. 전체 655와의 차이 235를 허용 가능한 손실로 해석하면 안 된다. 얕은 파일 수가 하한 아래로 줄면 두 검사의 역할을 다시 확인한다.",
};

/// src를 첫 성분으로 세는 최소 도달 깊이. 총파일 수만으로 놓치는 얕은 재귀를 찾는다.
/// 2026-09-25 16c4fdd1c에서 655파일 중 깊이 4 이하 429·깊이 5 이상 226·최대깊이 7이었다.
/// 파일 수 하한 420을 얕은 파일 수 이하에 둬 깊이 검사를 별도로 확인할 수 있게 한다.
/// 이 관계가 바뀌면 수집 하한과 깊이 검사를 함께 재검토해야 한다.
const MIN_DEPTH: usize = 5;

/// 순회 확인은 예외 목록의 변경과 독립돼야 한다. 매니페스트가 지정한 바이너리 진입점을 앵커로 사용한다.
const WALK_ANCHOR: &str = "src/main.rs";

fn walk_descends_far_enough(rels: &[String], min_depth: usize) -> Result<(), String> {
    let deepest = rels.iter().map(|r| r.split('/').count()).max().unwrap_or(0);
    if deepest >= min_depth {
        return Ok(());
    }
    Err(format!(
        "src 순회 깊이가 {deepest}로 최소 {min_depth}보다 작다. 재귀가 중간에 멈췄는지 확인한다. 트리가 실제로 얕아졌다면 최대 깊이를 다시 측정해 기준을 정한다."
    ))
}

fn walk_reached_anchor(rels: &[String], anchor: &str) -> Result<(), String> {
    if rels.iter().any(|r| r == anchor) {
        return Ok(());
    }
    Err(format!(
        "순회 결과에 {anchor}가 없다. 바이너리 진입점의 위치와 수집을 확인한다. 진입점이 이동했다면 WALK_ANCHOR를 비우지 말고 새 경로로 갱신한다."
    ))
}

fn is_allowed(rel: &str) -> bool {
    is_allowed_in(rel, ALLOWED_PATHS)
}

/// 합성 명부로도 동일한 경로 매칭을 검증하도록 목록을 인자로 받는다.
fn is_allowed_in(rel: &str, allowed: &[&str]) -> bool {
    allowed.iter().any(|p| {
        if let Some(dir) = p.strip_suffix('/') {
            rel.starts_with(dir) && rel.as_bytes().get(dir.len()) == Some(&b'/')
        } else {
            rel == *p
        }
    })
}

fn is_scan_target(found: &Walked) -> bool {
    found.rel.ends_with(".rs")
}

/// 임의 이름의 CARGO_TARGET_DIR도 제외하도록 빌드 캐시 표식을 사용하는 순회다.
fn walk_src(root: &Path) -> Result<Vec<Walked>, String> {
    walk_src_under(&root.join("src"), root, &SRC_FLOOR)
}

/// 작은 합성 트리도 같은 순회로 확인할 수 있도록 경로·하한을 인자로 받는다.
fn walk_src_under(src_dir: &Path, rel_base: &Path, floor: &Floor) -> Result<Vec<Walked>, String> {
    walk_with_floor(
        src_dir,
        rel_base,
        floor,
        Descend::SkipBuildCaches,
        &is_scan_target,
    )
}

struct Rosters<'a> {
    allowed: &'a [&'a str],
    baseline: &'a [&'a str],
    test_only: &'a [&'a str],
    needle: &'a str,
}

/// 새 위반과 역방향 대조에 쓸 한시 참조·테스트 참조를 함께 반환한다.
struct Routed {
    new_violations: Vec<String>,
    baseline_hit: Vec<String>,
    test_only_hit: Vec<String>,
}

/// 합성 소스에도 같은 분류를 적용한다.
fn route(files: &[Walked], r: &Rosters) -> Routed {
    let mut out = Routed {
        new_violations: Vec::new(),
        baseline_hit: Vec::new(),
        test_only_hit: Vec::new(),
    };
    for file in files {
        let rel = &file.rel;
        if is_allowed_in(rel, r.allowed) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&file.path) else {
            continue; // 현재 검사는 읽지 못한 파일을 제외한다.
        };
        let mut hits = Vec::new();
        for (i, line) in contents.lines().enumerate() {
            if line.contains(r.needle) {
                hits.push(format!("  {}:{} — `{}`", rel, i + 1, line.trim()));
            }
        }
        if hits.is_empty() {
            continue;
        }
        if r.baseline.contains(&rel.as_str()) {
            out.baseline_hit.push(rel.clone());
        } else if r.test_only.contains(&rel.as_str()) {
            out.test_only_hit.push(rel.clone());
        } else {
            out.new_violations.extend(hits);
        }
    }
    out
}

#[test]
fn src_does_not_reference_tasty_cli() {
    let root = &tasty_doc_guards::repo_root();

    // 총수·깊이·특정 파일 도달을 각각 확인해 수집 누락을 찾는다.
    let files = walk_src(root).unwrap_or_else(|why| panic!("{why}"));
    let scanned: Vec<String> = files.iter().map(|f| f.rel.clone()).collect();
    for check in [
        walk_descends_far_enough(&scanned, MIN_DEPTH),
        walk_reached_anchor(&scanned, WALK_ANCHOR),
    ] {
        if let Err(why) = check {
            panic!("{why}");
        }
    }

    // 정책 예외와 겹치면 기존 위반을 제거하도록 요구할 수 없으므로 한시 목록과 분리한다.
    let overlap: Vec<&str> = BASELINE_FILES
        .iter()
        .copied()
        .filter(|f| is_allowed(f))
        .collect();
    assert!(
        overlap.is_empty(),
        "BASELINE_FILES 항목이 ALLOWED_PATHS 에도 걸린다 — 한시 허용과 영구 허용은 \
         분리되어야 한다(겹치면 그 항목은 영원히 비울 수 없다):\n  {}",
        overlap.join("\n  ")
    );

    // 테스트 예외는 제거할 기존 제품 참조가 아니므로 다른 목록과 분리한다.
    let cross: Vec<&str> = TEST_ONLY_FILES
        .iter()
        .map(|(f, _)| *f)
        .filter(|f| is_allowed(f) || BASELINE_FILES.contains(f))
        .collect();
    assert!(
        cross.is_empty(),
        "TEST_ONLY_FILES 항목이 ALLOWED_PATHS/BASELINE_FILES 에도 있다 — 성격이 다른 \
         목록이라 섞으면 안 된다:\n  {}",
        cross.join("\n  ")
    );

    let broken: Vec<String> = TEST_ONLY_FILES
        .iter()
        .filter_map(|(f, _)| declared_under_cfg_test(f, root).err())
        .collect();
    assert!(
        broken.is_empty(),
        "TEST_ONLY_FILES 면제의 전제가 깨졌다 — 면제는 `#[cfg(test)]` 전용 모듈에만 \
         유효하다:\n  {}",
        broken.join("\n  ")
    );

    let test_only: Vec<&str> = TEST_ONLY_FILES.iter().map(|(f, _)| *f).collect();
    let Routed {
        new_violations,
        baseline_hit,
        test_only_hit,
    } = route(
        &files,
        &Rosters {
            allowed: ALLOWED_PATHS,
            baseline: BASELINE_FILES,
            test_only: &test_only,
            needle: FORBIDDEN,
        },
    );

    assert!(
        new_violations.is_empty(),
        "본체 src에서 허용하지 않은 tasty-cli 참조를 찾았다:\n{}\n공통 기능은 양쪽이 사용하는 별도 크레이트에 둔다. BASELINE_FILES는 제거할 기존 위반만 기록하며 새 참조를 허용하지 않는다.",
        new_violations.join("\n")
    );

    let stale: Vec<&str> = BASELINE_FILES
        .iter()
        .copied()
        .filter(|f| !baseline_hit.iter().any(|h| h == f))
        .collect();
    assert!(
        stale.is_empty(),
        "BASELINE_FILES의 참조가 소스에서 사라졌다. 이동·삭제·수정을 확인하고 오래된 항목을 제거한다:\n  {}",
        stale.join("\n  ")
    );

    let stale_test_only: Vec<&str> = TEST_ONLY_FILES
        .iter()
        .map(|(f, _)| *f)
        .filter(|f| !test_only_hit.iter().any(|h| h == f))
        .collect();
    assert!(
        stale_test_only.is_empty(),
        "TEST_ONLY_FILES의 참조가 소스에서 사라졌다. 불필요해진 항목을 제거한다:\n  {}",
        stale_test_only.join("\n  ")
    );
}

#[test]
fn the_cfg_test_precondition_check_discriminates() {
    let root = &tasty_doc_guards::repo_root();

    assert!(
        declared_under_cfg_test("src/adapters/ipc/handler/cli_entry_tests.rs", root).is_ok(),
        "면제 대상이 실제로 `#[cfg(test)]` 아래 있는데 검사가 거부한다"
    );

    let ungated = declared_under_cfg_test("src/adapters/ipc/handler/completion_strategy.rs", root);
    assert!(
        ungated.is_err(),
        "게이트 없는 모듈을 통과시킨다 — 전제 검사가 판별력이 없다"
    );

    assert!(
        declared_under_cfg_test("src/does_not_exist/nope.rs", root).is_err(),
        "부모 모듈을 못 찾았는데 통과시킨다"
    );

    assert!(
        declared_under_cfg_test("src/adapters/ipc/handler/cli_entry_debug_tests.rs", root).is_ok(),
        "게이트가 `#[cfg(test)]` 보다 좁거나 선언 앞에 주석이 있다는 이유로 거부한다"
    );

    for gate in ["#[cfg(test)]", "#[cfg(all(test, debug_assertions))]"] {
        assert!(implies_test(gate), "test 를 함의하는 `{gate}` 를 거부한다");
    }
    for gate in [
        "#[cfg(any(test, debug_assertions))]",
        "#[cfg(debug_assertions)]",
        "#[cfg(all(debug_assertions, feature = \"gui\"))]",
        "#[cfg(not(test))]",
    ] {
        assert!(
            !implies_test(gate),
            "`{gate}` 는 test 없이도 참이 될 수 있는데 면제의 전제로 받는다"
        );
    }
}

/// 정책 예외는 파일의 존재를 확인한다. 현재 일치한 참조가 없다고 필요 없어진 정책으로 보지는 않는다.
#[test]
fn allowed_paths_point_at_paths_that_exist() {
    let root = &tasty_doc_guards::repo_root();
    let missing = tasty_doc_guards::missing_referents(root, ALLOWED_PATHS.iter().copied());
    assert!(
        missing.is_empty(),
        "면제가 없는 경로를 가리킨다 — 옮겼으면 항목도 옮기고, 사라졌으면 항목을 지워라: {missing:?}"
    );
}

/// 실제 수집과 빈 입력을 비교하고 낮은 기준에서는 같은 입력이 통과하는지도 확인한다.
#[test]
fn the_population_checks_separate_a_walked_tree_from_an_empty_one() {
    let root = &tasty_doc_guards::repo_root();

    let walked = walk_src(root).expect("실제 `src/` 순회가 하한에 걸렸다");
    let walked_rels: Vec<String> = walked.iter().map(|f| f.rel.clone()).collect();

    // 없는 경로는 공용 순회가 거부해야 한다. 깊이·앵커 판정에는 빈 목록을 직접 넘긴다.
    let dead = walk_with_floor(
        &root.join("src-no-such-directory"),
        root,
        &SRC_FLOOR,
        Descend::SkipBuildCaches,
        &is_scan_target,
    );
    assert!(
        dead.is_err(),
        "존재하지 않는 루트를 순회했는데 통과했다 — 총량 하한이 안 걸린다"
    );
    let empty_rels: Vec<String> = Vec::new();

    assert!(
        walk_descends_far_enough(&walked_rels, MIN_DEPTH).is_ok(),
        "실제 `src/` 순회를 깊이 판정이 거부한다 — 최소 깊이가 트리보다 깊다"
    );
    assert!(
        walk_descends_far_enough(&empty_rels, MIN_DEPTH).is_err(),
        "빈 순회를 깊이 판정이 통과시킨다 — 깊이 판정에 판별력이 없다"
    );
    assert!(
        walk_reached_anchor(&walked_rels, WALK_ANCHOR).is_ok(),
        "실제 순회가 앵커에 닿았는데 못 닿았다고 한다"
    );
    assert!(
        walk_reached_anchor(&empty_rels, WALK_ANCHOR).is_err(),
        "빈 순회인데 앵커에 닿았다고 한다 — 앵커 확인에 판별력이 없다"
    );

    assert!(
        walk_descends_far_enough(&empty_rels, 0).is_ok(),
        "최소 깊이 0 으로도 빈 순회가 거부된다 — 판정이 깊이 인자를 안 보고 있다"
    );
}

/// 실제 금지 문자열을 독립된 합성 입력에 넣고 네 분류를 비교한다. 기준 상수에서 입력을 만들면 상수 변경을 놓칠 수 있다.
#[test]
fn the_router_reports_a_planted_reference_and_routes_the_rest_away() {
    // 이유: 이 고유 접두어를 쓰는 테스트는 프로세스당 한 번 실행한다.
    // PID로 동시 프로세스를 구분하고 재사용된 PID의 이전 경로는 생성 전에 정리한다.
    let root = std::env::temp_dir().join(format!("tasty-layering-fixture-{}", std::process::id()));
    // 이전 실행 잔여물 제거 — 없으면 `NotFound` 라 실패가 정상 경로다.
    let _ = std::fs::remove_dir_all(&root);
    let src = root.join("src");
    std::fs::create_dir_all(src.join("zone/a/b")).expect("합성 트리를 만들지 못했다");
    std::fs::create_dir_all(src.join("adapters")).expect("합성 트리를 만들지 못했다");

    let write = |rel: &str, body: &str| {
        std::fs::write(src.join(rel), body).unwrap_or_else(|e| panic!("{rel}: {e}"));
    };
    write("main.rs", "fn main() {}\n");
    write("adapters/cli.rs", "pub use tasty_cli::Command;\n");
    write(
        "zone/a/b/leaker.rs",
        "fn f() { let _ = tasty_cli::run(); }\n",
    );
    write("zone/legacy.rs", "use tasty_cli::Legacy;\n");
    write("zone.rs", "#[cfg(test)]\nmod gated;\n\nmod ungated;\n");
    write("zone/gated.rs", "fn t() { let _ = tasty_cli::probe(); }\n");
    write("zone/ungated.rs", "pub fn plain() {}\n");
    write("zone/mentions.rs", "// tasty_cli 는 별도 크레이트다\n");

    // 작은 합성 트리에는 실제 저장소의 파일 수 하한을 적용할 수 없다.
    let floor = Floor {
        min: 4,
        measured: 8,
        measured_on: "2026-09-08",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: "이 합성 트리의 파일 수다. 새 조건을 시험하려고 파일을 \
                       더하는 것은 정상 변경이라 하한을 실제 개수에 맞추면 그 변경마다 실패한다.",
    };
    let files = walk_src_under(&src, &root, &floor).expect("합성 트리 순회가 하한에 걸렸다");
    let rels: Vec<String> = files.iter().map(|f| f.rel.clone()).collect();
    assert!(
        walk_reached_anchor(&rels, "src/main.rs").is_ok(),
        "합성 트리 순회가 앵커에 안 닿았다: {rels:?}"
    );
    assert!(
        walk_descends_far_enough(&rels, 4).is_ok(),
        "합성 트리 순회가 깊이 4 에 못 갔다: {rels:?}"
    );

    let routed = route(
        &files,
        &Rosters {
            allowed: &["src/adapters/cli.rs"],
            baseline: &["src/zone/legacy.rs"],
            test_only: &["src/zone/gated.rs"],
            needle: "tasty_cli::",
        },
    );

    assert_eq!(
        routed.new_violations.len(),
        1,
        "추가한 참조가 보고 대상에 포함되지 않았다: {:#?}",
        routed.new_violations
    );
    let hit = &routed.new_violations[0];
    assert!(
        hit.contains("src/zone/a/b/leaker.rs") && hit.contains(":1"),
        "위반 보고에 경로나 줄번호가 없다: {hit}"
    );

    assert_eq!(
        routed.baseline_hit,
        vec!["src/zone/legacy.rs".to_owned()],
        "한시 허용 참조가 해당 분류에 포함되지 않았다"
    );
    assert_eq!(
        routed.test_only_hit,
        vec!["src/zone/gated.rs".to_owned()],
        "범위 밖 참조가 해당 분류에 포함되지 않았다"
    );
    for bucket in [
        &routed.new_violations,
        &routed.baseline_hit,
        &routed.test_only_hit,
    ] {
        assert!(
            !bucket.iter().any(|h| h.contains("adapters/cli.rs")),
            "제외 경로가 검사 결과에 포함됐다: {bucket:#?}"
        );
        assert!(
            !bucket.iter().any(|h| h.contains("mentions.rs")),
            "`::` 없는 이름 언급을 참조로 셌다: {bucket:#?}"
        );
    }

    assert!(
        declared_under_cfg_test("src/zone/gated.rs", &root).is_ok(),
        "게이트가 붙은 선언을 거부한다"
    );
    let why = declared_under_cfg_test("src/zone/ungated.rs", &root)
        .expect_err("게이트 없는 선언을 통과시켰다");
    assert!(
        why.contains("test 게이트가 아니라"),
        "거부는 했는데 이유가 게이트가 아니다: {why}"
    );

    // 이유: 검사 후 임시 경로 정리는 판정에 영향을 주지 않는다.
    let _ = std::fs::remove_dir_all(&root);
}
