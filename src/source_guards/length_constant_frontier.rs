//! 길이 상수가 `f32` 로 다시 새는 것을 막는 전선(frontier) 가드.
//!
//! [`docs/concepts/typed-length.md`] 는 내부 소스에서 길이를 `PhysicalPx`/`LogicalPx`
//! 로 다루도록 규정한다. 그 규칙의 집행은 셋으로 나뉜다:
//!
//! 1. **두 좌표계를 섞는 것** — 컴파일러가 막는다(`PhysicalPx + LogicalPx` 는 타입 에러).
//! 2. **변환을 빠뜨리는 것** — `src/dpi_conversion_guard.rs` 가 막는다(`.value()` 로 벗긴
//!    뒤 scale factor 를 곱하거나 나누는 형태).
//! 3. **애초에 타입을 안 쓰고 선언하는 것** — 여기다. 앞의 둘은 이미 타입이 붙은 값에만
//!    걸리므로, `const W: f32 = 96.0;` 처럼 처음부터 `f32` 인 상수는 둘 다 통과한다.
//!
//! # 술어는 "전부" 가 아니라 "전선 밖" 이다
//!
//! 본체의 길이 상수 전환은 진행 중이라 `src` 전체가 아직 0 이 아니다. 그래서 술어를
//! 좁혔다 — **면제 목록을 만들지 않고** 아직 안 한 영역을 [`FRONTIER`] 한 줄로 이름
//! 짓고, 그 밖에서는 0 을 요구한다. 전선에는 건수가 박혀 있어 **줄어들 수만 있다**:
//! 전선 안에 새로 늘려도 실패한다.
//!
//! 술어를 "산술에 안 섞이는 상수" 처럼 **쓰임**으로 정의하지 않은 이유는, 그 판정이
//! 함수 경계 추적을 요구하고 그 추적이 이 축에서 계속 틀렸기 때문이다. 오분류 목록이
//! 아직 자라고 있다는 것이 논거다 — 같은 이름의 독립 선언을 한 상수로 셈 · 반환값을
//! 인자로 셈 · 외부 연관함수를 우리 것으로 셈 · 다른 상수의 초기화식 안 산술을 못 봄 ·
//! 여러 줄에 걸친 산술과 호출을 못 봄 · 수신자 자리의 동명 메서드를 못 봄. 가드가 그
//! 추적에 기대면 가드의 정확도가 그 추적의 정확도로 내려앉는다. 여기 술어는 **선언 한
//! 줄만 보고** 결정된다. 근거 전문은 [ADR-0161].
//!
//! # 이 가드가 못 잡는 것
//!
//! 아래는 설계상 사각이고, 각각을 겨냥한 변이 테스트가 사각의 **모양**을 못박는다.
//! 사각을 좁히려면 판정기를 고치고 그 변이를 함께 옮겨라 — 목록만 지우면 사각은 남는다.
//!
//! - `#[cfg(test)]` 안의 선언. 타입 정책이 겨냥하는 것은 화면에 나가는 코드다.
//! - 이름이 [`NON_LENGTH_HINTS`] 에 걸리는 것(`_RATIO`·`_ALPHA` 등). 길이인데 그런
//!   이름을 쓰면 통과한다.
//! - 값이 0 초과 1 미만인 것. 배율과 구분할 선언-국소 수단이 없다.
//! - `const` 가 아닌 것 — `static`, 함수 안의 `let`.
//!
//! # 전선이 왜 선언만 옮겨서는 안 줄어드는가
//!
//! 남은 것을 선언 한 줄씩 `LogicalPx` 로 바꾸면 전선이 0 이 될 것 같지만, 실측하면
//! **남은 전부가 상수 아닌 값과 한 식에서 섞인다**. 그 상대는 두 종류다 — egui 가
//! f32 로 주고 f32 로 받는 사각 기하(`rect.width()`·`vec2`·`TextWrapping.max_width`)
//! 와, 우리가 아직 안 넓힌 f32 관문(`popup::content_margin()` 같은 `-> f32` 헬퍼).
//! 그래서 선언만 옮기면 덧셈 한복판에 `.value()` 가 **새로 생긴다** — 파일 단위로 세면
//! 벗기기 총수가 줄지 않고 늘었다. 타입을 넓히는 대신 타입을 버리는 자리를 만드는 것이라
//! 정책에 반한다.
//!
//! 즉 이 전선의 잔여는 **선언의 성질이 아니라 경계의 성질**이다. 줄이려면 관문 헬퍼의
//! 시그니처를 먼저 넓혀야 하고, egui 기하에 직접 닿는 값은 정책상 남는 것이 맞다
//! (`docs/concepts/typed-length.md` "외부 API 경계에서만 `.value()`").
//!
//! # 왜 `src/` 만 스캔하는가
//!
//! 갤러리(`crates/tasty-gallery`)도 같은 전환을 받고 있지만 잔여가 **한 곳에 안 몰려
//! 있다** — components · widgets · popup_frame · icons 넷으로 갈린다. 전선을 거기까지
//! 넓히면 [`FRONTIER`] 가 줄 하나가 아니라 네 줄이 되고, 그때부터 그것은 전선이 아니라
//! 면제 목록이다(줄이 늘 자리가 생긴다). 갤러리 잔여가 한 곳으로 모이면 그때 넓힌다.

