//! 시각 토큰 드리프트 가드 — **UI 폰트 토큰 값을 복사한 이름 상수**를 막는다.
//!
//! # 왜 `tests/` 가 아니라 여기인가 (관례를 깬 이유)
//!
//! 소스 스캔 가드의 관례 자리는 `tests/*.rs` 이고 자매 가드
//! (`tests/design_token_adherence.rs`)도 거기 있다. 이 파일만 본체 crate 안에 있는
//! 이유는 **자동으로 실행되는 잡의 수가 자리마다 다르기** 때문이다. 직접 재서 얻은
//! 값만 적는다(채널 표 전체는 여기 옮겨 적지 않는다 — 아래 정본 참조):
//!
//! ```text
//! 이 자리(본체 crate 의 #[cfg(test)] 모듈)   자동 실행 잡 2 개
//! tests/*.rs (자매 가드의 자리)              자동 실행 잡 1 개
//! ```
//!
//! 차이는 **한 잡**이다. 워크스페이스 전체를 도는 잡은 통합 테스트도 실행하므로 양쪽을
//! 다 보지만, `--lib --bins` 로 좁혀 도는 잡은 `tests/*.rs` 를 **타깃으로 만들지도
//! 않는다.** 그래서 이 파일을 `tests/` 로 "관례에 맞춰" 옮기면 **자동 실행이 사라지는
//! 것이 아니라 두 잡 중 하나로 준다.** 소스 스캔 가드는 실행되지 않으면 존재하지 않는
//! 것과 같아서(본체가 스캔이라 컴파일만으로는 아무것도 검사되지 않는다) 그 한 잡의
//! 차이를 기꺼이 산다 — **관례를 깨는 근거는 "0 대 1" 이 아니라 "1 대 2" 다.**
//!
//! > **이 문단의 만료 조건**(사람이 읽는 조건이 아니라 판정 가능한 형태로 적는다):
//! > `.github/workflows/crossplatform-check.yml` 의 단위 테스트 스텝이 `--lib --bins`
//! > 를 **포함하고** 헤드리스 테스트 스텝이 **포함하지 않을** 때만 위 "1 대 2" 가 참이다.
//! > 두 스텝이 같은 범위로 수렴하면 차이가 0 이 되고 이 파일이 여기 있을 이유도 사라진다.
//! > 한때 반대 방향으로 틀렸던 적이 있다 — 헤드리스가 `--lib --bins` 로 좁혀져 있던
//! > 시절의 서술("`tests/` 는 자동 실행 채널이 없다")을 그 스텝이 넓어진 뒤에도 들고
//! > 있었다. **근거를 적는 것이 근거의 유효성을 지켜 주지 않는다.**
//!
//! 그리고 그 잡들이 **초록인지는 별개로 확인해야 한다** — 채널이 있다는 것은 채널이
//! 건강하다는 뜻이 아니다. 한 층 더 있다: **잡이 애초에 발화하는가** 도 따로다(트리거가
//! 이 레포에서 일어나지 않는 이벤트면 그 잡은 active 로 보이면서 실행 이력이 0 이다).
//! 트리거·러너를 포함한 채널 정본은
//! [`docs/dev-guide/ci-gates.md`](../docs/dev-guide/ci-gates.md) 하나다.
//!
//! (`--lib --bins` 로 좁혀 도는 잡은 Windows 러너라 CRLF 체크아웃을 계산에 넣었다 —
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
///
/// **그 "갈라지면 안 된다" 는 오래 주석으로만 있었고 실제로 갈라졌다.** 자매 쪽이
/// `src/gfx/gpu` 를 디렉토리로 넓히는 동안 이쪽은 `shell_setup.rs` 한 파일로 남았다.
/// 지금은 [`the_two_sister_guards_scan_the_same_roots`] 가 그 불변식을 판정한다.
///
/// 개별 `.rs` 파일을 루트로 등재하지 않는다 — 그 디렉토리에 나중에 생기는 파일이 기본
/// 제외가 되고, 그 누락은 아무 신호도 내지 않는다
/// (`docs/adr/0133-guard-scan-population-is-pinned-not-enumerated.md`).
const SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu",
    "crates/tasty-gallery/src",
    "crates/tasty-ui-widgets/src",
    "crates/tasty-egui-theme/src",
];

