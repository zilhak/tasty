use super::*;
use crate::app::settings_edit::apply_registry_edit;
use std::sync::Arc;

type Reg = Arc<tasty_file_format::FileFormatRegistry>;

fn ext_decl(id: &str, ext: &str) -> DetectorDecl {
    DetectorDecl {
        id: id.into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec![ext.into()],
        }],
    }
}

/// host `$directory`, plugin `markdown`(md)를 가진 registry.
fn registry() -> (Reg, FileFormatRegistry) {
    let reg: Reg = Arc::new(tasty_file_format::FileFormatRegistry::new());
    reg.install_host_defaults(
        "[[detector]]\nid = \"$directory\"\n[[detector.rule]]\nkind = \"is_directory\"\n",
    );
    reg.install_plugin_detectors("com.tasty.markdown", &[ext_decl("markdown", "md")]);
    let catalog = FileFormatRegistry::new(reg.clone());
    (reg, catalog)
}

fn rows(catalog: &FileFormatRegistry, fh: &FileHandlerEditDraft) -> Vec<DetRow> {
    detector_rows(&catalog.detector_snapshots(), fh)
}

fn row<'a>(rows: &'a [DetRow], id: &str) -> &'a DetRow {
    rows.iter()
        .find(|r| r.id.as_str() == id)
        .unwrap_or_else(|| panic!("{id} 행이 없다"))
}

fn commit(fh: &mut FileHandlerEditDraft, catalog: &FileFormatRegistry) {
    let r = rows(catalog, fh);
    commit_detector(fh, &r);
}

fn form(id: &str, ext: &str, glob: &str) -> AddDetectorForm {
    AddDetectorForm {
        open: true,
        id_input: id.into(),
        extensions_input: ext.into(),
        path_glob_input: glob.into(),
        tried: false,
    }
}

fn save(reg: &Reg, fh: FileHandlerEditDraft) {
    let handler = tasty_file_handler::FileHandlerRegistry::new();
    for edit in fh.into_edits() {
        apply_registry_edit(reg, &handler, crate::hook_handler::global(), edit);
    }
}

fn user_contribution(reg: &Reg, id: &str) -> bool {
    reg.has_user_contribution(&DetectorId(id.into()))
}

/// (a) 새 detector id는 입력 그대로 저장한다 — `user/` 같은 접두사가 붙지 않는다.
#[test]
fn a_new_detector_is_saved_under_the_id_typed() {
    let (reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("my-doc", "doc", ""),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    assert_eq!(fh.add_detector.len(), 1);
    assert!(!fh.add_detector_form.open, "Add에 성공하면 카드가 닫힌다");
    let r = rows(&catalog, &fh);
    assert_eq!(row(&r, "my-doc").pending, Some(DetPending::Add));
    save(&reg, fh);
    assert!(reg.detector(&DetectorId("my-doc".into())).is_some());
    assert!(reg.detector(&DetectorId("user/my-doc".into())).is_none());
}

/// (b) 이미 있는 plugin detector id로 추가하면 오류가 아니라 patch이고, 저장하면 출처에 user가 더해진다.
#[test]
fn an_existing_plugin_id_becomes_a_user_patch_not_an_error() {
    let (reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("markdown", "mdx", ""),
        ..FileHandlerEditDraft::default()
    };
    let check = check_detector_form(&fh.add_detector_form, &rows(&catalog, &fh));
    assert_eq!(check.err_id, None);
    assert!(check.exists);
    assert_eq!(
        check.info,
        Some(t_fmt(
            "settings.file_handler.detectors.info_patch",
            "markdown"
        ))
    );
    commit(&mut fh, &catalog);
    let r = rows(&catalog, &fh);
    let md = row(&r, "markdown");
    assert_eq!(
        md.pending,
        Some(DetPending::Patch {
            replaces_user: false
        })
    );
    let d = det_row_display(md);
    let labels: Vec<&str> = d.origins.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["plugin:com.tasty.markdown", "user"]);
    save(&reg, fh);
    assert_eq!(
        reg.rule_origins(&DetectorId("markdown".into())),
        vec![
            RuleOrigin::Plugin("com.tasty.markdown".into()),
            RuleOrigin::User
        ]
    );
}

