//! 계층 가드 — 본체 GUI/런타임 코드(`src/`)가 `tasty-cli` 크레이트를 직접
//! 참조하면 fail 한다.
//!
//! 배경: `tasty-cli` 는 **바이너리의 진입 계층**(인자 파싱 + 그 파싱 결과로
//! 실행되는 커맨드 구현)이다. GUI 런타임·IPC 핸들러·앱 상태가 그 크레이트
//! 내부를 들여다보면 의존 방향이 뒤집힌다 — 런타임이 CLI 를 소비하는 형태가
//! 되어, CLI 쪽 타입 변경이 GUI 를 깨고 GUI 재사용 목적의 로직이 CLI 안에
//! 눌러앉는다. 재사용되는 코어(ssh / remote browse / stream 등)는 CLI 가
//! 아니라 양쪽이 함께 쓰는 별도 크레이트에 있어야 한다.
//!
//! 이 가드가 없으면 위반이 컴파일 에러로 잡히지 않는다. `src/adapters/cli.rs`
//! 가 `pub use tasty_cli::*;` 로 **와일드카드 재수출**을 하고 있어 본체 어디서든
//! `crate::cli::` 경로로 CLI 크레이트 전체에 닿기 때문이다. 경계를 만드는
//! 리팩터는 가드를 먼저 세워야 이행 중에 새 위반이 안 들어온다.
//!
//! **세 목록은 성격이 다르다 (합치지 말 것)**:
//! - [`ALLOWED_PATHS`] — **영구 허용**. 바이너리가 CLI 파서를 소유하는 정당한
//!   의존(진입점 / boot 경로 / 재수출 지점). 비울 대상이 아니다.
//! - [`BASELINE_FILES`] — **한시 허용**. 이행 중인 기존 위반의 스냅샷.
//!   **줄어들기만 해야 한다.** 실제 위반이 사라지면 목록에서도 지워야 통과한다
//!   (역방향 검사).
//! - [`TEST_ONLY_FILES`] — **범위 밖**. `#[cfg(test)]` 로만 컴파일되는 모듈.
//!   프로덕션 바이너리에 그 참조가 들어가지 않으므로 이 가드가 겨냥하는 의존
//!   방향 역전이 애초에 일어나지 않는다. 이행 대상이 아니라 **성격이 다른 것**이라
//!   베이스라인과 섞지 않는다. 근거
//!   [ADR-0123](../docs/adr/0123-layering-guard-excludes-cfg-test-modules.md).
//!
//! 주석 안의 언급도 위반으로 본다 — 주석이 옛 경로를 가리키면 그것도 실제
//! 오정보이므로 코드와 같이 갱신되어야 한다.
//!
//! 선례: `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`(구조 템플릿) · `tests/no_emoji_in_source.rs`.

use std::path::{Path, PathBuf};
use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};

/// 금지 패턴 — 크레이트 경로 참조. `use` / 타입 위치 / 주석 어디에 있든 잡는다.
const FORBIDDEN: &str = "tasty_cli::";

/// **영구 허용** 경로(repo-relative). `/` 로 끝나면 접두사(디렉토리) 매칭.
///
/// - `src/main.rs`: 바이너리 진입점. CLI 파서를 소유하는 주체다.
/// - `src/boot.rs`, `src/boot/`: 프로세스 기동 경로. 파싱된 커맨드를 분기하고
///   종료 시 CLI 측 집계를 회수하는, 진입점의 연장선이다.
/// - `src/adapters/cli.rs`: 재수출 지점 그 자체. 참조가 여기 한 곳에 모이는 것이
///   목표 상태라, 이 파일은 비울 대상이 아니다.
const ALLOWED_PATHS: &[&str] = &[
    "src/main.rs",
    "src/boot.rs",
    "src/boot/",
    "src/adapters/cli.rs",
];

