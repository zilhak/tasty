//! `Theme` 이 나르는 **색도 치수도 아닌 값**에 이름표를 요구한다.
//!
//! [ADR-0174](../../../docs/adr/0174-theme-carries-reduced-motion.md) 는 `reduced_motion`
//! 을 `Theme` 에 실으면서 재검토 조건을 하나 달았다 — **"`Theme` 이 실어야 하는 색·치수가
//! 아닌 값이 셋 이상으로 늘어난다"**. 그 조건의 주어는 판정 시점에 레포가 읽을 수 있는
//! 사실이라 채널을 지을 수 있는데(ADR-0220 의 (가2)) 지금까지 없었다. 이 파일이 그 자리다.
//!
//! # 이 트리거는 좌변을 스스로 정하지 않는다
//!
//! "색·치수가 아닌 값" 을 세는 방법이 셋이고 값이 갈린다(실측 2026-09-08):
//!
//! - 타입으로 엄격히 — `HexColor`·`LogicalPx` 가 아닌 필드 전부 → **5**
//! - 설정에서 오는 런타임 값만(`ThemeRuntime` 한 덩이) → **2**
//! - 순수하게 비-시각인 것만(나머지를 색·치수 파생으로 봄) → **1**
//!
//! 저자 의도는 둘째다 — Decision 절이 `Settings::theme_runtime()` 을 지목하고 "값이 하나
//! 더 늘어도 호출부 모양은 안 변한다" 로 그 덩이를 센다. 그런데 **그것만 세면 `Theme` 에
//! 직접 들어온 값을 못 본다**(`is_light`·`surface_themes`·`line_height_ui` 셋이 그렇게
//! 들어왔다). 그래서 여기서는 **둘 다 센다.** 물음이 둘이므로 시험도 둘이고, 실패문이
//! 어느 쪽인지 말한다.
//!
//! # 명부는 사람이 적는다
//!
//! 아래 [`NON_VISUAL_ROSTER`] 를 소스에서 뽑지 않는다. 뽑으면 오늘의 필드 목록에 대한
//! 항진명제가 되어 무엇이 들어와도 초록이다. 사람이 적은 명부와 실물을 **대칭으로** 비교해
//! 새로 들어온 것과 사라진 것을 둘 다 잡는다.
//!
//! # 왜 타입 이름을 리터럴로 쓰면서 다른 가드에 안 앉는가
//!
//! `src/source_guards/` 의 넷이 길이 타입을 좌변으로 세는데, 그것들이 찾는 것은 **생성자
//! 호출 형태**(여는 괄호가 붙은 것)이지 타입 이름 자체가 아니다. 이 파일은 이름만 문자열로
//! 두고 생성자 형태를 한 번도 만들지 않는다 — doc 주석의 예시에서도 그렇다.

use std::collections::BTreeSet;

use tasty_doc_guards::repo_root;

/// 좌변 둘. 경로는 상수로 두고 문자열로 펴지 않는다 — 레포 상대 경로를 손으로
/// 평탄화하는 자리를 `src/source_guards/repo_relative_paths.rs` 가 센다.
const THEME_SRC: &str = "crates/tasty-type-appearance/src/theme.rs";
const RUNTIME_SRC: &str = "crates/tasty-themes/src/state.rs";

/// 색과 치수를 나르는 타입. 이 둘로 선언된 필드는 이 판정에서 "시각 값" 이다.
const VISUAL_TYPES: &[&str] = &["HexColor", "LogicalPx"];

