//! Theme의 색·길이 타입 이외 필드와 ThemeRuntime의 설정 필드를 등록 목록과 비교한다.
//! 둘을 따로 봐야 Theme에 직접 추가된 값도 빠뜨리지 않는다.
//! ADR-0037은 시각 값 외 런타임 설정이 셋 이상 필요할 때 별도 컨텍스트를 검토하도록 한다.
//! 등록 목록의 추가·삭제는 사람이 용도와 사유를 검토하며, 소스에서 자동 생성하지 않는다.

use std::collections::BTreeSet;

use tasty_doc_guards::repo_root;

const THEME_SRC: &str = "crates/tasty-type-appearance/src/theme.rs";
const RUNTIME_SRC: &str = "crates/tasty-themes/src/state.rs";

/// 이 타입으로 직접 선언한 필드를 색·길이로 분류한다.
const VISUAL_TYPES: &[&str] = &["HexColor", "LogicalPx"];

/// Theme의 나머지 필드와 해당 타입을 사용하는 이유.
const NON_VISUAL_ROSTER: &[(&str, &str)] = &[
    (
        "line_height_ui",
        "폰트 크기에 곱하는 배수다 — 길이가 아니라 비율이라 치수 타입을 못 쓴다",
    ),
    ("ui_zoom", "ThemeRuntime으로 전달하는 호스트 UI 배율이다."),
    (
        "reduced_motion",
        "ThemeRuntime으로 전달하는 모션 감소 설정이다(ADR-0037).",
    ),
    (
        "is_light",
        "밝은 테마인지 나타내는 플래그이며 색에서 파생된다.",
    ),
    (
        "surface_themes",
        "surface별로 사용할 테마 이름을 저장하는 맵이다.",
    ),
];

/// ThemeRuntime이 전달하는 설정 필드. 늘면 ADR-0037의 별도 컨텍스트 조건을 검토한다.
const RUNTIME_ROSTER: &[&str] = &["ui_zoom", "reduced_motion"];

/// 2026-09-08 측정113필드보다 낮게 둔 파싱 하한. 실제 필드 정리를 허용할 여유를 둔다.
const MIN_THEME_FIELDS: usize = 80;

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

/// 공개 구조체의 공개 필드 이름·타입을 읽는다. 본문 범위는 중괄호로 구분한다.
fn pub_fields(src: &str, struct_name: &str) -> Vec<(String, String)> {
    let head = format!("pub struct {struct_name} ");
    let alt = format!("pub struct {struct_name}{{");
    let start = src
        .find(&head)
        .or_else(|| src.find(&alt))
        .and_then(|at| src[at..].find('{').map(|b| at + b + 1));
    let Some(begin) = start else {
        return Vec::new();
    };
    // find의 바이트 오프셋과 맞도록 char_indices를 써서 한글 주석 뒤에서도 위치를 유지한다.
    let mut depth = 1usize;
    let mut end = src.len();
    for (off, ch) in src[begin..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = begin + off;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &src[begin..end];

    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some((name, ty)) = rest.split_once(':') else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.is_empty() {
            continue;
        }
        let ty = ty.trim().trim_end_matches(',').trim();
        if ty.is_empty() {
            continue;
        }
        out.push((name.to_string(), ty.to_string()));
    }
    out
}

/// 그 타입이 색이나 치수를 나르는가 — 제네릭 인자를 벗기지 않고 **머리 이름**만 본다.
fn is_visual(ty: &str) -> bool {
    let head = ty.split(['<', ' ']).next().unwrap_or(ty).trim();
    VISUAL_TYPES.contains(&head)
}

