//! 시각 토큰 드리프트 가드 — **UI 폰트 토큰 값을 복사한 이름 상수**를 막는다.
//!
//! # 왜 `tests/` 가 아니라 여기인가 (관례를 깬 이유)
//!
//! 소스 스캔 가드의 관례 자리는 `tests/*.rs` 이고 자매 가드
//! (`tests/design_token_adherence.rs`)도 거기 있다. 그런데 통합 테스트는 **컴파일만**
//! 자동으로 검사되고 **실행은 수동 트리거**다. 일반 테스트에겐 "컴파일은 본다" 가
//! 부분적 안전망이지만 **런타임에 소스를 읽는 스캔 가드에겐 0** 이다 — 본체가 스캔이라
//! 실행되지 않으면 존재하지 않는 것과 같다. lib/bin 유닛 테스트는 Windows·headless
//! 두 `--lib --bins` 잡에서 **실행**된다. 그래서 이 가드는 `#[cfg(test)]` 모듈로 본체
//! crate 안에 둔다.
//!
//! 그 잡들이 **초록인지는 별개로 확인해야 한다** — 채널이 있다는 것은 채널이 건강하다는
//! 뜻이 아니다. 트리거·러너를 포함한 채널 정본은
//! [`docs/dev-guide/ci-gates.md`](../docs/dev-guide/ci-gates.md) 하나이고, 여기 옮겨
//! 적지 않는다.
//!
//! **이 파일을 `tests/` 로 "관례에 맞춰" 옮기지 마라 — 옮기는 순간 자동 실행 채널을
//! 잃는다.** (Windows 잡에서도 도는 자리라 CRLF 체크아웃을 계산에 넣었다 —
//! [`discriminate::crlf_checkout_reads_the_same`].)
//!
//! # 무엇을 잡나
//!
//! `const BODY_FONT_SIZE: f32 = 13.0;` 처럼 **UI 폰트 스케일과 값이 같은 이름 상수**가
//! 폰트 자리(`.size(` · `FontId::proportional/monospace/new(`)에 오는 것을 잡는다.
//!
//! 자매 가드는 인라인 리터럴(`.size(13.0)`)만 막고 **명명 const 경유는 설계상 허용**
//! 한다 — 그게 스케일 **밖** 값(9.5 · 10.5 · 12.5 …)의 권장 해결책이기 때문이다
//! ([ADR-0126](../docs/adr/0126-off-scale-font-values-are-not-snapped-to-tokens.md)).
//! 그런데 그 허용은 **값이 스케일 밖일 때만** 정당하다. 값이 토큰과 같으면 그 const 는
//! 토큰의 복사본이고, 복사본은 `ui_zoom` 을 타지 않아 zoom≠1 에서 조용히 갈라진다.
//! ADR-0126 이 그 세 자리를 우연히 감싸고 있었고, 이 가드가 그 틈을 닫는다.
//!
//! # 한계 (자매 가드의 "가드가 막지 못하는 것" 과 같은 성격)
//!
//! - 값 해석은 **`const NAME: f32 = <숫자>;` / `= LogicalPx(<숫자>)` 리터럴만** 따라간다.
//!   계산식(`13.0 * 1.0`)·다른 const 참조·`let` 변수는 못 따라간다.
//! - const 표는 **스캔 루트 안에서만** 모은다. 루트 밖에 정의된 이름은 판정되지 않는다.
//! - 폰트/위젯 판별은 수신자 look-back 이다 — `Spinner::new()` 가 같은 문(`;` 이전)에
//!   있으면 지름으로 보고 건너뛴다. 그 밖의 `.size(` 는 폰트로 본다.

use std::path::{Path, PathBuf};

/// 스캔 대상 (repo-relative). 자매 가드 `tests/design_token_adherence.rs` 의
/// `SCAN_ROOTS` 와 같은 집합이다 — 같은 축을 보므로 갈라지면 안 된다.
const SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu/shell_setup.rs",
    "crates/tasty-gallery/src",
    "crates/tasty-ui-widgets/src",
    "crates/tasty-egui-theme/src",
];