/// **한시 허용** — 이행 중인 위반의 스냅샷. 줄어들기만 한다.
///
/// **현재 비어 있다.** 재사용 코어는 전부 별도 크레이트로 분리됐다 —
/// ssh 위임은 `tasty-ssh`, 원격 조회/생성은 `tasty-remote`, 클라이언트 IPC
/// 연결은 `tasty_ipc::client`. 본체는 그쪽을 직접 참조한다.
///
/// **새 항목을 추가해서는 안 된다.** 여기에 이름을 적어 통과시키는 것은
/// 위반을 해소한 게 아니라 가드를 끄는 것이다.
const BASELINE_FILES: &[&str] = &[];

/// **범위 밖** — `#[cfg(test)]` 전용 모듈. `(경로, 사유)` 쌍으로 적는다
/// (`crates/tasty-doc-guards/tests/no_todo_file_citation.rs` 의 `ALLOWLIST` 규약).
///
/// 여기 이름을 올리는 것은 위반을 눈감아 주는 것이 아니라 **그 파일이 프로덕션
/// 빌드에 존재하지 않음**을 주장하는 것이다. 그래서 가드는 그 주장을 검사한다 —
/// 부모 모듈이 정말 `#[cfg(test)]` 로 선언하고 있는지, 그리고 목록이 실제보다
/// 넓지 않은지(위반이 사라졌으면 지워야 한다).
const TEST_ONLY_FILES: &[(&str, &str)] = &[(
    "src/adapters/ipc/handler/cli_entry_tests.rs",
    "CLI 가 조립한 params 를 프로덕션 핸들러가 실제로 읽는지 검증한다. tasty 는 lib \
     타깃이 없는 바이너리 크레이트라 tests/ 통합 테스트에서 핸들러·AppState 픽스처에 \
     아예 닿을 수 없다(가시성이 아니라 링크 대상이 없다).",
)];

