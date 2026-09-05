//! 소스 이모지 재유입 가드 — 코드/매니페스트에 픽토그래픽 이모지가 다시 박히면 fail 한다.
//!
//! 배경: 플러그인 매니페스트(`tasty-plugin.toml`)의 `icon` 필드에 오프-디자인 이모지
//! (📝🌐🖼️ 등)가 손으로 박혀 있었다. 프로젝트 원칙은 "아이콘은 디자인 폴더
//! (`icons.json`)의 SVG 라인아이콘에서 추출해 쓴다" 이므로 이모지 플레이스홀더는
//! 위반이다. 이 가드는 정리 결과를 유지한다 — 누가 `.rs`/`tasty-plugin.toml` 에 이모지를
//! 다시 넣으면 `cargo test --workspace` 에서 fail 하고 (기본 조합의 그 잡은 수동 전용이고
//! 자동 실행은 `check-headless` 잡에서만 일어난다 — `docs/dev-guide/ci-gates.md`)
//! `파일:라인 + 코드포인트(U+XXXX) + 문자` 를 출력한다. 선례: `tests/design_token_adherence.rs`.
//!
//! **금지 범위(false positive 0 으로 좁힘)**: 픽토그래픽 이모지 대부분
//! (`U+1F000..=1FAFF`) + regional indicator=국기(`U+1F1E6..=1F1FF`). 이 범위가 소스의
//! 진짜 이모지(📝🌐🖼️😀🦀👨👩👧👦🇰🇷👍📁📂📄)를 전부 잡고, `⚠`(26A0)·`→`(2192)·
//! `▶`(25B6)·`⌘`(2318)·`✓`(2713)·`▦`(25A6) 같은 **의도적 텍스트 기호**는 안 잡는다.
//!
//! **allowlist(파일 단위)**: 이모지가 테스트 입력 자체인 소수 파일만 스캔에서 제외한다.

use std::path::{Path, PathBuf};

/// 스캔에서 제외할 파일(repo-relative) — 이모지가 테스트의 본질이라 제거하면 검증이 무의미해지는 곳.
/// - `tasty-memory/src/scope.rs`: "이모지 키 거부" 단위테스트(😀 가 입력).
/// - `tasty-terminal/src/disk_scrollback.rs`: 터미널 셀 이모지 렌더 테스트(🦀).
/// - `tasty-terminal/tests/scrollback_capture.rs`: 이모지 폭/ZWJ/flag/skin-tone 캡처 테스트.
const ALLOWLIST_FILES: &[&str] = &[
    "crates/tasty-memory/src/scope.rs",
    "crates/tasty-terminal/src/disk_scrollback.rs",
    "crates/tasty-terminal/tests/scrollback_capture.rs",
];

/// 순회에서 통째로 가지치기할 디렉토리명. 빌드 산출물·워크트리·VCS·의존성.
const PRUNE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// gitignored 로컬 폴더 이름의 조각. 리터럴로 두면 이 파일이 비-git 경로 참조 금지
/// (`docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md`) 를 어긴다 — 인용이
/// 아니라 순회 입력이지만, 조각으로 조립하면 예외 등록 없이 규칙을 지킬 수 있다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

/// 가지치기 대상 디렉토리인지 — 빌드 산출물 + gitignored 로컬 폴더 **둘 다**.
///
/// 작업 폴더 쪽(꼬리가 붙은 이름)만 자르던 자리다. 세션 설정 폴더 쪽도 gitignored 라
/// 같은 근거로 순회 대상이 아닌데 빠져 있었고, 같은 물음에 답하는 다른 사본들과
/// 집합이 하나 어긋나 있었다(2026-09-06 실측). 이 커밋으로 순회가 **줄어들지만**
/// 오늘 결과는 불변이다 — 그 폴더 아래 `.rs` 가 0 개다.
fn is_pruned(name: &str) -> bool {
    PRUNE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
}

/// 금지 코드포인트인지 — 픽토그래픽 이모지(1F000..1FAFF) + regional indicator(1F1E6..1F1FF).
fn is_forbidden_emoji(cp: u32) -> bool {
    (0x1F000..=0x1FAFF).contains(&cp) || (0x1F1E6..=0x1F1FF).contains(&cp)
}

/// 스캔 대상 파일인지 — repo-relative 경로 기준.
/// - 파일명이 `tasty-plugin.toml` 이면 대상(어디에 있든).
/// - `.rs` 이면서 `src/` · `crates/*/src/` · `crates/*/tests/` 아래면 대상.
fn is_scan_target(rel: &str) -> bool {
    if rel.rsplit('/').next() == Some("tasty-plugin.toml") {
        return true;
    }
    if !rel.ends_with(".rs") {
        return false;
    }
    if rel.starts_with("src/") {
        return true;
    }
    // crates/<name>/src/... 또는 crates/<name>/tests/...
    if let Some(rest) = rel.strip_prefix("crates/") {
        let mut parts = rest.splitn(2, '/');
        let _name = parts.next();
        if let Some(after) = parts.next() {
            return after.starts_with("src/") || after.starts_with("tests/");
        }
    }
    false
}

