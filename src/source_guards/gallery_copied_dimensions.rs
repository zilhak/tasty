//! **갤러리가 되풀이한 본체 치수가 아직 같은가.**
//!
//! 자매 가드([`super::gallery_specimen_parity`])는 "specimen 이 **있는가**" 를 묻는다.
//! 여기는 물음이 한 단계 위다 — **그 specimen 이 되풀이한 수가 본체 값과 같은가.**
//!
//! # 왜 중복 제거가 처방이 아닌가
//!
//! 갤러리는 별도 크레이트고 본체 bin 에 **의존할 수 없다.** 그래서 specimen 이 본체 치수를
//! 다시 적는 것은 게으름이 아니라 **구조적**이고, 없앨 수 없다. 없앨 수 없는 사본에 필요한
//! 것은 통일이 아니라 **갈라졌을 때 조용하지 않은 것**이다.
//!
//! # 실물 — "사본이라고 자백하는 사본" 과 그보다 나쁜 것
//!
//! `catalog/components/info_modal.rs` 는 440·140·360 을 직접 선언하고 주석에 "본체
//! `info_modal.rs` 의 `DEFAULT_WIDTH`" 라고 적는다. 사본임을 스스로 밝히지만, 밝힌다고
//! 따라가지는 않는다.
//!
//! 그보다 한 단계 나쁜 자리가 있다. **두 쪽이 이미 다른 규칙인데 수만 우연히 같다:**
//!
//! | | 본체 | 갤러리 |
//! |---|---|---|
//! | 버튼행 높이 | `FOOTER_ROOM` = 고정 48 | `item_height_interactive + spacing_lg + spacing_xs` |
//!
//! 오늘 28+16+4 = 48 이라 아무 데도 안 걸린다. **테마가 움직이면 갤러리만 따라가고 본체는
//! 그 자리에 남는다.** 값 비교로는 영영 안 보이고, 갈라지는 순간에도 화면 말고는 아무 신호가
//! 없다. 그 자리를 여기서 시끄럽게 만든다.
//!
//! # 좌우를 같은 열거로 — 비대칭이 자리 셋을 가리고 있었다
//!
//! 처음엔 본체 쪽을 **언제나 리터럴 상수**로 가정했다. 그 가정 때문에 방향이 뒤집힌 사본
//! (본체가 테마 파생 · 갤러리가 리터럴)은 명부에 **적을 수조차 없었다.** 좌우를 같은
//! [`Side`] 로 만들자 popup 제목바 높이 · 콘텐츠 여백 · 타이틀바 우측 여백 셋이 그냥 행으로
//! 들어왔다. 제목바 높이는 갤러리가 "**본체 popup 상수** — 제목바 높이" 라고 주석에 적는데
//! **본체엔 그런 상수가 없다** — 사본임을 자백하면서 상대를 잘못 지목한 자리다.
//!
//! # `ThemeSum` 은 값만 재지 않는다 — **그 파일이 아직 그 필드를 부르는가**도 본다
//!
//! 이게 없으면 파생이 리터럴로 바뀌어도 `theme.rs` 에서 같은 합이 나와 **조용히 초록**이다.
//! 실측으로 확인했다: `popup.rs::title_bar_height()` 를 `LogicalPx(28.0)` 으로 바꾸면 값은
//! 그대로 28 인데 이 검사가 빨개진다. 값이 안 움직이는 변이를 잡는 것이 이 검사의 전부다.
//!
//!
//! # 찍는 표에는 **자동 채널이 없다** (R494)
//!
//! libtest 는 통과한 테스트의 출력을 삼키고, 이 레포의 어느 회차 스텝도
//! `--show-output`·`--nocapture` 를 안 쓴다. 그러니 아래 `println!` 은 **초록 회차 어디에도
//! 안 나온다** — 손으로 `cargo test --bin tasty <이 모듈> -- --nocapture` 로 볼 때만 보인다.
//! 그것을 알고 둔다: 자동으로 지키는 것은 **단정**이고, 표는 사람이 눈으로 확인할 때 쓰는
//! 도구다. 표가 채널을 가진 것처럼 쓰지 마라.
//! # 이 가드는 지금 결정을 내리지 않는다
//!
//! 어느 쪽으로 통일할지는 설계 결정이다(본체를 테마 파생으로 바꾸면 **비-기본 테마에서 본체
//! 동작이 바뀐다** — 픽셀 0 은 기본 테마·기본 배율에서만 참이다). 그래서 여기서는 오늘 둘이
//! 같다는 것만 못 박고, **갈라지는 순간 실패 메시지가 그 결정을 요구한다.** 값을 지금 정하지
//! 말고, 정해야 할 때 정하라고 코드가 말하게 한다.

use tasty_doc_guards::source_text::mask_non_code;

const THEME: &str = "crates/tasty-type-appearance/src/theme.rs";
/// 생성 semantic 토큰 — `Alias` 가 한 단 따라가는 곳.
const SEMANTIC: &str = "crates/tasty-design-tokens/src/generated/semantic.rs";
/// 그 semantic 토큰이 다시 가리키는 원시 토큰.
const PRIMITIVE: &str = "crates/tasty-design-tokens/src/generated/primitive.rs";

