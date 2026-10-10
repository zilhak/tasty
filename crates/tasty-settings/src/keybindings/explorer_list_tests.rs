//! 탐색기 목록 키(이동·이름 변경·휴지통)의 범위·기본값 시험.

use super::KeybindingSettings;

const MOVES: [(&str, &str); 8] = [
    ("up", "up"),
    ("down", "down"),
    ("left", "left"),
    ("right", "right"),
    ("home", "home"),
    ("end", "end"),
    ("page_up", "pageup"),
    ("page_down", "pagedown"),
];

fn presets() -> [(&'static str, KeybindingSettings); 4] {
    [
        ("tasty", KeybindingSettings::preset_tasty()),
        ("mac", KeybindingSettings::preset_mac()),
        ("windows", KeybindingSettings::preset_windows()),
        ("linux", KeybindingSettings::preset_linux()),
    ]
}

/// 모든 프리셋이 방향키·Home/End·PageUp/Down 으로 옮기고 Shift 를 더하면 선택을 넓힌다.
#[test]
fn every_preset_moves_with_the_plain_keys_and_extends_with_shift() {
    for (name, kb) in presets() {
        for (id, key) in MOVES {
            let cursor = format!("explorer_cursor_{id}");
            let extend = format!("explorer_extend_{id}");
            assert_eq!(kb.get_field(&cursor), Some(key), "{name} {cursor}");
            assert_eq!(
                kb.get_field(&extend),
                Some(format!("shift+{key}").as_str()),
                "{name} {extend}"
            );
        }
    }
}

/// 방향키는 전역 목록 밖에 있어 터미널·팔레트·webview 선점 경로가 읽지 않는다.
#[test]
fn list_keys_are_outside_the_global_list_but_settable() {
    assert_eq!(KeybindingSettings::EXPLORER_LIST_BINDING_FIELDS.len(), 20);
    for (id, label) in KeybindingSettings::EXPLORER_LIST_BINDING_FIELDS {
        assert!(
            KeybindingSettings::GENERAL_BINDING_FIELDS
                .iter()
                .all(|(f, _)| f != id),
            "{id}"
        );
        assert_eq!(KeybindingSettings::label_key_for(id), Some(*label));
    }
    let mut kb = KeybindingSettings::preset_tasty();
    assert!(kb.set_field("explorer_cursor_down", "j"));
    assert_eq!(kb.explorer_cursor_down, vec!["j".to_string()]);
}

/// 충돌은 목록 이동 키끼리만 본다. 전역 키에 같은 조합이 있어도 충돌로 보지 않는다.
#[test]
fn list_key_conflicts_stay_inside_their_scope() {
    let mut kb = KeybindingSettings::preset_tasty();
    assert_eq!(
        kb.find_conflict("explorer_cursor_up", "down"),
        Some(("explorer_cursor_down", 0))
    );
    assert_eq!(
        kb.find_conflict("explorer_cursor_up", "shift+home"),
        Some(("explorer_extend_home", 0))
    );
    kb.explorer_go_up = vec!["alt+left".into()];
    assert_eq!(kb.find_conflict("explorer_cursor_left", "alt+left"), None);
    assert_eq!(kb.find_conflict("explorer_go_up", "up"), None);
}

/// 이름 변경의 F2 는 탭 이름 변경과 같은 키지만 범위가 달라 충돌로 보지 않는다. 그래서 기존 설정에
/// rename_tab 이 있어도 새 기본값을 지우지 않는다.
#[test]
fn item_keys_share_f2_with_tab_rename_without_a_conflict() {
    for (name, kb) in presets() {
        assert_eq!(kb.get_field("explorer_rename"), Some("f2"), "{name}");
        assert_eq!(kb.get_field("rename_tab"), Some("f2"), "{name}");
        assert_eq!(kb.find_conflict("explorer_rename", "f2"), None, "{name}");
        for combo in &kb.explorer_trash {
            assert_eq!(kb.find_conflict("explorer_trash", combo), None, "{name}");
        }
    }
    let mut kb = KeybindingSettings::preset_tasty();
    let existing = std::collections::HashSet::from(["rename_tab".to_string()]);
    kb.remove_conflicts_from_defaults(&existing);
    assert_eq!(kb.explorer_rename, vec!["f2".to_string()]);
}
