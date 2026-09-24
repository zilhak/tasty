//! 길이로 보이는 f32·f64 상수의 미전환 범위를 기록하고 그 밖의 새 선언을 금지한다.
//! 길이는 LogicalPx·PhysicalPx로 표현해야 하지만 선언의 쓰임까지 추적하지 못해 이름·값으로 추정한다.
//!
//! 등록한 src·갤러리·플랫폼 소스만 검사한다. test 전용 코드, 이름이 NON_LENGTH_HINTS에 해당하는 상수,
//! 0 초과 1 미만 값, static·let 선언은 제외한다. 다른 크레이트는 검사하지 않는다.
//! 상수 이름이나 값만으로 길이와 배율을 완전히 구별할 수 없어 누락과 오탐이 모두 가능하다.
//!
//! 미전환 영역의 건수는 늘 수 없고 줄면 기록도 함께 줄인다.
//! 선언만 타입으로 바꾼 뒤 내부 산술에서 바로 .value()로 벗기지 않도록 소비하는 필드·함수의 타입도
//! 먼저 검토한다. 외부 egui·winit API에 값을 넘기는 경계와 내부 타입 미전환을 구별한다.

use super::test_gate::blank_test_modules;
use super::{mask_non_code, rust_sources};

const SCANNED: &[&str] = &["src/", "crates/tasty-gallery/", "crates/tasty-platform/"];

/// 미전환 범위별 건수와 사유. 한 영역의 감소가 다른 영역의 증가를 가리지 않도록 따로 기록한다.
const FRONTIERS: &[(&str, usize, &str)] = &[
    (
        "crates/tasty-platform/src/window_chrome.rs",
        1,
        "winit 이 창 좌표를 f64 로 주고 이 판정이 그 좌표와 직접 비교된다 — 경계가 f64 다",
    ),
    (
        "src/app/modal/shake.rs",
        1,
        "흔들기 오프셋이 winit outer_position(f64)에 그대로 더해진다 — 같은 f64 경계",
    ),
];

/// 픽셀 길이가 아닐 가능성을 나타내는 이름 조각. 길이에 이 이름을 써도 검사에서 제외되는 한계가 있다.
const NON_LENGTH_HINTS: &[&str] = &[
    "ALPHA", "RATIO", "FACTOR", "OPACITY", "GAMMA", "FRAC", "SCALE", "SPEED", "_MS", "SECS",
    "WEIGHT", "MULT", "PERCENT", "_PCT", "ASPECT", "ZOOM", "DURATION", "_HZ", "_FPS", "DELAY",
    "FADE", "FREQ",
];