/// 한쪽이 그 수를 **어떻게 적고 있는가**. 좌우가 같은 열거인 것이 핵심이다.
///
/// 처음엔 본체 쪽을 언제나 리터럴 상수로 가정했는데, 그 비대칭이 **자리 셋을 가렸다** —
/// 본체가 테마에서 파생하고 갤러리가 리터럴을 드는, 방향이 뒤집힌 사본들이다. 좌우를 같은
/// 열거로 만들면 그 셋이 그냥 행으로 들어온다.
#[derive(Clone, Copy)]
enum Side {
    /// `const NAME: LogicalPx = LogicalPx(n);` — 값을 **되풀이한다**.
    Lit(&'static str, &'static str),
    /// `const NAME: LogicalPx = <..>::TOKEN;` — 생성 토큰을 한 단 따라간다.
    Alias(&'static str, &'static str),
    /// 그 파일이 `Theme` 필드들의 **합**으로 낸다 — 규칙이 이미 다르고 값만 같다.
    ThemeSum(&'static str, &'static [&'static str]),
}

/// (무엇인가, 본체 쪽, 갤러리 쪽).
const COPIED: &[(&str, Side, Side)] = &[
    (
        "모달 폭",
        Side::Lit("src/adapters/ui/info_modal.rs", "DEFAULT_WIDTH"),
        Side::Lit(GALLERY_INFO_MODAL, "WIDTH"),
    ),
    (
        "모달 높이 하한(clamp)",
        Side::Lit("src/adapters/ui/info_modal.rs", "MIN_HEIGHT"),
        Side::Lit(GALLERY_INFO_MODAL, "MIN_HEIGHT"),
    ),
    (
        "모달 높이 상한(clamp)",
        Side::Lit("src/adapters/ui/info_modal.rs", "MAX_HEIGHT"),
        Side::Lit(GALLERY_INFO_MODAL, "MAX_HEIGHT"),
    ),
    (
        "모달 본문 아래 버튼행 높이",
        Side::Lit("src/adapters/ui/info_modal.rs", "FOOTER_ROOM"),
        Side::ThemeSum(
            GALLERY_INFO_MODAL,
            &["item_height_interactive", "spacing_lg", "spacing_xs"],
        ),
    ),
    (
        "popup 제목바 높이",
        Side::ThemeSum("src/adapters/ui/popup.rs", &["item_height_interactive"]),
        Side::Lit(GALLERY_POPUP_FRAME, "TITLE_BAR_HEIGHT"),
    ),
    (
        "popup 콘텐츠 상하 여백",
        Side::ThemeSum("src/adapters/ui/popup.rs", &["spacing_xs"]),
        Side::Alias(GALLERY_POPUP_FRAME, "CONTENT_MARGIN"),
    ),
    (
        "popup 타이틀바 우측 끝 여백",
        Side::ThemeSum("src/adapters/ui/popup.rs", &["spacing_xs"]),
        Side::Alias(GALLERY_POPUP_FRAME, "TITLE_BTN_EDGE_PAD"),
    ),
    (
        "종료 확인 창 폭",
        Side::Lit("src/app/modal/quit.rs", "WINDOW_W"),
        Side::Lit(GALLERY_QUIT_MODAL, "WINDOW_W"),
    ),
    (
        "종료 확인 창 높이",
        Side::Lit("src/app/modal/quit.rs", "WINDOW_H"),
        Side::Lit(GALLERY_QUIT_MODAL, "WINDOW_H"),
    ),
    (
        "포트 스캐너 즐겨찾기 컬럼 폭",
        Side::Lit("src/adapters/ui/popup/port_scanner.rs", "FAV_COL_WIDTH"),
        Side::Lit(GALLERY_PORT_SCANNER, "FAV_COL_WIDTH"),
    ),
    (
        "프리셋 leaf 요약 표시 폭 임계",
        Side::Lit(
            "src/adapters/ui/preset/demo_layout.rs",
            "LEAF_SUMMARY_MIN_W",
        ),
        Side::Lit(GALLERY_PRESET_EDITOR, "LEAF_SUMMARY_MIN_W"),
    ),
    (
        "프리셋 leaf 요약 표시 높이 임계",
        Side::Lit(
            "src/adapters/ui/preset/demo_layout.rs",
            "LEAF_SUMMARY_MIN_H",
        ),
        Side::Lit(GALLERY_PRESET_EDITOR, "LEAF_SUMMARY_MIN_H"),
    ),
];

const GALLERY_QUIT_MODAL: &str = "crates/tasty-gallery/src/catalog/components/quit_modal.rs";
const GALLERY_PORT_SCANNER: &str = "crates/tasty-gallery/src/catalog/components/port_scanner.rs";
const GALLERY_PRESET_EDITOR: &str = "crates/tasty-gallery/src/catalog/components/preset_editor.rs";
const GALLERY_INFO_MODAL: &str = "crates/tasty-gallery/src/catalog/components/info_modal.rs";
const GALLERY_POPUP_FRAME: &str = "crates/tasty-gallery/src/catalog/popup_frame.rs";

/// `const NAME: LogicalPx = LogicalPx(<수>);` 의 (줄번호, 수). 순수 함수 — 합성 입력을
/// 그대로 먹인다.
///
/// 줄번호까지 내는 이유는 실패 메시지 때문이다. "값이 갈라졌다, 정해라" 만 띄우면 **무엇을**
/// 정해야 하는지 사람이 모른 채 메시지를 본다 — 좌표가 있어야 그 자리로 갈 수 있다.
fn const_site(masked: &str, name: &str) -> Option<(usize, f32)> {
    // `find_const_line` 이 이미 1-기반 줄번호를 낸다 — 여기서 또 더하면 좌표가 한 줄씩
    // 밀린다. 값 비교는 그래도 통과하므로 **표만 조용히 틀린다**(그 형태를 아래 detector 가
    // 잡았다).
    let (line_no, line) = find_const_line(masked, name)?;
    // `LogicalPx(n)` 이 흔하지만 **맨 `f32` 상수**도 같은 치수를 든다(본체
    // `port_scanner::FAV_COL_WIDTH` 가 그 형태다). 그 형태를 못 읽으면 그 쌍은 명부에
    // 적을 수조차 없다 — 자료구조의 비대칭이 "그 자리에 아무것도 없다" 로 보이는 형태다.
    let rest = match line.rfind("LogicalPx(") {
        Some(at) => &line[at + "LogicalPx(".len()..],
        None => line.split('=').nth(1)?.trim(),
    };
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    Some((line_no, num.parse().ok()?))
}

/// `Theme` 기본 표의 `name: LogicalPx(<수>),` (줄번호, 값).
fn theme_site(masked: &str, name: &str) -> Option<(usize, f32)> {
    let needle = format!("{name}: LogicalPx(");
    let (idx, line) = masked
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim_start().starts_with(&needle))?;
    let open = line.find(&needle)? + needle.len();
    let num: String = line[open..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    Some((idx + 1, num.parse().ok()?))
}

/// `[pub[(crate)]] const NAME:` 로 시작하는 줄을 찾는다.
///
/// 가시성 접두사를 허용하는 이유는 생성 토큰이 `pub(crate) const` 이기 때문이다. 이름은
/// **접두사가 아니라 낱말**로 맞춘다 — `MIN_HEIGHT` 로 `MIN_HEIGHT_LG` 를 집으면 엉뚱한
/// 값이 비교된다.
fn find_const_line<'a>(masked: &'a str, name: &str) -> Option<(usize, &'a str)> {
    let needle = format!("const {name}:");
    masked.lines().enumerate().find_map(|(i, l)| {
        let t = l
            .trim_start()
            .strip_prefix("pub(crate) ")
            .or_else(|| l.trim_start().strip_prefix("pub "))
            .unwrap_or(l.trim_start());
        t.starts_with(&needle).then_some((i + 1, l))
    })
}

/// `const NAME: LogicalPx = <..>::TOKEN;` 의 (줄번호, 토큰 이름).
///
/// 사본이 **값을 되풀이하지 않고 토큰을 가리키는** 형태다. 값 비교를 하려면 한 단
/// 따라가야 하고, 그 한 단을 안 따라가면 이 쌍은 아예 못 적는다.
fn alias_target(masked: &str, name: &str) -> Option<(usize, String)> {
    let (line_no, line) = find_const_line(masked, name)?;
    let rhs = line.split('=').nth(1)?.trim().trim_end_matches(';').trim();
    let token = rhs.rsplit("::").next()?.trim();
    (!token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()))
    .then(|| (line_no, token.to_string()))
}