use std::path::Path;

use super::{mask_non_code, rust_sources};

/// 아직 전환하지 않은 영역과 그 시점의 건수. 건수는 상한이라 전선은 줄어들 수만 있다.
const FRONTIER: (&str, usize, &str) = (
    "src/adapters/ui/",
    5,
    "쓰이는 식이 egui 기하이거나 우리 f32 관문이라, 선언이 아니라 그 경계와 함께 닫힌다",
);

/// 이 조각이 이름에 있으면 길이로 세지 않는다. 배율·시간·굵기·투명도처럼 픽셀이
/// 아닌 양들이다.
const NON_LENGTH_HINTS: &[&str] = &[
    "ALPHA", "RATIO", "FACTOR", "OPACITY", "GAMMA", "FRAC", "SCALE", "SPEED", "_MS", "SECS",
    "WEIGHT", "MULT", "PERCENT", "_PCT", "ASPECT", "ZOOM", "DURATION", "_HZ", "_FPS", "DELAY",
    "FADE",
];

/// 마스킹된 소스에서 `f32` 로 선언된 **길이로 보이는** 상수의 (줄번호, 이름).
///
/// 순수 함수다 — 합성 스니펫을 그대로 먹일 수 있다.
fn length_constants(masked: &str) -> Vec<(usize, String)> {
    let masked = blank_test_modules(masked);
    let mut out = Vec::new();
    for (idx, line) in masked.lines().enumerate() {
        let Some((name, value)) = parse_f32_const(line) else {
            continue;
        };
        if NON_LENGTH_HINTS.iter().any(|h| name.contains(h)) {
            continue;
        }
        if value.parse::<f32>().is_ok_and(|v| v > 0.0 && v < 1.0) {
            continue;
        }
        out.push((idx + 1, name));
    }
    out
}

/// `const NAME: f32 = VALUE;` 한 줄에서 이름과 값 문자열. 들여쓰기와 `pub` 가시성
/// 수식어는 무시한다.
fn parse_f32_const(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("pub").map_or(rest, |r| {
        let r = r.trim_start();
        r.strip_prefix('(')
            .and_then(|r| r.split_once(')'))
            .map_or(r, |(_, after)| after.trim_start())
    });
    let rest = rest.trim_start().strip_prefix("const ")?;
    let (name, rest) = rest.split_once(':')?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("f32")?;
    let value = rest.trim_start().strip_prefix('=')?.trim();
    let value = value.strip_suffix(';').unwrap_or(value).trim();
    Some((name.to_string(), value.to_string()))
}

/// `#[cfg(test)] mod ... { ... }` 블록을 줄 구조를 보존한 채 지운다. 중괄호가 없는
/// `#[cfg(test)] mod name;` 은 별도 파일이라 [`test_gated_modules`] 가 따로 뺀다.
fn blank_test_modules(masked: &str) -> String {
    let bytes: Vec<char> = masked.chars().collect();
    let mut out: String = masked.to_string();
    let mut from = 0usize;
    while let Some(rel) = masked[from..].find("#[cfg(test)]") {
        let at = from + rel;
        from = at + "#[cfg(test)]".len();
        let Some(open) = masked[from..].find('{').map(|o| from + o) else {
            continue;
        };
        // 여는 중괄호 앞에 세미콜론이 있으면 그 attribute 는 블록이 아니다.
        if masked[from..open].contains(';') {
            continue;
        }
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in bytes
            .iter()
            .enumerate()
            .skip(masked[..open].chars().count())
        {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_char) = close else { continue };
        let start_byte = open;
        let end_byte = masked
            .char_indices()
            .nth(close_char)
            .map_or(masked.len(), |(b, c)| b + c.len_utf8());
        let region: String = masked[start_byte..end_byte]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out.replace_range(start_byte..end_byte, &region);
    }
    out
}

/// `#[cfg(test)] mod NAME;` 로 선언된 자식 모듈 이름들. 그 모듈의 파일 전체가 테스트다.
fn test_gated_modules(masked: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut prev_was_gate = false;
    for line in masked.lines() {
        let t = line.trim();
        if prev_was_gate
            && let Some(rest) = t.strip_prefix("mod ")
            && let Some(name) = rest.strip_suffix(';')
        {
            out.push(name.trim().to_string());
        }
        prev_was_gate = t == "#[cfg(test)]";
    }
    out
}

