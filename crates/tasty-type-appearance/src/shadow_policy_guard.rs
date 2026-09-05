//! 그림자 정책 집행 가드 — "떠 있는 표면 그림자는 `SHADOW_POPOVER` 1종" 을 소스에서 강제한다.
//!
//! `theme.rs` 주석이 "허용된 단 하나의 popover 그림자, 새 그림자 금지" 를 선언하지만
//! 그것을 집행하는 장치가 없어, `shell_setup.rs` 가 그 선언을 어긴 채(색이 앱 배경으로
//! 표류한 halo) 아무도 모르게 남아 있었다. 이 가드가 그 종류를 잡는다.
//!
//! # 왜 lib 유닛 테스트인가 (관례 예외 — `tests/` 로 되돌리지 마라)
//! 소스를 런타임에 스캔하는 드리프트 가드다. `tests/*.rs` 로 두면 실행 채널이 **헤드리스
//! 조합 하나**뿐이다(정본 `docs/dev-guide/ci-gates.md`) — 기본 조합의 자동 잡은
//! `--lib --bins` 라 통합 타깃을 못 본다. 스캔이 본체인 가드에겐 채널 하나를 통째로 잃는
//! 것이다. 크레이트 `src/` 안 `#[cfg(test)]` 로 두면 Windows 잡(`--lib --bins`)과 헤드리스
//! 잡(전체 스위트) 두 자동 잡에서 **실행**된다. 이 가드는 egui 를
//! 부르지 않는 순수 텍스트 스캔이라 `--no-default-features`(egui-compat off)에서도 선다.
//! 관례(`tests/*_chokepoint.rs`)를 깨는 이유는 그 자동 실행 하나다 — `tests/` 로 옮기면
//! 조용히 자동 채널을 잃는다.
//!
//! # 스캔 루트와 하한
//! 레포 루트는 `CARGO_MANIFEST_DIR/../..` 로 올라가 찾는다(이 크레이트는
//! `crates/tasty-type-appearance`). 그 경로가 틀리면 스캔 대상이 0 개가 되고 가드는
//! **초록**이 된다 — [`SCAN_FILE_FLOOR`] 하한이 그 거짓 초록을 잡는 유일한 장치다.
//! Windows 잡에서도 도므로 경로는 `std::path` 로만 다루고(구분자 하드코딩 금지) 줄은
//! `trim_end`(CRLF) 를 거친다.
//!
//! # 정책의 구멍 (기록만 — 여기서 메우지 않는다)
//! 이 가드는 **명시적 `Shadow {}` 리터럴 생성**만 본다. `crates/tasty-egui-theme` 은
//! `visuals.window_shadow` 를 매핑하지 않아(그 크레이트 주석이 밝힌다) egui 기본 그림자가
//! 잔류할 수 있는데, 그건 리터럴 생성이 아니라 이 스캔에 안 잡힌다. 그리고 **호스트
//! 실모달은 현재 그림자가 없다** — 그것이 "없어야 한다" 로 결정된 것인지는 미정이다(별개
//! 디자인 결정). "허용 그림자 1종" 정책은 egui 기본 그림자에 대해 아무 말도 하지 않는다
//! = 정책의 구멍이다. 메우는 것은 이 가드가 아니라 그 디자인 결정의 몫이다.
//!
//! # 검출은 순수 함수 — 면제 변이를 합성 입력으로 찌른다
//! 판정([`is_shadow_literal`]·[`shadow_literal_violations`]·[`geom_reassign_violations`])은
//! 파일 순회·경로 처리와 분리된 순수 함수다. 그래서 각 면제(반환타입 화살표 · `==`
//! 비교 · 자기 제외)를 겨냥한 변이를 레포에 진짜 위반을 심지 않고 **합성 문자열**로
//! 유닛 테스트에 붙박는다(아래 `mutation_*`). 못 잡는 것이 **의도**인 입력(scrim
//! 오버레이)도 `intended_miss_*` 로 고정해, 나중에 판정기를 넓혀 그걸 잡기 시작하면
//! 그 결정이 테스트 실패로 드러나게 한다 — 의도된 한계와 버그를 구분한다.

use std::fs;
use std::path::{Path, PathBuf};

