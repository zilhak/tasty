//! 원격 끊음·보류 표본이 키트와 같은 제목·본문으로 그려지는지 그린 글자로 잰다.

use super::*;
use crate::catalog::components::painted::{PaintedText, paint};
use RemoteRowPopupKind as K;

/// 글자 잉크 경계의 왼쪽 끝은 첫 글리프의 측면 여백만큼 다르다. 같은 열로 보는 허용치.
const SAME_COLUMN: f32 = 3.0;

/// `body` 글자 바로 위, 같은 왼쪽 열에 그려진 글자(상태 블록의 제목).
fn title_above<'a>(texts: &'a [PaintedText], body: &PaintedText) -> Option<&'a str> {
    texts
        .iter()
        .filter(|t| {
            (t.rect.min.x - body.rect.min.x).abs() < SAME_COLUMN
                && t.rect.max.y <= body.rect.min.y + 0.5
        })
        .max_by(|a, b| a.rect.max.y.total_cmp(&b.rect.max.y))
        .map(|t| t.text.as_str())
}

/// 그린 글자 가운데 `body` 를 찾아 그 위 제목과 함께 돌려준다. 같은 본문이 여럿이면 모두다.
fn titles_of(texts: &[PaintedText], body: &str) -> Vec<String> {
    texts
        .iter()
        .filter(|t| t.text == body)
        .map(|t| title_above(texts, t).unwrap_or_default().to_owned())
        .collect()
}

/// 한 표본만 그려 제목·본문을 잰다. 모든 언어에서 키트 문구(lang 파일)를 그대로 그린다.
#[test]
fn every_language_draws_the_remote_disconnect_and_the_hold_with_their_titles() {
    let theme = crate::host_shell::latte_theme();
    for lang in langs() {
        let tx = |key| t_lang(lang, key).to_owned();
        for (kind, shape, title, body) in [
            (
                K::Server,
                false,
                "remote.row.popup.user_title",
                "remote.row.popup.server_body",
            ),
            (
                K::Server,
                true,
                "remote.row.popup.user_title",
                "remote.row.popup.server_body",
            ),
            (
                K::Held,
                false,
                "remote.row.popup.idle_title",
                "remote.row.popup.held_body",
            ),
            (
                K::Held,
                true,
                "remote.row.popup.user_title",
                "remote.row.popup.held_body",
            ),
        ] {
            let painted = paint(|ui| popup_content(ui, &theme, Pop::new(kind, shape).lang(lang)));
            assert_eq!(
                titles_of(&painted.texts, &tx(body)),
                [tx(title)],
                "{lang} {kind:?} shape={shape}"
            );
        }
    }
}

/// 빈 workspace 와 마지막 화면 절이 원격 끊음·보류 표본을 그린다(첫 무대).
#[test]
fn both_popup_specs_draw_a_remote_disconnect_and_a_hold() {
    let theme = crate::host_shell::latte_theme();
    let tx = |key| t_lang("en", key).to_owned();
    let bare = paint(|ui| draw_bare(ui, &theme));
    assert_eq!(
        titles_of(&bare.texts, &tx("remote.row.popup.server_body")),
        [tx("remote.row.popup.user_title")]
    );
    assert_eq!(
        titles_of(&bare.texts, &tx("remote.row.popup.held_body"))[0],
        tx("remote.row.popup.idle_title")
    );
    let last = paint(|ui| draw_last_screen(ui, &theme));
    assert_eq!(
        titles_of(&last.texts, &tx("remote.row.popup.server_body"))[0],
        tx("remote.row.popup.user_title")
    );
    assert_eq!(
        titles_of(&last.texts, &tx("remote.row.popup.held_body")),
        [tx("remote.row.popup.user_title")]
    );
}
