//! File Picker 상태 표본의 카드마다 로딩·연결 끊김 본문이 그 상태의 앱 키 값인지 본다.
use super::indicator::NO_PROFILE_STATE_CARDS;
use super::{FpState, STATE_CARDS, Variant, card, draw_states};
use crate::i18n::t;

/// 카드 한 장을 그리고 칠한 글자를 모은다.
fn painted(v: Variant) -> Vec<String> {
    painted_with(|ui, theme| card(ui, theme, v))
}

/// `draw` 를 그리고 칠한 글자를 모은다.
fn painted_with(draw: impl Fn(&mut egui::Ui, &tasty_type_appearance::theme::Theme)) -> Vec<String> {
    let theme = crate::host_shell::latte_theme();
    let ctx = egui::Context::default();
    let mut texts = Vec::new();
    // 첫 프레임은 폰트가 확정되지 않아 galley 가 비는 경우가 있어 두 번 돈다.
    for _ in 0..2 {
        let full = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| draw(ui, &theme));
        });
        texts.clear();
        for clipped in &full.shapes {
            collect(&clipped.shape, &mut texts);
        }
    }
    texts
}

fn collect(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
    match shape {
        egui::epaint::Shape::Text(text) => out.push(text.galley.text().to_owned()),
        egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

/// 표본 표에서 묶음 이름으로 카드를 찾아 표본이 그리는 변형 그대로 돌려준다.
fn card_named(cards: &[(&'static str, Variant)], label: &str) -> Variant {
    cards
        .iter()
        .find(|(l, _)| *l == label)
        .unwrap_or_else(|| panic!("no state card {label}"))
        .1
}

fn state_card(label: &str) -> Variant {
    card_named(&STATE_CARDS, label)
}

const NO_PROFILE_VIEW: &str = "View · file-picker-remote-states-no-profile — ";

fn no_profile_card(rest: &str) -> Variant {
    card_named(&NO_PROFILE_STATE_CARDS, &format!("{NO_PROFILE_VIEW}{rest}"))
}

/// 본문 자리에 `want` 키 값이 있고 같은 블록의 다른 본문 키 값과 갤러리 전용 문장은 없다.
fn assert_body(label: &str, texts: &[String], want: &[&'static str], not: &[&'static str]) {
    for key in want {
        assert!(
            texts.iter().any(|s| s == t(key)),
            "{label}: {key} = {:?} 가 없다: {texts:?}",
            t(key)
        );
    }
    for key in not {
        assert!(
            !texts.iter().any(|s| s == t(key)),
            "{label}: {key} 를 그렸다: {texts:?}"
        );
    }
    assert_no_gallery_only(label, texts);
}

/// 호스트를 넣던 갤러리 전용 영어 문장(앱 키 없음)의 조각.
const GALLERY_ONLY: [&str; 3] = ["over SSH", "SSH tunnel", "Loading folder"];

fn assert_no_gallery_only(label: &str, texts: &[String]) {
    let found: Vec<&String> = texts
        .iter()
        .filter(|s| GALLERY_ONLY.iter().any(|g| s.contains(g)))
        .collect();
    assert!(found.is_empty(), "{label}: {found:?}");
}

/// 카드별 단언 밖의 자리(빈 폴더·권한 거부·다중 선택 카드, Meta·설명, 빈 호스트 View)까지 표본 전체가
/// 갤러리 전용 문장을 그리지 않는다.
#[test]
fn the_whole_states_specimen_draws_no_gallery_only_sentence() {
    let texts = painted_with(draw_states);
    assert!(
        texts
            .iter()
            .any(|s| s == t("filepicker.loading_body_remote")),
        "the specimen was drawn: {texts:?}"
    );
    assert_no_gallery_only("draw_states", &texts);
}

#[test]
fn each_remote_state_card_draws_the_app_body_of_its_state() {
    let loading = state_card("loading (remote)");
    assert!(!loading.host.is_empty() && loading.state == FpState::Loading);
    assert_body(
        "named loading",
        &painted(loading),
        &["filepicker.loading", "filepicker.loading_body_remote"],
        &["filepicker.loading_body_local"],
    );

    let lost = state_card("connection lost (remote)");
    assert!(!lost.host.is_empty() && lost.state == FpState::ErrorConn);
    assert_body(
        "named session lost",
        &painted(lost),
        &[
            "filepicker.error_conn.title",
            "filepicker.error_conn.session_lost",
            "filepicker.error_conn.reconnect",
        ],
        &["filepicker.error_conn.timeout"],
    );

    assert_body(
        "no-profile loading",
        &painted(no_profile_card("loading (remote, no profile name)")),
        &["filepicker.loading", "filepicker.loading_body_remote"],
        &["filepicker.loading_body_local"],
    );
    assert_body(
        "no-profile session lost",
        &painted(no_profile_card(
            "connection lost — session lost (remote, no profile name)",
        )),
        &[
            "filepicker.error_conn.title",
            "filepicker.error_conn.session_lost",
        ],
        &["filepicker.error_conn.timeout"],
    );
    assert_body(
        "no-profile timeout",
        &painted(no_profile_card(
            "connection lost — timeout (remote, no profile name)",
        )),
        &[
            "filepicker.error_conn.title",
            "filepicker.error_conn.timeout",
        ],
        &["filepicker.error_conn.session_lost"],
    );
}

/// 카드 한 장의 경로 막대 Up 이 눌리는지(클릭 감지). 경로 막대의 두 Sm 정사각 버튼 중 왼쪽이 Up, 오른쪽이 Refresh 다.
fn up_and_refresh_clickable(v: Variant) -> (bool, bool) {
    let theme = crate::host_shell::latte_theme();
    let side = tasty_ui_widgets::ControlSize::Sm.height(&theme);
    let ctx = egui::Context::default();
    for _ in 0..2 {
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| card(ui, &theme, v));
        }));
    }
    let mut squares: Vec<(egui::Rect, bool)> = ctx.viewport(|vp| {
        vp.prev_pass
            .widgets
            .layers()
            .flat_map(|(_, rects)| rects.iter())
            .filter(|w| (w.rect.width() - side).abs() < 0.5 && (w.rect.height() - side).abs() < 0.5)
            .map(|w| (w.rect, w.sense.senses_click()))
            .collect()
    });
    // 경로 막대는 서로 다른 자리의 Sm 정사각 버튼 둘이 한 줄에 선 유일한 줄이다(머리 ✕ 는 한 자리에 위젯이 겹쳐 둘로
    // 잡히고, 로딩 표시 등은 다른 줄).
    squares.sort_by(|a, b| a.0.top().total_cmp(&b.0.top()));
    let rows: Vec<Vec<(egui::Rect, bool)>> = squares
        .chunk_by(|a, b| (a.0.top() - b.0.top()).abs() < 0.5)
        .filter(|row| row.len() == 2 && (row[0].0.left() - row[1].0.left()).abs() > 1.0)
        .map(<[_]>::to_vec)
        .collect();
    assert_eq!(rows.len(), 1, "두 버튼이 선 줄 하나: {rows:?}");
    let mut bar = rows[0].clone();
    bar.sort_by(|a, b| a.0.left().total_cmp(&b.0.left()));
    (bar[0].1, bar[1].1)
}

/// 세션 소실 카드(이름 있는 원격·빈 호스트)는 Up 이 꺼지고, 응답 시간 초과와 로딩 카드는 켜져 있다. Up 은 정본 fe45514
/// FilePickerFrame `sessionLost` 와 앱 path_bar `up_enabled` 를 따른다. Refresh 는 앱처럼 모두 켜 둔다(정본
/// overlay-frames.jsx:3328 은 `sessionLost` 때 끈다 — 후속 후보).
#[test]
fn a_session_lost_card_disables_up() {
    for (label, v, lost) in [
        (
            "named session lost",
            state_card("connection lost (remote)"),
            true,
        ),
        (
            "no-profile session lost",
            no_profile_card("connection lost — session lost (remote, no profile name)"),
            true,
        ),
        (
            "no-profile timeout",
            no_profile_card("connection lost — timeout (remote, no profile name)"),
            false,
        ),
        ("named loading", state_card("loading (remote)"), false),
    ] {
        let (up, refresh) = up_and_refresh_clickable(v);
        assert_eq!(up, !lost, "{label}: Up");
        assert!(refresh, "{label}: Refresh");
    }
}
