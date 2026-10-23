//! 보이는 항목이 없는 목록(빈 폴더, 거르기 결과 0건)에서 새 항목 이름 입력을 여는 시험이다.
//! 상태 화면 대신 목록 영역을 그려 이름 행이 보이고, 입력을 닫으면 상태 화면으로 돌아와야 한다.

use std::path::PathBuf;

use egui::{Event, Modifiers};
use tasty_model::ExplorerViewMode;

use super::{DIR, Harness, MODES};
use crate::i18n::t;

fn key(key: egui::Key) -> Vec<Event> {
    vec![Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]
}

fn empty(mode: ExplorerViewMode) -> Harness {
    let mut h = Harness::new(mode);
    h.view.entries = Default::default();
    h
}

fn shows(h: &Harness, text: &str) -> bool {
    h.texts.iter().any(|s| s == text)
}

/// 이름 입력을 열고 "abc" 를 넣은 뒤 Enter 로 확정하면 그 이름의 만들기 동작이 나온다.
fn type_and_confirm(h: &mut Harness, mode: ExplorerViewMode) {
    h.view.start_create(PathBuf::from(DIR), true);
    h.settle();
    assert!(
        h.ctx.memory(|m| m.focused()).is_some(),
        "{mode:?}: the name input is drawn and takes focus"
    );
    assert!(
        shows(h, "New folder"),
        "{mode:?}: the name row is on screen"
    );
    h.frame(vec![Event::Text("abc".into())]);
    h.frame(key(egui::Key::Enter));
    assert!(
        matches!(&h.action, Some(super::super::ExplorerAction::Create { name, .. }) if name == "abc"),
        "{mode:?}: {:?}",
        h.action
    );
}

#[test]
fn an_empty_folder_shows_the_name_row_and_creates_the_typed_name() {
    for mode in MODES {
        let mut h = empty(mode);
        h.frame(Vec::new());
        assert!(shows(&h, t("explorer.state.empty")), "{mode:?}");
        type_and_confirm(&mut h, mode);
    }
}

#[test]
fn escape_in_an_empty_folder_returns_to_the_empty_screen() {
    for mode in MODES {
        let mut h = empty(mode);
        h.frame(Vec::new());
        h.view.start_create(PathBuf::from(DIR), false);
        h.settle();
        assert!(!shows(&h, t("explorer.state.empty")), "{mode:?}");
        h.frame(key(egui::Key::Escape));
        assert!(h.view.create.is_none(), "{mode:?}");
        h.frame(Vec::new());
        assert!(shows(&h, t("explorer.state.empty")), "{mode:?}");
    }
}

#[test]
fn an_empty_mirror_folder_shows_the_name_row_and_sends_the_typed_name() {
    for mode in MODES {
        let mut h = empty(mode);
        h.view.mirror_ws_id = Some(7);
        h.frame(Vec::new());
        type_and_confirm(&mut h, mode);
    }
}

#[test]
fn a_filter_without_matches_shows_the_name_row_while_creating() {
    for mode in MODES {
        let mut h = Harness::new(mode);
        h.frame(Vec::new());
        // 막 연 Find 칸이 글자를 받는다.
        h.view.open_find();
        h.settle();
        h.frame(vec![Event::Text("zzz".into())]);
        h.frame(Vec::new());
        assert_eq!(h.view.find_query(), "zzz", "{mode:?}");
        let none = super::super::state_screen::no_filter_matches_title("zzz");
        assert!(shows(&h, &none), "{mode:?}: {:?}", h.texts);
        type_and_confirm(&mut h, mode);
        h.frame(Vec::new());
        assert!(shows(&h, &none), "{mode:?}: back to the no-match screen");
    }
}