/// 자매 가드의 소스에서 `const SCAN_ROOTS` 블록의 문자열 리터럴을 뽑는다.
///
/// 두 가드는 각각 lib 유닛과 통합 타깃이라 상수를 **공유할 수 없다**(통합 테스트의
/// 아이템은 본체 crate 에서 안 보인다). 그래서 값을 나누는 대신 소스를 읽어 대조한다 —
/// 이 파일은 이미 같은 방식으로 소스를 스캔하므로 새 의존이 아니다.
fn sister_scan_roots(src: &str) -> Vec<String> {
    let Some(start) = src.find("const SCAN_ROOTS: &[&str] = &[") else {
        return Vec::new();
    };
    let rest = &src[start..];
    let Some(end) = rest.find("];") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in rest[..end].lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut in_string = false;
        let mut buf = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if c == '"' {
                    out.push(std::mem::take(&mut buf));
                    in_string = false;
                } else {
                    buf.push(c);
                }
            } else if c == '"' {
                in_string = true;
            } else if c == '/' && chars.get(i + 1) == Some(&'/') {
                // 주석 — 줄의 나머지는 코드가 아니다. **줄 앞이든 뒤든 자른다.**
                // 자매 파일의 이 블록에는 실제로 자유 주석이 있고, 거기에 경로를
                // 따옴표로 인용하면(`// "src/gfx" 는 제외`) 그것이 루트로 뽑혀
                // `assert_eq!` 가 **거짓 빨강**을 낸다. 방향은 안전하지만 메시지가
                // "두 자매 가드의 스캔 루트가 갈라졌다" 라 원인을 잘못 가리킨다.
                break;
            }
            i += 1;
        }
    }
    out
}

/// UI 폰트 스케일의 값 — `font_size_micro`(10) · `caption`(11) · `body`(13) ·
/// `heading`(13) · `max`(14). 이 다섯이 UI 스케일 전부다
/// (`docs/design/systems/theme.md` "UI 폰트 스케일").
///
/// 콘텐츠 폰트(`font_size_term_sm`(12) · `term`(14) · `term_lg`(16) · `prose_h1`(20))는
/// 스케일이 다르지만 `term` 은 값이 14 라 `max` 와 겹친다. 겹쳐도 판정은 옳다 —
/// 어느 쪽 의도든 **토큰을 쓰라**가 답이고, 어느 토큰인지는 고치는 사람이 정한다.
const UI_FONT_TOKEN_VALUES: &[f32] = &[10.0, 11.0, 13.0, 14.0];

/// **UI semantic 이 배정되지 않은 DTCG primitive 폰트 값.** ADR-0126 은 이 자리를
/// 명명 const 로 두는 것을 허용하되 **이름에 primitive 임을 남기라**고 요구한다 —
/// 호출 자리에서 "토큰인가 미배정 primitive 인가" 가 이름만으로 갈리게 하려는 것이다.
///
/// 값은 ADR 본문이 명시한 둘(12 · 16)만 본다. 다른 primitive(17 · 20)까지 넓히지 않은
/// 이유는 그것들이 semantic 을 갖고 있어(brand-wordmark · prose) 같은 처지가 아니기
/// 때문이다 — 규칙이 쓰여 있는 범위만 강제한다.
const UNMAPPED_PRIMITIVE_FONT_VALUES: &[f32] = &[12.0, 16.0];

/// 폰트 크기를 받는 호출 형태. 접두 뒤 첫 인자가 크기다.
const FONT_CALLS: &[&str] = &[
    ".size(",
    "FontId::proportional(",
    "FontId::monospace(",
    "FontId::new(",
];