/// `rel` 이 부모 모듈에서 `#[cfg(test)]` 로 선언돼 있는지 확인한다.
///
/// 면제의 근거가 "프로덕션 빌드에 안 들어간다" 이므로, 누가 `#[cfg(test)]` 를 떼면
/// 면제가 조용히 프로덕션 참조를 허용하게 된다. 그 순간 여기서 떨어져야 한다.
fn declared_under_cfg_test(rel: &str, root: &Path) -> Result<(), String> {
    let path = Path::new(rel);
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Err(format!("{rel}: 모듈 이름을 뽑을 수 없다"));
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    // `foo/bar/baz.rs` 의 부모 모듈은 `foo/bar.rs` 또는 `foo/bar/mod.rs`.
    let candidates = [
        root.join(dir).with_extension("rs"),
        root.join(dir).join("mod.rs"),
    ];
    let Some((parent_rel, src)) = candidates.iter().find_map(|c| {
        std::fs::read_to_string(c)
            .ok()
            .map(|s| (rel_of(c, root), s))
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
    // 선언 바로 앞의 빈 줄 아닌 줄이 cfg(test) 게이트여야 한다.
    let gate = lines[..i].iter().rev().find(|l| !l.trim().is_empty());
    match gate {
        Some(g) if g.trim() == "#[cfg(test)]" => Ok(()),
        Some(g) => Err(format!(
            "{rel}: 부모 모듈 `{parent_rel}` 의 `{decl}` 앞이 `#[cfg(test)]` 가 아니라 \
             `{}` 다 — 면제의 전제(프로덕션 빌드에 안 들어간다)를 이 가드가 확인할 수 \
             있어야 하므로 정확히 `#[cfg(test)]` 형태로 적는다",
            g.trim()
        )),
        None => Err(format!(
            "{rel}: 부모 모듈 `{parent_rel}` 의 `{decl}` 앞에 게이트가 없다 — 면제의 \
             전제(프로덕션 빌드에 안 들어간다)가 깨졌다"
        )),
    }
}

/// 순회가 실제로 트리를 봤음을 보장하는 하한 — 값 하나가 아니라 **무엇의 함수인지**와
/// 함께 선언한다. 이 형태와 그 이유는 `tasty_doc_guards::floored_walk` 에 있다.
const SRC_FLOOR: Floor = Floor {
    min: 300,
    measured: 591,
    measured_on: "2026-09-06",
    why_this_gap: "이 모수는 `src/` 의 `.rs` 개수이고, 그것을 움직이는 것은 주로 크레이트 \
                   분해다 — 한 번에 수십 개가 `crates/` 로 옮겨 가므로 좁은 여유는 정상적인 \
                   이동에 빨개지고, 그러면 사람이 하한을 내리는 습관을 들인다. 절반쯤 벌려 \
                   두고 통째로 비는 사고만 잡는다. 얕고 넓은 순회는 이 값이 아니라 깊이와 \
                   앵커가 막으므로 여유가 넓어도 그쪽은 안 새 나간다.",
};

/// 순회가 닿아야 할 최소 깊이(`src` 를 1 로 센 경로 성분 수).
///
/// [`SRC_FLOOR`] 는 **총량만** 본다 — 재귀가 중간에 멈춰도 얕은 파일만으로 그 하한을
/// 넘길 수 있다. 실측 2026-09-06: 깊이 4 이하가 408 개라 그것만으로 하한 300 을 넘는다.
/// 그 사고를 잡는 것이 이 값이다. 실측 최대 깊이는 6 이고 깊이 5 이상이 183 개다.
const MIN_DEPTH: usize = 5;

/// 순회 도달을 고정하는 앵커. **[`ALLOWED_PATHS`] 와 분리한다 — 물음이 다르다.**
///
/// 저쪽은 "이 파일은 참조해도 되는가"(면제)를 묻고 여기는 "순회가 거기 닿았는가"를
/// 묻는다. 한때 앵커를 `ALLOWED_PATHS` 에서 파생시켰는데, 그러면 **면제를 줄이는 정당한
/// 청소가 순회 확인을 조용히 없앤다**: 실측 2026-09-06 기준 그 목록의 `src/main.rs` 와
/// `src/boot.rs` 는 `tasty_cli::` 참조가 0 건이라, 목록에서 지워도 새 위반이 안 생기고
/// 앵커만 사라진다.
///
/// 이 파일을 고른 것은 이름이 좋아서가 아니라 **구조적으로 불멸**이기 때문이다 — 루트
/// `Cargo.toml` 에 `[[bin]]` 선언이 없으므로 cargo 의 기본 규칙에서 이것이 바이너리
/// 진입점이고, 없으면 크레이트가 빌드되지 않는다.
const WALK_ANCHOR: &str = "src/main.rs";

/// 순회가 충분히 깊이 내려갔는지 판정한다. 하한과 같은 이유로 최소 깊이를 인자로 받는다.
fn walk_descends_far_enough(rels: &[String], min_depth: usize) -> Result<(), String> {
    let deepest = rels.iter().map(|r| r.split('/').count()).max().unwrap_or(0);
    if deepest >= min_depth {
        return Ok(());
    }
    Err(format!(
        "`src/` 순회가 깊이 {deepest} 까지만 내려갔다(최소 {min_depth}) — 총량 하한은 \
         얕고 넓은 순회를 통과시키므로 이것이 따로 필요하다. 재귀가 중간에 멈추지 \
         않았는지 확인하라.\n\
         ★ 이 값을 내려서 통과시키지 마라. 트리가 정말 얕아졌으면 \
         `find src -name '*.rs' | awk -F/ '{{print NF}}' | sort -n | tail -1` 로 실제 \
         최대 깊이를 재고 그보다 한 단계 아래로 잡아라."
    ))
}

/// 앵커가 순회 결과에 나타났는지 판정한다.
fn walk_reached_anchor(rels: &[String], anchor: &str) -> Result<(), String> {
    if rels.iter().any(|r| r == anchor) {
        return Ok(());
    }
    Err(format!(
        "순회가 `{anchor}` 에 닿지 않았다 — 그 파일은 이 크레이트의 바이너리 진입점이라 \
         실재가 보장된다. 순회 결과에 없으면 그 가지를 통째로 못 본 것이다.\n\
         ★ 이 앵커를 지워서 통과시키지 마라 — 순회 확인이 통째로 사라진다. 진입점이 \
         정말 옮겨졌으면 `WALK_ANCHOR` 를 새 진입점으로 **바꿔라**(비우지 마라)."
    ))
}

fn is_allowed(rel: &str) -> bool {
    ALLOWED_PATHS.iter().any(|p| {
        if let Some(dir) = p.strip_suffix('/') {
            rel.starts_with(dir) && rel.as_bytes().get(dir.len()) == Some(&b'/')
        } else {
            rel == *p
        }
    })
}

/// 스캔 대상인지 — `.rs` 파일 하나.
fn is_scan_target(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("rs")
}

/// `src/` 를 순회한다. 가지치기는 이름이 아니라 **성질**로 한다 — 빌드 산출물 디렉토리의
/// 이름은 `CARGO_TARGET_DIR` 하나로 무엇이든 될 수 있어서 이름 목록은 그것을 못 따라간다.
/// 하한은 공용 순회가 강제하므로 여기서 빠뜨릴 수 없다.
fn walk_src(root: &Path) -> Result<Vec<PathBuf>, String> {
    walk_with_floor(
        &root.join("src"),
        &SRC_FLOOR,
        Descend::SkipBuildCaches,
        &is_scan_target,
    )
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn src_does_not_reference_tasty_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // 아래 판정들은 전부 "순회가 모은 것" 위에서 돌아간다. 그 순회가 비면 모든
    // 판정이 조용히 통과한다 — 그러니 위반을 세기 전에 인구를 먼저 확인한다.
    // 셋이 서로 다른 사고를 잡는다: 총량(빈 순회) · 깊이(중간에 멈춘 재귀) ·
    // 앵커(특정 가지 누락). 총량은 공용 순회가 자기 실패문과 함께 본다.
    let files = walk_src(root).unwrap_or_else(|why| panic!("{why}"));
    let scanned: Vec<String> = files.iter().map(|f| rel_of(f, root)).collect();
    for check in [
        walk_descends_far_enough(&scanned, MIN_DEPTH),
        walk_reached_anchor(&scanned, WALK_ANCHOR),
    ] {
        if let Err(why) = check {
            panic!("{why}");
        }
    }

    // 두 목록이 겹치면 "베이스라인을 비운다" 가 성립하지 않는다 — 영구 허용
    // 항목은 실제 위반이 남아 있어도 역방향 검사에 걸리지 않기 때문이다.
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

    // 세 목록은 성격이 다르므로 서로 겹치면 안 된다. 특히 테스트 전용 면제가
    // BASELINE_FILES 에 섞이면 "줄어들기만 하는 이행 스냅샷" 이라는 그 목록의
    // 의미가 거짓이 된다(테스트 모듈은 없앨 대상이 아니다).
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

    // 면제의 전제를 실제로 검사한다 — 부모 모듈의 `#[cfg(test)]` 게이트.
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

    let mut new_violations = Vec::new();
    let mut baseline_hit = Vec::new();
    let mut test_only_hit = Vec::new();
    for file in files {
        let rel = rel_of(&file, root);
        if is_allowed(&rel) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&file) else {
            continue; // 비-UTF8 은 경로 참조를 담을 수 없다.
        };
        let mut hits = Vec::new();
        for (i, line) in contents.lines().enumerate() {
            if line.contains(FORBIDDEN) {
                hits.push(format!("  {}:{} — `{}`", rel, i + 1, line.trim()));
            }
        }
        if hits.is_empty() {
            continue;
        }
        if BASELINE_FILES.contains(&rel.as_str()) {
            baseline_hit.push(rel);
        } else if TEST_ONLY_FILES.iter().any(|(f, _)| *f == rel) {
            test_only_hit.push(rel);
        } else {
            new_violations.extend(hits);
        }
    }

    assert!(
        new_violations.is_empty(),
        "본체(`src/`)가 `tasty-cli` 크레이트를 직접 참조한다 — 의존 방향이 뒤집힌다.\n\
         재사용이 필요한 코어(ssh / remote browse / stream 등)는 CLI 가 아니라 양쪽이 \
         함께 쓰는 별도 크레이트에 두고, 본체는 그쪽을 참조할 것.\n\
         BASELINE_FILES 는 이행 중인 기존 위반의 스냅샷이라 **줄어들기만 해야 한다** — \
         새 항목 추가 금지.\n{}",
        new_violations.join("\n")
    );

    // 역방향 — 베이스라인이 실제보다 넓으면 위반이 사라져도 가드가 느슨한 채로 남는다.
    let stale: Vec<&str> = BASELINE_FILES
        .iter()
        .copied()
        .filter(|f| !baseline_hit.iter().any(|h| h == f))
        .collect();
    assert!(
        stale.is_empty(),
        "BASELINE_FILES 에 있으나 실제 위반이 없다 — 참조를 걷어냈으면 목록에서도 지울 것 \
         (남겨두면 그 파일에 위반이 다시 들어와도 통과한다). 파일이 사라졌거나 이름이 \
         바뀐 경우도 같다:\n  {}",
        stale.join("\n  ")
    );

    // 역방향 — 면제 목록도 실제보다 넓으면 안 된다. 참조가 사라졌거나 파일이
    // 없어졌으면 목록에서도 지워야 통과한다(BASELINE_FILES 와 같은 대우).
    let stale_test_only: Vec<&str> = TEST_ONLY_FILES
        .iter()
        .map(|(f, _)| *f)
        .filter(|f| !test_only_hit.iter().any(|h| h == f))
        .collect();
    assert!(
        stale_test_only.is_empty(),
        "TEST_ONLY_FILES 에 있으나 실제 참조가 없다 — 면제가 필요 없어졌으면 목록에서도 \
         지울 것(남겨두면 그 파일이 나중에 무엇을 참조해도 통과한다):\n  {}",
        stale_test_only.join("\n  ")
    );
}

/// 전제 검사가 **판별력이 있는지** 고정한다.
///
/// 면제 목록의 값은 "이 파일은 프로덕션 빌드에 안 들어간다" 는 주장이고, 그 주장을
/// 검사하는 것이 `declared_under_cfg_test` 다. 검사가 아무거나 통과시키면 면제가
/// 그냥 예외 목록이 된다 — 그래서 게이트가 없는 실제 형제 모듈로 반대편을 고정한다.
#[test]
fn the_cfg_test_precondition_check_discriminates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // 게이트가 있는 실제 면제 대상 → Ok.
    assert!(
        declared_under_cfg_test("src/adapters/ipc/handler/cli_entry_tests.rs", root).is_ok(),
        "면제 대상이 실제로 `#[cfg(test)]` 아래 있는데 검사가 거부한다"
    );

    // 같은 부모의 게이트 **없는** 형제 모듈 → Err. 검사가 선언 앞을 실제로 읽는다는
    // 뜻이다(파일 존재 여부나 이름만 보는 게 아니다).
    let ungated = declared_under_cfg_test("src/adapters/ipc/handler/completion_strategy.rs", root);
    assert!(
        ungated.is_err(),
        "게이트 없는 모듈을 통과시킨다 — 전제 검사가 판별력이 없다"
    );

    // 부모 모듈이 아예 없는 경로 → Err (조용한 통과 금지).
    assert!(
        declared_under_cfg_test("src/does_not_exist/nope.rs", root).is_err(),
        "부모 모듈을 못 찾았는데 통과시킨다"
    );
}