/// 스캔 대상 최소 파일 수. 실측 1081(2026-09-04, `src/` + `crates/*/src/`). 경로
/// 상향(`../..`)이 틀려 스캔이 비면 가드가 거짓 초록이 되는 것을 막는 하한 — 실측보다
/// 넉넉히 낮게(파일 재구성 여유) 두되 0/소수로 붕괴하는 것은 잡는다.
const SCAN_FILE_FLOOR: usize = 800;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn scan_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs(&root.join("src"), &mut files);
    if let Ok(cr) = fs::read_dir(root.join("crates")) {
        for e in cr.flatten() {
            let src = e.path().join("src");
            if src.is_dir() {
                collect_rs(&src, &mut files);
            }
        }
    }
    // 이 가드 파일 자신은 판정 needle 과 뮤테이션 합성 입력을 담는 것이 본질이라
    // (판정기를 합성 문자열로 찌르는 변이가 붙박여 있다) 연속 needle 이 불가피하게
    // 등장한다. 자기 제외를 없애려면 그 needle 이 파일에 연속으로 없어야 하는데 —
    // doc 시연과 변이 입력이 그걸 요구하므로 불가능하다(needle 조각 결합은 doc 주석엔
    // 못 쓰고, 변이마다 강제할 장치도 없다). 그래서 파일 통째 면제한다.
    // 한계(강제되지 않음): 이 파일 안 판정기 밖에 실제 위반을 새로 들이면 못 잡는다.
    // 그 면제가 **실효 있음**(제외 없으면 오탐)은
    // `mutation_self_file_is_excluded_but_would_otherwise_flag` 가 고정하고, 판정기 밖에
    // 새 그림자 코드를 두지 않는다는 규율은 리뷰가 지킨다.
    let self_file = repo_root().join("crates/tasty-type-appearance/src/shadow_policy_guard.rs");
    files.retain(|f| f != &self_file);
    files
}

/// `Shadow {` 구조체 리터럴을 여는 줄인가. 함수 반환타입 선언
/// (`-> egui::epaint::Shadow {`)은 리터럴 생성이 아니라 제외한다. scrim 등
/// `from_black_alpha` 로 그리는 배경 오버레이는 `Shadow` 타입이 아니라 대상 밖이다
/// — **구조체 리터럴만** 본다. 이 판단을 넓히면 scrim 오탐이 시작된다.
///
/// 반환타입 제외는 **줄이 아니라 위치**로 판정한다. 줄 어딘가에 `->` 한 토큰이 있다는
/// 이유만으로 통째 면제하면(면제 창 = 줄) 같은 줄의 진짜 생성을 화살표가 가린다 —
/// 예: `|| -> u32 { let s = egui::Shadow { .. } }`. 그래서 화살표가 리터럴 **직전
/// 타입 위치**(리터럴 앞에 있고 그 사이에 대입 `=` 이 없음)일 때만 반환타입으로 본다.
fn is_shadow_literal(line: &str) -> bool {
    let t = line.trim_end();
    // `= Shadow {` 는 명백한 대입 생성 — 줄의 `->` 유무와 무관하게 위반 후보.
    if t.contains("= Shadow {") {
        return true;
    }
    for pat in ["egui::Shadow {", "epaint::Shadow {"] {
        if let Some(pos) = t.find(pat) {
            let before = &t[..pos];
            // 화살표가 리터럴 앞에 있고 그 뒤로 대입이 없으면 반환타입 선언(생성 아님).
            let is_return_type = before
                .rfind("->")
                .is_some_and(|a| !before[a..].contains('='));
            if !is_return_type {
                return true;
            }
        }
    }
    false
}

/// `theme.rs` 에서 `to_egui` 함수의 줄 범위 `[start, end]` 를 중괄호 깊이로 구한다.
/// 이름이 아니라 **구조**(함수 body 중괄호 밸런스)로 허용 범위를 정한다 — 그래야
/// `to_egui` 를 그 위치 밖으로 옮기는 뮤테이션이 잡힌다(리터럴이 범위 밖이 된다).
fn to_egui_range(theme_src: &str) -> (usize, usize) {
    let lines: Vec<&str> = theme_src.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("fn to_egui"))
        .expect("to_egui 정의를 찾지 못함 — 정본이 사라졌다");
    let mut depth: i32 = 0;
    let mut opened = false;
    for (i, l) in lines.iter().enumerate().skip(start) {
        depth += l.matches('{').count() as i32;
        depth -= l.matches('}').count() as i32;
        if l.contains('{') {
            opened = true;
        }
        if opened && depth == 0 {
            return (start, i);
        }
    }
    panic!("to_egui 함수 끝을 찾지 못함");
}