/// (c) plugin detector를 끄기만 한 user patch에도 Remove가 보이고 출처는 "user override"다.
/// (d) 그 Remove를 저장하면 plugin detector는 남고 user 기여만 사라진다.
#[test]
fn a_disabled_only_patch_is_removable_and_removing_it_keeps_the_plugin_detector() {
    let (reg, catalog) = registry();
    reg.set_user_detector_disabled(&DetectorId("markdown".into()), true);
    let mut fh = FileHandlerEditDraft::default();
    let r = rows(&catalog, &fh);
    let md = row(&r, "markdown");
    let d = det_row_display(md);
    assert_eq!(d.trailing, FhTrailing::Remove);
    assert_eq!(
        d.origins.last().map(|o| o.label.as_str()),
        Some(t("settings.file_handler.detectors.origin_user_override"))
    );
    assert!(!d.switch_on, "끈 patch가 Switch에 보인다");

    fh.remove_detector.insert(md.id.clone());
    let r = rows(&catalog, &fh);
    let d = det_row_display(row(&r, "markdown"));
    assert_eq!(row(&r, "markdown").pending, Some(DetPending::Unpatch));
    assert_eq!(
        d.pending,
        Some(t(
            "settings.file_handler.detectors.pending_override_removed"
        ))
    );
    assert!(d.switch_on, "사용자 기여를 뺀 켜기 상태를 보인다");
    assert!(d.switch_disabled);
    assert!(d.origins.last().is_some_and(|o| o.struck));
    assert_eq!(d.trailing, FhTrailing::Undo);

    save(&reg, fh);
    let id = DetectorId("markdown".into());
    assert!(reg.detector(&id).is_some(), "plugin detector는 남는다");
    assert!(!user_contribution(&reg, "markdown"));
}

/// (e) 등록된 `$directory`의 user patch는 허용하고, 등록되지 않은 새 `$` id는 거절한다.
#[test]
fn a_reserved_id_can_be_patched_but_not_created() {
    let (reg, catalog) = registry();
    let fh = FileHandlerEditDraft::default();
    let r = rows(&catalog, &fh);
    let ok = check_detector_form(&form("$directory", "", "**/.git"), &r);
    assert_eq!(ok.err_id, None);
    assert!(ok.exists);
    let no = check_detector_form(&form("$foo", "foo", ""), &r);
    assert_eq!(
        no.err_id,
        Some(t("settings.file_handler.detectors.err_reserved"))
    );
    // registry도 같은 규칙이다.
    assert!(reg.upsert_user_detector(ext_decl("$foo", "foo")).is_err());
}

#[test]
fn id_characters_follow_the_product_rule() {
    let (_reg, catalog) = registry();
    let fh = FileHandlerEditDraft::default();
    let r = rows(&catalog, &fh);
    for bad in ["Docker_File", "a.b", "x/y", &"a".repeat(65)] {
        assert_eq!(
            check_detector_form(&form(bad, "x", ""), &r).err_id,
            Some(t("settings.file_handler.detectors.err_id_chars")),
            "{bad}"
        );
    }
    assert_eq!(
        check_detector_form(&form(&"a".repeat(64), "x", ""), &r).err_id,
        None
    );
}

/// 확장자도 glob도 없으면 필드 간 오류다. Add는 id만 있으면 눌리고 오류는 누른 뒤에 보인다.
#[test]
fn no_rules_is_a_cross_field_error_shown_after_add() {
    let (_reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("notes", "", "  "),
        ..FileHandlerEditDraft::default()
    };
    let check = check_detector_form(&fh.add_detector_form, &rows(&catalog, &fh));
    assert!(check.can_add());
    assert_eq!(
        check.err_rules,
        Some(t("settings.file_handler.detectors.err_no_rules"))
    );
    commit(&mut fh, &catalog);
    assert!(fh.add_detector.is_empty());
    assert!(fh.add_detector_form.tried);
    assert!(fh.add_detector_form.open, "오류가 있으면 카드가 남는다");
    assert!(!check_detector_form(&form("", "x", ""), &[]).can_add());
}

