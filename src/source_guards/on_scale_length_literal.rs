//! 길이 생성자와 등록된 egui 기하 호출에서 size-* 값과 같은 숫자 리터럴을 센다.
//! 길이 타입 사용 여부를 검사하는 length_constant_frontier와 달리, 이름 있는 토큰 대신 숫자를 쓴 후보를 찾는다.
//! 반경·폰트 크기는 다른 스케일이므로 여기서 검사하지 않는다. 토큰 목록은 생성 파일에서 읽는다.
//!
//! 0, clamp·비교의 1, 등록된 정규화 좌표·전시용 값, test 전용 코드와 토큰 선언은 별도로 집계한다.
//! 지역 변수의 숫자는 길이 문맥이 없으면 찾지 못하며 스케일 밖 값도 검사하지 않는다.
//! 숫자 양옆의 곱셈·나눗셈은 배율로 추정해 제외한다. 이름·텍스트 문맥만으로 실제 단위를 입증하지는 못한다.
//!
//! 영역별 남은 수는 정확히 맞아야 한다. 줄면 기록도 낮추고, 늘면 실제 새 사용·스케일 확대·
//! 수집 방식 변경을 구별한다. 당장 토큰을 쓸 수 없다면 역할과 남긴 이유를 해당 선언에 적는다.
//! 값이 같다는 이유만으로 역할이 다른 토큰을 적용하거나 스케일 밖 값을 반올림해서는 안 된다.
//! 기준은 [ADR-0035]와 [ADR-0039]를 따른다.
//!
//! [ADR-0035]: ../../docs/adr/0035-shared-design-and-theme.md
//! [ADR-0039]: ../../docs/adr/0039-typed-length-and-dpi-boundaries.md

use super::test_gate::blank_test_modules;
use super::{mask_non_code, repo_root, rust_sources};

const SIZE_SCALE_SOURCE: &str = "crates/tasty-design-tokens/src/generated/primitive.rs";

const LEN_CTOR: &[&str] = &["LogicalPx", "PhysicalPx"];

/// 길이 후보를 찾을 egui/emath 호출 이름. 목록 밖의 호출은 검사하지 않는다.
const EGUI_LENGTH_HEADS: &[&str] = &[
    "vec2",
    "pos2",
    "splat",
    "shrink",
    "expand",
    "expand2",
    "from_min_size",
    "from_min_max",
    "add_space",
    "max_height",
    "min_height",
    "max_width",
    "min_width",
    "desired_width",
    "exact_width",
    "fixed_size",
    "at_least",
    "at_most",
    "set_width",
    "set_height",
    "set_min_width",
    "set_min_height",
    "set_max_width",
    "set_max_height",
    "set_min_size",
    "size",
    "inner_margin",
    "outer_margin",
    "symmetric",
    "same",
    "circle",
    "circle_filled",
    "circle_stroke",
];

/// 다른 사용처가 참조할 값을 정의하는 파일은 제외한다. 제외 수는 별도 검사한다.
const DECLARATION_SITES: &[(&str, usize, &str)] = &[
    (
        "crates/tasty-design-tokens/src/generated/",
        49,
        "스케일 자신 — 이 파일이 곧 size-* 의 정본이다",
    ),
    (
        "crates/tasty-type-appearance/src/theme.rs",
        47,
        "Theme 의 값표 — 다른 자리가 참조해야 할 이름(border_width 등)이 여기 산다",
    ),
];

/// 전시용 값을 (파일, 호출 이름, 정확한 수, 사유)로 등록한다. 줄 번호는 무관한 편집에도 바뀌므로 사용하지 않는다.
const DISPLAY_SPECIMENS: &[(&str, &str, usize, &str)] = &[(
    "crates/tasty-gallery/src/catalog/components/prim_spinner.rs",
    "size",
    5,
    "스피너를 여러 크기로 보여주는 것이 이 카드의 목적이다 — 토큰으로 바꾸면 전시가 사라진다",
)];

/// 픽셀이 아닌 정규화 좌표를 파일·호출 이름별로 등록하고 수를 맞춘다.
const UNIT_SPACE_SITES: &[(&str, &str, usize, &str)] = &[(
    "crates/tasty-plugin-image/src/render.rs",
    "pos2",
    4,
    "텍스처 UV — 0..1 정규화 좌표라 픽셀이 아니다. 전체 텍스처를 가리키는 \
     `pos2(1.0, 1.0)` 의 1 은 1px 가 아니라 100% 다",
)];

