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
//! 선례: `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`(구조 템플릿) · `crates/tasty-doc-guards/tests/no_emoji_in_source.rs`.

//! 뿌리는 `tasty_doc_guards::repo_root()` 로 얻는다. `CARGO_MANIFEST_DIR` 을 그대로 쓰면
//! 그 값이 이 타깃이 사는 패키지를 가리켜, **스캔 뿌리가 파일과 함께 움직인다** — 이 파일이
//! 루트 `tests/` 에서 이 크레이트로 옮겨오며 실제로 그럴 뻔했다. `repo_root()` 는 표지
//! 파일로 자기가 잡은 경로를 검증한다.

// 이유: 이 파일은 합성 트리를 만들어 라우팅을 재는 양성 대조를 갖는다. 그 정리
// 코드(`let _ = remove_dir_all`)는 실패해도 할 일이 없다 — 이전 실행 잔여물이 없으면
// `NotFound` 가 정상 경로다. 그리고 전수 가드
// (`crates/tasty-doc-guards/tests/let_underscore_documented.rs`)는 테스트 본문을
// 제외하므로 여기서 나는 경고는 정책상 조치 대상이 아니다 —
// `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

use std::path::Path;
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, normalized_rel, walk_with_floor};

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
const TEST_ONLY_FILES: &[(&str, &str)] = &[
    (
        "src/adapters/ipc/handler/cli_entry_tests.rs",
        "CLI 가 조립한 params 를 프로덕션 핸들러가 실제로 읽는지 검증한다. tasty 는 lib \
         타깃이 없는 바이너리 크레이트라 tests/ 통합 테스트에서 핸들러·AppState 픽스처에 \
         아예 닿을 수 없다(가시성이 아니라 링크 대상이 없다).",
    ),
    (
        "src/adapters/ipc/handler/cli_entry_debug_tests.rs",
        "위 형제와 재는 것이 같고, 갈린 이유는 배치 규율이다 — debug 로 게이트된 항목은 \
         `mod` 선언에 cfg 가 붙은 파일에 모은다. 그래서 이 파일의 게이트는 \
         `#[cfg(all(test, debug_assertions))]` 이고 형제보다 **좁다**.",
    ),
];

/// 이 cfg 속성이 **`test` 없이는 참이 될 수 없는가**.
///
/// 면제의 근거는 "프로덕션 빌드에 안 들어간다" 이므로 `#[cfg(test)]` 보다 **좁은** 것은
/// 함께 받아야 한다 — `#[cfg(all(test, debug_assertions))]` 은 test 를 함의하므로 전제를
/// 더 강하게 만족한다. 반대로 `any(...)` 는 받지 않는다: `test` 가 거짓인 갈래로도 참이
/// 될 수 있어 그 순간 프로덕션 참조를 허용하게 된다. 그래서 판정은 "test 가 들어 있는가"
/// 가 아니라 **"test 없이 참이 될 수 있는가"** 다.
fn implies_test(gate: &str) -> bool {
    let g: String = gate.chars().filter(|c| !c.is_whitespace()).collect();
    g == "#[cfg(test)]" || (g.starts_with("#[cfg(all(test,") && g.ends_with(")]"))
}

/// `rel` 이 부모 모듈에서 test 게이트로 선언돼 있는지 확인한다.
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
    // 선언 바로 앞의 빈 줄·줄주석 아닌 줄이 cfg(test) 게이트여야 한다. 줄주석을 건너뛰는
    // 것은 속성과 항목 사이에 설명을 두는 흔한 형태를 살리기 위해서다 — 그 자리에서
    // 멈추면 주석을 단 순간 면제의 전제가 깨진 것처럼 읽힌다.
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