/// UI 폰트 스케일의 값 — `font_size_micro`(10) · `caption`(11) · `body`(13) ·
/// `heading`(13) · `max`(14). 이 다섯이 UI 스케일 전부다
/// (`docs/design/systems/theme.md` "UI 폰트 스케일").
///
/// 콘텐츠 폰트(`font_size_term_sm`(12) · `term`(14) · `term_lg`(16) · `prose_h1`(20))는
/// 스케일이 다르지만 `term` 은 값이 14 라 `max` 와 겹친다. 겹쳐도 판정은 옳다 —
/// 어느 쪽 의도든 **토큰을 쓰라**가 답이고, 어느 토큰인지는 고치는 사람이 정한다.
const UI_FONT_TOKEN_VALUES: &[f32] = &[10.0, 11.0, 13.0, 14.0];

/// 폰트 크기를 받는 호출 형태. 접두 뒤 첫 인자가 크기다.
const FONT_CALLS: &[&str] = &[
    ".size(",
    "FontId::proportional(",
    "FontId::monospace(",
    "FontId::new(",
];

/// `const NAME: f32 = 13.0;` / `const NAME: LogicalPx = LogicalPx(13.0);` 를 모은다.
///
/// 이름이 파일을 넘어 참조되므로(예 `CENTER_GLYPH_SIZE` 는 `tasty-ui-widgets` 에
/// 있고 host popup 이 쓴다) 표는 스캔 루트 **전체에서 하나로** 만든다.
fn collect_numeric_consts(lines: &[&str], out: &mut Vec<(String, f32)>) {
    for line in lines {
        let t = line.trim_start();
        let t = t.strip_prefix("pub(crate) ").unwrap_or(t);
        let t = t.strip_prefix("pub ").unwrap_or(t);
        let Some(rest) = t.strip_prefix("const ") else {
            continue;
        };
        let Some((name, rhs)) = rest.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        let Some((_, value)) = rhs.split_once('=') else {
            continue;
        };
        let v = value.trim().trim_end_matches(';').trim();
        let v = v
            .strip_prefix("LogicalPx(")
            .map(|s| s.trim_end_matches(')'))
            .unwrap_or(v);
        if let Some(n) = numeric_literal(v.trim()) {
            out.push((name.to_string(), n));
        }
    }
}

/// 숫자 리터럴인가 — `13` · `13.0` · `13.0f32`.
fn numeric_literal(tok: &str) -> Option<f32> {
    let t = tok.trim().trim_end_matches("f32").trim_end_matches("f64");
    if t.is_empty() || !t.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    t.parse::<f32>().ok()
}