/// semantic 토큰 하나의 값. `SPACE_XS -> primitive::SIZE_4 -> 4.0` 두 단을 따라간다.
fn token_value(semantic: &str, primitive: &str, token: &str) -> Option<(String, f32)> {
    let (sem_line, line) = find_const_line(semantic, token)?;
    let prim = line
        .split('=')
        .nth(1)?
        .trim()
        .trim_end_matches(';')
        .rsplit("::")
        .next()?
        .trim()
        .to_string();
    let (prim_line, value) = const_site(primitive, &prim)?;
    Some((
        format!("{token}@{SEMANTIC}:{sem_line} → {prim}={value}@{PRIMITIVE}:{prim_line}"),
        value,
    ))
}

fn read(rel: &str) -> String {
    mask_non_code(&read_raw(rel))
}

/// 마스크 **안 한** 원문. doc 에 무엇이 적혔는지를 묻는 자리에만 쓴다 — [`read`] 는
/// 주석을 지우므로 그 물음이 통째로 사라진다(실측으로 그 함정을 밟았다).
fn read_raw(rel: &str) -> String {
    std::fs::read_to_string(super::repo_root().join(rel)).unwrap_or_default()
}

/// 한 쪽을 값 하나 + 좌표 한 줄로 푼다. 좌우가 같은 함수를 지나므로 방향이 뒤집힌 사본도
/// 그냥 행이 된다.
fn resolve(side: &Side, theme: &str, semantic: &str, primitive: &str) -> (String, f32) {
    match side {
        Side::Lit(file, name) => {
            let (line, value) = const_site(&read(file), name).unwrap_or_else(|| {
                panic!("`{file}` 에서 `{name}` 을 못 읽었다 — 이름이 바뀌었으면 명부를 따라 고쳐라")
            });
            (format!("{file}:{line}  {name} = {value}"), value)
        }
        Side::Alias(file, name) => {
            let src = read(file);
            let (line, token) = alias_target(&src, name).unwrap_or_else(|| {
                panic!(
                    "`{file}:{name}` 이 토큰을 가리키는 형태가 아니다 — 리터럴로 바뀌었으면 \
                     명부의 이 행을 `Lit` 으로 옮겨라. 형태가 바뀐 것을 조용히 넘기면 \
                     사본이 늘어난 순간을 놓친다"
                )
            });
            let (trace, value) = token_value(semantic, primitive, &token).unwrap_or_else(|| {
                panic!("토큰 `{token}` 의 값을 못 따라갔다 — 못 따라가면 이 쌍은 비교 자체가 거짓이 된다")
            });
            (format!("{file}:{line}  {name} = {trace}"), value)
        }
        Side::ThemeSum(file, names) => {
            // ★ 그 파일이 **아직도 그 필드로 파생하는가**를 함께 본다. 이게 없으면 파생이
            // 리터럴로 바뀌어도 여기서는 theme.rs 에서 같은 합을 내 **조용히 초록**이다.
            let src = read(file);
            let mut sum = 0.0;
            let mut terms = Vec::new();
            for n in *names {
                assert!(
                    src.contains(n),
                    "`{file}` 이 더 이상 `{n}` 을 안 부른다 — 파생이 사라졌으면 이 자리는 \
                     테마를 안 따르는 값이 된 것이다. 명부의 이 행을 `Lit` 으로 옮기고 \
                     그것이 의도인지 정해라"
                );
                let (line, v) = theme_site(theme, n).unwrap_or_else(|| {
                    panic!("`{THEME}` 에서 `{n}` 의 기본값을 못 읽었다 — 못 읽으면 합이 작아져 이 단정이 거짓으로 빨개진다")
                });
                sum += v;
                terms.push(format!("{n}={v}@{THEME}:{line}"));
            }
            (
                format!("{file}  테마 파생 {} = {sum}", terms.join(" + ")),
                sum,
            )
        }
    }
}

