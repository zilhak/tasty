//! 시각 토큰 준수 가드 — 간격/폰트 크기/선 굵기 리터럴의 재유입을 차단한다.
//!
//! `CLAUDE.md` "UI 디자인 (필수)" 가 강제하는 네 축 중 색은 clippy
//! `disallowed_methods`(deny)가 컴파일 단계에서 막고, 나머지 셋(간격·폰트 크기·선
//! 굵기)을 이 파일이 맡는다. 셋 다 같은 형태(`<접두>(<숫자>`)라 판정기
//! [`violating_prefix`] 하나를 공유한다 — 토큰을 넘기는 정상 코드
//! (`Stroke::new(th.border_width.value(), ..)`)는 숫자로 시작하지 않아 걸리지 않는다.
//!
//! design-tokens-02 가 `add_space`/`Margin` 의 off-grid 리터럴을 typed 헬퍼
//! (`vspace`/`hspace`/`margin_all`/`margin_sym` + `th.spacing_*` / `STRUCT_GAP_*`)로
//! 이식했다. 이 가드는 그 결과를 되돌림 없이 유지한다 — 소스에 `add_space(8.0)` 이나
//! `Margin::same(12)` 같은 **인라인 숫자 리터럴**을 다시 넣으면 `cargo test --workspace`
//! (`.github/workflows/test.yml`)에서 fail 한다. 선례: `tests/cli_naming_count_drift.rs`.
//!
//! **스코프 밖(의도적)**: `const NAME: LogicalPx = LogicalPx(N)` 같은 **명명 구조 상수**는
//! 금지하지 않는다 — 그게 구조값(사이드바 폭·카드 크기·control nudge)의 *권장* 해결책이다
//! — structural 값은 magic number 대신 명명 const 로 둬야 의미가 이름에 남는다.
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

/// primitive 색 필드 접근 스캔 대상 — host UI 계층 + 위젯 크레이트. design-tokens-05 의
/// semantic 접근자 전수 이식이 완료된 범위(현재 0). 위젯 크레이트도 primitive 절대 불가
/// (ADR-0033): 재사용 위젯이라도 색은 semantic role 접근자로만 읽는다. 제외:
/// - `crates/tasty-gallery/src`: 팔레트 데모가 raw primitive 를 의도적으로 노출.
const COLOR_SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu/shell_setup.rs",
    "crates/tasty-ui-widgets/src",
];

/// raw 픽토그래픽 글리프 스캔 대상 — **host 전용**(widgets/gallery 미포함). gallery
/// specimen 은 ↑↓↵→◀▶ 를 대량 사용하므로 SCAN_ROOTS 재사용 시 오검출된다(연구 §3).
/// 플러그인(`crates/tasty-plugin-*`)도 미포함 — S-11 과 비중첩.
const GLYPH_SCAN_ROOTS: &[&str] = &["src/view", "src/adapters/ui", "src/gfx/gpu/shell_setup.rs"];

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

/// 금지 패턴: `<prefix>` 뒤 (공백 무시) 첫 문자가 숫자면 인라인 리터럴로 본다.
/// typed 헬퍼(`margin_all(th.spacing_md)`)·토큰(`spacing_xs.value()`)은 숫자로 시작하지
/// 않으므로 걸리지 않는다.
///
/// 폰트 크기는 `th.font_size_*` 또는 component 접근자(`th.badge_font_size()` 등),
/// 선 굵기는 `th.border_width`/`focus_ring_width`/`icon_stroke_width` 로 바꾼다.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "add_space(",
    "Margin::same(",
    "Margin::symmetric(",
    "inner_margin(",
    "FontId::proportional(",
    "FontId::monospace(",
    "Stroke::new(",
    // `RichText::new(..).size(13.5)` — `FontId::*` 와 **같은 결함의 다른 표기**다.
    // egui 에서 폰트 크기를 지정하는 두 경로가 이 둘이라 한쪽만 막으면 다른 쪽으로
    // 그대로 재유입된다. `Spinner::size()`(위젯 지름)도 같은 이름이라 함께 걸리는데,
    // 그건 폰트가 아니므로 아래 allowlist 로 그 자리만 면제한다.
    ".size(",
];

/// 숫자 인자 검사로는 잡히지 않는 **금지 형태**. 접두 규칙은 "접두 뒤 첫 문자가
/// 숫자인가" 를 보는데, 구조체 리터럴은 필드명이 먼저 와서 그 검사를 그대로
/// 빠져나간다(`egui::Stroke { width: 2.0, color }`). 실제 회피 변형 검증에서 이
/// 형태로 한 번 뚫렸다.
///
/// 여기 있는 형태는 **값과 무관하게** 금지다 — 토큰을 넣든 리터럴을 넣든 쓰지 말고
/// `Stroke::new(<토큰>, ..)` 를 쓴다. 그래야 접두 규칙이 계속 유효하다. 현재 스캔
/// 범위 안 출현 0 이라 allowlist 가 필요 없다.
const FORBIDDEN_FORMS: &[&str] = &["Stroke {"];

