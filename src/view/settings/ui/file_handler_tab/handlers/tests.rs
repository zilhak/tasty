use super::*;
use crate::app::settings_edit::apply_registry_edit;
use std::sync::Arc;

type Reg = Arc<tasty_file_handler::FileHandlerRegistry>;

/// host `html-system`(50) · `directory-system`(50)을 가진 registry.
fn registry() -> (Reg, FileHandlerRegistry) {
    let reg: Reg = Arc::new(tasty_file_handler::FileHandlerRegistry::new());
    reg.install_host_defaults(
        "[[handler]]\nid = \"html-system\"\ndetector = \"html\"\npriority = 50\n\
         [handler.action]\nkind = \"system\"\n\
         [[handler]]\nid = \"directory-system\"\ndetector = \"$directory\"\npriority = 50\n\
         [handler.action]\nkind = \"system\"\n",
    );
    let catalog = FileHandlerRegistry::new(reg.clone());
    (reg, catalog)
}

fn commit(fh: &mut FileHandlerEditDraft, catalog: &FileHandlerRegistry) {
    let r = handler_rows(catalog, fh);
    commit_handler(fh, &r);
}

fn form(short: &str, detector: Option<&str>) -> AddHandlerForm {
    AddHandlerForm {
        open: true,
        short_name_input: short.into(),
        detector: detector.map(Into::into),
        action_kind: AddHandlerActionKind::System,
        ..AddHandlerForm::default()
    }
}

fn save(reg: &Reg, fh: FileHandlerEditDraft) {
    let format = tasty_file_format::FileFormatRegistry::new();
    for edit in fh.into_edits() {
        apply_registry_edit(&format, reg, crate::hook_handler::global(), edit);
    }
}

#[test]
fn a_new_handler_is_saved_as_user_short_with_default_priority() {
    let (reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_handler_form: form("notes-view", Some("html")),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    assert!(!fh.add_handler_form.open);
    let rows = handler_rows(&catalog, &fh);
    let new = rows
        .iter()
        .find(|r| r.id.as_str() == "user/notes-view")
        .expect("새 행");
    assert_eq!(new.pending, Some(HdlPending::Add));
    assert_eq!(new.priority, 100);
    assert_eq!(row_trailing(new), FhTrailing::Undo);
    save(&reg, fh);
    let h = reg
        .handler(&HandlerId("user/notes-view".into()))
        .expect("저장된 handler");
    assert_eq!(h.priority, 100);
    assert_eq!(h.owner, HandlerOwner::User);
}

#[test]
fn rows_sort_by_priority_then_id_and_host_rows_are_locked() {
    let (_reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_handler_form: AddHandlerForm {
            priority_input: "10".into(),
            ..form("zz", Some("html"))
        },
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    let rows = handler_rows(&catalog, &fh);
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        ["user/zz", "host/directory-system", "host/html-system"]
    );
    assert_eq!(row_trailing(&rows[1]), FhTrailing::Lock);
}

#[test]
fn the_same_short_name_replaces_the_earlier_user_handler() {
    let (reg, catalog) = registry();
    reg.upsert_user_handler(UserHandlerUpsertDecl {
        id: "user/v".into(),
        detector: Some("html".into()),
        priority: Some(70),
        display_name_i18n_key: None,
        disabled: None,
        action: Some(UserHandlerActionDecl::System),
    })
    .expect("seed");
    let mut fh = FileHandlerEditDraft {
        add_handler_form: form("v", Some("$directory")),
        ..FileHandlerEditDraft::default()
    };
    let rows = handler_rows(&catalog, &fh);
    let check = check_handler_form(&fh.add_handler_form, &rows);
    assert!(check.exists);
    assert_eq!(check.full_id, "user/v");
    commit_handler(&mut fh, &rows);
    let rows = handler_rows(&catalog, &fh);
    let v = rows.iter().find(|r| r.id.as_str() == "user/v").expect("v");
    assert_eq!(v.pending, Some(HdlPending::Replace));
    assert_eq!(v.detector, "$directory");
    assert_eq!(rows.iter().filter(|r| r.id.as_str() == "user/v").count(), 1);
    undo_handler(&mut fh, v);
    let rows = handler_rows(&catalog, &fh);
    let v = rows.iter().find(|r| r.id.as_str() == "user/v").expect("v");
    assert_eq!((v.pending, v.priority), (None, 70));
}

#[test]
fn field_errors_follow_the_design_rules() {
    let rows = Vec::new();
    let bad = check_handler_form(&form("My_View", Some("html")), &rows);
    assert_eq!(
        bad.err_short,
        Some(t("settings.file_handler.handlers.err_short_chars"))
    );
    let long = check_handler_form(&form(&"a".repeat(33), Some("html")), &rows);
    assert!(long.err_short.is_some());
    let prio = check_handler_form(
        &AddHandlerForm {
            priority_input: "1.5".into(),
            ..form("v", Some("html"))
        },
        &rows,
    );
    assert_eq!(
        prio.err_priority,
        Some(t("settings.file_handler.handlers.err_priority_int"))
    );
    let ipc = |m: &str| {
        check_handler_form(
            &AddHandlerForm {
                action_kind: AddHandlerActionKind::Ipc,
                action_ipc_method: m.into(),
                ..form("v", Some("html"))
            },
            &rows,
        )
    };
    assert!(ipc("open").err_method.is_some());
    assert!(ipc("com..open").err_method.is_some());
    assert_eq!(ipc("com.example.foo.open").err_method, None);
    assert!(!ipc("").can_add, "IPC는 메서드가 있어야 Add가 눌린다");
    assert!(!check_handler_form(&form("v", None), &rows).can_add);
}

/// 카드가 열린 동안 고른 detector가 사라지면 값을 비우고 Add가 막힌다.
#[test]
fn a_vanished_detector_clears_the_selection() {
    let mut f = form("v", Some("gone"));
    forget_vanished_detector(&mut f, &["html".to_string()]);
    assert_eq!(f.detector, None);
    assert!(!check_handler_form(&f, &[]).can_add);
    let mut kept = form("v", Some("html"));
    forget_vanished_detector(&mut kept, &["html".to_string()]);
    assert_eq!(kept.detector.as_deref(), Some("html"));
}

#[test]
fn errors_block_the_commit_and_keep_the_card_open() {
    let (_reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_handler_form: AddHandlerForm {
            priority_input: "x".into(),
            ..form("v", Some("html"))
        },
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    assert!(fh.add_handler.is_empty());
    assert!(fh.add_handler_form.open && fh.add_handler_form.tried);
}