/// `Theme` 에 실린 **색도 치수도 아닌** 값의 명부 — 이름과 **왜 그것이 시각 값이 아닌지**.
///
/// 사유를 함께 두는 이유는 다음 사람이 줄을 더할 때 "이것도 그런 값인가" 를 스스로 묻게
/// 하려는 것이다. 이름만 있으면 명부가 통과 목록이 된다.
const NON_VISUAL_ROSTER: &[(&str, &str)] = &[
    (
        "line_height_ui",
        "폰트 크기에 곱하는 배수다 — 길이가 아니라 비율이라 치수 타입을 못 쓴다",
    ),
    (
        "ui_zoom",
        "host UI 배율. 설정에서 오고 `ThemeRuntime` 이 나른다",
    ),
    (
        "reduced_motion",
        "접근성 모션 감소. 설정에서 오고 `ThemeRuntime` 이 나른다 — ADR-0174 가 실은 값이다",
    ),
    (
        "is_light",
        "테마가 밝은 쪽인가. 색에서 파생되지만 자기는 색이 아니다",
    ),
    (
        "surface_themes",
        "surface 별 테마 이름의 맵. 색 묶음을 **고르는** 이름이지 색이 아니다",
    ),
];

/// `ThemeRuntime` 이 나르는 설정 값의 명부 — ADR-0174 가 "값이 하나 더 늘어도 호출부
/// 모양은 안 변한다" 고 말한 그 덩이다. 셋이 되는 순간이 그 ADR 의 재검토 시점이다.
const RUNTIME_ROSTER: &[&str] = &["ui_zoom", "reduced_motion"];

/// `Theme` 필드 수의 하한 — **연기 검사**다.
///
/// 파싱이 죽으면 필드 0 이 되고, 빈 집합은 명부와 대칭 비교했을 때 "명부에만 있다" 로
/// 빨개지기는 한다. 그래도 하한을 따로 두는 이유는 실패문이 다르기 때문이다 —
/// "명부를 고쳐라" 가 아니라 **"구조체를 못 읽었다"** 라고 말해야 한다.
///
/// 값의 근거: 2026-09-08 실측 113. 여유가 큰 것은 이 수가 회차마다 몇 개씩 움직이는
/// 모수라서다 — 조이면 색 토큰 하나 추가에도 이 하한을 손대야 한다.
const MIN_THEME_FIELDS: usize = 80;

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

/// `pub struct <이름> { … }` 의 `pub` 필드를 `(이름, 타입)` 으로 읽는다.
///
/// 순수 함수다 — 합성 입력을 그대로 먹일 수 있어야 아래 단위 시험이 이 레포의 오늘
/// 상태가 아니라 **메커니즘**을 잰다.
///
/// 중괄호를 세어 본문을 잘라낸다. 정규식으로 바꾸지 마라 — 필드 타입에 중첩 제네릭이
/// 들어오면(`BTreeMap<String, Vec<T>>`) 게으른 매칭이 본문을 일찍 닫는다.
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
    // ★ `char_indices` 로 걷는다. `find` 가 주는 것은 **바이트** 오프셋이라, 문자 벡터로
    //   바꿔 인덱싱하면 비-ASCII 가 있는 파일에서 시작점이 어긋난다 — 이 레포의 소스는
    //   주석이 한글이라 그 어긋남이 조용한 부분 파싱으로 나온다(실측: 113 필드가 38 로
    //   읽혔다). 아래 단위 시험의 합성 입력에 한글 주석을 둔 것이 그것을 잡으라고 있는 것이다.
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

