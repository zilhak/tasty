//! 값이 `size-*` 스케일 **안에 있는데** 숫자로 쓴 길이 자리를 세는 가드.
//!
//! [`length_constant_frontier`](super::length_constant_frontier) 와 축이 다르다. 그쪽은
//! **선언의 타입**(`const W: f32 = 96.0;`)을 묻고, 여기는 **값의 출처**를 묻는다 —
//! `egui::vec2(400.0, 320.0)` 은 타입 위반이 아니지만 `size-400`·`size-320` 토큰의 값을
//! 손으로 다시 적은 것이다. 토큰이 움직여도 이 자리는 안 따라간다.
//!
//! # 판정: 자리가 가족을 정하고, 값이 그 가족 안인지를 본다
//!
//! [ADR-0126] 의 "판정은 가족별로 한다" 를 그대로 집행한다. 여기서 보는 자리는
//! **`size-*` 가족을 재는 자리** 하나뿐이다 — 길이 타입 생성자([`LEN_CTOR`])와 egui
//! 기하 인자([`EGUI_LENGTH_HEADS`]). `.corner_radius(`(→`radius-*`)나 `FontId`
//! (→`font-size-*`)는 **여기서 안 본다.** 여러 가족을 합집합으로 재면 부류 하나가
//! 통째로 숨는다 — 그 착시가 이 가드가 생긴 계기다.
//!
//! 스케일은 손으로 안 적는다. [`SIZE_SCALE_SOURCE`] 의 생성 파일에서 읽으므로 토큰이
//! 늘거나 줄면 판정도 같이 움직인다. 그 파일이 사라지거나 이름이 바뀌면 스케일이 비고
//! 이 가드의 0 은 "없다" 가 아니라 "안 봤다" 가 된다 — [`the_size_scale_is_not_empty`]
//! 가 그 공허를 막는다.
//!
//! # 이 가드가 못 보는 것
//!
//! 아래는 설계상 사각이다. **여기 0 이 나온다고 "없다" 가 아니다.** 넷 중 셋은
//! [`the_blind_spots_are_still_the_size_they_say`] 가 건수를 **실측으로** 들고 있어
//! 사각이 조용히 자라지 않는다. 사각을 좁히려면 술어를 고치고 그 수를 함께 옮겨라.
//!
//! - **`0.0`** — 실측 건수는 위 테스트가 든다. `size-0` 이 실재하지만 기하에서 0 은
//!   디자인 결정이 아니라 덧셈의 항등원이다. `vec2(SIZE_0.value(), ..)` 는 정합이
//!   아니라 소음이다.
//! - **테스트 게이트 안** — 화면에 안 나가는 코드다. 판정은 [`super::test_gate`].
//! - **값을 선언하는 자리**([`DECLARATION_SITES`]) — 다른 자리가 참조해야 할 이름이
//!   사는 곳이라, 여기를 판정하면 선언에게 자기 자신을 참조하라고 요구하게 된다.
//! - **지역 변수 선언**(`let x = 20.0;`) — 이 가드가 **재지 않는** 유일한 사각이다.
//!   선언 자리에는 길이 문맥이 없어 같은 술어로는 길이인지 배율인지 안 갈린다. 축을
//!   열 때 쓴 임시 스캐너로 80 건을 셌고, 그 도구는 커밋되지 않았으므로 이 수는
//!   여기서 다시 재지지 않는다. 재려면 술어가 먼저 있어야 한다.
//!
//! 한때 다섯째가 있었다 — **문자열·주석 안의 수.** `"window-button-size(24) …"` 의 24
//! 가 `.size(` 인자로 계상돼 축의 수를 4 만큼 부풀렸다. 손수 만든 마스커를 한 벌 더
//! 두는 대신 [`mask_non_code`] 를 쓰면서 닫혔다.
//!
//! # 상한은 여유가 아니다 — 양방향 래칫
//!
//! [`AREAS`] 의 수는 상한인 동시에 **하한**이다. 늘면 새 위반이고, 줄어도 실패한다 —
//! `scripts/check-allow-reason.sh` 가 같은 형태이고 이유도 같다: **남는 여유가 곧 안
//! 보는 구간이다.** 슬라이스로 고칠 때마다 그 줄의 수를 같이 내려야 한다. 형태만
//! 베끼면 다음 사람이 여유를 남기고, 그 여유 안에서는 이 가드가 아무것도 안 막는다.
//!
//! [ADR-0126]: ../../docs/adr/0126-off-scale-font-values-are-not-snapped-to-tokens.md

use super::test_gate::blank_test_modules;
use super::{mask_non_code, repo_root, rust_sources};

/// `size-*` 스케일의 정본. 생성 파일이라 DTCG 원본이 바뀌면 여기도 같이 바뀐다.
const SIZE_SCALE_SOURCE: &str = "crates/tasty-design-tokens/src/generated/primitive.rs";

/// 길이 타입 생성자. 인자가 곧 픽셀이다.
const LEN_CTOR: &[&str] = &["LogicalPx", "PhysicalPx"];

/// 인자를 길이로 먹는 egui/emath 기하 머리. 여기 없는 머리는 0 이 아니라 미측정이다.
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