/// 새 id 추가의 Undo는 행을 없애고, patch의 Undo는 원래 행으로, 삭제 대기의 Undo는 대기를 푼다.
#[test]
fn undo_restores_each_pending_kind() {
    let (_reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("notes", "note", ""),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    fh.add_detector_form = form("markdown", "mdx", "");
    commit(&mut fh, &catalog);
    fh.detector_enabled
        .insert(DetectorId("notes".into()), false);
    let r = rows(&catalog, &fh);
    undo_detector(&mut fh, row(&r, "notes"));
    undo_detector(&mut fh, row(&r, "markdown"));
    assert!(fh.add_detector.is_empty());
    assert!(fh.detector_enabled.is_empty());
    let r = rows(&catalog, &fh);
    assert!(r.iter().all(|x| x.pending.is_none()));
    assert!(r.iter().all(|x| x.id.as_str() != "notes"));

    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("notes", "note", ""),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    fh.add_detector_form = form("notes", "note, notes", "");
    let check = check_detector_form(&fh.add_detector_form, &rows(&catalog, &fh));
    assert_eq!(
        check.info,
        Some(t_fmt(
            "settings.file_handler.detectors.info_replace",
            "notes"
        ))
    );
    commit(&mut fh, &catalog);
    assert_eq!(fh.add_detector.len(), 1, "같은 id의 앞선 추가를 바꾼다");
}

/// 꺼진 detector에 규칙을 더해도 저장 뒤 꺼진 채다 — 추가가 사용자 기여를 통째로 바꾼다.
#[test]
fn patching_a_switched_off_detector_keeps_it_off() {
    let (reg, catalog) = registry();
    reg.set_user_detector_disabled(&DetectorId("markdown".into()), true);
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("markdown", "mdx", ""),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    let r = rows(&catalog, &fh);
    assert!(!row(&r, "markdown").want_on);
    save(&reg, fh);
    let det = reg
        .detector(&DetectorId("markdown".into()))
        .expect("markdown");
    assert!(det.disabled);
}

/// 같은 초안에서 추가한 detector를 끄면 저장 뒤에도 꺼져 있다(추가 → 켜기 → 삭제 순서).
#[test]
fn a_switch_change_on_a_new_detector_survives_save() {
    let (reg, catalog) = registry();
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("notes", "note", ""),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    fh.detector_enabled
        .insert(DetectorId("notes".into()), false);
    save(&reg, fh);
    let det = reg.detector(&DetectorId("notes".into())).expect("notes");
    assert!(det.disabled);
}

/// user 와 plugin 이 같은 rule 을 적은 detector 에서도 출처에 `user` 가 남고 Remove 가 보인다 —
/// finalize 된 rule 에는 plugin origin 만 남으므로 그것을 읽으면 둘 다 사라진다.
#[test]
fn a_user_rule_shared_with_a_plugin_keeps_the_user_origin_and_remove_button() {
    let (reg, catalog) = registry();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file-formats.toml");
    std::fs::write(
        &path,
        "[[detector]]\nid = \"markdown\"\nicon = \"u\"\n\
         [[detector.rule]]\nkind = \"extension\"\nvalues = [\"md\"]\n",
    )
    .expect("write user config");
    reg.install_user_config(&path);
    let fh = FileHandlerEditDraft::default();
    let r = rows(&catalog, &fh);
    let d = det_row_display(row(&r, "markdown"));
    let labels: Vec<&str> = d.origins.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["plugin:com.tasty.markdown", "user"]);
    assert_eq!(d.trailing, FhTrailing::Remove);
    assert_eq!(
        tasty_ui_widgets::fh_summary_text(&d.summary),
        "ext md  ·  ext md"
    );
}