fn is_in_unit_space(hit: &Hit) -> bool {
    UNIT_SPACE_SITES
        .iter()
        .any(|(path, head, _, _)| hit.rel == *path && hit.head == *head)
}

/// 영역별 남은 수와 사유. 증가와 감소를 모두 확인한다.
const AREAS: &[(&str, usize, &str)] = &[
    (
        "src/adapters/ui/popup/",
        48,
        // popup의 기본 크기·열 최소폭·스크롤 상한 중 대응하는 역할의 토큰이 없는 값이 남아 있다.
        // 같은 숫자의 폭·점 크기 토큰을 높이·간격에 대신 쓰지 않는다.
        "popup 기본 크기표 — vec2(400.0, 320.0) 처럼 정의 옆에 값이 그대로 박혀 있다",
    ),
    (
        "src/adapters/ui/",
        // 본체 chrome의 역할별 치수다. 테마 접근자가 생긴 값은 옮기되 같은 숫자의 다른 역할과 혼동하지 않는다.
        50,
        "나머지 host chrome(사이드바·타이틀바·서피스 장식)",
    ),
    (
        "src/view/",
        // 설정·플러그인 화면에서 역할에 맞는 토큰이 아직 없는 치수와 폰트 값이 남아 있다.
        34,
        "설정 화면의 폼 레이아웃",
    ),
    (
        "src/",
        10,
        "그 밖의 본체 gfx·state·app 치수. 역할에 맞는 토큰과 외부 API 경계 여부를 위치별로 검토한다.",
    ),
    (
        "crates/tasty-gallery/",
        // 전시용 프레임·스크롤 위치와 본체를 재현하는 치수를 구별해 해당 선언의 근거를 유지한다.
        // 100에서 101로 늘어난 것은 권한 화면 specimen의 콘텐츠 컬럼 폭 560이다.
        // specimen마다 자기 폭을 상수로 두는 것이 이 카탈로그의 방식이고, 그 폭에
        // 대응하는 Theme 값은 없다.
        101,
        "갤러리 specimen은 배율 검사에서 제외돼도 스케일 검사는 받는다(ADR-0039). 이름 붙은 치수와 인라인 값, 전시 목적을 별도로 분류한다.",
    ),
    (
        "crates/tasty-ui-widgets/",
        // 아바타·파일 선택기·원격 도구의 치수는 같은 값의 다른 토큰으로 대체할 수 없다.
        // FH_TARGET_MONO_ADVANCE는 한 글자를 더할 때 잰 폭 증가분이므로 폰트가 바뀌면 다시 측정한다.
        // 스케일 밖 값은 이 집계에 포함되지 않는다.
        15,
        "공용 위젯",
    ),
    (
        "crates/",
        15,
        "나머지 크레이트(dag-layout·model·plugin 뷰어·settings·geometry)",
    ),
];

struct Hit {
    rel: String,
    line: usize,
    head: String,
    value: f32,
    /// 제외된 하한의 수도 따로 검증하므로 찾은 항목에서 삭제하지 않고 표시한다.
    floor: bool,
}