/// 스캔 예외 — **파일 통째가 아니라 (경로, 접두) 쌍**이다. 파일 하나를 통째로 빼면
/// 그 파일이 *다른* 형태의 위반을 새로 들여도 잡히지 않으므로, 정당한 그 한 형태만
/// 면제한다. 새 항목에는 왜 그 파일에서 그 접두가 정당한지 사유를 남긴다.
const ALLOWLIST_PREFIXES: &[(&str, &str)] = &[
    // typed 간격 헬퍼의 구현 자체 — 내부에서 raw `add_space`/`Margin` 을 호출하고
    // doc 주석에 예시 리터럴을 담는다. 폰트·선굵기는 면제 대상이 아니다.
    ("crates/tasty-ui-widgets/src/spacing.rs", "add_space("),
    ("crates/tasty-ui-widgets/src/spacing.rs", "Margin::same("),
    (
        "crates/tasty-ui-widgets/src/spacing.rs",
        "Margin::symmetric(",
    ),
    ("crates/tasty-ui-widgets/src/spacing.rs", "inner_margin("),
    // spinner 크기 카탈로그 — 이 specimen 의 **내용 자체가** 여러 지름을 나란히
    // 보이는 것이다. `.size()` 는 여기서 폰트가 아니라 위젯 지름이고, 값이 하나로
    // 수렴하면 specimen 이 성립하지 않는다. 폰트·간격·선굵기는 면제 대상이 아니다.
    (
        "crates/tasty-gallery/src/catalog/components/prim_spinner.rs",
        ".size(",
    ),
];