/// **값을 선언하는 자리** — 여기는 판정하지 않는다. 건수는 모듈 문서의 사각 목록에
/// 대응하며 [`the_blind_spots_are_still_the_size_they_say`] 가 실측으로 든다.
const DECLARATION_SITES: &[(&str, usize, &str)] = &[
    (
        "crates/tasty-design-tokens/src/generated/",
        44,
        "스케일 자신 — 이 파일이 곧 size-* 의 정본이다",
    ),
    (
        "crates/tasty-type-appearance/src/theme.rs",
        47,
        "Theme 의 값표 — 다른 자리가 참조해야 할 이름(border_width 등)이 여기 산다",
    ),
];

/// 영역별 잔여와 그 사유. 수는 상한이자 **하한**이다 — 위 "양방향 래칫" 참조.
const AREAS: &[(&str, usize, &str)] = &[
    (
        "src/adapters/ui/popup/",
        48,
        "popup 기본 크기표 — vec2(400.0, 320.0) 처럼 정의 옆에 값이 그대로 박혀 있다",
    ),
    (
        "src/adapters/ui/",
        49,
        "나머지 host chrome(사이드바·타이틀바·서피스 장식)",
    ),
    ("src/view/", 47, "설정 화면의 폼 레이아웃"),
    (
        "src/",
        14,
        "그 밖의 본체(gfx·state·app) — GPU/상태 쪽이라 자리마다 사정이 다르다",
    ),
    (
        "crates/tasty-gallery/",
        98,
        "갤러리 specimen — 배율에는 면제지만(ADR-0135) 스케일에는 아니다",
    ),
    ("crates/tasty-ui-widgets/", 4, "공용 위젯"),
    (
        "crates/",
        26,
        "나머지 크레이트(dag-layout·model·plugin 뷰어·settings·geometry)",
    ),
];

/// 한 자리의 관측.
struct Hit {
    rel: String,
    line: usize,
    head: String,
    value: f32,
}

/// 생성된 primitive 에서 `size-*` 값 집합을 읽는다.
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

/// 리터럴을 감싸는 **가장 안쪽 호출 머리**. 짝 없는 여는 기호가 `(` 가 아니면(블록이나
/// 구조체 리터럴 안이면) 인자 자리가 아니므로 `None`.
fn head_of(text: &[char], at: usize) -> Option<String> {
    let (mut paren, mut brace) = (0usize, 0usize);
    let mut j = at;
    let open = loop {
        if j == 0 {
            return None;
        }
        j -= 1;
        match text[j] {
            ')' => paren += 1,
            '}' => brace += 1,
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
        .map(str::to_owned)
}

/// 곱셈·나눗셈의 피연산자면 길이가 아니라 **배율**이다.
///
/// 실측으로 걸렸다: `d * 0.5`(지름→반지름) · `s * 0.6`(형상 비례) · `5.0 / 18.0`. 셋 다
/// `circle_filled(` · `pos2(` 안이라 **자리로는 길이인데 값은 길이가 아니다.** 이 필터가
/// 없으면 `0.5` 하나가 137 건으로 최다값이 되어 축 전체가 비율에 파묻힌다. 그래서 이
/// 술어가 이 가드의 판별력 자체이고, 문서가 아니라
/// [`a_factor_is_not_a_length`](detector::a_factor_is_not_a_length) 가 못박는다.
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

/// 마스킹된 소스에서 `size-*` 값과 같은 수가 길이 자리에 박힌 (줄번호, 머리, 값).
/// `0.0` 도 포함해 돌려준다 — 그 사각의 크기를 재는 쪽이 따로 있다.
///
/// 순수 함수다 — 합성 스니펫을 그대로 먹일 수 있다.
fn on_scale_literals(masked: &str, scale: &[f32]) -> Vec<(usize, String, f32)> {
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
            out.push((line, head, value));
        }
    }
    out
}

/// 레포 전수 스캔. `blank_tests` 가 참이면 테스트 게이트 안을 지운다 — 그 차이가 곧
/// "테스트 안" 사각의 크기다.
fn scan(blank_tests: bool) -> Vec<Hit> {
    let scale = size_scale();
    let files = rust_sources();
    // 파일 단위 판정("이 파일이 출하되나")은 `shipping_scope` 의 일이다 — 선언 간선의
    // 전이 폐쇄를 따르고 `#[path]` 도 푼다. 여기서 다시 세지 않는다.
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
        for (line, head, value) in on_scale_literals(&masked, &scale) {
            out.push(Hit {
                rel: rel.clone(),
                line,
                head,
                value,
            });
        }
    }
    out
}

/// 이 가드가 실제로 판정하는 집합 — 출하물이고, 선언 자리가 아니고, 0 이 아닌 것.
fn judged() -> Vec<Hit> {
    scan(true)
        .into_iter()
        .filter(|h| h.value != 0.0)
        .filter(|h| {
            !DECLARATION_SITES
                .iter()
                .any(|(p, _, _)| h.rel.starts_with(p))
        })
        .collect()
}

