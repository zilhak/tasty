//! **갤러리가 되풀이한 본체 *규칙* 이 아직 같은가.**
//!
//! 자매 가드 [`super::gallery_copied_dimensions`] 는 되풀이한 **수**를 본다. 여기는 되풀이한
//! **매핑**을 본다 — 어떤 갈래가 무엇으로 가는가. 두 물음이라 파일도 둘이다(하나로 합치면
//! 이름표가 명제보다 넓어진다).
//!
//! # 왜 필요한가 — 가드가 하나 있으면 그 옆도 덮인 것처럼 읽힌다
//!
//! `catalog/toast_card.rs` 는 **바로 위 이웃**(`ToastKind` 열거)에 대해서는 대조를 갖고 있다.
//! 그 파일의 `mod tests` 가 정본 크레이트를 dev-dep 으로 끌어와 두 `ALL` 을 런타임에 양방향
//! 대조한다. 그런데 **그 열거를 소비하는 매핑은 아무것도 안 봤다.** 읽는 사람에게는 "이
//! 파일은 본체와 대조된다" 로 보이는데, 덮인 것은 열거뿐이었다.
//!
//! 그 착시가 이 축에서 반복된 형태다 — 자매 가드의 존재 축("specimen 이 **있는가**")도
//! specimen 이 본체와 달라도 초록이었다. **있다 ≠ 맞다.**
//!
//! # 왜 중복 제거가 처방이 아닌가
//!
//! 갤러리는 정본 `ToastKind` 를 import 할 수 없다 — 그 크레이트가 termwiz/터미널 모델을
//! 끌고 오고, 그 사실을 `toast_card.rs` 가 자기 doc 에 적어 둔다. 그래서 매핑이 두 벌인 것은
//! 게으름이 아니라 **구조적**이고, 없앨 수 없다. 없앨 수 없는 사본에 필요한 것은 통일이
//! 아니라 **갈라졌을 때 조용하지 않은 것**이다.
//!
//! # 무엇을 비교하는가 — 받는 쪽 이름은 안 본다
//!
//! 본체는 `th.accent_primary()`, 갤러리는 `theme.accent_primary()` 다. 수신자 이름은 그 자리의
//! 지역 변수명이라 **갈라져도 결함이 아니다.** 비교하는 것은 (갈래, 부르는 이름) 짝이다.
//!
//! # 이 가드가 못 보는 것
//!
//! - **`match` 한 줄 팔만 읽는다.** 팔이 블록이거나 가드(`if`)를 달면 못 읽고, 그때는 팔 수가
//!   맞지 않아 **조용히 통과하는 것이 아니라 빨개진다**(수가 0 이 되면 하한이 잡는다).
//! - **이름이 같은데 뜻이 다른 것**은 못 가른다. 양쪽 `accent_primary` 가 서로 다른 테마
//!   접근자를 가리키게 되는 날은 이 가드 밖이다.
//! - 표에는 자동 채널이 없다(자매 가드와 같은 사정) — `println!` 은 통과한 테스트에서
//!   삼켜진다. 자동으로 지키는 것은 **단정**이다.

use tasty_doc_guards::source_text::mask_non_code;

/// (무엇인가, 본체 파일, 본체 함수, 갤러리 파일, 갤러리 함수).
const COPIED_RULES: &[(&str, &str, &str, &str, &str)] = &[(
    "토스트 kind → accent 색",
    "src/adapters/ui/toast.rs",
    "accent_color",
    "crates/tasty-gallery/src/catalog/toast_card.rs",
    "accent_color",
)];

/// `fn <이름>(` 이후 함수 본문의 한 줄 `match` 팔을 (갈래, 부르는 이름)으로 모은다.
///
/// 순수 함수다 — 합성 입력을 그대로 먹여 대조한다.
fn match_arms(masked: &str, fn_name: &str) -> Vec<(String, String)> {
    let needle = format!("fn {fn_name}(");
    let mut out = Vec::new();
    let mut inside = false;
    let mut depth = 0i32;
    for line in masked.lines() {
        if !inside {
            if line.contains(&needle) {
                inside = true;
                depth = brace_delta(line);
            }
            continue;
        }
        depth += brace_delta(line);
        if let Some((lhs, rhs)) = line.split_once("=>")
            && let Some(arm) = lhs.trim().trim_end_matches('|').rsplit("::").next()
            && let Some(target) = called_name(rhs)
        {
            let arm = arm.trim();
            if !arm.is_empty() && arm != "_" {
                out.push((arm.to_string(), target));
            }
        }
        if depth <= 0 {
            break;
        }
    }
    out
}

fn brace_delta(line: &str) -> i32 {
    line.chars().filter(|c| *c == '{').count() as i32
        - line.chars().filter(|c| *c == '}').count() as i32
}

/// `th.accent_primary().into(),` → `accent_primary`. **수신자 이름은 버린다.**
fn called_name(rhs: &str) -> Option<String> {
    let head = rhs.trim().split('(').next()?;
    let name = head.rsplit('.').next()?.trim();
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| name.to_string())
}