/// `path` 하위를 재귀 순회하며 스캔 대상 파일을 모은다. `is_pruned` 는 가지치기.
fn gather(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        let rel = rel_of(path, root);
        if is_scan_target(&rel) {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if is_pruned(name) {
                continue;
            }
        }
        gather(&p, root, out);
    }
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 스캔 하한 — [ADR-0133] 의 두 용도 중 **연기 검사**다("경로가 틀렸거나 읽기에 실패했다"
/// 를 잡는 용도). **모수 고정**("이만큼 봤으니 다 봤다")으로 쓰지 않는다 — 실제 모수가
/// 하한보다 크면 그 차이만큼 사각을 갖고도 초록이기 때문이다.
///
/// 이 가드가 "위반 0" 을 내는 이유는 둘이다: 정말 없거나, **아무것도 안 봤거나.**
/// [`gather`] 는 디렉토리를 못 읽으면 `return` 으로 조용히 빠져나가므로, 하한이 없으면
/// 순회가 깨진 날 정확히 초록이 뜬다. 실측으로 확인했다 — 스캔 루트를 빈 디렉토리로
/// 바꾸면 이 가드는 아무 말 없이 통과했다.
///
/// 값의 근거: 2026-09-05 기준 **[`gather`] 가 실제로 걷어 온 파일 수 1120** 이다(레포
/// 전체가 아니라 [`is_scan_target`] 을 통과한 집합). 아래쪽으로 넉넉한 여유를 둔다 —
/// 순회가 통째로 깨진 경우를 결정적으로 잡는 것이 목적이고, 몇 퍼센트의 누락까지 조이면
/// 레포가 줄어드는 날 거짓 빨강이 된다.
///
/// [ADR-0133]: ../docs/adr/0133-guard-scan-population-is-pinned-not-enumerated.md
const MIN_SCANNED_FILES: usize = 700;

/// 스캔이 믿을 만한가.
///
/// 판정을 함수로 뽑아 둔다 — 단언 안에 인라인으로 두면 그 값이 무엇을 가르는지 시험할
/// 자리가 없고, 하한 자신이 장식이 된다.
fn scan_is_credible(found: usize) -> bool {
    found >= MIN_SCANNED_FILES
}

/// 하한을 겨냥한 변이 — 하한 자신이 판정을 하는지 본다.
#[test]
fn the_scan_refuses_to_report_zero_from_an_empty_walk() {
    assert!(!scan_is_credible(0), "빈 스캔을 믿을 만하다고 판정했다");
    assert!(!scan_is_credible(MIN_SCANNED_FILES - 1));
    assert!(scan_is_credible(MIN_SCANNED_FILES));
}

#[test]
fn no_emoji_in_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    gather(root, root, &mut files);
    assert!(
        scan_is_credible(files.len()),
        "스캔 대상이 {}개다(하한 {MIN_SCANNED_FILES}) — 순회가 깨졌다. 위반 0 은 이 상태에서 \
         아무 뜻도 없다",
        files.len()
    );

    let mut violations = Vec::new();
    for file in files {
        let rel = rel_of(&file, root);
        if ALLOWLIST_FILES.contains(&rel.as_str()) {
            continue;
        }
        let contents = std::fs::read_to_string(&file).expect("소스 파일 read 실패");
        for (i, line) in contents.lines().enumerate() {
            for ch in line.chars() {
                let cp = ch as u32;
                if is_forbidden_emoji(cp) {
                    violations.push(format!("  {}:{} — U+{:04X} `{}`", rel, i + 1, cp, ch));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "소스(.rs / tasty-plugin.toml)에 픽토그래픽 이모지(U+1F000–1FAFF / 국기 U+1F1E6–1F1FF)가 \
         재유입됨 — 아이콘은 디자인 SVG 라인아이콘(`icons::*`)에서 쓰고, 매니페스트 icon 이모지· \
         주석 이모지는 제거할 것. 테스트 입력이 본질인 파일은 ALLOWLIST_FILES 에 추가:\n{}",
        violations.join("\n")
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
fn allowlist_files_point_at_paths_that_exist() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let missing = tasty_doc_guards::missing_referents(root, ALLOWLIST_FILES.iter().copied());
    assert!(
        missing.is_empty(),
        "면제가 없는 경로를 가리킨다 — 옮겼으면 항목도 옮기고, 사라졌으면 항목을 지워라: {missing:?}"
    );
}