/// 한 줄의 f32·f64 const 선언에서 길이 후보를 찾는다. 줄 번호는 1부터 시작한다.
fn length_constants(masked: &str) -> Vec<(usize, String)> {
    let masked = blank_test_modules(masked);
    let mut out = Vec::new();
    for (idx, line) in masked.lines().enumerate() {
        let Some((name, value)) = parse_bare_float_const(line) else {
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

fn parse_bare_float_const(line: &str) -> Option<(String, String)> {
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
    let rest = rest
        .strip_prefix("f32")
        .or_else(|| rest.strip_prefix("f64"))?;
    let value = rest.trim_start().strip_prefix('=')?.trim();
    let value = value.strip_suffix(';').unwrap_or(value).trim();
    Some((name.to_string(), value.to_string()))
}

fn scan() -> Vec<(String, usize, String)> {
    let files = rust_sources();
    // 파일의 test 전용 여부는 shipping_scope의 모듈 선언·타깃 분석을 사용한다.
    let gated = tasty_doc_guards::shipping_scope::test_only_files(&super::repo_root(), &files);
    let mut out = Vec::new();
    for (path, text) in &files {
        if gated.contains(path) {
            continue;
        }
        let rel = path.to_string_lossy().replace('\\', "/");
        if !SCANNED.iter().any(|root| rel.starts_with(root)) {
            continue;
        }
        for (line, name) in length_constants(&mask_non_code(text)) {
            out.push((rel.clone(), line, name));
        }
    }
    out
}

#[test]
fn no_bare_float_length_constant_lives_outside_the_conversion_frontier() {
    let hits = scan();
    let outside: Vec<_> = hits
        .iter()
        .filter(|(rel, _, _)| !FRONTIERS.iter().any(|(f, _, _)| rel.starts_with(f)))
        .collect();

    assert!(
        outside.is_empty(),
        "미전환 범위 밖에서 길이로 보이는 부동소수 상수를 찾았다. LogicalPx/PhysicalPx 타입으로 선언한다:\n{}",
        outside
            .iter()
            .map(|(rel, line, name)| format!("  {rel}:{line}  {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    for (frontier, budget, why) in FRONTIERS {
        let n = hits
            .iter()
            .filter(|(rel, _, _)| rel.starts_with(frontier))
            .count();
        assert!(
            n <= *budget,
            "미전환 범위 {frontier}의 상수가 {n}개로 상한 {budget}을 넘었다(사유: {why}). 새 길이 상수에는 타입을 지정한다."
        );
    }
}

#[test]
fn every_scanned_unit_actually_has_files() {
    let files = rust_sources();
    for root in SCANNED {
        let n = files
            .iter()
            .filter(|(rel, _)| rel.to_string_lossy().replace('\\', "/").starts_with(root))
            .count();
        assert!(
            n > 0,
            "검사 범위 `{root}`에서 파일을 찾지 못했다. 이동·경로 오류를 확인한다."
        );
    }
}

#[test]
fn the_frontier_budget_is_not_slack() {
    // 건수가 줄었는데 상한을 그대로 두면 새 미전환 선언을 허용하므로 범위별로 정확히 맞춘다.
    let hits = scan();
    for (frontier, budget, _) in FRONTIERS {
        let n = hits
            .iter()
            .filter(|(rel, _, _)| rel.starts_with(frontier))
            .count();
        assert_eq!(
            n, *budget,
            "미전환 범위 `{frontier}`의 건수가 바뀌었다. 줄었다면 FRONTIERS의 상한도 낮춘다. 증가 여부는 별도 검사에서 확인한다."
        );
    }
}

#[test]
fn every_frontier_line_points_at_something() {
    let files = rust_sources();
    for (frontier, _, _) in FRONTIERS {
        let n = files
            .iter()
            .filter(|(rel, _)| {
                rel.to_string_lossy()
                    .replace('\\', "/")
                    .starts_with(frontier)
            })
            .count();
        assert!(
            n > 0,
            "미전환 범위 `{frontier}`에서 파일을 찾지 못했다. 해당 상한의 대상 경로를 확인한다."
        );
    }
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
        assert_eq!(
            length_constants("const RESIZE_EDGE_MARGIN: f64 = 8.0;"),
            vec![(1, "RESIZE_EDGE_MARGIN".to_string())]
        );
        assert!(length_constants("const PANEL_WIDTH: LogicalPx = LogicalPx(96.0);").is_empty());
        assert!(length_constants("static PANEL_WIDTH: f32 = 96.0;").is_empty());
        assert!(length_constants("let panel_width: f32 = 96.0;").is_empty());
    }

    #[test]
    fn it_skips_the_shapes_named_in_the_module_doc() {
        assert!(length_constants("const SPLIT_RATIO: f32 = 0.3;").is_empty());
        assert!(length_constants("const HOVER_ALPHA: f32 = 12.0;").is_empty());
        assert!(length_constants("const SHAKE_FREQUENCY: f64 = 3.0;").is_empty());
        assert!(length_constants("const HAIRLINE: f32 = 0.5;").is_empty());
        // 이름·값으로 배율을 구별하지 못하는 알려진 오탐이다.
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
}