/// `Theme` 에 색도 치수도 아닌 값이 들어오면 명부에 이름과 사유가 있어야 한다.
///
/// ADR-0174 의 재검토 조건("셋 이상")이 발화하는 자리다. 대칭으로 비교하므로 **사라진
/// 것도** 잡는다 — 명부에만 남은 이름은 그 값이 `Theme` 을 떠났다는 뜻이고, 그때도
/// ADR 을 다시 봐야 한다.
#[test]
fn every_non_visual_theme_field_is_on_the_declared_roster() {
    let fields = pub_fields(&read(THEME_SRC), "Theme");
    // 측정값을 단정보다 앞에 둔다 — 빨간 경로에서도 모수가 남아야 한다.
    eprintln!(
        "[theme-payload] Theme 필드 {} · 비-시각 {} · 명부 {}",
        fields.len(),
        fields.iter().filter(|(_, t)| !is_visual(t)).count(),
        NON_VISUAL_ROSTER.len()
    );
    assert!(
        fields.len() >= MIN_THEME_FIELDS,
        "`Theme` 을 {} 필드밖에 못 읽었다(하한 {MIN_THEME_FIELDS}). 명부를 고치기 전에 \
         구조체 독법부터 봐라 — {THEME_SRC} 의 `pub struct Theme` 이 옮겨졌거나 형태가 바뀌었다",
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
        "`Theme` 의 색·치수가 아닌 값이 명부와 다르다.\n  \
         새로 들어온 것: {added:?}\n  명부에만 남은 것: {gone:?}\n\n  \
         처방: {THEME_SRC} 를 되돌리는 것이 아니라 **명부에 줄을 더해라** — 이름과 \
         `왜 그것이 색도 치수도 아닌지`를 함께 적는다.\n  \
         ★ 그리고 그 수가 셋을 넘으면 ADR-0174 의 재검토 조건이 발화한 것이다: \
         `Theme` 이 색·치수 밖의 값을 계속 실을지, 별도 자리로 가를지를 그 ADR 에서 다시 정해라."
    );
}

/// `ThemeRuntime` 이 나르는 설정 값의 이름이 명부와 같다.
///
/// ADR-0174 의 저자가 실제로 센 덩이가 이것이다 — Decision 절이 `Settings::theme_runtime()`
/// 을 지목한다. 위 시험과 **다른 물음**이라 시험을 가른다: 저쪽은 `Theme` 이 무엇을 싣는가,
/// 이쪽은 **설정에서 오는 값이 몇인가**다. 한 시험에 넣으면 실패문이 어느 쪽인지 못 말한다.
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
        "`ThemeRuntime` 을 한 필드도 못 읽었다 — {RUNTIME_SRC} 의 구조체 독법을 먼저 봐라"
    );
    assert_eq!(
        actual, declared,
        "설정에서 오는 값의 이름이 명부와 다르다.\n  \
         처방: 명부를 실물에 맞춘 뒤, **그 수가 셋이 됐는지 세라** — 셋이면 ADR-0174 의 \
         재검토 조건이 발화한 것이고, 그 ADR 은 그때 `Theme` 대신 별도 자리를 보라고 적어 뒀다."
    );
}

/// 파서가 무엇을 필드로 보고 무엇을 안 보는지 못 박는다.
///
/// 입력은 전부 합성이다 — 이 레포의 실물이나 위 상수에서 뽑으면 그 값에 대한 항진명제가
/// 되고, 그때 초록은 규칙이 아니라 오늘의 데이터를 재는 것이 된다.
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
    // 중첩 제네릭이 본문을 일찍 닫으면 `four` 가 사라진다 — 위 단정이 그것을 잡는다.
    assert_eq!(a[1].1, "BTreeMap<String, Vec<u8>>");
    // 다음 구조체로 새지 않는다.
    assert_eq!(
        pub_fields(src, "Bravo")
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec!["five"]
    );
    // 없는 구조체는 빈 목록 — 하한 단정이 그 갈래를 말이 되는 문장으로 바꾼다.
    assert!(pub_fields(src, "Charlie").is_empty());
}

/// 시각 판정이 머리 이름만 보는지 못 박는다.
#[test]
fn the_visual_test_reads_the_head_of_the_type() {
    assert!(is_visual("HexColor"));
    assert!(is_visual("LogicalPx"));
    // 제네릭 인자에 시각 타입이 들어 있어도 그 필드는 시각 값이 아니다.
    assert!(!is_visual("BTreeMap<String, HexColor>"));
    assert!(!is_visual("bool"));
    assert!(!is_visual("f32"));
}
