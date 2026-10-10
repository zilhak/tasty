use super::*;
use crate::app::explorer_files::history::{Entry, Source};
use crate::app::explorer_files::job::UndoStep;
use std::path::PathBuf;

fn entry(cut: bool, undo: Vec<UndoStep>) -> Entry {
    Entry {
        source: Source {
            paths: vec!["/src/a".into()],
            destination: "/dest".into(),
            cut,
        },
        undo,
    }
}

/// 되돌릴 수 있는 이동 단계: 옮겨 간 자리에 항목이 있고 원래 자리는 비어 있다.
fn live_move(dir: &std::path::Path, name: &str) -> UndoStep {
    let to = dir.join(name);
    std::fs::write(&to, b"x").unwrap();
    UndoStep::Moved {
        from: dir.join(format!("gone-{name}")),
        to,
    }
}

#[test]
fn rows_name_the_operation_and_count_and_are_absent_without_history() {
    assert!(history_items(None, None).is_empty());
    let dir = tempfile::tempdir().unwrap();
    let undo = entry(
        true,
        vec![live_move(dir.path(), "a"), live_move(dir.path(), "b")],
    );
    let redo = entry(false, vec![UndoStep::Created(PathBuf::from("/x"), None)]);
    let rows = history_items(Some(&undo), Some(&redo));
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].label,
        crate::i18n::t_fmt(
            "explorer.menu.undo",
            &crate::i18n::t_count("explorer.menu.op_move", 2, &["2"])
        )
    );
    assert!(rows[0].enabled);
    assert_eq!(rows[0].tooltip, None);
    assert_eq!(
        rows[1].label,
        crate::i18n::t_fmt(
            "explorer.menu.redo",
            &crate::i18n::t_count("explorer.menu.op_copy", 1, &["1"])
        )
    );
    assert!(rows[1].enabled);
}

/// 되돌릴 수 없게 된 단계는 행을 끄고 이유를 툴팁에 적는다.
#[test]
fn a_stale_undo_row_is_disabled_with_the_reason_as_its_tooltip() {
    let dir = tempfile::tempdir().unwrap();
    let stale = entry(
        true,
        vec![UndoStep::Moved {
            from: dir.path().join("a"),
            to: dir.path().join("missing"),
        }],
    );
    let rows = history_items(Some(&stale), None);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].enabled);
    assert_eq!(
        rows[0].tooltip.as_deref(),
        Some(
            crate::i18n::t_fmt(
                "explorer.menu.undo_stale",
                crate::i18n::t("explorer.result.gone")
            )
            .as_str()
        )
    );
}

/// 행 묶음은 마지막 "구분선 · Properties" 앞에 자기 구분선과 함께 들어가 구분선 사이에 놓인다.
/// 행이 없으면 메뉴를 바꾸지 않는다.
#[test]
fn rows_go_before_the_properties_group() {
    let menu = || {
        vec![
            MenuItem::new(1, "Copy path"),
            MenuItem::separator(),
            MenuItem::new(70, "Properties"),
        ]
    };
    let mut items = menu();
    insert_rows(&mut items, Vec::new());
    assert_eq!(items.len(), 3);
    insert_rows(
        &mut items,
        vec![MenuItem::new(UNDO, "Undo"), MenuItem::new(REDO, "Redo")],
    );
    let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
    assert_eq!(ids, vec![1, 0, UNDO, REDO, 0, 70]);
    assert!(items[1].is_separator() && items[4].is_separator());
}
