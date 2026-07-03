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

/// 간격 스캔 대상 (repo-relative). host UI 계층 + 갤러리 + 위젯 크레이트.
const SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu/shell_setup.rs",
    "crates/tasty-gallery/src",
    "crates/tasty-ui-widgets/src",
];

/// primitive 색 필드 접근 스캔 대상 — **host UI 계층만**. design-tokens-05 의 semantic
/// 접근자 전수 이식이 완료된 범위다(현재 0). 제외:
/// - `crates/tasty-gallery/src`: 팔레트 데모가 raw primitive 를 의도적으로 노출.
/// - `crates/tasty-ui-widgets/src`: 위젯 내부가 아직 `theme.subtext0`/`theme.crust` 등
///   primitive 를 직접 쓴다 — 위젯 크레이트 이식은 별도 결정 대기(이 가드 스코프 밖).
const COLOR_SCAN_ROOTS: &[&str] = &["src/view", "src/adapters/ui", "src/gfx/gpu/shell_setup.rs"];

/// Theme 의 primitive(Catppuccin) 색 필드명. semantic 접근자(`text_primary()` 등)가 아닌
/// 평면 필드 직접 접근(`th.blue`/`theme.surface0`)을 host UI 에서 금지하기 위한 목록.
/// `text` 는 `text_primary`/`text_muted` 등 semantic 접근자의 접두라 경계 검사로 가른다.
const PRIMITIVE_COLOR_FIELDS: &[&str] = &[
    "crust",
    "mantle",
    "base",
    "surface0",
    "surface1",
    "surface2",
    "overlay0",
    "overlay1",
    "overlay2",
    "text",
    "subtext1",
    "subtext0",
    "blue",
    "green",
    "red",
    "yellow",
    "peach",
    "mauve",
    "teal",
    "sky",
    "lavender",
    "flamingo",
    "pink",
    "maroon",
    "rosewater",
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

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `line` 에 `th.<primitive>` / `theme.<primitive>` 평면 필드 접근이 있으면 그 표현을 돌려준다.
/// 앞뒤 경계를 검사해 `th.text_primary()`(semantic) 나 `mytheme.blue` 오검출을 배제한다.
fn violating_color(line: &str) -> Option<String> {
    for receiver in ["th.", "theme."] {
        let mut from = 0;
        while let Some(idx) = line[from..].find(receiver) {
            let start = from + idx;
            // receiver 앞 문자가 word char 면 `xtheme.` 같은 부분매치 → 스킵.
            let before_ok =
                start == 0 || !is_word_char(line[..start].chars().next_back().unwrap_or(' '));
            let after = &line[start + receiver.len()..];
            if before_ok {
                for &field in PRIMITIVE_COLOR_FIELDS {
                    if let Some(rest) = after.strip_prefix(field) {
                        // 필드명 뒤 문자가 word char 면 semantic 접근자(text_primary 등) → 스킵.
                        let next = rest.chars().next();
                        if !matches!(next, Some(c) if is_word_char(c)) {
                            return Some(format!("{receiver}{field}"));
                        }
                    }
                }
            }
            from = start + receiver.len();
        }
    }
    None
}

/// `target` 하위 `.rs` 파일을 모아, 각 라인에 `detect` 를 적용해 위반을 수집한다.
/// 주석 라인(`//`)·allowlist 파일은 스킵.
fn collect_violations(
    root: &Path,
    target: &str,
    detect: &dyn Fn(&str) -> Option<String>,
    out: &mut Vec<String>,
) {
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
            if let Some(hit) = detect(line) {
                out.push(format!("  {}:{} — `{}`", rel, i + 1, hit));
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
    let detect = |line: &str| violating_prefix(line).map(|p| format!("{p}<숫자>"));
    let mut violations = Vec::new();
    for target in SCAN_ROOTS {
        collect_violations(root, target, &detect, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "인라인 간격/마진 리터럴이 재유입됨 — typed 헬퍼(vspace/hspace/margin_all/margin_sym \
         + th.spacing_* / STRUCT_GAP_*)로 바꿀 것:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_primitive_color_field_access_in_host_ui() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for target in COLOR_SCAN_ROOTS {
        collect_violations(root, target, &violating_color, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "host UI 에 primitive 색 필드 직접 접근이 재유입됨 — semantic 접근자\
         (accent_*/surface_*/text_*/border_* 등)로 바꿀 것:\n{}",
        violations.join("\n")
    );
}