/// 레포 전수 스캔 — (경로, 줄번호, 이름).
fn scan() -> Vec<(String, usize, String)> {
    let files = rust_sources();
    let mut gated: Vec<String> = Vec::new();
    for (rel, text) in &files {
        // 자식 모듈이 사는 디렉토리는 `mod.rs` 면 자기 부모, `foo.rs` 면 `foo/` 다.
        let dir = if rel.file_name().is_some_and(|n| n == "mod.rs") {
            rel.parent().unwrap_or(Path::new("")).to_path_buf()
        } else {
            rel.with_extension("")
        };
        let dir = dir.to_string_lossy().replace('\\', "/");
        for name in test_gated_modules(&mask_non_code(text)) {
            gated.push(format!("{dir}/{name}.rs"));
            gated.push(format!("{dir}/{name}/mod.rs"));
        }
    }
    let mut out = Vec::new();
    for (rel, text) in &files {
        let rel = rel.to_string_lossy().replace('\\', "/");
        if !rel.starts_with("src/") || gated.contains(&rel) {
            continue;
        }
        for (line, name) in length_constants(&mask_non_code(text)) {
            out.push((rel.clone(), line, name));
        }
    }
    out
}

#[test]
fn no_f32_length_constant_lives_outside_the_conversion_frontier() {
    let hits = scan();
    let (frontier, budget, why) = FRONTIER;
    let (inside, outside): (Vec<_>, Vec<_>) = hits
        .iter()
        .partition(|(rel, _, _)| rel.starts_with(frontier));

    assert!(
        outside.is_empty(),
        "전선 밖에서 f32 길이 상수가 발견됐다 — `LogicalPx` 로 선언하라:\n{}",
        outside
            .iter()
            .map(|(rel, line, name)| format!("  {rel}:{line}  {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        inside.len() <= budget,
        "전선({frontier}, 사유: {why})의 f32 길이 상수가 {} 개로 상한 {budget} 을 넘었다. \
         전선은 줄어들 수만 있다 — 새 길이 상수는 `LogicalPx` 로 선언하라",
        inside.len()
    );
}

#[test]
fn the_frontier_budget_is_not_slack() {
    // 상한이 실제 건수보다 크면 그만큼 조용히 새 위반을 받아준다. 같아야 한다.
    let inside = scan()
        .into_iter()
        .filter(|(rel, _, _)| rel.starts_with(FRONTIER.0))
        .count();
    assert_eq!(
        inside, FRONTIER.1,
        "전선 건수가 바뀌었다. 줄였으면 FRONTIER 의 수도 같이 줄여라(늘었으면 위 테스트가 잡는다)"
    );
}

#[cfg(test)]
mod detector {
    use super::*;

    #[test]
    fn it_reads_the_declaration_line_only() {
        assert_eq!(
            length_constants("const PANEL_WIDTH: f32 = 96.0;"),
            vec![(1, "PANEL_WIDTH".to_string())]
        );
        assert_eq!(
            length_constants("    pub const ROW_H: f32 = 28.0;"),
            vec![(1, "ROW_H".to_string())]
        );
        assert_eq!(
            length_constants("pub(crate) const GAP: f32 = 4.0;"),
            vec![(1, "GAP".to_string())]
        );
        // 이미 타입이 붙었으면 대상이 아니다.
        assert!(length_constants("const PANEL_WIDTH: LogicalPx = LogicalPx(96.0);").is_empty());
        // 상수가 아닌 선언은 이 가드의 사각이다.
        assert!(length_constants("static PANEL_WIDTH: f32 = 96.0;").is_empty());
        assert!(length_constants("let panel_width: f32 = 96.0;").is_empty());
    }

    #[test]
    fn it_skips_the_shapes_named_in_the_module_doc() {
        // 이름 힌트.
        assert!(length_constants("const SPLIT_RATIO: f32 = 0.3;").is_empty());
        assert!(length_constants("const HOVER_ALPHA: f32 = 12.0;").is_empty());
        // 0 초과 1 미만.
        assert!(length_constants("const HAIRLINE: f32 = 0.5;").is_empty());
        // 이름 힌트에도 값 범위에도 안 걸리는 배율은 잡힌다 — 알려진 오탐이다.
        assert_eq!(
            length_constants("const DOUBLE: f32 = 2.0;"),
            vec![(1, "DOUBLE".to_string())]
        );
    }

    #[test]
    fn it_blanks_test_modules_without_moving_line_numbers() {
        let src = "const A_WIDTH: f32 = 1.0;\n#[cfg(test)]\nmod t {\n    const B_WIDTH: f32 = 2.0;\n}\nconst C_WIDTH: f32 = 3.0;\n";
        assert_eq!(
            length_constants(src),
            vec![(1, "A_WIDTH".to_string()), (6, "C_WIDTH".to_string())]
        );
    }

    #[test]
    fn it_names_test_gated_child_modules() {
        assert_eq!(
            test_gated_modules("mod a;\n#[cfg(test)]\nmod b;\nmod c;\n"),
            vec!["b".to_string()]
        );
    }
}