/// 전부를 한 번에 본다 — 하나만 덮으면 나머지가 조용하다.
///
/// 갈라진 것 하나에서 멈추지 않고 **다 재고 나서** 실패한다. 첫 어긋남에서 멈추면 사람이
/// 한 번에 하나씩만 보게 되고, 그건 이 축에서 특히 나쁘다 — 여럿이 같은 결정 하나에
/// 매달려 있어서 따로 보면 같은 결정을 여러 번 내리게 된다.
#[test]
fn the_gallery_still_agrees_with_the_dimensions_it_restates() {
    // 하한이 아니라 **정확한 수**다. 하한이면 행을 빼는 것이 가장 싼 수선이 되고, 그건
    // 사본을 없앤 것이 아니라 **보는 눈을 없앤 것**이다.
    assert_eq!(
        COPIED.len(),
        12,
        "사본 명부가 {} 쌍이다(기록 12). 쌍을 빼는 것은 갈라짐을 고친 것이 아니라 안 보게 \
         만든 것이다 — 사본이 실제로 사라졌으면 이 수를 내리고, 새 사본을 찾았으면 올려라",
        COPIED.len()
    );
    let theme = read(THEME);
    let semantic = read(SEMANTIC);
    let primitive = read(PRIMITIVE);

    let mut listed = Vec::new();
    let mut split = Vec::new();
    for (what, host_side, gallery_side) in COPIED {
        let (left, host) = resolve(host_side, &theme, &semantic, &primitive);
        let (right, value) = resolve(gallery_side, &theme, &semantic, &primitive);
        let mark = if host == value { "=" } else { "≠" };
        listed.push(format!(
            "  [{mark}] {what}\n        본체   {left}\n        갤러리 {right}"
        ));
        if host != value {
            split.push((*what).to_string());
        }
    }

    let table = listed.join("\n");
    let headline = format!("갈라진 치수 {}: {}", split.len(), split.join(" · "));
    assert!(
        split.is_empty(),
        "{}\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        headline,
        table,
        "둘 중 어느 쪽으로 통일할지 정해라 — 이건 가드의 오탐이 아니다.",
        "본체를 테마 파생으로 바꾸면 비-기본 테마에서 본체 동작이 바뀌고,",
        "갤러리를 고정값으로 바꾸면 specimen 이 테마를 안 따른다.",
        "그 선택이 지금 필요해졌다는 것이 이 실패의 내용이다.",
        "명부에서 이 쌍을 빼는 것은 이행이 아니다 — 사본은 그대로 남고 보는 눈만 사라진다.",
        "★ 빼는 것이 이행인 경우는 하나뿐이다: 두 쪽이 **같은 항목 하나**를 읽게 되어 \
         비교할 두 값이 없어졌을 때. 본보기는 `popup_frame::TITLE_BTN_SIZE` — 갤러리와 \
         본체가 둘 다 `tasty_ui_widgets::tokens::POPUP_TITLE_BTN_SIZE` 를 읽는다. \
         그때는 쌍이 사라진 것이 아니라 사본이 사라진 것이고, 그 커밋이 양쪽에서 같은 \
         이름을 부르는 것을 보여준다."
    );
    // 초록일 때 무엇을 맞춰 봤는지 남긴다 — 다 봤는지는 이 목록으로만 보인다.
    println!("[사본 치수] {} 쌍\n{table}", COPIED.len());
}

/// 자백했지만 **사본이 아닌** 자리 — 두 쪽이 같은 항목 하나를 읽는다.
///
/// 이 명부가 [`COPIED`] 의 금지문에 적어 둔 예외의 실물이다. 쌍이 아니라 사본이 사라진
/// 형태라 비교할 두 값이 없다.
/// (갤러리 파일, 갤러리 상수, **두 쪽이 함께 읽는 토큰**, 그 토큰을 읽는 본체 파일, 사유).
///
/// 앞의 셋째·넷째 칸은 사유를 **기계가 다시 물을 수 있게** 하려고 있다. 산문만 있으면
/// 그 전제가 거짓이 되어도 표는 그대로 남아 면제만 살아남는다 — 이름·경로로 면제하는
/// 표의 공통 약점이다([`the_checkable_roster_premises_still_hold`]).
const SHARES_ONE_ITEM: &[(&str, &str, &str, &str, &str)] = &[(
    GALLERY_POPUP_FRAME,
    "TITLE_BTN_SIZE",
    "tasty_ui_widgets::tokens::POPUP_TITLE_BTN_SIZE",
    "src/adapters/ui/popup.rs",
    "본체와 갤러리가 둘 다 `tasty_ui_widgets::tokens::POPUP_TITLE_BTN_SIZE` 를 읽는다",
)];