/// `line` 에 금지 prefix + 숫자 인자가 있으면 매칭된 prefix 를 돌려준다.
/// `rel` 파일에 대해 그 접두가 [`ALLOWLIST_PREFIXES`] 에 있으면 건너뛴다.
fn violating_prefix(rel: &str, line: &str, next_line: &str) -> Option<&'static str> {
    for &form in FORBIDDEN_FORMS {
        if !ALLOWLIST_PREFIXES.contains(&(rel, form)) && line.contains(form) {
            return Some(form);
        }
    }
    for &prefix in FORBIDDEN_PREFIXES {
        if ALLOWLIST_PREFIXES.contains(&(rel, prefix)) {
            continue;
        }
        let mut from = 0;
        while let Some(idx) = line[from..].find(prefix) {
            let after = &line[from + idx + prefix.len()..].trim_start();
            // 접두가 줄 끝에서 열린 채 끝나면(`.size(` 뒤에 아무것도 없음) 인자는 다음
            // 줄에 있다. 한 줄만 보면 `.size(\n    13.0)` 형태로 그냥 빠져나간다 —
            // 실제로 이 회피 변형에 한 번 뚫렸다. `rustfmt` 가 짧은 호출은 한 줄로
            // 되돌리므로 레포에 들어오긴 어렵지만, 가드의 회피 난이도를 포매터
            // 하나에만 의존시키지 않는다.
            let probe = if after.is_empty() {
                next_line.trim_start()
            } else {
                after
            };
            if matches!(probe.chars().next(), Some(c) if c.is_ascii_digit()) {
                return Some(prefix);
            }
            from += idx + prefix.len();
        }
    }
    None
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `line` 에 `th.<primitive>` / `theme.<primitive>` 평면 필드 접근이 있으면 그 표현을 돌려준다.
/// 앞뒤 경계를 검사해 `th.text_primary()`(semantic) 나 `mytheme.blue` 오검출을 배제한다.
fn violating_color(_rel: &str, line: &str, _next: &str) -> Option<String> {
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

/// `target` 하위 `.rs` 파일을 모아, 각 라인에 `detect(rel, line)` 을 적용해 위반을
/// 수집한다. 주석 라인(`//`)은 스킵 — 파일 단위 면제는 없다([`ALLOWLIST_PREFIXES`]).
fn collect_violations(
    root: &Path,
    target: &str,
    detect: &dyn Fn(&str, &str, &str) -> Option<String>,
    out: &mut Vec<String>,
) {
    let path = root.join(target);
    let mut files = Vec::new();
    gather_rs_files(&path, &mut files);
    assert!(
        !files.is_empty(),
        "스캔 루트 `{target}` 에서 .rs 파일을 하나도 찾지 못했다 — 경로가 바뀌었거나 \
         읽기에 실패했다. 조용한 미스캔은 위양성보다 나쁘므로 여기서 실패시킨다."
    );
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let contents = std::fs::read_to_string(&file).expect("소스 파일 read 실패");
        // 판정기가 "다음 줄" 을 볼 수 있어야 한다 — 호출 인자가 줄바꿈으로 넘어간
        // 형태(`.size(` 개행 `13.0)`)를 한 줄만 보고는 잡지 못한다.
        let lines: Vec<&str> = contents.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // 주석 라인(// 로 시작)은 스킵 — doc/설명의 예시 리터럴 false positive 방지.
            if line.trim_start().starts_with("//") {
                continue;
            }
            let next = lines.get(i + 1).copied().unwrap_or("");
            if let Some(hit) = detect(&rel, line, next) {
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
    let entries = std::fs::read_dir(path).unwrap_or_else(|e| {
        panic!(
            "스캔 대상 디렉토리를 읽을 수 없다: {} — {e}. 조용히 건너뛰면 가드가 \
             아무것도 검사하지 않은 채 통과한다.",
            path.display()
        )
    });
    for entry in entries.flatten() {
        gather_rs_files(&entry.path(), out);
    }
}

#[test]
fn no_inline_visual_token_literals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // 형태 규칙(`FORBIDDEN_FORMS`)은 숫자 인자와 무관하게 금지라 `<숫자>` 를 붙이지
    // 않는다 — 붙이면 "숫자만 빼면 통과" 로 잘못 읽힌다.
    let detect = |rel: &str, line: &str, next: &str| {
        violating_prefix(rel, line, next).map(|p| {
            if FORBIDDEN_FORMS.contains(&p) {
                format!("{p}` — 구조체 리터럴 형태 자체가 금지(`Stroke::new(<토큰>, ..)` 를 쓴다)`")
            } else {
                format!("{p}<숫자>")
            }
        })
    };
    let mut violations = Vec::new();
    for target in SCAN_ROOTS {
        collect_violations(root, target, &detect, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "인라인 시각 토큰 리터럴이 재유입됨 — 각 축의 대체 수단으로 바꿀 것:\n\
         · 간격/마진 → typed 헬퍼(vspace/hspace/margin_all/margin_sym) + th.spacing_* / \
         STRUCT_GAP_*\n\
         · 폰트 크기 → th.font_size_micro/caption/body/heading/max, 또는 역할을 이름에 \
         담은 component 접근자(th.badge_font_size() · th.tag_font_size() · \
         th.kbd_font_size() 등)\n\
         · 선 굵기 → th.border_width(1) / th.focus_ring_width(2) / th.icon_stroke_width(1.5)\n\
         대응 토큰이 없는 구조값은 명명 const(`const NAME: LogicalPx = LogicalPx(N)`)로 \
         승격한다 — 그건 스코프 밖이다:\n{}",
        violations.join("\n")
    );
}

/// 픽토그래픽 글리프 금지 범위(Tier-A) — UI 프로포셔널 폰트에서 tofu 나는 계열만
/// 좁게 잡는다: 이모지·픽토그래프(U+1F000–1FAFF) + 딩뱃(U+2700–27BF). 화살표(↑↓→↵)·
/// 기하도형(▲▼▾)·기술기호(⌘)·경고기호(⚠)는 kbd 힌트·라벨 구분자·콤보 affordance 로
/// 정당하게 쓰이므로 **범위 밖**(연구 §3). CJK·따옴표 등 텍스트도 자동 제외된다.
fn is_forbidden_pictographic(cp: u32) -> bool {
    (0x1F000..=0x1FAFF).contains(&cp) || (0x2700..=0x27BF).contains(&cp)
}

/// `line` 에서 픽토그래픽 글리프를 찾으면 그 표현을 돌려준다. **두 형태 모두** 검사:
/// ① 리터럴 코드포인트(누가 📂 를 그대로 붙여넣음) ② `\u{HEX}` 이스케이프 파싱 후 범위검사.
/// 주석 라인 skip 은 상위 `collect_violations` 가 처리한다.
fn violating_glyph(_rel: &str, line: &str, _next: &str) -> Option<String> {
    // ① 리터럴 char.
    for ch in line.chars() {
        let cp = ch as u32;
        if is_forbidden_pictographic(cp) {
            return Some(format!("U+{cp:04X} `{ch}`"));
        }
    }
    // ② `\u{HEX}` 이스케이프.
    let needle = "\\u{";
    let mut from = 0;
    while let Some(rel) = line[from..].find(needle) {
        let start = from + rel + needle.len();
        let Some(close_rel) = line[start..].find('}') else {
            break;
        };
        let hex = &line[start..start + close_rel];
        if let Ok(cp) = u32::from_str_radix(hex, 16)
            && is_forbidden_pictographic(cp)
        {
            return Some(format!("\\u{{{hex}}}"));
        }
        from = start + close_rel + 1;
    }
    None
}

#[test]
fn no_raw_pictographic_glyph() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for target in GLYPH_SCAN_ROOTS {
        collect_violations(root, target, &violating_glyph, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "host UI 소스에 raw 픽토그래픽 글리프(이모지 U+1F000–1FAFF / 딩뱃 U+2700–27BF)가 \
         재유입됨 — SVG line-icon(`icons::*`)으로 바꿀 것. 리터럴·`\\u{{}}` 양형태 모두 금지:\n{}",
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
