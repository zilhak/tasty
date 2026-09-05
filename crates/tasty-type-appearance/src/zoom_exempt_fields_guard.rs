//! `Theme` 의 `LogicalPx` 필드 중 **host UI zoom 을 안 타는 것의 집합을 고정**한다.
//!
//! # 왜 필요한가
//!
//! `Theme::with_colors_and_zoom` 은 sizing 토큰에 `zoomed()` 를 곱해 배율을 먹인다.
//! 일부 필드는 **의도적으로** 그 곱셈을 건너뛴다(1px 보더, 고정 px 크롬, 터미널 콘텐츠
//! 폰트 등). 문제는 그 면제가 **위반 집합이 아니라 표시 없는 집합**이라는 것이다 —
//! 새 필드를 `zoomed()` 없이 더해도 아무것도 빨개지지 않으므로, 면제가 결정이 아니라
//! 사고로 늘어난다.
//!
//! 사유 자체는 세 곳에 산문으로 흩어져 있었고 **서로 어긋나 있었다**(한쪽에만 있는
//! 항목이 양방향으로 존재했고, 두 필드는 어느 목록에도 없었다). 그래서 이 파일의
//! [`EXEMPT`] 를 **정본**으로 삼고, 산문은 이 목록을 가리키게 한다.
//!
//! # 대조 형태 — 이름 집합 동등
//!
//! 하한(`>= n`)은 새 필드의 조용한 합류를 못 막고, 건수 고정(`== n`)은 빨개지기는 해도
//! **무엇이 늘었는지 말하지 않는다.** 집합 동등만이 "누가 새로 면제됐는가" 를 이름으로
//! 뱉는다. 그래서 여기서는 [`EXEMPT`] 의 이름 집합과 소스 실측 집합의 **동등**을 본다.
//!
//! # 거짓 초록 방지
//!
//! 스캔 대상은 `CARGO_MANIFEST_DIR/src/theme.rs` 하나다. 파일을 못 읽으면 필드가 0 개가
//! 되어 집합 동등이 "전부 사라졌다" 로 빨개지지만, 그 메시지가 오해를 부르므로
//! [`FIELD_FLOOR`] 로 **읽었는가** 를 먼저 단정한다 — 0 건과 "파일을 못 읽었다" 를 가른다.

use std::fs;
use std::path::PathBuf;

/// `Theme` 의 `LogicalPx` 필드 총수 하한. 실측 59 (2026-09-05).
/// 이 값은 "스캔이 파일을 읽었는가" 를 보는 것이지 면제 건수가 아니다.
const FIELD_FLOOR: usize = 40;

fn theme_source() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("theme.rs");
    fs::read_to_string(p).unwrap_or_default()
}

/// 면제 사유의 갈래. 새 면제를 더할 때 **어느 갈래인지 말할 수 없으면 면제가 아니다.**
///
/// # 갈래는 결정이고, 그 결정이 관측되는지는 성질이다
///
/// 갈래 자체(왜 면제인가)는 강제할 수 없다. `tab_bar_height` 는 탭바 밖
/// (`banner` · `egui_panels` · `overlay`)에서도 정당하게 소비되므로 "소비 자리로
/// 갈래를 판정한다" 는 성립하지 않는다. 그래서 가드가 갈래에 대해 강제하는 것은
/// **같은 결정이 두 곳(이 목록과 필드 doc)에 같게 적혀 있다**는 것뿐이다.
///
/// 다만 **그 결정이 픽셀로 관측되는가**는 강제 가능한 성질이고, 그것은 따로 본다
/// ([`the_unobservable_exemptions_are_exactly_the_pinned_ones`]). 한때 이 자리에
/// "`tab_indicator_width`(2.0)는 면제인데 `focus_ring_width`(2.0)는 zoom 을 탄다 —
/// 같은 값 반대 판정이니 성질이 없다" 고 적었는데 **틀렸다**: 지원 배율
/// `0.85 / 1.0 / 1.2` 에서 둘 다 반올림 결과가 2 라, 판정이 갈려도 **픽셀은 같다.**
/// 반례가 아니라 "차이가 관측되지 않는 쌍" 이었다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// UI 크롬이 아니라 렌더 콘텐츠 — UI 배율 축 밖이다.
    Content,
    /// hairline. 굵어지면 형상이 뭉개지거나 정책(1px 보더)을 깬다.
    Hairline,
    /// 탭바 크롬 — 컨테이너와 그 안의 폰트가 **함께** 고정이라 클리핑이 안 난다.
    TabBar,
    /// 상태바 크롬 — 위와 같은 이유.
    StatusBar,
    /// CSD 타이틀바 크롬 — OS 창 장식 기하라 배율과 독립이다.
    Titlebar,
}