/// 자백하면서 **다르다고 밝힌** 자리. 사본이 아니라 의도된 차이다.
/// (갤러리 파일, 갤러리 상수, **본체에 있으면 이 사유가 무너지는 이름**, 사유).
///
/// 셋째 칸이 사유의 반증 조건이다 — "같은 치수가 아니다" 는 본체에 대응하는 이름이 생기는
/// 순간 다시 봐야 한다. 그것만 기계가 묻는다.
const DECLARED_DIFFERENT: &[(&str, &str, &str, &str)] = &[(
    "crates/tasty-gallery/src/catalog/components/script_manager.rs",
    "FRAME_MAX_W",
    "FRAME_MAX_W",
    "본체는 settings content 폭을 상속하고 갤러리 미러만 카드로 감싸 bound 한다 — \
     같은 치수가 아니라는 것을 그 자리가 스스로 적는다",
)];

/// 갤러리에서 **본체를 지목하는 doc 이 붙은** 길이 상수 선언을 모은다.
///
/// 문자열 리터럴만 덮은 사본을 쓴다 — 물어야 할 것이 **주석에 무엇이 적혔는가**라
/// 주석까지 덮으면 물음 자체가 사라진다(두 물음은 서로의 답을 지운다).
fn sites_that_claim_a_host_counterpart(src: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start().trim_start_matches("pub ");
        if !t.starts_with("const ") || !t.contains(": LogicalPx") {
            continue;
        }
        let Some(name) = t
            .trim_start_matches("const ")
            .split(':')
            .next()
            .map(str::trim)
        else {
            continue;
        };
        let mut doc = String::new();
        let mut j = i;
        while j > 0 && lines[j - 1].trim_start().starts_with("///") {
            doc.insert_str(0, lines[j - 1].trim_start());
            j -= 1;
        }
        if doc.contains("본체") {
            out.push((i + 1, name.to_string()));
        }
    }
    out
}

/// **자백한 사본은 전부 명부에 있어야 한다.**
///
/// 이것이 이 축의 판별자다. 종전에는 "갤러리 파일 이름과 본체 파일 이름이 같은가" 라는
/// **이름 바늘**로 갈랐는데, 그건 양쪽으로 틀린다(같은 이름인데 사본이 아닌 자리, 다른
/// 이름인데 사본인 자리). 여기서는 **그 자리가 스스로 적은 문장**을 읽고, 그 문장이
/// 명부에서 해소되는지를 묻는다 — 내용 판별자다.
///
/// # 이 판별자가 못 보는 것
///
/// **자백 안 한 사본은 안 보인다.** 갤러리 `preset_editor::LEAF_ICON_ONLY_MIN` 이 그
/// 예다 — 본체 `demo_layout.rs` 에 같은 이름·같은 값이 있는데 그 자리의 doc 은 본체를
/// 언급하지 않아 여기 안 걸린다. 그러니 이 수는 **사본의 수가 아니라 자백의 수**다.
/// 이름표를 명제보다 넓게 달지 않기 위해 적어 둔다.
#[test]
fn every_gallery_constant_that_claims_a_host_dimension_is_accounted_for() {
    // **자기 손으로 순회하지 않는다.** 직접 `read_dir` 을 돌면 그 순회가 조용히 좁아져도
    // "자백한 자리 0" 이 언제나 참이 된다. 공용 스캐너의 모수는 `scan_population` 이
    // git 목록과 **집합 동등**으로 못 박아 두었다.
    let mut claims: Vec<(String, usize, String)> = Vec::new();
    let mut files = 0usize;
    for (rel_path, raw) in super::rust_sources() {
        if !rel_path.starts_with("crates/tasty-gallery/src") {
            continue;
        }
        files += 1;
        let rel = rel_path.to_string_lossy().to_string();
        let src = tasty_doc_guards::source_text::mask_literals(&raw);
        for (line, name) in sites_that_claim_a_host_counterpart(&src) {
            claims.push((rel.clone(), line, name));
        }
    }
    assert!(
        files >= 80,
        "갤러리 소스를 {files} 개밖에 못 골랐다(하한 80) — 접두사가 트리와 안 맞으면 \
         스캐너가 성해도 이 판정만 공허해진다"
    );

    // 하나도 못 찾으면 아래 판정이 공허하게 참이 된다. 그 0 은 초록보다 조용하다.
    assert!(
        claims.len() >= 8,
        "본체를 지목하는 갤러리 상수를 {} 개밖에 못 찾았다(하한 8) — 수집이 깨졌으면 \
         아래 판정은 전부 공허하다",
        claims.len()
    );

    let mut unaccounted = Vec::new();
    for (rel, line, name) in &claims {
        let in_roster = COPIED.iter().any(|(_, _, g)| match g {
            Side::Lit(f, n) | Side::Alias(f, n) => f == rel && n == name,
            Side::ThemeSum(f, _) => f == rel,
        });
        let shared = SHARES_ONE_ITEM
            .iter()
            .any(|(f, n, ..)| *f == rel && *n == name);
        let different = DECLARED_DIFFERENT
            .iter()
            .any(|(f, n, ..)| *f == rel && *n == name);
        if !in_roster && !shared && !different {
            unaccounted.push(format!("  {rel}:{line}  {name}"));
        }
    }

    assert!(
        unaccounted.is_empty(),
        "본체를 지목해 놓고 어디에도 안 걸린 갤러리 상수가 {} 개다:\n{}\n\n\
         그 자리는 스스로 사본이라고 적었는데 그 주장을 **아무도 확인하지 않는다** — \
         갈라져도 화면 말고는 신호가 없다. 셋 중 하나를 해라:\n\
         · 본체에 대응하는 치수가 있으면 `COPIED` 에 쌍으로 올려라(본체가 인자 자리에 \
         리터럴을 박고 있으면 **먼저 본체에 이름을 줘라** — 이름이 없으면 가리킬 좌표가 없다)\n\
         · 두 쪽이 같은 항목 하나를 읽으면 `SHARES_ONE_ITEM` 에 사유와 함께 올려라\n\
         · 의도적으로 다른 치수면 `DECLARED_DIFFERENT` 에 사유와 함께 올려라\n\
         ★ 주석에서 \"본체\" 라는 낱말을 지우는 것은 **이행이 아니다** — 사본은 그대로 남고 \
         이 판별자만 못 보게 된다.",
        unaccounted.len(),
        unaccounted.join("\n")
    );

    println!(
        "[자백한 사본] {} 자리 · 명부 {} 쌍 · 공유 {} · 의도적 차이 {}",
        claims.len(),
        COPIED.len(),
        SHARES_ONE_ITEM.len(),
        DECLARED_DIFFERENT.len()
    );
}