/// 순회가 실제로 트리를 봤음을 보장하는 하한 — 값 하나가 아니라 **무엇의 함수인지**와
/// 함께 선언한다. 이 형태와 그 이유는 `tasty_doc_guards::floored_walk` 에 있다.
const SRC_FLOOR: Floor = Floor {
    min: 420,
    // 좌변의 사실은 `tasty_doc_guards::floored_walk::populations::SRC_RS` 하나가 갖는다 — 같은 모수를 재는 자리가 이
    // 파일 말고 하나 더 있고, 값을 각자 적어 두었더니 591 과 598 로 갈려 있었다.
    measured: tasty_doc_guards::floored_walk::populations::SRC_RS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::SRC_RS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::SRC_RS.counted_on,
    why_this_gap: "이 모수는 `src/` 의 `.rs` 개수다. 이 자리의 여유는 움직임이 아니라 \
                   **계기 사이의 경계**가 정한다. 이 가드에는 하한 말고 깊이 하한과 앵커가 \
                   있고, 깊이 하한이 맡는 사고는 '재귀가 중간에 멈춰 얕은 파일만 모인 \
                   것' 이다. 실측 2026-09-08(`eea7530d2`): 깊이 4 이하가 **420** 개다. \
                   하한을 그보다 높이면 그 절단을 하한이 먼저 잡아 버려 **깊이 하한이 \
                   한 번도 안 물린다** — 안 걸리는 술어는 없는 술어보다 나쁘다. 그래서 \
                   하한을 정확히 420 에 둔다: 깊이 절단은 깊이 하한이, 그보다 큰 대량 \
                   손실은 이 하한이 잡는다. 이 수는 늘기만 해 왔으므로(같은 창에서 감소 \
                   사건 13 건의 최대가 9) 하한이 그 아래 있는 성질은 유지된다. ★ 여유 185 \
                   는 견딜 폭이 아니라 **다른 계기가 맡은 구간**이다 — 그 구간이 비면 이 \
                   문장이 거짓이 되고, 그때는 여유가 아니라 계기 배치를 다시 봐야 한다.",
};

/// 순회가 닿아야 할 최소 깊이(`src` 를 1 로 센 경로 성분 수).
///
/// [`SRC_FLOOR`] 는 **총량만** 본다 — 재귀가 중간에 멈춰도 얕은 파일만으로 그 하한에
/// 닿을 수 있다. 그 사고를 잡는 것이 이 값이고, **[`SRC_FLOOR`] 의 하한이 그러라고 그
/// 수에 맞춰져 있다.**
///
/// 실측 2026-09-08(`ae61c8521`): 깊이 4 이하가 **420** 개, 깊이 5 이상이 185 개, 최대
/// 깊이는 6 이다. `SRC_FLOOR.min` 이 정확히 420 이라, 재귀가 깊이 4 에서 멈춘 순회는
/// 하한을 **아슬아슬하게 통과하고**(420 ≥ 420) 여기서 걸린다. 하한을 421 로 올리면 그
/// 절단을 하한이 먼저 잡아 **이 값이 한 번도 안 물린다** — 안 걸리는 술어는 없는 술어보다
/// 나쁘다.
///
/// 그래서 이 두 상수는 각자 고를 수 있는 값이 아니라 **한 쌍**이다. 이 문장이 거짓이
/// 되는 값이 있다: 깊이 4 이하가 `SRC_FLOOR.min` 아래로 내려가거나 그 하한이 깊이 4
/// 이하의 수를 넘으면, 역할 분담이 깨진 것이므로 여유가 아니라 계기 배치를 다시 봐야
/// 한다.
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

/// 생산 명부로 묻는다. 명부를 공급하는 것이 이 함수의 일이고, 판정 자체는
/// [`is_allowed_in`] 이 한다 — 그래야 대체 명부로도 같은 판정을 태울 수 있다.
fn is_allowed(rel: &str) -> bool {
    is_allowed_in(rel, ALLOWED_PATHS)
}

/// 면제 판정. 명부를 **인자로** 받는다 — 상수에서 읽으면 이 술어는 생산 트리 밖에서
/// 한 번도 못 돌고, 그러면 접두 매칭이 죽어도 위반 0 이 그대로 초록으로 나간다.
fn is_allowed_in(rel: &str, allowed: &[&str]) -> bool {
    allowed.iter().any(|p| {
        if let Some(dir) = p.strip_suffix('/') {
            rel.starts_with(dir) && rel.as_bytes().get(dir.len()) == Some(&b'/')
        } else {
            rel == *p
        }
    })
}