/// host만 기여한 detector는 자물쇠이고 툴팁에 출처를 적는다.
#[test]
fn a_host_only_detector_is_locked() {
    let (_reg, catalog) = registry();
    let r = rows(&catalog, &FileHandlerEditDraft::default());
    let d = det_row_display(row(&r, "$directory"));
    assert_eq!(d.trailing, FhTrailing::Lock);
    assert_eq!(
        d.locked_tooltip,
        t_fmt("settings.file_handler.common.locked", "host")
    );
    assert_eq!(tasty_ui_widgets::fh_summary_text(&d.summary), "dir");
}

fn disabled_decl(id: &str, ext: &str, disabled: bool) -> DetectorDecl {
    DetectorDecl {
        disabled: Some(disabled),
        ..ext_decl(id, ext)
    }
}

/// 목록은 판정 순서와 같은 설치 순서다. id 순이면 나중에 설치한 `aaa`가 `markdown` 앞에 온다.
#[test]
fn detector_rows_follow_install_order_not_id_order() {
    let (reg, catalog) = registry();
    reg.install_plugin_detectors("com.example.late", &[ext_decl("aaa", "aaa")]);
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("my-doc", "doc", ""),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    let r = rows(&catalog, &fh);
    let ids: Vec<&str> = r.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["$directory", "markdown", "aaa", "my-doc"]);
}

/// plugin이 꺼 둔 detector를 사용자가 켜 둔 뒤 규칙을 더해도 켜진 채다.
/// 새 사용자 기여에 켜짐을 적지 않으면 병합이 plugin의 꺼짐으로 돌아간다.
#[test]
fn patching_a_user_enabled_detector_over_a_disabled_plugin_keeps_it_on() {
    let (reg, catalog) = registry();
    reg.install_plugin_detectors(
        "com.example.nb",
        &[disabled_decl("notebook", "ipynb", true)],
    );
    let id = DetectorId("notebook".into());
    reg.set_user_detector_disabled(&id, false);
    assert!(!reg.detector(&id).expect("notebook").disabled);
    let mut fh = FileHandlerEditDraft {
        add_detector_form: form("notebook", "nb", ""),
        ..FileHandlerEditDraft::default()
    };
    commit(&mut fh, &catalog);
    let r = rows(&catalog, &fh);
    assert!(row(&r, "notebook").want_on, "대기 행도 켜져 보인다");
    save(&reg, fh);
    assert!(!reg.detector(&id).expect("notebook").disabled);
}

/// host·plugin의 켜기는 다른 출처의 끄기를 취소하지 않는다. 먼저 설치한 plugin이 끄고 나중 plugin이
/// 켜도 기본은 꺼짐이고, Unpatch 행 Switch도 사용자 기여를 지운 뒤 돌아갈 꺼짐을 보인다.
#[test]
fn a_plugin_enable_does_not_cancel_another_plugins_disable() {
    let (reg, catalog) = registry();
    reg.install_plugin_detectors("com.example.a", &[disabled_decl("log", "log", true)]);
    reg.install_plugin_detectors("com.example.b", &[disabled_decl("log", "txt", false)]);
    let id = DetectorId("log".into());
    assert!(reg.detector(&id).expect("log").disabled);
    reg.set_user_detector_disabled(&id, false);
    let mut fh = FileHandlerEditDraft::default();
    fh.remove_detector.insert(id.clone());
    let r = rows(&catalog, &fh);
    let log = row(&r, "log");
    assert_eq!(log.pending, Some(DetPending::Unpatch));
    assert!(!log.base_on);
    assert!(!det_row_display(log).switch_on);
    save(&reg, fh);
    assert!(reg.detector(&id).expect("log").disabled);
}

/// 거꾸로 먼저 켜고 나중에 끄면 기본은 꺼짐이다.
#[test]
fn a_later_disabling_contribution_wins_the_base_state() {
    let (reg, catalog) = registry();
    reg.install_plugin_detectors("com.example.a", &[disabled_decl("log", "log", false)]);
    reg.install_plugin_detectors("com.example.b", &[disabled_decl("log", "txt", true)]);
    let r = rows(&catalog, &FileHandlerEditDraft::default());
    assert!(!row(&r, "log").base_on);
    assert!(
        reg.detector(&DetectorId("log".into()))
            .expect("log")
            .disabled
    );
}
