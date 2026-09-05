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
//! 네 번째 자리는 그보다 한 단계 나쁘다. **두 쪽이 이미 다른 규칙인데 수만 우연히 같다:**
//!
//! | | 본체 | 갤러리 |
//! |---|---|---|
//! | 버튼행 높이 | `FOOTER_ROOM` = 고정 48 | `item_height_interactive + spacing_lg + spacing_xs` |
//!
//! 오늘 28+16+4 = 48 이라 아무 데도 안 걸린다. **테마가 움직이면 갤러리만 따라가고 본체는
//! 그 자리에 남는다.** 값 비교로는 영영 안 보이고, 갈라지는 순간에도 화면 말고는 아무 신호가
//! 없다. 그 자리를 여기서 시끄럽게 만든다.
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

/// 갤러리 쪽이 그 수를 어떻게 적고 있는가.
#[derive(Clone, Copy)]
enum GallerySide {
    /// 같은 값을 상수로 **되풀이한다**. 갈라지면 둘 중 하나가 낡은 것이다.
    Restated(&'static str),
    /// 그 수를 테마 이름들의 **합**으로 낸다 — 규칙이 이미 다르고 값만 같다.
    ThemeSum(&'static [&'static str]),
}

/// (본체 파일, 본체 상수, 갤러리 파일, 갤러리 쪽 표현, 무엇인가).
const COPIED: &[(&str, &str, &str, GallerySide, &str)] = &[
    (
        "src/adapters/ui/info_modal.rs",
        "DEFAULT_WIDTH",
        "crates/tasty-gallery/src/catalog/components/info_modal.rs",
        GallerySide::Restated("WIDTH"),
        "모달 폭",
    ),
    (
        "src/adapters/ui/info_modal.rs",
        "MIN_HEIGHT",
        "crates/tasty-gallery/src/catalog/components/info_modal.rs",
        GallerySide::Restated("MIN_HEIGHT"),
        "높이 하한(clamp)",
    ),
    (
        "src/adapters/ui/info_modal.rs",
        "MAX_HEIGHT",
        "crates/tasty-gallery/src/catalog/components/info_modal.rs",
        GallerySide::Restated("MAX_HEIGHT"),
        "높이 상한(clamp)",
    ),
    (
        "src/adapters/ui/info_modal.rs",
        "FOOTER_ROOM",
        "crates/tasty-gallery/src/catalog/components/info_modal.rs",
        GallerySide::ThemeSum(&["item_height_interactive", "spacing_lg", "spacing_xs"]),
        "본문 아래 버튼행 높이",
    ),
];

/// `const NAME: LogicalPx = LogicalPx(<수>);` 의 (줄번호, 수). 순수 함수 — 합성 입력을
/// 그대로 먹인다.
///
/// 줄번호까지 내는 이유는 실패 메시지 때문이다. "값이 갈라졌다, 정해라" 만 띄우면 **무엇을**
/// 정해야 하는지 사람이 모른 채 메시지를 본다 — 좌표가 있어야 그 자리로 갈 수 있다.
fn const_site(masked: &str, name: &str) -> Option<(usize, f32)> {
    let needle = format!("const {name}:");
    let (idx, line) = masked
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim_start().starts_with(&needle))?;
    let open = line.rfind("LogicalPx(")? + "LogicalPx(".len();
    let num: String = line[open..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    Some((idx + 1, num.parse().ok()?))
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

fn read(rel: &str) -> String {
    mask_non_code(&std::fs::read_to_string(super::repo_root().join(rel)).unwrap_or_default())
}

/// 넷을 한 번에 본다 — 하나만 덮으면 나머지 셋이 조용하다.
///
/// 갈라진 것 하나에서 멈추지 않고 **넷을 다 재고 나서** 실패한다. 첫 어긋남에서 멈추면
/// 사람이 한 번에 하나씩만 보게 되고, 그건 이 축에서 특히 나쁘다 — 넷이 같은 결정 하나에
/// 매달려 있어서 따로 보면 같은 결정을 네 번 내리게 된다.
#[test]
fn the_gallery_still_agrees_with_the_dimensions_it_restates() {
    // 하한이 아니라 **정확한 수**다. 하한이면 행을 빼는 것이 가장 싼 수선이 되고, 그건
    // 사본을 없앤 것이 아니라 **보는 눈을 없앤 것**이다.
    assert_eq!(
        COPIED.len(),
        4,
        "사본 명부가 {} 쌍이다(기록 4). 쌍을 빼는 것은 갈라짐을 고친 것이 아니라 안 보게 \
         만든 것이다 — 사본이 실제로 사라졌으면(한쪽이 없어졌으면) 이 수를 내리고, \
         새 사본을 찾았으면 올려라",
        COPIED.len()
    );
    let theme = read(THEME);

    let mut listed = Vec::new();
    let mut split = Vec::new();
    for (host_rel, host_const, gallery_rel, side, what) in COPIED {
        let host_src = read(host_rel);
        let (host_line, host) = const_site(&host_src, host_const).unwrap_or_else(|| {
            panic!("본체 `{host_rel}` 에서 `{host_const}` 를 못 읽었다 — 이름이 바뀌었으면 명부를 따라 고쳐라")
        });
        let left = format!("{host_rel}:{host_line}  {host_const} = {host}");
        let (right, value) = match side {
            GallerySide::Restated(name) => {
                let gallery_src = read(gallery_rel);
                let (line, value) = const_site(&gallery_src, name)
                    .unwrap_or_else(|| panic!("갤러리 `{gallery_rel}` 에서 `{name}` 을 못 읽었다"));
                (format!("{gallery_rel}:{line}  {name} = {value}"), value)
            }
            GallerySide::ThemeSum(names) => {
                let mut sum = 0.0;
                let mut terms = Vec::new();
                for n in *names {
                    let (line, v) = theme_site(&theme, n).unwrap_or_else(|| {
                        panic!("`{THEME}` 에서 `{n}` 의 기본값을 못 읽었다 — 못 읽으면 합이 작아져 이 단정이 거짓으로 빨개진다")
                    });
                    sum += v;
                    terms.push(format!("{n}={v}@{THEME}:{line}"));
                }
                (
                    format!("{gallery_rel}  테마 파생 {} = {sum}", terms.join(" + ")),
                    sum,
                )
            }
        };
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
        "{}\n\n{}\n{}\n{}\n{}\n{}\n{}",
        headline,
        table,
        "둘 중 어느 쪽으로 통일할지 정해라 — 이건 가드의 오탐이 아니다.",
        "본체를 테마 파생으로 바꾸면 비-기본 테마에서 본체 동작이 바뀌고,",
        "갤러리를 고정값으로 바꾸면 specimen 이 테마를 안 따른다.",
        "그 선택이 지금 필요해졌다는 것이 이 실패의 내용이다.",
        "명부에서 이 쌍을 빼는 것은 이행이 아니다 — 사본은 그대로 남고 보는 눈만 사라진다."
    );
    // 초록일 때 무엇을 맞춰 봤는지 남긴다 — 넷을 다 봤는지는 이 목록으로만 보인다.
    println!("[사본 치수] {} 쌍\n{table}", COPIED.len());
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