/// **UI 반경 토큰의 값** — `SIZING.corner_radius_sm`(2) · `corner_radius`(4) ·
/// `corner_radius_lg`(8). 주석이 아니라 `theme.rs:410-412` 의 정의에서 온 값이다.
///
/// 이 축이 폰트 축보다 나쁜 이유: `corner_radius*` 는 `Theme` 에서 `zoomed()` 를
/// **타고**(`theme.rs` 의 `corner_radius: zoomed(SIZING.corner_radius)`) 명명 const 는
/// 안 탄다. 그래서 반경 사본은 배율 0.85 / 1.2 에서 토큰과 **다른 픽셀로 그려진다** —
/// 폰트 사본과 달리 시각적 회귀가 실재한다.
const UI_RADIUS_TOKEN_VALUES: &[f32] = &[2.0, 4.0, 8.0];

/// 반경을 받는 호출 형태. 자매 가드(`tests/design_token_adherence.rs`)의
/// `FORBIDDEN_PREFIXES` 와 같은 둘이다 — 그쪽은 **리터럴**을, 여기는 **토큰 값을 복사한
/// 명명 const** 를 막는다. 두 판정이 합쳐져야 이 축의 우회로가 닫힌다.
const RADIUS_CALLS: &[&str] = &[".corner_radius(", "CornerRadius::same("];

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

/// 한 줄에서 **폰트 자리에 온 명명 상수**를 전부 뽑는다 — `(이름, 호출 시작 위치)`.
///
/// 파일 순회·경로 처리와 분리한 **순수 함수**라, 판정을 합성 문자열로 찌를 수 있다.
/// 두 판정기(`const_font_violations` · `primitive_name_violations`)가 이것을 공유하므로
/// "어디까지가 폰트 자리인가" 의 정의가 한 곳에만 있다.
///
/// 한 줄에 폰트 호출이 둘 이상 올 수 있다 — 첫 개만 보면 뒤 호출이 앞 호출의 판정
/// (특히 스피너 면제)에 묻힌다. 변이로 확인한 실제 누락이라 커서로 전부 훑는다.
fn font_call_args(line: &str) -> Vec<(String, usize)> {
    call_args(line, FONT_CALLS)
}

