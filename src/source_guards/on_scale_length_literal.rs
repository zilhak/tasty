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
//! **총합은 어디에도 안 적는다.** [`AREAS`] 둘째 항의 합이 그것이다. 적는 순간 그 수는
//! 두 곳에 살게 되고, 값이 두 곳에 있는데 하나만 움직이는 사고가 이 레포에서 반복된
//! 형태다. 보고에 총합이 필요하면 그 자리에서 더해라.
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

/// **전시 대상 명부** — 자리 단위다. 부류로 면제하면 다음 사람이 아무 자리나 "전시" 라고
/// 부른다. 항목은 (파일, 호출 머리, 자리 수, 사유)이고 수는 상한이자 하한이다.
///
/// 줄 번호로 적지 않는 이유는 그 수가 위쪽 편집마다 낡기 때문이다 — (파일, 머리)는
/// 그 자리가 무엇인지로 식별한다.
const DISPLAY_SPECIMENS: &[(&str, &str, usize, &str)] = &[(
    "crates/tasty-gallery/src/catalog/components/prim_spinner.rs",
    "size",
    5,
    "스피너를 여러 크기로 보여주는 것이 이 카드의 목적이다 — 토큰으로 바꾸면 전시가 사라진다",
)];

/// 영역별 잔여와 그 사유. 수는 상한이자 **하한**이다 — 위 "양방향 래칫" 참조.
const AREAS: &[(&str, usize, &str)] = &[
    (
        "src/adapters/ui/popup/",
        48,
        "popup 기본 크기표 — vec2(400.0, 320.0) 처럼 정의 옆에 값이 그대로 박혀 있다",
    ),
    (
        "src/adapters/ui/",
        44,
        "나머지 host chrome(사이드바·타이틀바·서피스 장식)",
    ),
    ("src/view/", 31, "설정 화면의 폼 레이아웃"),
    (
        "src/",
        16,
        "그 밖의 본체(gfx·state·app) — GPU/상태 쪽이라 자리마다 사정이 다르다. \
         이 수가 마지막으로 오른 것은 위반이 늘어서가 아니라 `LogicalSize::new(400, 200)` \
         처럼 **세지 않는 형태**로 숨어 있던 값이 이름을 얻어 보이게 됐기 때문이다 — \
         바늘 밖에 있던 것이 안으로 들어오면 이 수는 오른다",
    ),
    (
        "crates/tasty-gallery/",
        85,
        "갤러리 specimen — 배율에는 면제지만(ADR-0135) 스케일에는 아니다. \
         한 항목이 아니다 — 모양은 `the_gallery_share_is_one_question_or_it_is_not` 이, \
         갈래는 `the_gallery_share_splits_into_four_kinds` 가 든다",
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

/// `&Theme` 를 거쳐 나오는 길이 값들. **이름이 있는 값의 집합**이다.
///
/// 두 자리에서 읽는다 — `Theme` 의 DEFAULT 값표(손으로 쓰는 semantic 이름)와 생성된
/// component 접근자(디자인 토큰에서 나온 이름). 어느 쪽도 손으로 안 적는다.
///
/// 이 집합에 **없는** 값은 이름을 붙일 수 없다 — 그 자리의 처방은 "토큰으로 바꿔라" 가
/// 아니라 "이 치수에 이름을 줄 것인가" 이고 그건 디자인 결정이다. 반대로 있다고 해서
/// 그 이름이 **그 자리에 맞는** 이름이라는 뜻은 아니다(값이 같을 뿐일 수 있다).
/// 그래서 이 술어는 한 방향으로만 결론을 낸다: **없으면 못 고친다.**
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
            // 필드 **선언**(`pub name: LogicalPx,`)이 아니라 **값**이 붙은 줄만 본다.
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

/// 이 가드가 실제로 판정하는 집합 — 출하물이고, 선언 자리가 아니고, 0 이 아니고,
/// **전시 대상이 아닌** 것.
///
/// 전시 대상은 [`DISPLAY_SPECIMENS`] 에 **자리로** 적혀 있다 — 부류가 아니다. 그 수를
/// 토큰으로 바꾸면 specimen 이 보여주려던 것이 사라지고, 래칫에 남겨 두면 다음 사람이
/// 고칠 수 **없는** 자리를 향해 달린다. 빠진 몫은 [`considered`] 와
/// [`the_gallery_share_splits_into_four_kinds`] 가 계속 세므로 조용히 사라지지 않는다.
fn judged() -> Vec<Hit> {
    considered()
        .into_iter()
        .filter(|h| !is_a_registered_display_specimen(h))
        .collect()
}

/// 그 자리가 [`DISPLAY_SPECIMENS`] 에 **등록돼 있는가.** 등록되지 않은 전시 후보는
/// 빠지지 않는다 — 면제는 부류가 아니라 자리다.
fn is_a_registered_display_specimen(hit: &Hit) -> bool {
    DISPLAY_SPECIMENS
        .iter()
        .any(|(path, head, _, _)| hit.rel == *path && hit.head == *head)
}

/// 전시 대상을 빼기 **전**의 집합. 빼는 몫의 크기를 재는 쪽이 이것을 쓴다 — 빼 놓고
/// 그 수를 안 재면 그 부류는 존재하지 않는 것이 된다.
fn considered() -> Vec<Hit> {
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

/// 경로 → 마스킹된 원문. 여러 판정이 같은 사본을 쓴다.
fn masked_sources() -> std::collections::BTreeMap<String, String> {
    rust_sources()
        .iter()
        .map(|(p, t)| (p.to_string_lossy().replace('\\', "/"), mask_non_code(t)))
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

/// 한 자리에 **붙어 있는 주석 블록**. 선언 줄 바로 위의 연속된 `//`·`///` 줄이다.
///
/// 입력은 [`mask_literals`] 를 거친 사본이어야 한다 — 문자열만 덮고 **주석은 남긴다.**
/// [`mask_non_code`] 와 판정기가 다른 이유는 물음이 다르기 때문이다: 저쪽은 "코드에 X 가
/// 있나", 이쪽은 "**주석이 달려 있나**". 저 판정기로 물으면 주석이 통째로 사라져 답이
/// 항상 "없다" 가 된다.
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

/// 그 자리가 **디자인 산출물을 출처로 인용하는가.** 갤러리 specimen 은 디자인의 구조를
/// 1:1 로 전사하는 곳이라(`docs/design/systems/design-parity-notes.md` 의 구조 전사),
/// 거기서 나온 고정 치수는 스케일에서 뽑은 값이 아니라 **디자인이 그 자리에 정한 수**다.
fn cites_the_design(comment: &str) -> bool {
    const MARKS: &[&str] = &["jsx", "디자인", "시안", "design"];
    let lower = comment.to_lowercase();
    MARKS.iter().any(|m| lower.contains(m))
}

/// 그 줄이 **이름 붙은 치수를 선언**하는가(`const W: LogicalPx = LogicalPx(150.0);`).
///
/// 이름이 붙은 치수와 호출 인자에 그대로 박힌 수는 성질이 다르다. 앞쪽은 그 파일이 그
/// 수를 **하나의 치수로 인정**하고 이름과 사유를 붙인 것이고, 뒤쪽은 자리에 스며든
/// 여백이다. 처방도 다르다 — 뒤쪽은 테마 이름으로 옮기면 끝이지만, 앞쪽은 그 치수에
/// 대응하는 이름이 있어야 옮길 수 있다.
///
/// # 이 갈래의 이름 — **사본 축**
///
/// 갤러리 몫에서 이쪽이 제일 크다. 이 가드가 왜 판정하지 못하는지는 한 줄이다:
///
/// > **그 줄은 값을 쓰는 자리가 아니라 이름을 짓는 자리라, "리터럴을 토큰으로
/// > 바꿔라" 가 애초에 적용되지 않는다. 물어야 할 것은 "이 이름이 이미 있는 이름의
/// > 사본인가" 이고, 그 답은 값이 아니라 출처가 준다.**
///
/// 표본 하나가 그 물음을 그대로 보여준다. `catalog/popup_frame.rs` 는
/// `TITLE_BAR_HEIGHT` 를 28 로 **직접 선언**하는데, host 의 같은 치수는
/// `Theme.item_height_interactive` 에서 **파생**된다(`adapters/ui/popup.rs` 의 주석이
/// 그 유래를 적고 있다). 값이 같아서 지금은 아무 데도 안 걸리지만, 테마가 움직이면
/// host 만 따라 움직이고 갤러리는 그 자리에 남는다 — 갤러리가 존재하는 이유가 host
/// 와의 정합인데 그 정합이 **조용히** 깨지는 형태다. 값 비교로는 영영 안 보인다.
///
/// 그러나 이 갈래가 한 처방으로 쓸리지는 않는다. 같은 갈래 안에 있던 dag 캔버스의
/// 줌 상한은 길이 타입을 입은 **비율**이라 애초에 이 축의 대상이 아니었다(그래서
/// 타입을 벗겼다). 자리마다 출처를 물어야 하고, 그 물음은 이 가드 밖에 있다.
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
            //
            // 한 영역당 상한을 둔다. 테스트 하네스가 긴 실패 메시지를 자르기 때문이다 —
            // 실측으로 280 줄짜리 목록이 231 줄에서 잘렸고 **잘린 뒤쪽 영역은 통째로 안
            // 보였다.** 목록이 조용히 잘리면 "그 영역엔 자리가 없다" 로 읽힌다.
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
        "래칫이 실측과 어긋났다. 늘었으면 새 위반이고, **줄었으면 그 수를 같이 내려라** \
         — 남는 여유가 곧 안 보는 구간이다.\n\
         ★ 늘어서 빨간 경우 **이 수를 올리는 것은 이행이 아니다.** 아래 자리를 고치거나, \
         정말 못 고치면 왜 못 고치는지를 그 자리 주석에 적고 올려라 — 수만 올리면 그 자리는 \
         영영 안 보인다:\n{}",
        lines.join("\n")
    );
}

/// 그 값이 **specimen 이 전시하려고 바꾸는 값**인가. 같은 호출 머리가 그 파일에서 셋
/// 이상 나오고 서로 다른 값을 둘 이상 취하면, 그 수는 자리의 치수가 아니라 **보여주는
/// 대상**이다(`Spinner::new().size(12.0)` · `.size(24.0)` · `.size(14.0)`).
///
/// 토큰으로 바꾸면 전시가 사라진다 — 그래서 이 몫은 이 축의 위반이 아니다.
fn is_a_varied_specimen_value(masked: &str, hit: &Hit) -> bool {
    // 기하 생성자(`vec2`·`LogicalPx` …)는 어느 파일에나 여럿 나오고 값도 제각각이라
    // 변주 조건이 거의 항상 참이 된다. 전시는 **위젯을 설정하는 자리**에서 일어난다.
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
        .filter(|(_, head, _)| *head == hit.head)
        .map(|(_, _, v)| v)
        .collect();
    if same_head.len() < 3 {
        return false;
    }
    let mut distinct = same_head.clone();
    distinct.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    distinct.dedup();
    distinct.len() >= 2
}

/// 갤러리 몫이 **한 물음인가**를 잰다. 래칫의 3 분의 1 이 갤러리 한 자리에 있어서,
/// 그 몫이 단일 항목이 아니면 분모가 틀린 것이다.
///
/// 두 성질로 가른다 — 그 자리가 **디자인을 출처로 인용하는가**와, 그 파일이 **토큰
/// 이름을 이미 쓰는가**. 뒤쪽이 참인데 앞쪽도 참이면 "짝 중 한쪽만 테마" 이고, 뒤쪽이
/// 거짓이면 그 파일은 애초에 토큰을 소비하는 자리가 아니다.
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
    // R435 하한 — 주석 추출이 죽으면 `cites_the_design` 이 전부 거짓이 되고 이 단정은
    // 언제나 참이 된다. 그 상태의 0 은 "인용이 없다" 가 아니라 "안 읽었다" 다.
    assert!(
        with_comment >= 40,
        "주석이 붙은 자리가 {with_comment} 개뿐이다 — 주석 추출이 죽었다"
    );
    assert_eq!(
        (named_cited, named_plain, inline_cited, inline_plain),
        (22, 53, 1, 14),
        "갤러리 몫의 갈래가 바뀌었다 — 이름 붙은 치수(앞 둘)와 인라인 여백(뒤 둘)은 \
         처방이 다르다. 인라인을 줄였으면 뒤의 수를, 치수에 이름을 줬으면 앞의 수를 내려라"
    );
}

/// 갤러리 몫을 네 갈래로 가른다. 앞의 갈래(모양)가 "무엇인가" 라면 이쪽은 "그 자리에
/// 이름이 있는가" 다.
///
/// # ★ 값이 같다는 것은 이름이 맞다는 뜻이 아니다
///
/// 둘째 갈래는 **"같은 값을 가진 `Theme` 이름이 존재한다"** 까지만 말한다. 그것을 한때
/// "고칠 수 있는 것" 이라고 불렀는데, 그 이름표가 틀렸다는 것이 열 자리를 손으로 읽어
/// 드러났다 — 13 중 자리에 맞는 이름을 가진 것은 3 이었다.
///
/// 나머지 열은 값만 겹친다: 툴바 높이 32 옆에 있는 이름은 `button_height_lg` 이고,
/// 스크롤 최대 높이 200 옆에 있는 이름은 전부 **너비**(`settings_sidebar_width` 등)다.
/// 그 이름을 갖다 쓰면 픽셀은 그대로인 채 **틀린 결합**이 생긴다 — 다음에 그 토큰이
/// 움직일 때 아무 상관 없는 자리가 따라 움직인다.
///
/// 그래서 이 갈래는 처방이 아니라 **후보**다. 자리에 맞는 이름인지는 술어가 아니라
/// 사람이 정한다. 술어로 정하려 하면 값 일치가 곧 처방이 되어, 위의 결합을 자동으로
/// 만들어 낸다.
#[test]
fn the_gallery_share_splits_into_four_kinds() {
    let files = rust_sources();
    let named_values = theme_named_values();
    // R415 양성 대조 — 이름 집합이 비면 아래 판정이 전부 "이름 없음" 으로 쏠린다.
    assert!(
        named_values.len() > 30 && named_values.contains(&28.0),
        "Theme 이름 값을 못 읽었다({} 개) — 그 상태의 판정은 전부 거짓이다",
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
    // 같은 값을 두 곳에 적지 않는다 — 전시 몫은 명부가, 나머지의 합은 `AREAS` 가 이미
    // 들고 있다. 여기 손으로 적는 것은 **갈래별 크기**뿐이고, 그 둘과의 정합은 단정이
    // 확인한다. 짝을 만들면 한쪽만 움직인다.
    let roster: usize = DISPLAY_SPECIMENS.iter().map(|(.., n, _)| n).sum();
    let ratcheted = AREAS
        .iter()
        .find(|(a, ..)| *a == "crates/tasty-gallery/")
        .map_or(0, |(_, n, _)| *n);
    assert_eq!(
        (displayed, named_value, nameless, undecided),
        (roster, 10, 1, ratcheted - 10 - 1),
        "갤러리 몫의 갈래가 바뀌었다 — 전시(래칫 밖) · 같은 값의 이름이 있다 · 이름이 \
         없다 · 그 줄이 스스로 치수를 이름 짓는다"
    );
}

/// 전시 명부가 **실재를 가리키고, 그 크기가 기록과 같은가.** 그리고 명부 **밖**에
/// 전시로 보이는 자리가 없는가.
///
/// 앞쪽이 없으면 명부는 오타 하나로 조용히 비고, 빈 명부는 아무것도 안 뺀다(그러면
/// 이 축의 수가 소리 없이 늘어난다). 뒤쪽이 없으면 새 전시 specimen 이 등록 없이
/// 래칫에 쌓여 다음 사람이 고칠 수 없는 자리를 향해 달린다.
#[test]
fn the_display_roster_points_at_real_sites_and_covers_the_ones_that_look_like_it() {
    assert!(
        !DISPLAY_SPECIMENS.is_empty(),
        "전시 명부가 비었다 — 빈 명부는 아무것도 안 빼면서 언제나 통과한다"
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