fn size_scale() -> Vec<f32> {
    let text = std::fs::read_to_string(repo_root().join(SIZE_SCALE_SOURCE)).unwrap_or_default();
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.trim_start().split_once("const SIZE_") else {
            continue;
        };
        let Some((_, rest)) = rest.split_once("LogicalPx(") else {
            continue;
        };
        let Some((value, _)) = rest.split_once(')') else {
            continue;
        };
        if let Ok(v) = value.trim().parse::<f32>() {
            out.push(v);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup();
    out
}

/// Theme 기본값과 생성 component 접근자에서 숫자를 읽는다.
/// 같은 값의 이름은 치환 후보일 뿐 해당 위치에 맞는 역할인지는 사람이 판단한다.
/// 여기서 읽지 못한 계산식·값까지 이름이 없다고 증명하는 것은 아니다.
fn theme_named_values() -> Vec<f32> {
    let mut out = Vec::new();
    for (path, prefix) in [
        ("crates/tasty-type-appearance/src/theme.rs", ": LogicalPx("),
        (
            "crates/tasty-type-appearance/src/generated_component.rs",
            "LogicalPx((",
        ),
    ] {
        let text = std::fs::read_to_string(repo_root().join(path)).unwrap_or_default();
        for line in text.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.split_once(prefix).map(|(_, r)| r) else {
                continue;
            };
            let head: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = head.parse::<f32>() {
                out.push(v);
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup();
    out
}

/// 리터럴을 감싼 가장 안쪽 호출의 위치와 이름. 블록·구조체·첨자 내부는 제외하며 바깥 clamp도 확인할 수 있게 괄호 위치를 돌려준다.
fn head_span_of(text: &[char], at: usize) -> Option<(usize, usize, String)> {
    let (mut paren, mut brace, mut bracket) = (0usize, 0usize, 0usize);
    let mut j = at;
    let open = loop {
        if j == 0 {
            return None;
        }
        j -= 1;
        match text[j] {
            ')' => paren += 1,
            '}' => brace += 1,
            ']' => bracket += 1,
            '(' => {
                if paren == 0 {
                    break j;
                }
                paren -= 1;
            }
            '{' => {
                if brace == 0 {
                    return None;
                }
                brace -= 1;
            }
            '[' => {
                if bracket == 0 {
                    return None;
                }
                bracket -= 1;
            }
            _ => {}
        }
    };
    let mut k = open;
    while k > 0 && (text[k - 1].is_ascii_alphanumeric() || matches!(text[k - 1], '_' | ':')) {
        k -= 1;
    }
    let name: String = text[k..open].iter().collect();
    name.rsplit("::")
        .next()
        .filter(|s| !s.is_empty())
        .map(|n| (k, open, n.to_owned()))
}

fn head_of(text: &[char], at: usize) -> Option<String> {
    head_span_of(text, at).map(|(_, _, name)| name)
}

/// 값 1이 clamp나 비교 문턱에 쓰였는지 찾는다. 같은 위치의 다른 값은 제외하지 않는다.
/// 이름 붙인 치수의 const 선언은 하한 예외로 빼지 않는다.
fn is_the_degenerate_floor(text: &[char], at: usize, value: f32) -> bool {
    if value != 1.0 {
        return false;
    }
    let Some((name_start, open, _)) = head_span_of(text, at) else {
        return false;
    };
    // 비교의 >와 매치 =>·반환 ->를 구별한다. 단위 환산의 1을 하한으로 오인하면 안 된다.
    let mut i = name_start;
    while i > 0 && matches!(text[i - 1], ' ' | '\t' | '\n') {
        i -= 1;
    }
    if i > 0 {
        let c = text[i - 1];
        let prev = if i >= 2 { text[i - 2] } else { ' ' };
        let comparison = match c {
            '<' => true,
            '>' => !matches!(prev, '=' | '-'),
            '=' => matches!(prev, '<' | '>' | '!' | '='),
            _ => false,
        };
        if comparison {
            return true;
        }
    }
    matches!(
        head_span_of(text, open)
            .as_ref()
            .map(|(_, _, n)| n.as_str()),
        Some("max" | "min" | "clamp")
    )
}

/// 양옆의 곱셈·나눗셈은 배율로 추정해 제외한다. 호출 자체가 길이를 받아도 그 안의 모든 숫자가 길이는 아니다.
fn is_length_operand(text: &[char], start: usize, end: usize) -> bool {
    let mut i = start;
    while i > 0 && matches!(text[i - 1], ' ' | '\t' | '\n') {
        i -= 1;
    }
    if i > 0 && matches!(text[i - 1], '*' | '/') {
        return false;
    }
    let mut j = end;
    while j < text.len() && matches!(text[j], ' ' | '\t' | '\n') {
        j += 1;
    }
    !(j < text.len() && matches!(text[j], '*' | '/'))
}

/// 스케일에 있는 리터럴의 줄 번호·호출 이름·값·하한 여부. 별도 제외 집계를 위해 0과 하한도 반환한다.
fn on_scale_literals(masked: &str, scale: &[f32]) -> Vec<(usize, String, f32, bool)> {
    let text: Vec<char> = masked.chars().collect();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    while i < text.len() {
        if text[i] == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if !text[i].is_ascii_digit()
            || (i > 0 && (text[i - 1].is_ascii_alphanumeric() || matches!(text[i - 1], '_' | '.')))
        {
            i += 1;
            continue;
        }
        let start = i;
        while i < text.len() && text[i].is_ascii_digit() {
            i += 1;
        }
        if i + 1 < text.len() && text[i] == '.' && text[i + 1].is_ascii_digit() {
            i += 1;
            while i < text.len() && text[i].is_ascii_digit() {
                i += 1;
            }
        }
        let end = i;
        if end < text.len() && (text[end].is_ascii_alphanumeric() || matches!(text[end], '_' | '.'))
        {
            continue;
        }
        let Ok(value) = text[start..end].iter().collect::<String>().parse::<f32>() else {
            continue;
        };
        if !scale.contains(&value) || !is_length_operand(&text, start, end) {
            continue;
        }
        let Some(head) = head_of(&text, start) else {
            continue;
        };
        if LEN_CTOR.contains(&head.as_str()) || EGUI_LENGTH_HEADS.contains(&head.as_str()) {
            let floor = is_the_degenerate_floor(&text, start, value);
            out.push((line, head, value, floor));
        }
    }
    out
}

/// blank_tests에 따라 test 전용 코드를 제외한다. 두 결과의 차이로 제외 수를 센다.
fn scan(blank_tests: bool) -> Vec<Hit> {
    let scale = size_scale();
    let files = rust_sources();
    // test 전용 파일의 분류는 공용 shipping_scope를 사용한다.
    let gated = tasty_doc_guards::shipping_scope::test_only_files(&super::repo_root(), &files);
    let mut out = Vec::new();
    for (path, text) in &files {
        if blank_tests && gated.contains(path) {
            continue;
        }
        let rel = path.to_string_lossy().replace('\\', "/");
        let masked = mask_non_code(text);
        let masked = if blank_tests {
            blank_test_modules(&masked)
        } else {
            masked
        };
        for (line, head, value, floor) in on_scale_literals(&masked, &scale) {
            out.push(Hit {
                rel: rel.clone(),
                line,
                head,
                value,
                floor,
            });
        }
    }
    out
}

/// 실제 비교 대상에서는 등록된 전시용 값을 제외한다. 제외 수와 명부는 별도 검사한다.
fn judged() -> Vec<Hit> {
    considered()
        .into_iter()
        .filter(|h| !is_a_registered_display_specimen(h))
        .collect()
}

fn is_a_registered_display_specimen(hit: &Hit) -> bool {
    DISPLAY_SPECIMENS
        .iter()
        .any(|(path, head, _, _)| hit.rel == *path && hit.head == *head)
}

/// 전시용 값까지 포함한 비교 후보.
fn considered() -> Vec<Hit> {
    scan(true)
        .into_iter()
        .filter(|h| h.value != 0.0)
        .filter(|h| !h.floor)
        .filter(|h| !is_in_unit_space(h))
        .filter(|h| {
            !DECLARATION_SITES
                .iter()
                .any(|(p, _, _)| h.rel.starts_with(p))
        })
        .collect()
}

fn masked_sources() -> std::collections::BTreeMap<String, String> {
    rust_sources()
        .iter()
        .map(|(p, t)| (p.to_string_lossy().replace('\\', "/"), mask_non_code(t)))
        .collect()
}

fn area_of(rel: &str) -> Option<&'static str> {
    AREAS
        .iter()
        .filter(|(p, _, _)| rel.starts_with(p))
        .max_by_key(|(p, _, _)| p.len())
        .map(|(p, _, _)| *p)
}

/// 선언 바로 위의 연속 //·/// 주석을 읽는다. 입력은 리터럴만 가리고 주석은 남겨야 한다.
fn attached_comment(masked_literals: &str, line: usize) -> String {
    let lines: Vec<&str> = masked_literals.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = line.saturating_sub(1);
    while i > 0 {
        let t = lines.get(i - 1).map_or("", |l| l.trim());
        if t.starts_with("//") {
            out.push(t);
            i -= 1;
        } else {
            break;
        }
    }
    out.reverse();
    out.join("\n")
}

/// 붙은 주석에 디자인 출처를 나타내는 단어가 있는지 확인한다. 실제 출처의 진위는 검증하지 않는다.
fn cites_the_design(comment: &str) -> bool {
    const MARKS: &[&str] = &["jsx", "디자인", "시안", "design"];
    let lower = comment.to_lowercase();
    MARKS.iter().any(|m| lower.contains(m))
}

/// 해당 줄이 LogicalPx·PhysicalPx const 선언인지 확인한다. 이름을 붙였어도 대응하는 토큰과 역할이 같은지는 별도 검토가 필요하다.
fn declares_a_named_dimension(masked: &str, line: usize) -> bool {
    let Some(text) = masked.lines().nth(line.saturating_sub(1)) else {
        return false;
    };
    text.contains("const ") && (text.contains(": LogicalPx =") || text.contains(": PhysicalPx ="))
}

#[test]
fn every_on_scale_literal_lives_inside_a_recorded_area() {
    let hits = judged();
    let stray: Vec<String> = hits
        .iter()
        .filter(|h| area_of(&h.rel).is_none())
        .map(|h| format!("  {}:{}  {}({})", h.rel, h.line, h.head, h.value))
        .collect();
    assert!(
        stray.is_empty(),
        "등록된 영역 밖에 size-*와 같은 숫자를 쓴 길이 후보가 있다. 역할에 맞는 토큰을 사용하거나 AREAS에 영역과 남긴 이유를 기록한다:\n{}",
        stray.join("\n")
    );
}

#[test]
fn every_area_holds_exactly_the_count_it_records() {
    let hits = judged();
    let mut lines: Vec<String> = Vec::new();
    for (area, budget, why) in AREAS {
        let n = hits
            .iter()
            .filter(|h| area_of(&h.rel) == Some(*area))
            .count();
        if n != *budget {
            lines.push(format!("  {area}  기록 {budget} → 실측 {n}  ({why})"));
            // 같은 수집 결과로 남은 위치를 출력한다. 로그가 잘리지 않도록 영역별 출력 수를 제한한다.
            const PER_AREA: usize = 40;
            let mine: Vec<&Hit> = hits
                .iter()
                .filter(|h| area_of(&h.rel) == Some(*area))
                .collect();
            for h in mine.iter().take(PER_AREA) {
                lines.push(format!(
                    "      {}:{}  {}({})",
                    h.rel, h.line, h.head, h.value
                ));
            }
            if mine.len() > PER_AREA {
                lines.push(format!(
                    "      … 외 {} 자리(메시지 길이 상한)",
                    mine.len() - PER_AREA
                ));
            }
        }
    }
    assert!(
        lines.is_empty(),
        "영역별 기록과 실제 수집 수가 다르다. 줄었다면 기록도 낮춘다. 늘었다면 실제 새 사용인지 스케일·수집 범위가 바뀐 것인지 확인한다. 고칠 수 없는 치수는 해당 선언에 이유를 남긴 뒤 수를 갱신한다:\n{}",
        lines.join("\n")
    );
}

/// 같은 setter가 세 번 이상, 두 가지 이상의 값으로 나타나면 전시 후보로 분류한다. 실제 전시 목적은 사람이 확인한다.
fn is_a_varied_specimen_value(masked: &str, hit: &Hit) -> bool {
    // 일반 기하 생성자는 반복 사용이 흔하므로 전시 후보로 분류하지 않는다.
    const SETTERS: &[&str] = &[
        "size",
        "desired_width",
        "exact_width",
        "fixed_size",
        "max_height",
        "min_height",
        "max_width",
        "min_width",
    ];
    if !SETTERS.contains(&hit.head.as_str()) {
        return false;
    }
    let scale = size_scale();
    let same_head: Vec<f32> = on_scale_literals(masked, &scale)
        .into_iter()
        .filter(|(_, head, ..)| *head == hit.head)
        .map(|(_, _, v, _)| v)
        .collect();
    if same_head.len() < 3 {
        return false;
    }
    let mut distinct = same_head.clone();
    distinct.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    distinct.dedup();
    distinct.len() >= 2
}

/// 갤러리 후보를 이름 붙은 선언 여부와 디자인 출처 단어의 유무로 나눈다. 주석의 내용이 검사 입력이다.
#[test]
fn the_gallery_share_is_one_question_or_it_is_not() {
    let files = rust_sources();
    let hits = considered();
    let (mut named_cited, mut named_plain) = (0usize, 0usize);
    let (mut inline_cited, mut inline_plain) = (0usize, 0usize);
    let mut with_comment = 0usize;
    for h in hits
        .iter()
        .filter(|h| h.rel.starts_with("crates/tasty-gallery/"))
    {
        let raw = files
            .iter()
            .find(|(p, _)| p.to_string_lossy().replace('\\', "/") == h.rel)
            .map_or("", |(_, t)| t.as_str());
        let comment = attached_comment(&tasty_doc_guards::source_text::mask_literals(raw), h.line);
        if !comment.is_empty() {
            with_comment += 1;
        }
        let named = declares_a_named_dimension(&mask_non_code(raw), h.line);
        match (named, cites_the_design(&comment)) {
            (true, true) => named_cited += 1,
            (true, false) => named_plain += 1,
            (false, true) => inline_cited += 1,
            (false, false) => inline_plain += 1,
        }
    }
    assert!(
        with_comment >= 40,
        "주석이 붙은 후보가 {with_comment}개뿐이다. 실제 주석 감소와 추출 누락을 확인한다."
    );
    assert_eq!(
        (named_cited, named_plain, inline_cited, inline_plain),
        (27, 71, 0, 8),
        "갤러리 후보의 (이름 있음/없음, 디자인 언급 있음/없음) 분류 수가 바뀌었다. 해당 선언과 주석을 확인하고 기록을 갱신한다."
    );
}

/// 전시 후보·같은 Theme 값 존재·값 없음·이름 붙은 치수로 분류한다. 같은 값의 토큰이 있어도 해당 역할에 맞는다는 뜻은 아니다.
#[test]
fn the_gallery_share_splits_into_four_kinds() {
    let files = rust_sources();
    let named_values = theme_named_values();
    assert!(
        named_values.len() > 30 && named_values.contains(&28.0),
        "Theme 숫자 값 목록의 크기 또는 대조 값이 다르다(수집 {}개). 파싱 범위를 확인한다.",
        named_values.len()
    );

    let hits = considered();
    let (mut displayed, mut named_value, mut nameless, mut undecided) =
        (0usize, 0usize, 0usize, 0usize);
    for h in hits
        .iter()
        .filter(|h| h.rel.starts_with("crates/tasty-gallery/"))
    {
        let raw = files
            .iter()
            .find(|(p, _)| p.to_string_lossy().replace('\\', "/") == h.rel)
            .map_or("", |(_, t)| t.as_str());
        let masked = mask_non_code(raw);
        if is_a_varied_specimen_value(&masked, h) {
            displayed += 1;
        } else if !named_values.contains(&h.value) {
            nameless += 1;
        } else if declares_a_named_dimension(&masked, h.line) {
            undecided += 1;
        } else {
            named_value += 1;
        }
    }
    // 전시 수와 전체 갤러리 수는 기존 명부에서 읽어 중복 기록하지 않는다.
    let roster: usize = DISPLAY_SPECIMENS.iter().map(|(.., n, _)| n).sum();
    let ratcheted = AREAS
        .iter()
        .find(|(a, ..)| *a == "crates/tasty-gallery/")
        .map_or(0, |(_, n, _)| *n);
    assert_eq!(
        (displayed, named_value, nameless, undecided),
        (roster, 3, 0, ratcheted - 3),
        "갤러리의 전시 후보·같은 Theme 값·값 없음·이름 붙은 치수 분류 수가 달라졌다"
    );
}

/// 등록된 전시용 값의 개수를 맞추고 미등록 전시 후보도 찾는다.
#[test]
fn the_display_roster_points_at_real_sites_and_covers_the_ones_that_look_like_it() {
    assert!(
        !DISPLAY_SPECIMENS.is_empty(),
        "전시 명부가 비어 제외 범위를 대조할 수 없다"
    );
    let all = considered();
    for (path, head, budget, why) in DISPLAY_SPECIMENS {
        let n = all
            .iter()
            .filter(|h| h.rel == *path && h.head == *head)
            .count();
        assert_eq!(
            n, *budget,
            "전시 명부 `{path}` 의 `{head}`(사유: {why}) 자리가 {n} 개다. 늘었으면 전시가 \
             아닌 것이 섞였을 수 있고, 줄었으면 그 수를 같이 내려라"
        );
    }

    let masked = masked_sources();
    let unregistered: Vec<String> = all
        .iter()
        .filter(|h| !is_a_registered_display_specimen(h))
        .filter(|h| {
            masked
                .get(&h.rel)
                .is_some_and(|m| is_a_varied_specimen_value(m, h))
        })
        .map(|h| format!("  {}:{}  {}({})", h.rel, h.line, h.head, h.value))
        .collect();
    assert!(
        unregistered.is_empty(),
        "전시로 보이는데 명부에 없는 자리가 있다 — 전시가 맞으면 사유와 함께 등록하고, \
         아니면 토큰으로 바꿔라:\n{}",
        unregistered.join("\n")
    );
}

/// 빈 스케일이면 모든 후보가 사라지므로 먼저 크기를 확인한다.
#[test]
fn the_size_scale_is_not_empty() {
    let scale = size_scale();
    assert!(
        scale.len() > 10 && scale.contains(&24.0),
        "size-* 값을 {}개만 읽었다. `{SIZE_SCALE_SOURCE}`의 위치·선언 형식·수집 결과를 확인한다.",
        scale.len()
    );
}

/// 비교에서 제외한 0·test·하한·정규화 좌표·선언의 수도 확인한다.
#[test]
fn the_blind_spots_are_still_the_size_they_say() {
    let all = scan(false);
    let shipped = scan(true);
    let zeros = shipped.iter().filter(|h| h.value == 0.0).count();
    let in_tests = all.len() - shipped.len();
    let floors = shipped.iter().filter(|h| h.value != 0.0 && h.floor).count();
    let unit_space = shipped
        .iter()
        .filter(|h| h.value != 0.0 && !h.floor && is_in_unit_space(h))
        .count();
    assert_eq!(
        (zeros, in_tests),
        (182, 246),
        "제외한 0과 test 전용 코드의 수가 달라졌다. 실제 사용과 수집 범위의 변경을 확인하고 기록을 갱신한다."
    );
    let roster: usize = UNIT_SPACE_SITES.iter().map(|(.., n, _)| n).sum();
    assert_eq!(
        (floors, unit_space),
        // 값 1의 clamp·서브픽셀 비교 문턱을 별도로 센다.
        (18, roster),
        "값 1의 하한·정규화 좌표 수가 달라졌다. 별도 집계 대상의 변경을 확인하고 기록을 갱신한다."
    );
    for (path, head, budget, why) in UNIT_SPACE_SITES {
        let n = shipped
            .iter()
            .filter(|h| h.value != 0.0 && h.rel == *path && h.head == *head)
            .count();
        assert_eq!(
            n, *budget,
            "정규화 좌표 명부 `{path}` 의 `{head}`(사유: {why}) 자리가 {n} 개다. \
             늘었으면 픽셀인 것이 섞였을 수 있고, 줄었으면 그 수를 같이 내려라"
        );
    }
    for (site, budget, why) in DECLARATION_SITES {
        let n = shipped.iter().filter(|h| h.rel.starts_with(site)).count();
        assert_eq!(
            n, *budget,
            "선언 자리 `{site}`(사유: {why})의 건수가 바뀌었다"
        );
    }
}

#[cfg(test)]
mod detector {
    use super::*;

    const SCALE: &[f32] = &[0.0, 1.0, 4.0, 24.0, 400.0];

    fn hits(src: &str) -> Vec<(usize, String, f32)> {
        on_scale_literals(&mask_non_code(src), SCALE)
            .into_iter()
            .map(|(line, head, value, _)| (line, head, value))
            .collect()
    }

    #[test]
    fn it_reads_a_literal_sitting_in_a_length_position() {
        assert_eq!(
            hits("let s = egui::vec2(400.0, 24.0);"),
            vec![(1, "vec2".to_owned(), 400.0), (1, "vec2".to_owned(), 24.0)]
        );
        assert_eq!(
            hits("LogicalPx(24.0)"),
            vec![(1, "LogicalPx".to_owned(), 24.0)]
        );
    }

    #[test]
    fn a_zero_width_bound_is_counted_and_a_nonzero_bound_is_not_a_floor_exemption() {
        let zero = "(name_right - name_left).max(LogicalPx(0.0))";
        assert_eq!(hits(zero), vec![(1, "LogicalPx".to_owned(), 0.0)]);
        let dimension = "(name_right - name_left).max(LogicalPx(24.0))";
        assert_eq!(hits(dimension), vec![(1, "LogicalPx".to_owned(), 24.0)]);
        assert!(floors(dimension).is_empty());
    }

    #[test]
    fn a_test_viewport_is_counted_before_gating_but_not_as_shipped_geometry() {
        let test =
            "#[cfg(test)]\nmod layout_tests { fn narrow_width() { egui::vec2(400.0, 360.0); } }";
        assert_eq!(hits(test), vec![(2, "vec2".to_owned(), 400.0)]);
        assert!(hits(&blank_test_modules(&mask_non_code(test))).is_empty());
        let shipped = test.replace("#[cfg(test)]\n", "");
        assert_eq!(
            hits(&blank_test_modules(&mask_non_code(&shipped))),
            vec![(1, "vec2".to_owned(), 400.0)]
        );
    }

    #[test]
    fn a_factor_is_not_a_length() {
        assert!(hits("egui::vec2(d * 4.0, 1.0 / n)").is_empty());
        assert!(hits("egui::pos2(w / 4.0, 24.0 * k)").is_empty());
        assert_eq!(
            hits("egui::vec2(4.0, h)"),
            vec![(1, "vec2".to_owned(), 4.0)]
        );
    }

    /// 스케일 밖 값을 통과시킨다고 가까운 토큰으로 바꾸라는 뜻은 아니다. ADR-0035의 디자인 절차를 따른다.
    #[test]
    fn a_value_off_the_scale_is_not_this_guards_business() {
        assert!(hits("egui::vec2(17.0, 13.0)").is_empty());
    }

    #[test]
    fn another_familys_position_is_not_measured_here() {
        assert!(hits(".corner_radius(4.0)").is_empty());
        assert!(hits("FontId::proportional(24.0)").is_empty());
    }

    #[test]
    fn a_number_inside_prose_is_not_code() {
        assert!(hits("// vec2(24.0) 이라고 쓰면 된다").is_empty());
        assert!(hits("let doc = \"size(24.0) 원형\";").is_empty());
    }

    #[test]
    fn a_value_outside_an_argument_list_has_no_head() {
        assert!(hits("ui.horizontal(|ui| { let x = 24.0; });").is_empty());
    }

    #[test]
    fn a_subscript_is_not_a_length() {
        assert!(hits("egui::vec2(p[1], q)").is_empty());
        assert!(hits("egui::vec2(pts[0][1], q)").is_empty());
        assert_eq!(
            hits("egui::vec2(p[1], 24.0)"),
            vec![(1, "vec2".to_owned(), 24.0)]
        );
    }

    fn floors(src: &str) -> Vec<f32> {
        on_scale_literals(&mask_non_code(src), SCALE)
            .into_iter()
            .filter(|(.., floor)| *floor)
            .map(|(_, _, v, _)| v)
            .collect()
    }

    /// clamp 위치만으로 모든 값을 제외하지 않도록 값 1인 경우와 다른 치수를 대조한다.
    #[test]
    fn the_degenerate_floor_is_not_a_dimension() {
        assert_eq!(floors("(h - t).max(PhysicalPx(1.0))"), vec![1.0]);
        assert_eq!(floors("w.min(LogicalPx(1.0))"), vec![1.0]);
        assert_eq!(floors("(a - b).abs() < PhysicalPx(1.0)"), vec![1.0]);
        assert_eq!(floors("d <= LogicalPx(1.0)"), vec![1.0]);
        assert!(floors("w.max(LogicalPx(24.0))").is_empty());
        assert!(floors("let x = LogicalPx(1.0);").is_empty());
        assert!(floors("MouseWheelUnit::Point => LogicalPx(1.0),").is_empty());
        assert!(floors("fn f() -> LogicalPx(1.0)").is_empty());
        assert!(floors("const GLYPH_STROKE: LogicalPx = LogicalPx(1.0);").is_empty());
    }
}