/// 대문자 스네이크 이름인가 — 상수 참조의 구조적 표지.
fn is_screaming_snake(tok: &str) -> bool {
    tok.len() >= 3
        && tok.starts_with(|c: char| c.is_ascii_uppercase())
        && tok
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// `.size(` 의 수신자가 스피너인가 — **문 단위** 판정이다. 호출 앞 텍스트를 뒤로
/// 이어붙여 **마지막 `;` 이후**만 본다: 앞 문이 스피너였다는 이유로 뒤 문이 면제되면
/// 안 된다. 스피너의 `.size()` 는 폰트가 아니라 위젯 지름이라 이 축의 대상이 아니다.
fn spinner_receiver(lines: &[&str], at: usize, before_call: &str) -> bool {
    // 한 문이 12줄을 넘게 이어지는 형태는 레포에 없다 — 비용을 여기서 끊는다.
    let from = at.saturating_sub(12);
    let mut text = String::new();
    for prev in &lines[from..at] {
        text.push_str(prev);
        text.push('\n');
    }
    text.push_str(before_call);
    let stmt = match text.rfind(';') {
        Some(i) => &text[i + 1..],
        None => &text[..],
    };
    stmt.contains("Spinner::new()")
}

/// 한 파일의 위반. `consts` 는 스캔 루트 전체에서 모은 이름→값 표다.
fn const_font_violations(
    rel: &str,
    lines: &[&str],
    consts: &[(String, f32)],
    out: &mut Vec<String>,
) {
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for call in FONT_CALLS {
            // 한 줄에 폰트 호출이 둘 이상 올 수 있다 — 첫 개만 보면 뒤 호출이 앞
            // 호출의 판정(특히 스피너 면제)에 묻힌다. 변이로 확인한 실제 누락이다.
            let mut cursor = 0usize;
            while let Some(rel_at) = line[cursor..].find(call) {
                let at = cursor + rel_at;
                cursor = at + call.len();
                let rest = &line[cursor..];
                let Some(end) = rest.find([',', ')']) else {
                    continue;
                };
                let arg = rest[..end].trim();
                if !is_screaming_snake(arg) {
                    continue;
                }
                if *call == ".size(" && spinner_receiver(lines, i, &line[..at]) {
                    continue;
                }
                let Some((_, value)) = consts.iter().find(|(n, _)| n == arg) else {
                    continue;
                };
                if UI_FONT_TOKEN_VALUES.contains(value) {
                    out.push(format!(
                        "  {}:{} — `{}` = {} 는 UI 폰트 토큰과 같은 값이다",
                        rel,
                        i + 1,
                        arg,
                        value
                    ));
                }
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

/// 스캔 하한 — 이 아래로 떨어지면 경로가 틀렸거나 읽기에 실패한 것이다. 하한이
/// 없으면 스캔 대상 0개인 가드가 **초록으로** 통과한다.
const MIN_SCANNED_FILES: usize = 200;

#[test]
fn no_named_const_copies_a_ui_font_token() {
    // `CARGO_MANIFEST_DIR` 은 루트 패키지의 것이라 레포 루트다 — 크레이트 밖으로
    // 올라가는 상대경로(`../../..`)가 필요 없다.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut files = Vec::new();
    for target in SCAN_ROOTS {
        let path = root.join(target);
        let before = files.len();
        gather_rs_files(&path, &mut files);
        assert!(
            files.len() > before,
            "스캔 루트 `{target}` 에서 .rs 파일을 하나도 찾지 못했다"
        );
    }
    assert!(
        files.len() >= MIN_SCANNED_FILES,
        "스캔한 파일이 {}개뿐이다(하한 {MIN_SCANNED_FILES}) — 경로가 바뀌었을 것이다",
        files.len()
    );

    // 1) 이름→값 표를 먼저 전부 모은다. 2) 그 다음 폰트 자리를 판정한다.
    let mut sources = Vec::new();
    let mut consts = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let contents = std::fs::read_to_string(file).expect("소스 파일 read 실패");
        sources.push((rel, contents));
    }
    for (_, contents) in &sources {
        let lines: Vec<&str> = contents.lines().collect();
        collect_numeric_consts(&lines, &mut consts);
    }

    let mut violations = Vec::new();
    for (rel, contents) in &sources {
        let lines: Vec<&str> = contents.lines().collect();
        const_font_violations(rel, &lines, &consts, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "UI 폰트 토큰 값을 복사한 이름 상수가 폰트 자리에 있다 — 토큰을 직접 쓸 것\n\
         (`th.font_size_micro/caption/body/heading/max` 또는 역할 접근자 \
         `badge_font_size()` 등):\n\
         · const 는 `ui_zoom` 을 타지 않는다. 토큰은 탄다. zoom≠1 에서 갈라진다\n\
         · 스케일 **밖** 값(9.5 · 10.5 · 12.5 …)의 명명 const 는 그대로 허용된다 — \
         금지되는 것은 토큰 값의 **복사본**뿐이다(ADR-0126)\n{}",
        violations.join("\n")
    );
}

#[cfg(test)]
mod discriminate {
    use super::*;

    /// 판정의 **전제 검사** — 이 가드는 "이름에 FONT 가 들어가는가" 가 아니라
    /// "**폰트 자리에 온 상수의 값이 토큰 값인가**" 로 가른다. 축이 이름으로
    /// 미끄러지면 여기서 죽는다.
    #[test]
    fn the_axis_is_value_and_position_not_name() {
        let consts = vec![
            ("BODY_FONT_SIZE".to_string(), 13.0_f32),
            ("PALETTE_HINT_FONT_SIZE".to_string(), 10.5),
            ("SOMETHING_ELSE".to_string(), 13.0),
            ("LOADING_SPINNER_SIZE".to_string(), 14.0),
        ];
        let check = |lines: &[&str]| {
            let mut out = Vec::new();
            const_font_violations("f.rs", lines, &consts, &mut out);
            out
        };

        // 값이 토큰과 같다 → 이름이 무엇이든 잡힌다.
        assert_eq!(check(&["    .size(BODY_FONT_SIZE),"]).len(), 1);
        assert_eq!(check(&["    .size(SOMETHING_ELSE),"]).len(), 1);
        // 값이 스케일 밖이다 → 이름에 FONT 가 있어도 잡지 않는다(ADR-0126 이 허용).
        assert_eq!(check(&["    .size(PALETTE_HINT_FONT_SIZE),"]).len(), 0);
        // 폰트 자리가 아니다(스피너 지름) → 값이 토큰과 같아도 축 밖이다.
        assert_eq!(
            check(&[
                "    Spinner::new()",
                "        .size(LOADING_SPINNER_SIZE)",
                "        .color(c),",
            ])
            .len(),
            0
        );
        // 한 줄에 폰트 호출이 둘이어도 뒤엣것을 놓치지 않는다.
        assert_eq!(
            check(&[
                "    Spinner::new().size(LOADING_SPINNER_SIZE).show(ui, th); ui.label(RichText::new(x).size(BODY_FONT_SIZE));"
            ])
            .len(),
            1
        );
        // 같은 look-back 이 `;` 을 넘지 않는다 — 앞 문의 스피너가 뒤 문을 면제하지 못한다.
        assert_eq!(
            check(&[
                "    Spinner::new().size(LOADING_SPINNER_SIZE).show(ui, th);",
                "    ui.label(RichText::new(x).size(BODY_FONT_SIZE));",
            ])
            .len(),
            1
        );
        // `FontId::` 세 형태도 같은 자리다.
        assert_eq!(
            check(&["    egui::FontId::proportional(BODY_FONT_SIZE),"]).len(),
            1
        );
        assert_eq!(
            check(&["    egui::FontId::monospace(BODY_FONT_SIZE),"]).len(),
            1
        );
        // 주석 줄은 세지 않는다.
        assert_eq!(check(&["    // .size(BODY_FONT_SIZE)"]).len(), 0);
        // 표에 없는 이름은 값을 모르므로 판정하지 않는다(한계 — 모듈 doc 에 적었다).
        assert_eq!(check(&["    .size(UNKNOWN_CONST),"]).len(), 0);
    }

    /// CRLF 로 체크아웃된 트리(Windows 잡)에서도 같은 판정이어야 한다 —
    /// `str::lines()` 가 `\r` 을 떼지만, 인자 파싱이 `\r` 을 물면 이름이 어긋난다.
    #[test]
    fn crlf_checkout_reads_the_same() {
        let consts = vec![("BODY_FONT_SIZE".to_string(), 13.0_f32)];
        let src = "    .size(BODY_FONT_SIZE),\r\n    let x = 1;\r\n";
        let lines: Vec<&str> = src.lines().collect();
        let mut out = Vec::new();
        const_font_violations("f.rs", &lines, &consts, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn const_table_reads_both_shapes() {
        let mut out = Vec::new();
        collect_numeric_consts(
            &[
                "const A: f32 = 13.0;",
                "pub const B: LogicalPx = LogicalPx(11.0);",
                "pub(crate) const C: f32 = 9.5;",
                "const D: f32 = SOMETHING * 2.0;",
                "let e = 13.0;",
            ],
            &mut out,
        );
        assert_eq!(
            out,
            vec![
                ("A".to_string(), 13.0),
                ("B".to_string(), 11.0),
                ("C".to_string(), 9.5)
            ]
        );
    }
}