/// Theme의 색·길이 외 필드가 목록과 다르면 용도와 전달 경로를 다시 검토한다.
#[test]
fn every_non_visual_theme_field_is_on_the_declared_roster() {
    let fields = pub_fields(&read(THEME_SRC), "Theme");
    // 실패해도 파싱 결과를 확인할 수 있도록 먼저 출력한다.
    eprintln!(
        "[theme-payload] Theme 필드 {} · 비-시각 {} · 명부 {}",
        fields.len(),
        fields.iter().filter(|(_, t)| !is_visual(t)).count(),
        NON_VISUAL_ROSTER.len()
    );
    assert!(
        fields.len() >= MIN_THEME_FIELDS,
        "Theme에서 {}필드만 읽었다(하한 {MIN_THEME_FIELDS}). {THEME_SRC}의 구조체와 파싱 범위를 확인한다.",
        fields.len()
    );

    let actual: BTreeSet<&str> = fields
        .iter()
        .filter(|(_, ty)| !is_visual(ty))
        .map(|(n, _)| n.as_str())
        .collect();
    let declared: BTreeSet<&str> = NON_VISUAL_ROSTER.iter().map(|(n, _)| *n).collect();

    let added: Vec<&str> = actual.difference(&declared).copied().collect();
    let gone: Vec<&str> = declared.difference(&actual).copied().collect();
    assert!(
        added.is_empty() && gone.is_empty(),
        "Theme의 색·길이 외 필드가 등록 목록과 다르다. 추가: {added:?}, 삭제: {gone:?}. 실제 용도와 타입을 검토해 목록의 이름·사유를 갱신한다. 시각 값 외 런타임 설정이 셋 이상 필요하면 ADR-0037에 따라 별도 컨텍스트도 검토한다. 소스: {THEME_SRC}"
    );
}

/// 설정에서 전달하는 값은 ThemeRuntime 목록과 별도로 비교한다.
#[test]
fn the_runtime_bundle_still_carries_only_the_declared_settings_values() {
    let fields = pub_fields(&read(RUNTIME_SRC), "ThemeRuntime");
    eprintln!(
        "[theme-payload] ThemeRuntime 필드 {} · 명부 {}",
        fields.len(),
        RUNTIME_ROSTER.len()
    );
    let actual: BTreeSet<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
    let declared: BTreeSet<&str> = RUNTIME_ROSTER.iter().copied().collect();
    assert!(
        !actual.is_empty(),
        "ThemeRuntime 필드를 읽지 못했다. {RUNTIME_SRC}의 구조체와 파서를 확인한다."
    );
    assert_eq!(
        actual, declared,
        "ThemeRuntime의 설정 필드가 목록과 다르다. 변경된 값의 역할을 확인하고 목록을 갱신한다. 시각 값 외 런타임 설정이 셋 이상 필요하면 ADR-0037에 따라 별도 컨텍스트를 검토한다."
    );
}

/// 실제 구조체와 독립된 입력으로 공개 필드·중첩 타입·구조체 경계를 확인한다.
#[test]
fn the_field_reader_sees_pub_fields_and_nothing_else() {
    let src = "\
pub struct Alpha {
    pub one: HexColor,
    /// 주석 줄은 필드가 아니다.
    pub two: BTreeMap<String, Vec<u8>>,
    private_three: bool,
    pub four: bool,
}
pub struct Bravo {
    pub five: bool,
}";
    let a = pub_fields(src, "Alpha");
    assert_eq!(
        a.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        vec!["one", "two", "four"],
        "`pub` 이 아닌 필드와 주석은 안 센다"
    );
    assert_eq!(a[1].1, "BTreeMap<String, Vec<u8>>");
    assert_eq!(
        pub_fields(src, "Bravo")
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec!["five"]
    );
    assert!(pub_fields(src, "Charlie").is_empty());
}

#[test]
fn the_visual_test_reads_the_head_of_the_type() {
    assert!(is_visual("HexColor"));
    assert!(is_visual("LogicalPx"));
    assert!(!is_visual("BTreeMap<String, HexColor>"));
    assert!(!is_visual("bool"));
    assert!(!is_visual("f32"));
}