/// 주어진 호출 형태들의 **첫 인자가 명명 상수인** 자리를 모은다.
///
/// 경로 수식을 벗긴다 — `tasty_ui_widgets::tokens::BOOT_CARD_CORNER_RADIUS` 는
/// `BOOT_CARD_CORNER_RADIUS` 로 본다. 벗기지 않으면 **크레이트를 넘어 온 const 를
/// 통째로 놓친다**: 반경 호출자리의 명명 const 는 다수가 이 형태다(실측). 상수 표
/// (`collect_numeric_consts`)의 키가 벌거벗은 이름이라 마디를 맞춰야 조인된다.
fn call_args(line: &str, calls: &[&str]) -> Vec<(String, usize)> {
    let mut hits = Vec::new();
    for call in calls {
        let mut cursor = 0usize;
        while let Some(rel_at) = line[cursor..].find(call) {
            let at = cursor + rel_at;
            cursor = at + call.len();
            let rest = &line[cursor..];
            let Some(end) = rest.find([',', ')']) else {
                continue;
            };
            let arg = rest[..end].trim();
            let arg = arg.rsplit("::").next().unwrap_or(arg).trim();
            if is_screaming_snake(arg) {
                hits.push((arg.to_string(), at));
            }
        }
    }
    hits
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
        for (arg, at) in font_call_args(line) {
            if spinner_receiver(lines, i, &line[..at]) {
                continue;
            }
            let Some((_, value)) = consts.iter().find(|(n, _)| *n == arg) else {
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

/// 미배정 primitive 값을 가진 폰트 const 가 **이름에 그 사실을 담고 있는가**.
///
/// 이 축만은 판별이 **이름**이다 — 규칙 자체가 이름에 대한 것이기 때문이다(ADR-0126).
/// 다른 곳에서 "면제는 이름이 아니라 구조로" 를 지키는 것과 모순이 아니다: 저긴 *무엇을
/// 빼줄지*를 이름으로 정하지 말라는 것이고, 여긴 *이름이 규칙의 대상*이다.
///
/// 면제는 없다. 위반 자리를 먼저 전부 이름에 맞춘 뒤 이 가드를 넣었기 때문이다 —
/// 순서가 반대면 allowlist 를 부풀리게 된다.
fn primitive_name_violations(
    rel: &str,
    lines: &[&str],
    consts: &[(String, f32)],
    out: &mut Vec<String>,
) {
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for (arg, at) in font_call_args(line) {
            if spinner_receiver(lines, i, &line[..at]) {
                continue;
            }
            let Some((_, value)) = consts.iter().find(|(n, _)| *n == arg) else {
                continue;
            };
            if !UNMAPPED_PRIMITIVE_FONT_VALUES.contains(value) {
                continue;
            }
            let want = format!("PRIMITIVE_{}", *value as i64);
            if !arg.contains(&want) {
                out.push(format!(
                    "  {}:{} — `{}` = {} 는 semantic 이 없는 primitive 다. \
                     이름에 `{}` 를 담을 것",
                    rel,
                    i + 1,
                    arg,
                    value,
                    want
                ));
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

/// 스캔 루트를 읽어 `(파일 목록, 이름→값 표)` 를 만든다. 두 테스트가 공유한다.
///
/// 이름은 파일을 넘어 참조되므로 표는 **루트 전체에서 하나로** 모은 뒤에야 판정할 수
/// 있다 — 그래서 읽기와 판정이 두 단계다.
fn scan_sources() -> (Vec<(String, String)>, Vec<(String, f32)>) {
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
    (sources, consts)
}

/// 반경 자리에 온 명명 상수의 값이 반경 토큰 값이면 위반이다.
///
/// 폰트 쪽 `const_font_violations` 와 같은 축(**값 × 자리**)이고, 다른 점은 예외가
/// 없다는 것이다 — 폰트에는 스피너 수신자 예외가 있지만 반경에는 그런 자리가 없다.
fn const_radius_violations(
    rel: &str,
    lines: &[&str],
    consts: &[(String, f32)],
    out: &mut Vec<String>,
) {
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for (arg, _) in call_args(line, RADIUS_CALLS) {
            let Some((_, value)) = consts.iter().find(|(n, _)| *n == arg) else {
                continue;
            };
            if UI_RADIUS_TOKEN_VALUES.contains(value) {
                out.push(format!(
                    "  {}:{} — `{}` = {} 는 UI 반경 토큰과 같은 값이다",
                    rel,
                    i + 1,
                    arg,
                    value
                ));
            }
        }
    }
}

/// 반경 축의 **명명 const 우회로**를 닫는다. 자매 가드는 리터럴만 막으므로,
/// `const FOO: f32 = 8.0;` 를 만들어 `.corner_radius(FOO)` 로 쓰면 둘 다 통과했다.
///
/// ADR-0126 이 이 자리를 "자동 채널이 없는 유일한 트리거" 로 적어 뒀고, 그 트리거가
/// 사람 리뷰에서 실제로 발화해 이 판정이 생겼다.
#[test]
fn no_named_const_copies_a_radius_token() {
    let (sources, consts) = scan_sources();
    let mut violations = Vec::new();
    for (rel, contents) in &sources {
        let lines: Vec<&str> = contents.lines().collect();
        const_radius_violations(rel, &lines, &consts, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "UI 반경 토큰 값을 복사한 이름 상수가 반경 자리에 있다 — 토큰을 직접 쓸 것\n\
         (`th.corner_radius_sm/corner_radius/corner_radius_lg`):\n\
         · const 는 `ui_zoom` 을 타지 않는데 `corner_radius*` 토큰은 **탄다**. \
         배율 0.85 / 1.2 에서 사본만 고정돼 다른 픽셀로 그려진다\n\
         · 스케일 **밖** 값(3 · 6 · 12 …)의 명명 const 는 그대로 허용된다 — \
         금지되는 것은 토큰 값(2 · 4 · 8)의 **복사본**뿐이다(ADR-0126)\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_named_const_copies_a_ui_font_token() {
    let (sources, consts) = scan_sources();
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

/// **두 자매 가드가 같은 루트를 본다** — 이 파일의 `SCAN_ROOTS` doc 이 오래 주석으로만
/// 주장하던 불변식이다. 실제로 한 번 갈라졌고(자매가 `src/gfx/gpu` 를 디렉토리로 넓히는
/// 동안 이쪽은 파일 하나로 남았다), 갈라진 동안 **양쪽 다 초록이었다.**
///
/// 두 가드가 같은 축(디자인 토큰 리터럴/사본)을 보므로 모수가 다르면 한쪽만 보는 사각이
/// 생긴다. 상수를 공유할 수 없어(통합 테스트 아이템은 본체 crate 에서 안 보인다) 소스를
/// 읽어 대조한다.
///
/// 자매 파일이 없거나 형태가 바뀌어 파싱이 0 을 내면 **조용히 통과하지 않는다** — 빈
/// 목록도 실패로 본다.
#[test]
fn the_two_sister_guards_scan_the_same_roots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sister = root.join("tests/design_token_adherence.rs");
    let src = std::fs::read_to_string(&sister)
        .unwrap_or_else(|e| panic!("자매 가드를 읽지 못했다 ({}): {e}", sister.display()));

    let theirs = sister_scan_roots(&src);
    assert!(
        !theirs.is_empty(),
        "자매 가드에서 `const SCAN_ROOTS` 를 못 뽑았다 — 형태가 바뀌었으면 \
         `sister_scan_roots` 도 함께 고칠 것. 0 개를 통과로 세지 않는다"
    );

    let mine: Vec<String> = SCAN_ROOTS.iter().map(|s| (*s).to_string()).collect();
    let mut a = mine.clone();
    let mut b = theirs.clone();
    a.sort();
    b.sort();
    assert_eq!(
        a, b,
        "두 자매 가드의 스캔 루트가 갈라졌다 — 같은 축을 보므로 한쪽만 넓히면 \
         나머지 한쪽에 사각이 생긴다"
    );

    // 그리고 그 루트는 디렉토리여야 한다 — 파일 열거는 새 파일을 기본 제외로 만든다.
    for r in SCAN_ROOTS {
        assert!(
            !r.ends_with(".rs"),
            "스캔 루트에 개별 파일이 있다: {r}. 디렉토리로 쓸 것"
        );
    }
}

#[test]
fn unmapped_primitive_font_consts_say_so_in_their_name() {
    let (sources, consts) = scan_sources();
    let mut violations = Vec::new();
    for (rel, contents) in &sources {
        let lines: Vec<&str> = contents.lines().collect();
        primitive_name_violations(rel, &lines, &consts, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "semantic 이 없는 DTCG primitive 폰트 값을 쓰는 const 가 이름에 그 사실을 담고 \
         있지 않다 — ADR-0126 이 요구하는 형태다:\n\
         · 호출 자리에서 `.size(FOO_SIZE)` 만 보고는 그게 토큰인지 미배정 primitive 인지 \
         알 수 없다. 이름이 그걸 말해야 한다\n\
         · 선례: `ATTN_PRIMITIVE_12`\n\
         · semantic 이 생기면 const 가 통째로 사라지고 이 규칙도 그 자리에서 끝난다\n{}",
        violations.join("\n")
    );
}

#[cfg(test)]
mod discriminate {
    use super::*;

    /// `SCAN_ROOTS` 블록 파서가 **주석 안의 따옴표를 루트로 읽지 않는다**.
    ///
    /// 이 블록에는 자유 주석이 들어간다(자매 파일에 실제로 있다). 거기에 경로를
    /// 인용하면 파서가 그것을 루트로 뽑아 자매 대조가 **거짓 빨강**을 낸다 —
    /// 방향은 안전하지만 메시지가 원인을 잘못 가리키므로 진단이 헛돈다.
    #[test]
    fn the_scan_root_parser_ignores_quotes_inside_comments() {
        let src = "\
const SCAN_ROOTS: &[&str] = &[
    \"src\",
    // \"src/gfx\" 는 제외한다 — 이 따옴표는 루트가 아니다
    \"crates\", // \"tests\" 는 여기 없다
];
";
        assert_eq!(
            sister_scan_roots(src),
            vec!["src".to_string(), "crates".to_string()],
            "주석 안의 따옴표가 루트로 새어 들어왔다"
        );
    }

    /// 반경 축의 **전제 검사**. 폰트 축과 같은 축(값 × 자리)인지, 그리고 경로 수식을
    /// 벗기는 것이 실제로 동작하는지를 함께 잰다 — 반경 호출자리의 명명 const 는 다수가
    /// `크레이트::모듈::NAME` 형태라, 안 벗기면 **가드가 있어도 하나도 안 잡힌다.**
    #[test]
    fn the_radius_axis_strips_paths_and_checks_value_and_position() {
        let consts = vec![
            ("PILL_RADIUS".to_string(), 8.0_f32), // 토큰 값 = corner_radius_lg
            ("BOOT_CARD_CORNER_RADIUS".to_string(), 12.0), // 스케일 밖 — 허용
            ("SMALL_R".to_string(), 2.0),         // 토큰 값 = corner_radius_sm
        ];
        let check = |lines: &[&str]| {
            let mut out = Vec::new();
            const_radius_violations("f.rs", lines, &consts, &mut out);
            out
        };

        // ① 벌거벗은 이름 — 잡힌다
        assert_eq!(check(&["    .corner_radius(PILL_RADIUS)"]).len(), 1);
        // ② 경로 수식 — 벗기지 않으면 놓친다. 이 단언이 그 벗기기를 못박는다.
        assert_eq!(
            check(&["    .corner_radius(tasty_ui_widgets::tokens::PILL_RADIUS)"]).len(),
            1,
            "경로 수식된 const 를 놓쳤다 — 실제 호출자리의 지배적 형태다"
        );
        // ③ 다른 생성자 형태
        assert_eq!(check(&["    CornerRadius::same(SMALL_R)"]).len(), 1);
        // ④ 스케일 밖 값은 허용 — ADR-0126 이 명시한 형태다
        assert_eq!(
            check(&["    .corner_radius(BOOT_CARD_CORNER_RADIUS)"]).len(),
            0
        );
        // ⑤ 자리가 다르면 이 판정의 대상이 아니다(값은 같아도)
        assert_eq!(check(&["    .size(PILL_RADIUS)"]).len(), 0);
        // ⑥ 주석은 코드가 아니다
        assert_eq!(check(&["    // .corner_radius(PILL_RADIUS)"]).len(), 0);
        // ⑦ 표에 없는 이름은 값을 모르므로 판정하지 않는다(고발 기본값 금지)
        assert_eq!(check(&["    .corner_radius(UNKNOWN_RADIUS)"]).len(), 0);
    }

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

    /// **주석 스킵을 겨냥한 변이.** 판정기는 `//` 로 *시작하는* 줄만 건너뛴다 —
    /// 창을 "주석을 포함한 줄" 로 넓히면 뒤따르는 주석 한 조각이 진짜 위반을 가린다
    /// (706 이 `ci_channel_claims_match_workflows` 에서 정확히 그 형태로 뚫렸다).
    #[test]
    fn the_comment_skip_is_line_start_only() {
        let consts = vec![("BODY_FONT_SIZE".to_string(), 13.0_f32)];
        let check = |l: &str| {
            let mut out = Vec::new();
            const_font_violations("f.rs", &[l], &consts, &mut out);
            out.len()
        };
        // 주석 줄 — 안 센다(의도).
        assert_eq!(check("    // .size(BODY_FONT_SIZE)"), 0);
        // 코드 뒤에 주석이 붙은 줄 — **센다.** 주석이 사면권이 되면 안 된다.
        assert_eq!(check("    .size(BODY_FONT_SIZE), // 임시"), 1);
        assert_eq!(check("    .size(BODY_FONT_SIZE), /* 임시 */"), 1);
    }

    /// **의도된 false negative 를 고정한다.** 아래는 못 잡는 것이 설계다 — 나중에 누가
    /// 판정기를 넓히면 그 결정이 여기서 실패로 드러나고, 의도된 한계와 버그가 섞이지
    /// 않는다.
    #[test]
    fn the_intended_false_negatives_stay_false_negative() {
        let consts = vec![("BODY_FONT_SIZE".to_string(), 13.0_f32)];
        let check = |l: &str| {
            let mut out = Vec::new();
            const_font_violations("f.rs", &[l], &consts, &mut out);
            out.len()
        };
        // 소문자 지역 변수 — 값의 출처를 소스 스캔으로 따라갈 수 없다.
        assert_eq!(check("    .size(body_font_size)"), 0);
        // 인라인 리터럴 — 이 가드가 아니라 자매 가드(`design_token_adherence`)의 축이다.
        assert_eq!(check("    .size(13.0)"), 0);
        // 계산식 경유 — const 표가 리터럴 정의만 따라간다.
        assert_eq!(check("    .size(BODY_FONT_SIZE * 1.0)"), 0);
    }

    /// primitive 이름 규칙 판정기도 같은 축(값 + 위치)에서 돈다. 이름 검사는 **규칙의
    /// 대상이 이름이기 때문**이지, 판별을 이름으로 대신하는 것이 아니다.
    #[test]
    fn the_primitive_name_rule_checks_value_first() {
        let consts = vec![
            ("FOO_SIZE".to_string(), 12.0_f32),
            ("BAR_PRIMITIVE_12".to_string(), 12.0),
            ("BAZ_SIZE".to_string(), 16.0),
            ("QUX_PRIMITIVE_16".to_string(), 16.0),
            ("OFF_SCALE".to_string(), 12.5),
            ("UI_TOKEN".to_string(), 13.0),
            ("SPIN".to_string(), 16.0),
        ];
        let check = |lines: &[&str]| {
            let mut out = Vec::new();
            primitive_name_violations("f.rs", lines, &consts, &mut out);
            out.len()
        };
        assert_eq!(check(&["    .size(FOO_SIZE)"]), 1);
        assert_eq!(check(&["    .size(BAR_PRIMITIVE_12)"]), 0);
        assert_eq!(check(&["    .size(BAZ_SIZE)"]), 1);
        assert_eq!(check(&["    .size(QUX_PRIMITIVE_16)"]), 0);
        // 스케일 밖 값과 UI 토큰 값은 이 축이 아니다.
        assert_eq!(check(&["    .size(OFF_SCALE)"]), 0);
        assert_eq!(check(&["    .size(UI_TOKEN)"]), 0);
        // 폰트 자리가 아니면(스피너 지름) 값이 12·16 이어도 축 밖이다.
        assert_eq!(check(&["    Spinner::new().size(SPIN).show(ui, th);"]), 0);
        // 이름이 맞아도 값이 다른 primitive 면 잡힌다 — 이름만 보지 않는다.
        assert_eq!(
            check(&["    .size(QUX_PRIMITIVE_16)", "    .size(FOO_SIZE)"]),
            1
        );
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