/// 스캔 대상인지 — `.rs` 파일 하나.
fn is_scan_target(found: &Walked) -> bool {
    found.rel.ends_with(".rs")
}

/// `src/` 를 순회한다. 가지치기는 이름이 아니라 **성질**로 한다 — 빌드 산출물 디렉토리의
/// 이름은 `CARGO_TARGET_DIR` 하나로 무엇이든 될 수 있어서 이름 목록은 그것을 못 따라간다.
/// 하한은 공용 순회가 강제하므로 여기서 빠뜨릴 수 없다.
fn walk_src(root: &Path) -> Result<Vec<Walked>, String> {
    walk_src_under(&root.join("src"), root, &SRC_FLOOR)
}

/// 순회 자체. 뿌리와 하한을 **인자로** 받는다 — 둘 다 모수의 성질이지 이 함수의
/// 성질이 아니고, 상수로 박아 두면 이 순회는 `src/` 605 개짜리 트리에서만 돌 수 있다.
fn walk_src_under(src_dir: &Path, rel_base: &Path, floor: &Floor) -> Result<Vec<Walked>, String> {
    walk_with_floor(
        src_dir,
        rel_base,
        floor,
        Descend::SkipBuildCaches,
        &is_scan_target,
    )
}

/// 라우팅에 필요한 네 값. 상수로 읽지 않고 한 묶음으로 **받는다** — 이 가드가
/// 가르는 것은 네 갈래(면제 · 한시 허용 · 범위 밖 · 위반)이고, 그 갈래를 정하는 것이
/// 전부 이 넷이기 때문이다.
struct Rosters<'a> {
    allowed: &'a [&'a str],
    baseline: &'a [&'a str],
    test_only: &'a [&'a str],
    needle: &'a str,
}

/// 라우팅 결과. 세 갈래를 함께 낸다 — 위반은 앞으로, 나머지 둘은 **역방향 검사**
/// (명부가 실제보다 넓지 않은가)로 쓰인다.
struct Routed {
    new_violations: Vec<String>,
    baseline_hit: Vec<String>,
    test_only_hit: Vec<String>,
}

/// 순회가 모은 파일을 네 갈래로 가른다.
///
/// **이 함수가 이 가드의 전부다.** 시험 본문에 있을 때는 생산 트리로만 부를 수 있었고,
/// 생산 트리에서 위반은 0 이라 **보고 갈래(`new_violations`)에 오늘 입력이 하나도 없다**
/// (실측 2026-09-08, 트리 `b134d28e3`: `src/` 605 개 중 `tasty_cli::` 를 담은 파일 3 —
/// 면제 1 · 범위 밖 2 · 위반 0). 인자로 받게 만드는 것은 그 갈래에 입력을 넣을 방법을
/// 여는 일이다.
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
            continue; // 비-UTF8 은 경로 참조를 담을 수 없다.
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

    // 아래 판정들은 전부 "순회가 모은 것" 위에서 돌아간다. 그 순회가 비면 모든
    // 판정이 조용히 통과한다 — 그러니 위반을 세기 전에 인구를 먼저 확인한다.
    // 셋이 서로 다른 사고를 잡는다: 총량(빈 순회) · 깊이(중간에 멈춘 재귀) ·
    // 앵커(특정 가지 누락). 총량은 공용 순회가 자기 실패문과 함께 본다.
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
    let root = &tasty_doc_guards::repo_root();

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

    // 선언 앞에 **줄주석**이 낀 실제 대상 → Ok. 게이트는 `#[cfg(all(test,
    // debug_assertions))]` 라 `#[cfg(test)]` 보다 좁고, 그 사이에 설명 주석이 한 줄 있다.
    // 이 둘 중 하나라도 못 넘기면 위 목록의 두 번째 항목이 거짓 실패를 낸다.
    assert!(
        declared_under_cfg_test("src/adapters/ipc/handler/cli_entry_debug_tests.rs", root).is_ok(),
        "게이트가 `#[cfg(test)]` 보다 좁거나 선언 앞에 주석이 있다는 이유로 거부한다"
    );

    // 게이트 판정 자체의 양극성 — 실제 트리에 없는 형태까지 여기서 고정한다. 넓히는
    // 방향(`all`)은 받고 **좁히지 않는 방향(`any`)은 받으면 안 된다**: `any` 는 `test` 가
    // 거짓인 갈래로도 참이 되어 면제의 전제를 그 순간 잃는다.
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
    let root = &tasty_doc_guards::repo_root();
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
    let root = &tasty_doc_guards::repo_root();

    let walked = walk_src(root).expect("실제 `src/` 순회가 하한에 걸렸다");
    let walked_rels: Vec<String> = walked.iter().map(|f| f.rel.clone()).collect();

    // 반대편은 존재하지 않는 루트다. 순회는 `read_dir` 실패를 삼키고 빈 목록을 만드는데,
    // 그것이 바로 이 판정들이 겨냥하는 사고의 형태다. 총량은 공용 순회가 막으므로
    // 여기서는 그것이 실제로 막는지만 확인하고, 깊이·앵커는 빈 목록으로 따로 잰다.
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

