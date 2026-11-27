//! 원격 표본의 From ssh config 행을 그려진 글자로 확인한다. 행 오른쪽 Tag·버튼 글자는 본체가 같은 상태에서
//! 그리는 lang 키로 읽는다. 표본 창(520×460)에서는 이 목록이 스크롤 아래라 칠해지지 않으므로, 표본이 부르는
//! 섹션 함수를 따로 그린다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::components::painted::{PaintedText, paint};

use super::local_ssh_section;

fn themes() -> [Theme; 2] {
    [
        tasty_themes::mocha_fallback(),
        crate::host_shell::latte_theme(),
    ]
}

fn text<'a>(texts: &'a [PaintedText], s: &str) -> &'a PaintedText {
    texts
        .iter()
        .find(|t| t.text == s)
        .unwrap_or_else(|| panic!("{s:?} was not painted"))
}

/// 별칭 줄 위부터 대상 줄 아래까지와 세로로 겹치는 글자.
fn on_row<'a>(texts: &'a [PaintedText], alias: &str, target: &str) -> Vec<&'a str> {
    let top = text(texts, alias).rect.top();
    let bottom = text(texts, target).rect.bottom();
    texts
        .iter()
        .filter(|t| t.rect.top() < bottom && top < t.rect.bottom())
        .map(|t| t.text.as_str())
        .collect()
}

/// 정본처럼 네 호스트를 host · db.internal · edge · 긴 별칭 순서로 그린다. db.internal은 별칭만 프로필 host와
/// 맞아 in profiles Tag를 달고, edge는 대상 줄만 프로필과 같아 Add profile 버튼을 단다.
#[test]
fn the_ssh_config_rows_set_the_alias_match_apart_from_the_target_match() {
    let in_profiles = crate::i18n::t("remote_tool.local_ssh_in_profiles");
    let add = crate::i18n::t("remote_tool.local_ssh_add");
    for th in themes() {
        let p = paint(|ui| local_ssh_section(ui, &th));
        let rows = [
            ("host", "maya@host.local:22", false),
            ("db.internal", "maya@db.internal:22", true),
            ("edge", "root@edge.example.com:22", false),
            (
                "eu-west-build-farm-bastion-01",
                "ci-runner@bastion-01.eu-west.build.example.com:2222",
                false,
            ),
        ];
        let tops: Vec<f32> = rows
            .iter()
            .map(|(alias, ..)| text(&p.texts, alias).rect.top())
            .collect();
        assert!(
            tops.windows(2).all(|w| w[0] < w[1]),
            "ssh config rows are drawn top to bottom: {tops:?}"
        );
        assert!(
            p.texts.iter().all(|t| t.text != "prod-web"),
            "no prod-web ssh config row"
        );
        for (alias, target, imported) in rows {
            let line = on_row(&p.texts, alias, target);
            assert_eq!(
                line.contains(&in_profiles),
                imported,
                "{alias} in profiles Tag: {line:?}"
            );
            assert_eq!(
                line.contains(&add),
                !imported,
                "{alias} Add profile action: {line:?}"
            );
        }
    }
}