/// 면제가 가리키는 경로가 **실재하는가** — 참조 무결성.
///
/// **초록은 "이 면제가 아직 필요하다" 가 아니다**(ADR-0150). 가리키는 것이 실재한다는
/// 것뿐이고, 실재해도 그 면제가 아무것도 안 덮고 있을 수 있다. 두 축을 섞으면 "안 덮으면
/// 지워라" 라는 틀린 처방이 참조 무결성의 옷을 입고 돌아온다.
///
/// 경로가 썩으면 면제는 조용히 아무 일도 안 하게 되는데, 목록에는 "여기는 원래 위반해도
/// 된다" 는 신호가 남는다. 판정과 그 양극성 회귀는 [`tasty_doc_guards::missing_referents`].
#[test]
fn allowed_paths_point_at_paths_that_exist() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let missing = tasty_doc_guards::missing_referents(root, ALLOWED_PATHS.iter().copied());
    assert!(
        missing.is_empty(),
        "면제가 없는 경로를 가리킨다 — 옮겼으면 항목도 옮기고, 사라졌으면 항목을 지워라: {missing:?}"
    );
}

/// 인구 확인 셋이 **판별력이 있는지**, 그리고 **무엇 때문에 판별하는지** 고정한다.
///
/// 판정을 `assert!` 하나씩으로만 두면 초록일 때 그것들이 무엇을 걸러냈는지 안 보인다 —
/// 아무거나 통과시키는 판정도 똑같이 초록이다. 그래서 세 판정기를 실제 트리와 **빈 순회**
/// 양쪽에 걸어 갈래를 둘 다 태운다. 이 파일의 기존 대조
/// [`the_cfg_test_precondition_check_discriminates`] 와 같은 형태다.
///
/// **그리고 대조군 자신도 잰다.** 대조를 두었다는 것이 그 대조가 작동한다는 뜻은 아니다.
/// 여기서는 각 판정기를 **무력한 값**으로도 불러, 그 판정의 전부가 상수라는 것을 코드가
/// 말하게 한다 — 상수를 내리는 것이 곧 판정을 끄는 것이라는 사실을 산문이 아니라 실행으로
/// 고정하는 것이다. 그래야 실패문에 박은 금지가 근거를 갖는다.
#[test]
fn the_population_checks_separate_a_walked_tree_from_an_empty_one() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let walked = walk_src(root).expect("실제 `src/` 순회가 하한에 걸렸다");
    let walked_rels: Vec<String> = walked.iter().map(|f| rel_of(f, root)).collect();

    // 반대편은 존재하지 않는 루트다. 순회는 `read_dir` 실패를 삼키고 빈 목록을 만드는데,
    // 그것이 바로 이 판정들이 겨냥하는 사고의 형태다. 총량은 공용 순회가 막으므로
    // 여기서는 그것이 실제로 막는지만 확인하고, 깊이·앵커는 빈 목록으로 따로 잰다.
    let dead = walk_with_floor(
        &root.join("src-no-such-directory"),
        &SRC_FLOOR,
        Descend::SkipBuildCaches,
        &is_scan_target,
    );
    assert!(
        dead.is_err(),
        "존재하지 않는 루트를 순회했는데 통과했다 — 총량 하한이 안 걸린다"
    );
    let empty_rels: Vec<String> = Vec::new();

    // --- 갈래 둘: 실제 트리는 통과하고 빈 순회는 거부된다 ---
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

    // --- 대조군 자신: 판정력이 어디서 오는가 ---
    // 무력한 값으로 부르면 같은 빈 순회가 통과한다. 즉 이 판정들의 전부가 그 상수이고,
    // 상수를 내리는 것은 판정을 끄는 것과 같다. 실패문의 금지는 이 사실에 근거한다.
    assert!(
        walk_descends_far_enough(&empty_rels, 0).is_ok(),
        "최소 깊이 0 으로도 빈 순회가 거부된다 — 판정이 깊이 인자를 안 보고 있다"
    );
}