/// **정본 목록.** `zoomed()` 를 타지 않는 `Theme` 의 `LogicalPx` 필드 전부와 그 사유.
impl Reason {
    /// 이 갈래의 필드 doc 에 반드시 들어 있어야 하는 표지.
    fn marker(self) -> &'static str {
        match self {
            Reason::Content => "콘텐츠",
            Reason::Hairline => "hairline",
            Reason::TabBar => "탭바 크롬",
            Reason::StatusBar => "상태바 크롬",
            Reason::Titlebar => "CSD 타이틀바 크롬",
        }
    }
}

const EXEMPT: &[(&str, Reason)] = &[
    ("font_size_prose_h1", Reason::Content),
    ("font_size_term_sm", Reason::Content),
    ("font_size_term", Reason::Content),
    ("font_size_term_lg", Reason::Content),
    ("border_width", Reason::Hairline),
    ("icon_stroke_width", Reason::Hairline),
    ("tab_indicator_width", Reason::Hairline),
    ("tab_width", Reason::TabBar),
    ("tab_bar_height", Reason::TabBar),
    ("tab_bar_label_font_size", Reason::TabBar),
    ("tab_bar_arrow_font_size", Reason::TabBar),
    ("status_bar_height", Reason::StatusBar),
    ("titlebar_height", Reason::Titlebar),
    ("traffic_size", Reason::Titlebar),
    ("caption_width", Reason::Titlebar),
    ("window_button_size", Reason::Titlebar),
];

/// 필드 하나의 실측 결과: (이름, `zoomed()` 를 타는가).
type Field = (String, bool);

/// `Theme` 구조체 본문의 `LogicalPx` 필드를 생성자 초기화식과 짝지어 읽는다.
fn scan_fields(text: &str) -> Vec<Field> {
    let lines: Vec<&str> = text.lines().collect();
    let struct_head = concat!("pub struct ", "Theme {");
    let Some(s) = lines.iter().position(|l| l.starts_with(struct_head)) else {
        return Vec::new();
    };
    let Some(e) = lines
        .iter()
        .skip(s + 1)
        .position(|l| *l == "}")
        .map(|i| i + s + 1)
    else {
        return Vec::new();
    };

    let mut names = Vec::new();
    for line in &lines[s + 1..e] {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("pub ")
            && let Some(name) = rest.strip_suffix(": LogicalPx,")
            && !name.contains(' ')
        {
            names.push(name.to_string());
        }
    }

    // 생성자의 `Self { .. }` 는 zoom 클로저 정의 직후에 온다.
    let ctor_anchor = concat!("let zoo", "med = |px: LogicalPx|");
    let Some(c) = lines.iter().position(|l| l.contains(ctor_anchor)) else {
        return Vec::new();
    };

    let zoom_call = concat!("zoo", "med(");
    let mut out = Vec::new();
    for name in names {
        let head = format!("{name}: ");
        let init = lines[c..]
            .iter()
            .find(|l| l.trim_start().starts_with(&head))
            .map(|l| l.trim_start().to_string());
        match init {
            // 초기화식을 못 찾으면 면제로 세지 않는다 — 못 읽은 것을 통과로 바꾸지 않기
            // 위해, 아래 하한과 동등 대조가 그 결손을 드러내게 둔다.
            None => continue,
            Some(body) => {
                let zoomed = body.contains(zoom_call);
                out.push((name, zoomed));
            }
        }
    }
    out
}