/// 래칫 줄이 맡는 영역. 접두사가 긴 것부터 본다.
fn area_of(rel: &str) -> Option<&'static str> {
    AREAS
        .iter()
        .filter(|(p, _, _)| rel.starts_with(p))
        .max_by_key(|(p, _, _)| p.len())
        .map(|(p, _, _)| *p)
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
        "스케일 위의 값을 숫자로 쓴 자리가 래칫 밖에서 나왔다 — 토큰으로 바꾸거나, \
         영역을 AREAS 에 사유와 함께 등록하라:\n{}",
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
            // 남은 자리를 함께 낸다. 슬라이스는 "무엇이 남았나" 를 보고 고르는 작업이라,
            // 그 목록을 다른 도구로 다시 내면 두 도구의 수가 갈린다 — 이 축에서 이미
            // 한 번 겪었다(임시 스캐너의 수가 재현되지 않았다).
            for h in hits.iter().filter(|h| area_of(&h.rel) == Some(*area)) {
                lines.push(format!(
                    "      {}:{}  {}({})",
                    h.rel, h.line, h.head, h.value
                ));
            }
        }
    }
    assert!(
        lines.is_empty(),
        "래칫이 실측과 어긋났다. 늘었으면 새 위반이고, **줄었으면 그 수를 같이 내려라** \
         — 남는 여유가 곧 안 보는 구간이다:\n{}",
        lines.join("\n")
    );
}

/// 스케일이 비면 `contains` 가 항상 거짓이 되어 이 가드 전체가 조용히 통과한다.
#[test]
fn the_size_scale_is_not_empty() {
    let scale = size_scale();
    assert!(
        scale.len() > 10 && scale.contains(&24.0),
        "size-* 스케일을 못 읽었다({} 개). `{SIZE_SCALE_SOURCE}` 가 옮겨졌거나 형태가 \
         바뀌었다 — 그 상태의 0 은 통과가 아니라 측정 실패다",
        scale.len()
    );
}

/// 사각의 크기를 실측으로 든다. 사각을 적어 두기만 하면 그 수는 낡고, 낡은 수는
/// "여기엔 없다" 로 읽힌다.
#[test]
fn the_blind_spots_are_still_the_size_they_say() {
    let all = scan(false);
    let shipped = scan(true);
    let zeros = shipped.iter().filter(|h| h.value == 0.0).count();
    let in_tests = all.len() - shipped.len();
    assert_eq!(
        (zeros, in_tests),
        (167, 181),
        "0.0 사각과 테스트 사각의 크기가 바뀌었다. 늘었으면 이 가드가 안 보는 구간이 \
         자란 것이고, 줄었으면 그 수를 같이 내려라"
    );
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

    /// 곱셈·나눗셈의 피연산자는 길이가 아니라 배율이다. 이 술어가 없으면 `0.5` 하나가
    /// 최다값이 되어 축이 비율에 파묻힌다 — 가드의 판별력이 여기 걸려 있다.
    #[test]
    fn a_factor_is_not_a_length() {
        assert!(hits("egui::vec2(d * 4.0, 1.0 / n)").is_empty());
        assert!(hits("egui::pos2(w / 4.0, 24.0 * k)").is_empty());
        // 같은 값이 곱셈 밖에 있으면 다시 길이다 — 값이 아니라 자리가 판정한다.
        assert_eq!(
            hits("egui::vec2(4.0, h)"),
            vec![(1, "vec2".to_owned(), 4.0)]
        );
    }

    /// 스케일 밖의 값은 이 가드의 대상이 아니다. 그쪽은 ADR-0126 이 다루는 축이고,
    /// 처방이 반대다(토큰으로 스냅하지 **않는다**).
    #[test]
    fn a_value_off_the_scale_is_not_this_guards_business() {
        assert!(hits("egui::vec2(17.0, 13.0)").is_empty());
    }

    /// 자리가 가족을 정한다. `radius-*`·`font-size-*` 를 재는 자리는 여기서 안 본다 —
    /// 합집합으로 재면 부류 하나가 통째로 숨는다.
    #[test]
    fn another_familys_position_is_not_measured_here() {
        assert!(hits(".corner_radius(4.0)").is_empty());
        assert!(hits("FontId::proportional(24.0)").is_empty());
    }

    /// 코드가 아닌 자리의 수는 코드가 아니다. 실제로 `\"window-button-size(24) …\"` 의
    /// 24 가 `.size(` 인자로 계상돼 축의 수를 4 만큼 부풀린 적이 있다.
    #[test]
    fn a_number_inside_prose_is_not_code() {
        assert!(hits("// vec2(24.0) 이라고 쓰면 된다").is_empty());
        assert!(hits("let doc = \"size(24.0) 원형\";").is_empty());
    }

    /// 인자 자리가 아니면 머리가 없다 — 블록이나 구조체 리터럴 안의 값은 이 술어가
    /// 판정하지 않는다(모듈 문서의 "지역 변수 선언" 사각과 같은 이유).
    #[test]
    fn a_value_outside_an_argument_list_has_no_head() {
        assert!(hits("ui.horizontal(|ui| { let x = 24.0; });").is_empty());
    }
}