/// 판정1: `Shadow {}` 리터럴이 허용 범위(theme.rs 의 `to_egui`) 밖이면 위반.
/// 반환 `(위반 목록, 허용 범위 내 리터럴 수)`.
fn shadow_literal_violations(
    rel: &str,
    text: &str,
    allowed_range: Option<(usize, usize)>,
) -> (Vec<String>, usize) {
    let mut v = Vec::new();
    let mut allowed = 0;
    for (i, line) in text.lines().enumerate() {
        if is_shadow_literal(line) {
            if allowed_range.is_some_and(|(s, e)| i >= s && i <= e) {
                allowed += 1;
            } else {
                v.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    (v, allowed)
}

/// 판정2: `shadow_popover().to_egui()` 를 `let mut` 로 받은 변수의 기하 필드
/// (`offset`/`blur`/`spread`) 재대입은 위반. `color` 재대입은 허용(페이드 애니메이션 —
/// banner/modifier_hint 가 opacity 를 곱한다). 이름(`gamma_multiply`)이 아니라 **어느
/// 필드를 재대입하는가**로 가른다: 허용 함수를 통과했다는 것이 허용 값이 나온다는 뜻은
/// 아니다.
fn geom_reassign_violations(rel: &str, text: &str) -> Vec<String> {
    let mut vars: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("let mut ") && t.contains("shadow_popover") {
            let name: String = t["let mut ".len()..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                vars.push(name);
            }
        }
    }
    let mut v = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        for var in &vars {
            for field in ["offset", "blur", "spread"] {
                let assign = format!("{var}.{field} =");
                let eq = format!("{var}.{field} ==");
                if t.contains(&assign) && !t.contains(&eq) {
                    v.push(format!("{rel}:{}: {}", i + 1, t));
                }
            }
        }
    }
    v
}

fn rel_of(root: &Path, f: &Path) -> String {
    f.strip_prefix(root)
        .unwrap_or(f)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 정책: 모든 `Shadow {}` 생성은 `theme.rs::to_egui`(= `SHADOW_POPOVER` 변환) 안에서만.
/// 그 밖에서 그림자를 만들면 `theme.shadow_popover().to_egui()` 로 라우팅해야 한다.
#[test]
fn shadow_creation_is_confined_to_the_token_converter() {
    let root = repo_root();
    let files = scan_files();
    assert!(
        files.len() >= SCAN_FILE_FLOOR,
        "스캔 파일 {} < 하한 {} — 경로(../..)가 틀렸을 수 있다(거짓 초록 방지)",
        files.len(),
        SCAN_FILE_FLOOR
    );

    let theme_rs = root.join("crates/tasty-type-appearance/src/theme.rs");
    let theme_src = fs::read_to_string(&theme_rs).expect("theme.rs");
    let range = to_egui_range(&theme_src);

    let mut violations = Vec::new();
    let mut allowed_total = 0;
    for f in &files {
        let rel = rel_of(&root, f);
        let text = fs::read_to_string(f).unwrap_or_default();
        let allowed_range = if *f == theme_rs { Some(range) } else { None };
        let (v, allowed) = shadow_literal_violations(&rel, &text, allowed_range);
        violations.extend(v);
        allowed_total += allowed;
    }

    assert!(
        violations.is_empty(),
        "허용 위치(theme.rs::to_egui) 밖 Shadow 생성 — theme.shadow_popover() 로 라우팅해라:\n{}",
        violations.join("\n")
    );
    assert_eq!(
        allowed_total, 1,
        "정본 Shadow 생성(to_egui)이 정확히 1 개가 아니다 — 정본이 사라졌거나 중복됐다"
    );
}

/// 역방향: 허용 함수를 통과해도 그 결과의 기하를 덮어쓰면 정책 우회다.
#[test]
fn shadow_popover_result_is_not_geometrically_overridden() {
    let root = repo_root();
    let mut violations = Vec::new();
    for f in scan_files() {
        let rel = rel_of(&root, &f);
        let text = fs::read_to_string(&f).unwrap_or_default();
        violations.extend(geom_reassign_violations(&rel, &text));
    }
    assert!(
        violations.is_empty(),
        "shadow_popover() 결과의 기하 필드를 재대입한다(허용 함수 통과 ≠ 허용 값):\n{}",
        violations.join("\n")
    );
}

// ── 뮤테이션: 가드가 무엇을 잡고 무엇을 통과시키는지 자기증명 ──────────────────

#[test]
fn mutation_catches_shadow_literal_outside_converter() {
    let text = "fn draw() {\n    let s = egui::Shadow {\n        blur: 4,\n    };\n}\n";
    let (v, _) = shadow_literal_violations("x.rs", text, None);
    assert_eq!(v.len(), 1, "범위 밖 Shadow 리터럴을 잡아야 한다");
}

#[test]
fn mutation_discriminates_when_converter_moves_out_of_range() {
    // to_egui 는 비어 있고(1~2줄) Shadow{} 는 다른 함수에 있다 = to_egui 를 옮긴 형태.
    let text =
        "pub fn to_egui() {\n}\n\nfn other() {\n    egui::Shadow {\n        blur: 1,\n    };\n}\n";
    let range = to_egui_range(text);
    let (v, allowed) = shadow_literal_violations("theme.rs", text, Some(range));
    assert_eq!(
        allowed, 0,
        "to_egui 밖으로 나간 리터럴은 허용 카운트에 안 든다"
    );
    assert_eq!(v.len(), 1, "to_egui 밖 Shadow 리터럴은 위반");
}

#[test]
fn mutation_catches_geometry_reassignment() {
    let text = "let mut shadow = theme.shadow_popover().to_egui();\nshadow.offset = [1, 2];\n";
    assert_eq!(
        geom_reassign_violations("x.rs", text).len(),
        1,
        "기하 재대입은 위반"
    );
}

#[test]
fn mutation_allows_color_fade_on_popover_shadow() {
    // 실제 정당 케이스 형태(banner/modifier_hint) — color 만 곱한다. 반드시 PASS.
    let text = "let mut shadow = theme.shadow_popover().to_egui();\n\
                shadow.color = shadow.color.gamma_multiply(opacity);\n";
    assert!(
        geom_reassign_violations("x.rs", text).is_empty(),
        "color 페이드는 허용 — 정당한 사용을 죽이면 안 된다"
    );
}

// ── 면제 겨냥 변이: 각 면제 창 안쪽에 진짜 위반/정당을 심어 판정기를 찌른다 ────────

#[test]
fn mutation_arrow_on_line_does_not_hide_a_real_creation() {
    // 반환타입 화살표 면제가 **줄 단위**면 같은 줄의 실제 생성을 가린다(면제 창 과다).
    // 화살표가 리터럴 앞 타입 위치일 때만 반환타입으로 봐야 이 위반이 잡힌다.
    let hidden = "    let f = || -> u32 { let s = egui::Shadow { blur: 4 }; 0 };";
    assert!(
        is_shadow_literal(hidden),
        "줄에 `->` 가 있어도 대입(=) 뒤 실제 생성은 위반이다"
    );
    // 정당 면제 대상 — 화살표가 리터럴 직전 타입 위치(반환타입 선언)면 생성이 아니다.
    let return_type = "    pub fn to_egui(self) -> egui::epaint::Shadow {";
    assert!(
        !is_shadow_literal(return_type),
        "반환타입 선언은 생성이 아니다 — 정당 면제를 죽이면 안 된다"
    );
}

#[test]
fn mutation_geometry_comparison_is_not_a_reassignment() {
    // `offset ==` 비교 면제(`!contains("== ")`)가 진짜 대입(`=`)을 가리지 않는지 —
    // 비교는 통과하고 대입은 `mutation_catches_geometry_reassignment` 가 잡는다.
    let text = "let mut shadow = theme.shadow_popover().to_egui();\n\
                if shadow.offset == [0, 8] { paint(shadow); }\n";
    assert!(
        geom_reassign_violations("x.rs", text).is_empty(),
        "== 비교는 재대입이 아니다 — 정당 비교를 죽이면 안 된다"
    );
}

#[test]
fn mutation_self_file_is_excluded_but_would_otherwise_flag() {
    // 자기 제외가 **실효 있음**을 증명 — 이 파일을 그냥 판정기에 먹이면 판정 needle 과
    // 변이 합성 입력이 위반으로 잡힌다. 그래서 scan_files() 가 이 파일을 제외한다.
    // (제외 없으면 오탐 → 제외는 진짜 무언가를 가리는 면제다.)
    let self_src = fs::read_to_string(
        repo_root().join("crates/tasty-type-appearance/src/shadow_policy_guard.rs"),
    )
    .expect("self 파일 읽기");
    let (v, _) = shadow_literal_violations("self.rs", &self_src, None);
    assert!(
        !v.is_empty(),
        "이 파일엔 판정 needle 이 있어 제외 없으면 오탐된다(면제가 실효 있음)"
    );
    assert!(
        !scan_files()
            .iter()
            .any(|f| f.ends_with("shadow_policy_guard.rs")),
        "scan_files 는 자기 파일을 제외해야 한다"
    );
}

#[test]
fn intended_miss_scrim_overlay_is_not_a_shadow_literal() {
    // 의도된 false negative: scrim 은 `from_black_alpha` 배경 오버레이라 Shadow 타입이
    // 아니다 — 이 가드가 일부러 안 잡는다. 판정기를 넓혀 이걸 잡기 시작하면(scrim
    // 오탐) 이 테스트가 실패해 그 결정이 드러난다. 한계가 의도임을 붙박는다.
    assert!(!is_shadow_literal(
        "        p.rect_filled(rect, r, theme.scrim().to_egui());"
    ));
    assert!(!is_shadow_literal(
        "        let s = egui::Color32::from_black_alpha(128);"
    ));
}