/// `const NAME: ... = <초기화식>;` 의 초기화식 원문.
fn initializer_of(masked: &str, name: &str) -> Option<String> {
    let (_, line) = find_const_line(masked, name)?;
    Some(
        line.split_once('=')?
            .1
            .trim()
            .trim_end_matches(';')
            .trim()
            .to_string(),
    )
}

/// 갤러리 전체에서 그 상수가 **아직도 본체를 지목하고 있는가**.
///
/// 명부의 행이 사는 근거는 그 자리가 스스로 "본체" 라고 적었다는 것이다. 그 낱말을 지우면
/// 계상 판정에서 사라지고, 명부의 행만 남아 아무 자리도 안 지키게 된다 — 계상 판정이
/// 실패문에서 "그건 이행이 아니다" 라고 경고하는 바로 그 회피다. **명부에 오른 자리에
/// 한해서는** 그 회피를 여기서 빨갛게 만든다(명부에 오른 적 없는 자리는 여전히 안 보인다).
fn still_claims_a_host_counterpart(rel: &str, name: &str) -> bool {
    sites_that_claim_a_host_counterpart(&tasty_doc_guards::source_text::mask_literals(&read_raw(
        rel,
    )))
    .iter()
    .any(|(_, n)| n == name)
}

/// **면제 명부의 사유 중 기계로 확인할 수 있는 것을 확인한다.**
///
/// 면제의 근거가 산문이면, 근거가 거짓이 되어도 표는 그대로 남아 **면제만 살아남는다.**
/// 이 파일이 남에게 요구하는 것이 정확히 그것인데(계상 판정의 실패문 참조) 정작 자기
/// 명부 셋은 아무도 안 봤다. 실측으로 보였다: 갤러리 `TITLE_BTN_SIZE` 를
/// `LogicalPx::new(20.0)` 리터럴로 바꿔도 계상 판정은 **1 passed**, 출력이 글자까지
/// 같았다. 계상 판정이 묻는 것은 "명부에 있는가" 뿐이라 그 자리가 토큰을 읽든 수를 박든
/// 초록이다.
///
/// # 셋을 갈라서 — 검사되는 것과 안 되는 것
///
/// - **`SHARES_ONE_ITEM`** — 전제가 사실 진술이다("둘 다 이 토큰을 읽는다"). 여기서 셋을
///   묻는다: 갤러리 초기화식이 **경로식**인가, 본체가 **코드에서**(주석이 아니라) 같은
///   토큰을 부르는가, 그 토큰이 실재하고 값을 읽을 수 있는가.
/// - **`DECLARED_DIFFERENT`** — 전제의 절반만 사실 진술이다. **검사하는 쪽**: 본체에
///   대응하는 이름이 없다(생기면 "의도적 차이" 를 다시 봐야 한다) · 갤러리 쪽은 여전히
///   리터럴이다. **검사 못 하는 쪽**: "본체는 settings content 폭을 상속한다" 는 의미
///   판단이라 여기서 묻지 않는다 — 못 묻는다는 것을 적어 둔다.
/// - **`COPIED`** — 전제가 [`Side`] 의 모양 자체이고, 그 모양은
///   [`resolve`] 가 이미 매 회차 강제한다(`Alias` 가 리터럴이 되면 panic, `ThemeSum` 의
///   파일이 그 필드를 안 부르면 assert, `Lit` 이 경로식이 되면 값 파싱이 실패한다).
///   그래서 여기서 다시 묻지 않는다. **검사 못 하는 것 둘**: 행의 첫 칸(무엇인가)은
///   산문이고, "이 본체 자리가 **그 갤러리 자리의 짝**이다" 라는 지목은 이 모듈이 이름
///   바늘을 버린 이유 그대로 기계로 못 정한다.
///
/// 셋을 다 검사 가능하게 만들려고 비틀지 않았다. 산문이 필요한 자리는 산문으로 두고,
/// **산문이라는 것을 적는다.**
///
/// # R544 — 빨개졌을 때 가장 싼 초록화 경로
///
/// `SHARES_ONE_ITEM` 이 빨개지는 경로는 "갤러리가 토큰을 그만 읽는다" 이고, 그때 가장 싼
/// 초록화는 **명부 행을 `COPIED` 로 옮기는 것**이다 — 그러면 값 비교가 켜지므로 보호가
/// 줄지 않고 늘어난다. 행을 그냥 지우면 계상 판정이 빨개진다. 남은 싼 길 하나는 doc 에서
/// "본체" 를 지우는 것인데, 명부에 오른 자리에 한해 [`still_claims_a_host_counterpart`]
/// 가 그것을 빨갛게 만든다. **명부에 오른 적 없는 자리는 여전히 그 길로 빠져나간다** —
/// 계상 판정의 doc 이 적어 둔 구멍이고, 여기서 좁혀지지 않는다.
#[test]
fn the_checkable_roster_premises_still_hold() {
    assert!(
        !SHARES_ONE_ITEM.is_empty() && !DECLARED_DIFFERENT.is_empty(),
        "명부 둘 중 하나가 비었다 — 비면 아래 반복문이 통째로 안 돌고 이 시험은 공허한 \
         초록이 된다"
    );

    for (gallery_file, name, token_path, host_file, reason) in SHARES_ONE_ITEM {
        let short = token_path.rsplit("::").next().expect("토큰 경로");

        // ① 갤러리 쪽이 **값을 되풀이하지 않고 토큰을 가리키는가.** 이것이 "사본이 아니다"
        //    라는 사유의 본체다. 리터럴로 바뀌면 그 순간 사본이 하나 생긴 것이다.
        let init = initializer_of(&read(gallery_file), name).unwrap_or_else(|| {
            panic!("`{gallery_file}` 에서 `{name}` 의 초기화식을 못 읽었다 — 이름이 바뀌었으면 명부를 따라 고쳐라")
        });
        assert!(
            init.contains("::") && init.ends_with(short),
            "`{gallery_file}:{name}` 이 더 이상 `{token_path}` 를 가리키지 않는다 \
             (초기화식: `{init}`). `SHARES_ONE_ITEM` 의 사유가 거짓이 됐다 — \
             \"{reason}\".\n\
             이제 두 쪽이 **같은 항목 하나를 읽는 것이 아니라** 사본이 하나 생긴 것이다. \
             이 행을 `COPIED` 로 옮겨 값 비교를 켜라(그게 보호를 늘리는 쪽이다)."
        );

        // ② 본체가 그 토큰을 **코드에서** 부르는가. 주석까지 세면 호출을 지우고 언급만
        //    남겨도 초록이라, 마스크한 사본에서 묻는다 — 실제로 그 파일은 바로 윗줄
        //    주석에서 같은 이름을 언급한다.
        assert!(
            read(host_file).contains(token_path),
            "`{host_file}` 이 코드에서 `{token_path}` 를 안 부른다 — 본체가 그 항목을 \
             그만 읽었으면 두 쪽이 공유한다는 사유가 거짓이 됐다"
        );

        // ③ 그 항목이 실재하고 값을 읽을 수 있는가 — 비영 대조. 못 읽으면 위 둘이 \
        //    이름만 맞춘 것이 된다.
        let tokens = read("crates/tasty-ui-widgets/src/tokens.rs");
        let (_, value) = const_site(&tokens, short).unwrap_or_else(|| {
            panic!("`{short}` 가 위젯 토큰에 없거나 값을 못 읽었다 — 두 쪽이 읽는다는 그 항목이 실재해야 사유가 선다")
        });
        assert!(
            value > 0.0,
            "`{short}` 의 값을 {value} 로 읽었다 — 파서가 죽었으면 위 판정들이 이름만 \
             맞춘 채 초록이 된다"
        );

        assert!(
            still_claims_a_host_counterpart(gallery_file, name),
            "`{gallery_file}:{name}` 의 doc 이 더 이상 본체를 지목하지 않는다 — 낱말을 \
             지우면 계상 판정에서 사라지고 이 명부 행만 남는다. 그건 이행이 아니다"
        );
    }

    for (gallery_file, name, falsifier, reason) in DECLARED_DIFFERENT {
        // ① 갤러리 쪽은 여전히 자기 값을 든다 — 토큰을 읽기 시작했으면 "다른 치수" 가
        //    아니라 공유가 된 것이고, 그때는 `SHARES_ONE_ITEM` 이 맞는 자리다.
        let gallery = read(gallery_file);
        assert!(
            const_site(&gallery, name).is_some(),
            "`{gallery_file}:{name}` 이 리터럴 치수가 아니다 — `DECLARED_DIFFERENT` 의 \
             전제(\"갤러리 미러만 bound 한다\")가 흔들렸다. 사유: \"{reason}\""
        );

        // ② 본체에 같은 이름의 치수가 생겼는가 — 사유의 **반증 조건**이다. 생기면
        //    "의도적 차이" 인지 다시 봐야 한다.
        let mut host_hits = Vec::new();
        let mut host_files = 0usize;
        for (rel_path, raw) in super::rust_sources() {
            if !rel_path.starts_with("src/") {
                continue;
            }
            host_files += 1;
            if let Some((line, _)) = const_site(&mask_non_code(&raw), falsifier) {
                host_hits.push(format!("  {}:{line}", rel_path.to_string_lossy()));
            }
        }
        assert!(
            host_files >= 200,
            "본체 소스를 {host_files} 개밖에 못 골랐다(하한 200) — 순회가 좁아지면 \
             \"본체에 없다\" 가 안 봐서 나온 0 이 된다"
        );
        assert!(
            host_hits.is_empty(),
            "본체에 `{falsifier}` 라는 치수가 생겼다:\n{}\n\n\
             `DECLARED_DIFFERENT` 의 사유(\"{reason}\")는 본체에 대응하는 이름이 없다는 \
             것에 기대고 있었다. 같은 치수면 `COPIED` 로 옮기고, 여전히 다른 치수면 \
             사유를 그 자리에 맞게 다시 써라",
            host_hits.join("\n")
        );

        assert!(
            still_claims_a_host_counterpart(gallery_file, name),
            "`{gallery_file}:{name}` 의 doc 이 더 이상 본체를 지목하지 않는다 — 명부 행만 \
             남고 계상 판정은 그 자리를 못 보게 된다"
        );
    }

    // 반증 조건 검사의 비영 대조 — 같은 술어가 **있는 것은 찾는다**. 이게 없으면 위
    // `host_hits.is_empty()` 가 "파서가 죽어서 0" 인 것과 구분되지 않는다.
    let known = read("src/adapters/ui/info_modal.rs");
    assert!(
        const_site(&known, "FOOTER_ROOM").is_some(),
        "본체에 있는 것이 확실한 `FOOTER_ROOM` 도 못 찾았다 — 술어가 죽었으면 위의 \
         \"본체에 없다\" 는 전부 안 봐서 나온 0 이다"
    );
}