/// [`route`] 의 **보고 갈래**에 처음으로 입력을 넣는다 — 이 파일에서 생산 트리로는
/// 태울 수 없는 유일한 갈래다.
///
/// **왜 이것이 필요한지는 실측이다.** 트리 `b134d28e3` 기준 생산 `src/` 605 개 중
/// `tasty_cli::` 를 담은 파일은 3 이고 갈래는 면제 1 · 범위 밖 2 · **위반 0** 이다.
/// 그 상태에서 `route` 의 `else { new_violations.extend(hits) }` 를 통째로 비우고
/// 돌리면 이 파일의 네 시험이 **전부 통과한다**(실측 2026-09-08: rc=0 · 4 passed).
/// 이 가드의 존재 이유인 그 한 갈래를 지키는 것이 아무것도 없었다.
///
/// 반대 방향은 이미 지켜지고 있다 — `is_allowed` 를 항상 참으로 만들면 `test_only_hit`
/// 이 비어 아래 `src_does_not_reference_tasty_cli` 의 역방향 검사가 터진다. 그래서
/// 이 픽스처가 새로 얻는 것은 **보고 갈래 하나**이고, 그 크기를 과장하지 않는다.
///
/// ★ 심는 문자열 `tasty_cli::` 는 **리터럴로 쓴다.** [`FORBIDDEN`] 을 참조하면 그 상수를
/// 무엇으로 바꿔도 이 픽스처가 초록이라 동어반복이 된다(R1078). 이 이름은 가드 밖에서
/// 온다 — `crates/tasty-cli/Cargo.toml` 의 패키지 이름이다. 명부 셋도 마찬가지로
/// [`ALLOWED_PATHS`]·[`TEST_ONLY_FILES`] 를 안 읽고 이 시험이 직접 짓는다.
#[test]
fn the_router_reports_a_planted_reference_and_routes_the_rest_away() {
    // 이유: 이 자리는 **프로세스당 한 번만** 불린다. cargo 시험 하네스는 `#[test]` 를
    //       한 프로세스에서 한 번 돌리고, 이 접두를 짓는 자리는 이 바이너리에 하나뿐이다
    //       (이 파일에서 임시 경로를 짓는 자리 1 · `#[test]` 5). 그래서 같은 프로세스의
    //       재호출이 없고, pid 가 지는 축(프로세스 간)이 이 자리에 필요한 축의 전부다.
    //       아래 `remove_dir_all` 이 지우는 것은 앞 호출의 트리가 아니라 **pid 가
    //       재사용된 옛 프로세스의 잔재**다 — 그것은 단조 카운터로도 안 없어진다.
    let root = std::env::temp_dir().join(format!("tasty-layering-fixture-{}", std::process::id()));
    // 이전 실행 잔여물 제거 — 없으면 `NotFound` 라 실패가 정상 경로다.
    let _ = std::fs::remove_dir_all(&root);
    let src = root.join("src");
    std::fs::create_dir_all(src.join("zone/a/b")).expect("합성 트리를 만들지 못했다");
    std::fs::create_dir_all(src.join("adapters")).expect("합성 트리를 만들지 못했다");

    let write = |rel: &str, body: &str| {
        std::fs::write(src.join(rel), body).unwrap_or_else(|e| panic!("{rel}: {e}"));
    };
    // 앵커 — 순회가 뿌리에 닿았는가.
    write("main.rs", "fn main() {}\n");
    // 면제 경로. 참조를 담지만 보고되면 안 된다.
    write("adapters/cli.rs", "pub use tasty_cli::Command;\n");
    // ★ 보고돼야 하는 자리. 깊이도 함께 준다(재귀가 멈추면 이것부터 사라진다).
    write(
        "zone/a/b/leaker.rs",
        "fn f() { let _ = tasty_cli::run(); }\n",
    );
    // 한시 허용 — 오늘 생산 트리에서 `BASELINE_FILES` 가 비어 있어 입력이 0 인 갈래다.
    write("zone/legacy.rs", "use tasty_cli::Legacy;\n");
    // 범위 밖 — 부모가 test 게이트로 선언한다. 게이트 없는 형제를 같이 둔다.
    write("zone.rs", "#[cfg(test)]\nmod gated;\n\nmod ungated;\n");
    write("zone/gated.rs", "fn t() { let _ = tasty_cli::probe(); }\n");
    write("zone/ungated.rs", "pub fn plain() {}\n");
    // 미끼 — `::` 가 없으면 참조가 아니다. 이름만 나오는 산문은 안 센다.
    write("zone/mentions.rs", "// tasty_cli 는 별도 크레이트다\n");

    // 하한도 뿌리도 인자다. 여기 값은 **이 합성 트리의 성질**이지 생산 트리의 것이
    // 아니다 — `SRC_FLOOR` 를 그대로 쓰면 8 개짜리 트리가 하한 300 에 걸린다.
    let floor = Floor {
        min: 4,
        measured: 8,
        measured_on: "2026-09-08",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: "이 합성 트리의 파일 수다. 갈래를 하나 더 시험하려고 파일을 \
                       더하는 것은 정상 변경이라 실측에 붙이면 그때마다 빨개진다.",
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

    // ★ 오늘 생산 트리가 못 태우는 갈래 — 위반 하나가 좌표와 함께 나와야 한다.
    assert_eq!(
        routed.new_violations.len(),
        1,
        "심은 참조가 보고 갈래로 안 갔다: {:#?}",
        routed.new_violations
    );
    let hit = &routed.new_violations[0];
    assert!(
        hit.contains("src/zone/a/b/leaker.rs") && hit.contains(":1"),
        "위반 보고에 경로나 줄번호가 없다: {hit}"
    );

    // 나머지 세 갈래 — 면제는 사라지고, 한시 허용과 범위 밖은 각자 자리로 간다.
    assert_eq!(
        routed.baseline_hit,
        vec!["src/zone/legacy.rs".to_owned()],
        "한시 허용이 제 갈래로 안 갔다"
    );
    assert_eq!(
        routed.test_only_hit,
        vec!["src/zone/gated.rs".to_owned()],
        "범위 밖이 제 갈래로 안 갔다"
    );
    for bucket in [
        &routed.new_violations,
        &routed.baseline_hit,
        &routed.test_only_hit,
    ] {
        assert!(
            !bucket.iter().any(|h| h.contains("adapters/cli.rs")),
            "면제 경로가 어느 갈래로든 새어 나왔다: {bucket:#?}"
        );
        assert!(
            !bucket.iter().any(|h| h.contains("mentions.rs")),
            "`::` 없는 이름 언급을 참조로 셌다: {bucket:#?}"
        );
    }

    // 면제의 **전제**도 합성 트리에서 잰다. 생산 트리에서만 재면 이 판독기는 부모가
    // `<디렉토리>.rs` 인 한 가지 배치밖에 못 본다.
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

    // 정리 — 다음 완주가 이전 잔여물을 읽지 않게 한다. 실패해도 판정과 무관하다.
    let _ = std::fs::remove_dir_all(&root);
}