fn read(rel: &str) -> String {
    mask_non_code(&std::fs::read_to_string(super::repo_root().join(rel)).unwrap_or_default())
}

#[test]
fn the_gallery_still_maps_the_way_the_host_maps() {
    assert_eq!(
        COPIED_RULES.len(),
        1,
        "규칙 사본 명부가 {} 쌍이다(기록 1). 쌍을 빼는 것은 갈라짐을 고친 것이 아니라 \
         안 보게 만든 것이다",
        COPIED_RULES.len()
    );

    let mut listed = Vec::new();
    let mut split = Vec::new();
    for (what, host_rel, host_fn, gallery_rel, gallery_fn) in COPIED_RULES {
        let host = match_arms(&read(host_rel), host_fn);
        let gallery = match_arms(&read(gallery_rel), gallery_fn);

        // 팔을 하나도 못 읽으면 "두 빈 목록이 같다" 가 공허하게 참이 된다. 그 0 은 조용하다.
        assert!(
            host.len() >= 2,
            "본체 `{host_rel}::{host_fn}` 에서 match 팔을 {} 개밖에 못 읽었다(하한 2) — \
             팔이 블록이나 가드로 바뀌었으면 이 판정은 공허하다",
            host.len()
        );
        assert!(
            gallery.len() >= 2,
            "갤러리 `{gallery_rel}::{gallery_fn}` 에서 match 팔을 {} 개밖에 못 읽었다(하한 2)",
            gallery.len()
        );

        let same = host == gallery;
        let mark = if same { "=" } else { "≠" };
        let render = |v: &Vec<(String, String)>| {
            v.iter()
                .map(|(a, t)| format!("{a}→{t}"))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        listed.push(format!(
            "  [{mark}] {what}\n        본체   {host_rel}::{host_fn}  {}\n        갤러리 {gallery_rel}::{gallery_fn}  {}",
            render(&host),
            render(&gallery)
        ));
        if !same {
            split.push((*what).to_string());
        }
    }

    let table = listed.join("\n");
    assert!(
        split.is_empty(),
        "갈라진 규칙 {}: {}\n\n{}\n{}\n{}\n{}",
        split.len(),
        split.join(" · "),
        table,
        "갤러리 specimen 이 본체와 다른 매핑을 보여준다 — 화면 말고는 아무 신호가 없는 자리다.",
        "어느 쪽이 맞는지 정해서 **양쪽을 같게** 만들어라.",
        "★ 명부에서 이 쌍을 빼거나 주석의 \"동일\" 을 지우는 것은 이행이 아니다 — 사본은 \
         그대로 남고 보는 눈만 사라진다. 빼는 것이 이행인 경우는 하나뿐이다: 갤러리가 \
         정본을 직접 부르게 되어 매핑이 한 벌이 됐을 때."
    );
    println!("[사본 규칙] {} 쌍\n{table}", COPIED_RULES.len());
}

#[cfg(test)]
mod detector {
    use super::*;

    const HOST: &str = "\
fn accent_color(kind: ToastKind, th: &Theme) -> egui::Color32 {
    match kind {
        ToastKind::Info => th.accent_primary().into(),
        ToastKind::Error => th.accent_danger().into(),
    }
}
";

    /// 수신자 이름(`th` vs `theme`)은 그 자리의 지역 변수라 **갈라져도 결함이 아니다.**
    #[test]
    fn the_receiver_name_is_not_part_of_the_rule() {
        let gallery = HOST.replace("th.", "theme.");
        assert_eq!(
            match_arms(HOST, "accent_color"),
            match_arms(&gallery, "accent_color")
        );
    }

    #[test]
    fn it_reads_the_arm_and_what_it_calls() {
        assert_eq!(
            match_arms(HOST, "accent_color"),
            vec![
                ("Info".to_string(), "accent_primary".to_string()),
                ("Error".to_string(), "accent_danger".to_string()),
            ]
        );
    }

    /// 한 팔의 목적지가 바뀌면 목록이 갈라져야 한다 — 이게 이 가드의 존재 이유다.
    #[test]
    fn a_changed_destination_makes_the_two_lists_differ() {
        let drifted = HOST.replace("accent_danger", "accent_warning");
        assert_ne!(
            match_arms(HOST, "accent_color"),
            match_arms(&drifted, "accent_color")
        );
    }

    /// 팔이 사라지는 것도 갈라짐이다(수가 다르다).
    #[test]
    fn a_missing_arm_is_a_difference_too() {
        let short = HOST.replace(
            "        ToastKind::Error => th.accent_danger().into(),\n",
            "",
        );
        assert_eq!(match_arms(&short, "accent_color").len(), 1);
        assert_ne!(
            match_arms(HOST, "accent_color"),
            match_arms(&short, "accent_color")
        );
    }

    /// 함수 이름이 안 맞으면 0 이 나온다 — 그래서 레포 판정에 하한이 있다.
    #[test]
    fn a_name_that_is_not_there_yields_nothing_which_is_why_the_repo_test_has_a_floor() {
        assert!(match_arms(HOST, "no_such_fn").is_empty());
        assert!(match_arms("", "accent_color").is_empty());
    }
}
