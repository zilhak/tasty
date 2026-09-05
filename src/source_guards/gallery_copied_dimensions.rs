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

/// `const NAME: LogicalPx = LogicalPx(<수>);` 의 수. 순수 함수 — 합성 입력을 그대로 먹인다.
fn const_value(masked: &str, name: &str) -> Option<f32> {
    let needle = format!("const {name}:");
    let line = masked
        .lines()
        .find(|l| l.trim_start().starts_with(&needle))?;
    let open = line.rfind("LogicalPx(")? + "LogicalPx(".len();
    let num: String = line[open..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    num.parse().ok()
}

/// `Theme` 기본 표의 `name: LogicalPx(<수>),` 값.
fn theme_default(masked: &str, name: &str) -> Option<f32> {
    let needle = format!("{name}: LogicalPx(");
    let line = masked
        .lines()
        .find(|l| l.trim_start().starts_with(&needle))?;
    let open = line.find(&needle)? + needle.len();
    let num: String = line[open..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    num.parse().ok()
}

fn read(rel: &str) -> String {
    mask_non_code(&std::fs::read_to_string(super::repo_root().join(rel)).unwrap_or_default())
}

/// 넷을 한 번에 본다 — 하나만 덮으면 나머지 셋이 조용하다.
#[test]
fn the_gallery_still_agrees_with_the_dimensions_it_restates() {
    assert!(
        !COPIED.is_empty(),
        "명부가 비면 아무것도 안 보면서 통과한다"
    );
    let theme = read(THEME);

    let mut listed = Vec::new();
    for (host_rel, host_const, gallery_rel, side, what) in COPIED {
        let host_src = read(host_rel);
        let host = const_value(&host_src, host_const).unwrap_or_else(|| {
            panic!("본체 `{host_rel}` 에서 `{host_const}` 를 못 읽었다 — 이름이 바뀌었으면 명부를 따라 고쳐라")
        });
        match side {
            GallerySide::Restated(name) => {
                let gallery_src = read(gallery_rel);
                let value = const_value(&gallery_src, name)
                    .unwrap_or_else(|| panic!("갤러리 `{gallery_rel}` 에서 `{name}` 을 못 읽었다"));
                assert_eq!(
                    host, value,
                    "{what}: 본체 `{host_const}`={host} 인데 갤러리 `{name}`={value} 다. \
                     갤러리는 본체 값을 되풀이하는 자리라 둘 중 하나가 낡은 것이다 — \
                     본체가 옳으면 갤러리를 맞추고, 갤러리가 옳으면 그것은 디자인 변경이니 \
                     본체를 먼저 고쳐라"
                );
                listed.push(format!("  {what:<24} {host_const}={host}  = {name}"));
            }
            GallerySide::ThemeSum(names) => {
                let mut sum = 0.0;
                for n in *names {
                    sum += theme_default(&theme, n).unwrap_or_else(|| {
                        panic!(
                            "`{THEME}` 에서 `{n}` 의 기본값을 못 읽었다 — 못 읽으면 합이 \
                                작아져 이 단정이 거짓으로 빨개진다"
                        )
                    });
                }
                assert_eq!(
                    host,
                    sum,
                    "{what}: 본체는 **고정값** `{host_const}`={host} 이고 갤러리는 **테마 파생** \
                     ({})={sum} 이다. 값이 갈라졌다 — 둘 중 어느 쪽으로 통일할지 정해라. \
                     이건 가드의 오탐이 아니다. 본체를 테마 파생으로 바꾸면 비-기본 테마에서 \
                     본체 동작이 바뀌고, 갤러리를 고정값으로 바꾸면 specimen 이 테마를 안 따른다. \
                     그 선택이 지금 필요해졌다는 것이 이 실패의 내용이다",
                    names.join(" + ")
                );
                listed.push(format!(
                    "  {what:<24} {host_const}={host}  = {}",
                    names.join(" + ")
                ));
            }
        }
    }
    // 초록일 때 무엇을 맞춰 봤는지 남긴다 — 넷을 다 봤는지는 이 목록으로만 보인다.
    println!("[사본 치수] {} 쌍\n{}", COPIED.len(), listed.join("\n"));
}

#[cfg(test)]
mod detector {
    use super::*;

    #[test]
    fn it_reads_a_named_length_constant() {
        let src = "const A: LogicalPx = LogicalPx(440.0);\nconst B: LogicalPx = LogicalPx(12.5);";
        assert_eq!(const_value(src, "A"), Some(440.0));
        assert_eq!(const_value(src, "B"), Some(12.5));
    }

    /// 이름이 **접두사로만** 맞는 상수를 집으면 엉뚱한 값이 비교된다.
    #[test]
    fn a_longer_name_is_not_the_name_asked_for() {
        let src = "const MIN_HEIGHT_LG: LogicalPx = LogicalPx(999.0);";
        assert_eq!(const_value(src, "MIN_HEIGHT"), None);
    }

    #[test]
    fn a_theme_field_default_is_read_by_name() {
        let src =
            "            spacing_lg: LogicalPx(16.0),\n            spacing_xs: LogicalPx(4.0),";
        assert_eq!(theme_default(src, "spacing_lg"), Some(16.0));
        assert_eq!(theme_default(src, "spacing_xs"), Some(4.0));
    }

    #[test]
    fn a_name_that_is_not_there_is_not_invented() {
        assert_eq!(
            const_value("const A: LogicalPx = LogicalPx(1.0);", "B"),
            None
        );
        assert_eq!(
            theme_default("spacing_lg: LogicalPx(16.0),", "spacing_md"),
            None
        );
    }
}
