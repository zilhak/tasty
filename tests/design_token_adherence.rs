//! 간격/마진 토큰 준수 가드 — inline spacing rhythm 리터럴의 재유입을 차단한다.
//!
//! design-tokens-02 가 `add_space`/`Margin` 의 off-grid 리터럴을 typed 헬퍼
//! (`vspace`/`hspace`/`margin_all`/`margin_sym` + `th.spacing_*` / `STRUCT_GAP_*`)로
//! 이식했다. 이 가드는 그 결과를 되돌림 없이 유지한다 — 소스에 `add_space(8.0)` 이나
//! `Margin::same(12)` 같은 **인라인 숫자 리터럴**을 다시 넣으면 `cargo test --workspace`
//! (`.github/workflows/test.yml`)에서 fail 한다. 선례: `tests/cli_naming_count_drift.rs`.
//!
//! **스코프 밖(의도적)**: `const NAME: LogicalPx = LogicalPx(N)` 같은 **명명 구조 상수**는
//! 금지하지 않는다 — 그게 구조값(사이드바 폭·카드 크기·control nudge)의 *권장* 해결책이다
//! (2026-07-03-spacing-offgrid: (c) structural 은 magic number 대신 명명 const 로 둔다).
//! 이 가드가 잡는 건 4px 리듬 자리에 박힌 인라인 리터럴뿐이다.

use std::path::{Path, PathBuf};

/// 스캔 대상 (repo-relative). host UI 계층 + 갤러리 + 위젯 크레이트.
const SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu/shell_setup.rs",
    "crates/tasty-gallery/src",
    "crates/tasty-ui-widgets/src",
];

/// 스캔에서 제외할 파일 (repo-relative). typed 헬퍼 구현 — 내부에서 raw `add_space`/
/// `Margin` 을 정당하게 쓰고 doc 주석에 예시 리터럴을 포함한다.
const ALLOWLIST_FILES: &[&str] = &["crates/tasty-ui-widgets/src/spacing.rs"];

/// 금지 패턴: `<prefix>` 뒤 (공백 무시) 첫 문자가 숫자면 인라인 리터럴로 본다.
/// typed 헬퍼(`margin_all(th.spacing_md)`)·토큰(`spacing_xs.value()`)은 숫자로 시작하지
/// 않으므로 걸리지 않는다.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "add_space(",
    "Margin::same(",
    "Margin::symmetric(",
    "inner_margin(",
];

/// `line` 에 금지 prefix + 숫자 인자가 있으면 매칭된 prefix 를 돌려준다.
fn violating_prefix(line: &str) -> Option<&'static str> {
    for &prefix in FORBIDDEN_PREFIXES {
        let mut from = 0;
        while let Some(idx) = line[from..].find(prefix) {
            let after = &line[from + idx + prefix.len()..];
            let next = after.trim_start().chars().next();
            if matches!(next, Some(c) if c.is_ascii_digit()) {
                return Some(prefix);
            }
            from += idx + prefix.len();
        }
    }
    None
}

fn is_allowlisted(rel: &str) -> bool {
    ALLOWLIST_FILES.contains(&rel)
}

/// `dir_or_file` 하위 `.rs` 파일을 모아 각 위반을 `(rel_path, line_no, prefix, line)` 로.
fn collect_violations(root: &Path, target: &str, out: &mut Vec<String>) {
    let path = root.join(target);
    let mut files = Vec::new();
    gather_rs_files(&path, &mut files);
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if is_allowlisted(&rel) {
            continue;
        }
        let contents = std::fs::read_to_string(&file).expect("소스 파일 read 실패");
        for (i, line) in contents.lines().enumerate() {
            // 주석 라인(// 로 시작)은 스킵 — doc/설명의 예시 리터럴 false positive 방지.
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(prefix) = violating_prefix(line) {
                out.push(format!("  {}:{} — `{}<숫자>`", rel, i + 1, prefix));
            }
        }
    }
}

fn gather_rs_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        gather_rs_files(&entry.path(), out);
    }
}

#[test]
fn no_inline_spacing_literals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for target in SCAN_ROOTS {
        collect_violations(root, target, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "인라인 간격/마진 리터럴이 재유입됨 — typed 헬퍼(vspace/hspace/margin_all/margin_sym \
         + th.spacing_* / STRUCT_GAP_*)로 바꿀 것:\n{}",
        violations.join("\n")
    );
}