#[cfg(test)]
mod detector {
    use super::*;

    #[test]
    fn it_reads_a_named_length_constant() {
        let src = "const A: LogicalPx = LogicalPx(440.0);\nconst B: LogicalPx = LogicalPx(12.5);";
        assert_eq!(const_site(src, "A"), Some((1, 440.0)));
        assert_eq!(const_site(src, "B"), Some((2, 12.5)));
    }

    /// 이름이 **접두사로만** 맞는 상수를 집으면 엉뚱한 값이 비교된다.
    #[test]
    fn a_longer_name_is_not_the_name_asked_for() {
        let src = "const MIN_HEIGHT_LG: LogicalPx = LogicalPx(999.0);";
        assert_eq!(const_site(src, "MIN_HEIGHT"), None);
    }

    #[test]
    fn a_theme_field_default_is_read_by_name() {
        let src =
            "            spacing_lg: LogicalPx(16.0),\n            spacing_xs: LogicalPx(4.0),";
        assert_eq!(theme_site(src, "spacing_lg"), Some((1, 16.0)));
        assert_eq!(theme_site(src, "spacing_xs"), Some((2, 4.0)));
    }

    /// 생성 토큰은 `pub(crate) const` 이라 가시성 접두사를 못 넘으면 값 추적이 통째로 끊긴다.
    #[test]
    fn a_visibility_prefix_does_not_hide_the_constant() {
        assert_eq!(
            const_site(
                "pub(crate) const SIZE_4: LogicalPx = LogicalPx(4.0);",
                "SIZE_4"
            ),
            Some((1, 4.0))
        );
        assert_eq!(
            const_site("pub const W: LogicalPx = LogicalPx(9.0);", "W"),
            Some((1, 9.0))
        );
    }