/// `SIZING` 상수 블록에서 필드별 zoom 1 값을 읽는다.
fn scan_sizing_values(text: &str) -> Vec<(String, f32)> {
    let lines: Vec<&str> = text.lines().collect();
    let head = concat!("pub const SIZING: ", "ThemeSizing = ThemeSizing {");
    let Some(s) = lines.iter().position(|l| l.starts_with(head)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in lines.iter().skip(s + 1) {
        let t = line.trim();
        if t == "};" {
            break;
        }
        let Some((name, rest)) = t.split_once(": LogicalPx(") else {
            continue;
        };
        let Some((num, _)) = rest.split_once(')') else {
            continue;
        };
        if let Ok(v) = num.parse::<f32>() {
            out.push((name.to_string(), v));
        }
    }
    out
}

/// `Theme` 필드의 doc 주석 블록을 이름과 짝지어 읽는다.
fn scan_field_docs(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let struct_head = concat!("pub struct ", "Theme {");
    let Some(s) = lines.iter().position(|l| l.starts_with(struct_head)) else {
        return Vec::new();
    };
    let Some(e) = lines
        .iter()
        .skip(s + 1)
        .position(|l| *l == "}")
        .map(|i| i + s + 1)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in (s + 1)..e {
        let t = lines[i].trim();
        let Some(rest) = t.strip_prefix("pub ") else {
            continue;
        };
        let Some(name) = rest.strip_suffix(": LogicalPx,") else {
            continue;
        };
        if name.contains(' ') {
            continue;
        }
        let mut doc = String::new();
        let mut j = i;
        while j > s + 1 && lines[j - 1].trim_start().starts_with("///") {
            j -= 1;
            doc.insert_str(0, lines[j].trim_start().trim_start_matches("///"));
        }
        out.push((name.to_string(), doc));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn declared() -> BTreeSet<String> {
        EXEMPT.iter().map(|(n, _)| (*n).to_string()).collect()
    }

    fn measured(fields: &[Field]) -> BTreeSet<String> {
        fields
            .iter()
            .filter(|(_, z)| !*z)
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// A-2 — 소스에서 읽은 면제 집합과 정본 목록이 **이름 단위로 같아야** 한다.
    #[test]
    fn the_zoom_exempt_field_set_is_exactly_the_declared_one() {
        let fields = scan_fields(&theme_source());

        // ① 모수 단언이 먼저다. 파일을 못 읽으면 아래 동등 대조가 "전부 빠졌다" 로
        //    빨개지긴 하지만, 그 메시지는 원인을 가리키지 못한다.
        assert!(
            fields.len() >= FIELD_FLOOR,
            "LogicalPx 필드가 {} 개뿐이다 — 하한 {}. 스캔이 theme.rs 를 못 읽었을 \
             가능성이 높다(면제 집합 판정 이전의 문제다).",
            fields.len(),
            FIELD_FLOOR
        );
        // ② 비영 대조 — zoom 을 타는 쪽도 0 이 아니어야 파서가 살아 있는 것이다.
        let zoomed_count = fields.iter().filter(|(_, z)| *z).count();
        assert!(
            zoomed_count > 0,
            "zoom 을 타는 필드가 0 이다 — 초기화식 파싱이 죽었다 (필드 {} 개는 읽혔다)",
            fields.len()
        );

        let m = measured(&fields);
        let d = declared();
        let joined: Vec<&String> = m.difference(&d).collect();
        let gone: Vec<&String> = d.difference(&m).collect();
        assert!(
            joined.is_empty() && gone.is_empty(),
            "zoom 면제 집합이 정본과 다르다.\n  새로 면제된 필드: {joined:?}\n  \
             목록에만 있고 소스엔 없는 필드: {gone:?}\n\
             새 필드를 면제하려면 EXEMPT 에 **사유 갈래와 함께** 등록해라. \
             갈래를 못 고르겠으면 그건 면제가 아니라 누락이다."
        );
    }

    /// 판정기 대조 — `zoomed()` 유무를 실제로 가르는가. 모수 단언과 서로를 대체하지
    /// 않는다: 이 테스트는 "판정기가 죽었는가", 위 테스트는 "볼 것이 주어졌는가" 를 본다.
    #[test]
    fn the_scanner_tells_a_zoomed_initializer_from_a_bare_one() {
        let src = FIXTURE;
        let f = scan_fields(src);
        assert_eq!(f.len(), 3, "fixture 필드 3 개를 읽어야 한다: {f:?}");
        assert_eq!(
            f.iter().find(|(n, _)| n == "spacing_md").map(|(_, z)| *z),
            Some(true),
            "zoom 을 타는 초기화식을 못 알아봤다"
        );
        assert_eq!(
            f.iter().find(|(n, _)| n == "border_width").map(|(_, z)| *z),
            Some(false),
            "맨 초기화식을 zoom 으로 오인했다"
        );
    }

    /// R79 — 변이 대상을 이름으로 박지 않고 **가장 잘 숨는 형태**를 고른다: 기존 면제
    /// 갈래의 접두사를 그대로 쓰는 새 필드다. 같은 테스트에서 약한 형태 둘이 이 변이에
    /// **초록으로 남는다는 것**까지 단정한다 — 안 하면 "집합 동등이 더 낫다" 가 주장이다.
    #[test]
    fn a_new_field_that_borrows_an_exempt_prefix_is_caught_only_by_set_equality() {
        // 변이: 탭바 갈래의 접두사를 빌린 새 필드가 zoom 없이 합류한다.
        let mutant = FIXTURE.replace(
            "    pub border_width: LogicalPx,",
            "    pub border_width: LogicalPx,\n    pub tab_bar_close_size: LogicalPx,",
        );
        let mutant = mutant.replace(
            "            border_width: SIZING.border_width,",
            "            border_width: SIZING.border_width,\n            \
             tab_bar_close_size: SIZING.tab_bar_close_size,",
        );
        let fields = scan_fields(&mutant);
        assert_eq!(
            fields.len(),
            4,
            "변이가 안 얹혔다 — 필드가 4 개여야 한다: {fields:?}"
        );

        let exempt = measured(&fields);
        assert!(
            exempt.contains("tab_bar_close_size"),
            "변이 필드가 면제 집합에 안 들어왔다 — 변이가 무력했다"
        );

        // 약한 형태 ① 하한: 면제가 늘기만 했으므로 여전히 초록이다.
        let floor_form = exempt.len() >= 2;
        assert!(
            floor_form,
            "약한 형태(하한)가 이 변이에 **초록으로 남는다**"
        );

        // 약한 형태 ② 갈래 접두사: 알려진 접두사로 시작하므로 그대로 통과한다.
        let prefixes = [
            "tab_bar_",
            "titlebar_",
            "font_size_term",
            "border_",
            "icon_stroke_",
        ];
        let prefix_form = exempt
            .iter()
            .all(|n| prefixes.iter().any(|p| n.starts_with(p)));
        assert!(
            prefix_form,
            "약한 형태(갈래 접두사)가 이 변이에 **초록으로 남는다**"
        );

        // 강한 형태 — 집합 동등만이 이름을 대며 빨개진다.
        let declared_here: BTreeSet<String> =
            ["border_width".to_string(), "font_size_term".to_string()].into();
        let joined: Vec<&String> = exempt.difference(&declared_here).collect();
        assert_eq!(
            joined,
            vec![&"tab_bar_close_size".to_string()],
            "집합 동등이 변이 필드를 **이름으로** 지목해야 한다"
        );
    }

    /// 이 파일이 자기 바늘을 통짜로 담지 않는지 본다. 오늘은 스캔 대상이 `theme.rs`
    /// 하나라 걸리지 않지만 **그건 우연이다** — 스캔이 넓어지는 날 자기를 세기 시작한다.
    #[test]
    fn the_guard_does_not_carry_its_own_anchors() {
        let me = include_str!("zoom_exempt_fields_guard.rs");
        let struct_head = concat!("pub struct ", "Theme {");
        let ctor_anchor = concat!("let zoo", "med = |px: LogicalPx|");
        assert_eq!(
            me.matches(struct_head).count(),
            0,
            "가드가 구조체 앵커를 통짜로 담고 있다 — 쪼개진 형태로 되돌려라"
        );
        assert_eq!(
            me.matches(ctor_anchor).count(),
            0,
            "가드가 생성자 앵커를 통짜로 담고 있다 — 쪼개진 형태로 되돌려라"
        );
        // 비영 대조 — 쪼갠 조각은 실제로 이 파일에 있다. "0 회" 와 "파일을 못 읽었다" 를 가른다.
        assert!(me.contains("LogicalPx"), "가드 소스를 못 읽었다");
    }

    /// 실물 대조 — 스캐너가 진짜 소스에서 알려진 필드를 짚어내는가(동일성).
    #[test]
    fn the_scanner_finds_known_fields_in_the_real_source() {
        let fields = scan_fields(&theme_source());
        let by = |n: &str| fields.iter().find(|(f, _)| f == n).map(|(_, z)| *z);
        assert_eq!(
            by("border_width"),
            Some(false),
            "border_width 는 면제여야 한다"
        );
        assert_eq!(
            by("spacing_md"),
            Some(true),
            "spacing_md 는 zoom 을 타야 한다"
        );
        assert_eq!(
            by("focus_ring_width"),
            Some(true),
            "focus_ring_width 는 zoom 을 탄다"
        );
    }

    /// 정본 목록 자체의 위생 — 중복 이름이 있으면 집합 동등이 조용히 느슨해진다.
    #[test]
    fn the_declared_list_has_no_duplicates() {
        assert_eq!(
            declared().len(),
            EXEMPT.len(),
            "EXEMPT 에 중복 이름이 있다 — 집합으로 접으면 {} 개로 줄어든다",
            declared().len()
        );
    }

    // 앵커 두 개를 `concat!` 로 쪼갠다 — 안 쪼개면 이 fixture 자체가 자기 바늘을
    // 통짜로 담게 되고, 스캔이 넓어지는 날 가드가 자기를 센다(R80 이 실제로 잡았다).
    /// R98 자기적용 — 갈래가 장식이 되지 않게, 각 면제 필드의 doc 이 자기 갈래의
    /// 표지를 담고 있는지 본다. 이것은 **주장의 참을 검사하지 않는다**(성질이 없다는
    /// 근거는 `Reason` 의 doc 에 반례 둘로 적었다). 검사하는 것은 같은 결정이 두 곳에
    /// 같게 적혀 있다는 것뿐이고, 그 덕에 한쪽만 조용히 바뀌는 일이 없어진다.
    #[test]
    fn every_exemption_repeats_its_category_in_the_field_doc() {
        let docs = scan_field_docs(&theme_source());
        assert!(
            docs.len() >= FIELD_FLOOR,
            "필드 doc 을 {} 개만 읽었다 — 하한 {}. 스캔이 죽었다.",
            docs.len(),
            FIELD_FLOOR
        );
        let mut bad = Vec::new();
        for (name, reason) in EXEMPT {
            let Some((_, doc)) = docs.iter().find(|(n, _)| n == name) else {
                bad.push(format!("{name}: 필드를 못 찾았다"));
                continue;
            };
            if !doc.contains(reason.marker()) {
                bad.push(format!("{name}: doc 에 '{}' 가 없다", reason.marker()));
            }
        }
        assert!(
            bad.is_empty(),
            "면제 갈래와 필드 doc 이 어긋난다:\n  {}",
            bad.join("\n  ")
        );
    }

    /// 표지가 실제로 변별력이 있는지 — 아무 doc 에나 걸리면 위 테스트는 무의미하다.
    #[test]
    fn the_category_markers_do_not_match_every_field() {
        let docs = scan_field_docs(&theme_source());
        let exempt: std::collections::BTreeSet<&str> = EXEMPT.iter().map(|(n, _)| *n).collect();
        let markers = [
            Reason::Content,
            Reason::Hairline,
            Reason::TabBar,
            Reason::StatusBar,
            Reason::Titlebar,
        ];
        let stray: Vec<&String> = docs
            .iter()
            .filter(|(n, _)| !exempt.contains(n.as_str()))
            .filter(|(_, d)| markers.iter().any(|m| d.contains(m.marker())))
            .map(|(n, _)| n)
            .collect();
        assert!(
            stray.is_empty(),
            "면제가 아닌 필드가 갈래 표지를 담고 있다 — 표지가 변별력을 잃었다: {stray:?}"
        );
        // 비영 대조 — 면제 쪽에서는 실제로 걸린다.
        let hit = docs
            .iter()
            .filter(|(n, _)| exempt.contains(n.as_str()))
            .filter(|(_, d)| markers.iter().any(|m| d.contains(m.marker())))
            .count();
        assert_eq!(
            hit,
            EXEMPT.len(),
            "면제 {} 중 {hit} 만 표지를 담았다",
            EXEMPT.len()
        );
    }

    /// `AppearanceSettings::ui_scale_factor_for` 의 값. 의존 방향이 반대라 복사한다.
    /// **사본이라 원본을 따라가지 않는다** — 원본에 배율이 추가돼도 여기는 그대로이고,
    /// 그 어긋남은 원본 크레이트의 `the_supported_ui_scale_set_is_pinned` 가 잡는다.
    const SUPPORTED_ZOOMS: [f32; 3] = [0.85, 1.0, 1.2];

    /// zoom 을 태우든 안 태우든 지원 배율 전체에서 **같은 픽셀**이 나오는 면제.
    /// 이 둘에게 면제는 결정이 아니라 무상이다 — 어느 쪽을 골라도 관측되지 않는다.
    const UNOBSERVABLE: &[&str] = &["border_width", "tab_indicator_width"];

    /// 면제가 픽셀로 **관측되는가**를 집합으로 고정한다.
    ///
    /// 건수가 아니라 집합인 이유: 이 수는 두 가지로 움직인다 — 토큰 **값**이 바뀌거나
    /// 지원 배율 **집합**이 바뀌거나. 둘 다 결함이 아니라 설계 변경이라, 문턱으로
    /// 박으면 "몇 개가 늘었다" 만 알고 **어느 면제가 무상에서 유상으로 바뀌었는지**를
    /// 모른다. 그 전이가 정확히 사람이 판단해야 하는 순간이다.
    #[test]
    fn the_unobservable_exemptions_are_exactly_the_pinned_ones() {
        let src = theme_source();
        let values: std::collections::BTreeMap<String, f32> =
            scan_sizing_values(&src).into_iter().collect();
        assert!(
            values.len() >= FIELD_FLOOR,
            "SIZING 값을 {} 개만 읽었다 — 하한 {}. 파서가 죽었다.",
            values.len(),
            FIELD_FLOOR
        );

        let mut measured: Vec<&str> = Vec::new();
        let mut missing: Vec<&str> = Vec::new();
        for (name, _) in EXEMPT {
            let Some(v) = values.get(*name) else {
                missing.push(name);
                continue;
            };
            if SUPPORTED_ZOOMS.iter().all(|z| (v * z).round() == *v) {
                measured.push(name);
            }
        }
        assert!(missing.is_empty(), "SIZING 에 없는 면제 필드: {missing:?}");

        let m: std::collections::BTreeSet<&str> = measured.into_iter().collect();
        let p: std::collections::BTreeSet<&str> = UNOBSERVABLE.iter().copied().collect();
        assert_eq!(
            m,
            p,
            "면제의 관측 가능성이 바뀌었다.\n  새로 '무상' 이 된 것: {:?}\n               '유상' 으로 바뀐 것: {:?}\n             값이 바뀌었거나 지원 배율 집합이 바뀌었다. 어느 쪽이든 디자인 판단이 필요하다.",
            m.difference(&p).collect::<Vec<_>>(),
            p.difference(&m).collect::<Vec<_>>()
        );

        // 비영 대조 — 나머지 면제는 실제로 관측된다. 전부 무상이면 이 축이 무의미하다.
        assert!(
            EXEMPT.len() > p.len(),
            "면제 전부가 관측 불가다 — 이 축에 걸린 것이 없다는 뜻이라 의심스럽다"
        );
    }

    /// 판별식이 살아 있는가 — 면제 밖에서도 같은 성질이 갈려야 한다.
    /// (`focus_ring_width` 2.0 은 면제가 아닌데 무상이고, `toast_accent_width` 3.0 은
    /// 1.2 에서 4 가 되어 유상이다. 판정이 값에서 나온다는 증거다.)
    #[test]
    fn the_observability_test_discriminates_outside_the_exempt_set() {
        let values: std::collections::BTreeMap<String, f32> =
            scan_sizing_values(&theme_source()).into_iter().collect();
        let free = |n: &str| {
            let v = values
                .get(n)
                .unwrap_or_else(|| panic!("{n} 을 SIZING 에서 못 찾았다"));
            SUPPORTED_ZOOMS.iter().all(|z| (v * z).round() == *v)
        };
        assert!(
            free("focus_ring_width"),
            "focus_ring_width(2.0)는 무상이어야 한다"
        );
        assert!(
            !free("toast_accent_width"),
            "toast_accent_width(3.0)는 유상이어야 한다"
        );
        assert!(
            !free("icon_stroke_width"),
            "icon_stroke_width(1.5)는 유상이어야 한다"
        );
    }

    const FIXTURE: &str = concat!(
        "pub struct ",
        "Theme {\n",
        "    pub spacing_md: LogicalPx,\n",
        "    pub border_width: LogicalPx,\n",
        "    pub font_size_term: LogicalPx,\n",
        "}\n",
        "\n",
        "impl Theme {\n",
        "    fn build() -> Self {\n",
        "        let zoo",
        "med = |px: LogicalPx| LogicalPx((px.value() * z).round());\n",
        "        Self {\n",
        "            spacing_md: zoo",
        "med(SIZING.spacing_md),\n",
        "            border_width: SIZING.border_width,\n",
        "            font_size_term: SIZING.font_size_term,\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
}