    #[test]
    fn an_alias_names_the_token_it_points_at() {
        let src =
            "const CONTENT_MARGIN: LogicalPx = tasty_design_tokens::generated::semantic::SPACE_XS;";
        assert_eq!(
            alias_target(src, "CONTENT_MARGIN"),
            Some((1, "SPACE_XS".to_string()))
        );
    }

    /// 리터럴로 바뀐 사본을 별칭으로 읽으면 **엉뚱한 토큰 이름**이 나온다 — 그때는 못 읽는
    /// 것이 옳다(명부를 `Lit` 으로 옮기라는 실패가 뜬다).
    #[test]
    fn a_literal_is_not_an_alias() {
        assert_eq!(
            alias_target(
                "const CONTENT_MARGIN: LogicalPx = LogicalPx(4.0);",
                "CONTENT_MARGIN"
            ),
            None
        );
    }

    #[test]
    fn a_token_is_followed_two_hops_to_its_value() {
        let semantic = "pub const SPACE_XS: LogicalPx = super::primitive::SIZE_4;";
        let primitive = "pub(crate) const SIZE_4: LogicalPx = LogicalPx(4.0);";
        let (trace, v) =
            token_value(semantic, primitive, "SPACE_XS").expect("두 단을 따라가야 한다");
        assert_eq!(v, 4.0);
        assert!(
            trace.contains("SIZE_4"),
            "추적 문자열이 중간 단을 안 보여주면 좌표가 반쪽이다"
        );
    }

    /// 한 단만 따라가고 멈추면 값이 안 나온다 — 조용히 0 이 되지 않고 `None` 이어야 한다.
    #[test]
    fn a_token_whose_primitive_is_missing_is_not_guessed() {
        let semantic = "pub const SPACE_XS: LogicalPx = super::primitive::SIZE_4;";
        assert_eq!(token_value(semantic, "", "SPACE_XS"), None);
    }

    #[test]
    fn a_name_that_is_not_there_is_not_invented() {
        assert_eq!(
            const_site("const A: LogicalPx = LogicalPx(1.0);", "B"),
            None
        );
        assert_eq!(
            theme_site("spacing_lg: LogicalPx(16.0),", "spacing_md"),
            None
        );
    }
}
