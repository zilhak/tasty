//! Delayed receipt regressions drive the production state accessors and result collector.
use super::*;
fn open(ctx: &egui::Context) -> FileRequest {
    let mut state = read_ui(ctx);
    state.passkey_view = Sub::Form;
    state.kform = PasskeyForm {
        name: "owned".into(),
        kind: "inline".into(),
        editing_original: Some("owned".into()),
        ..Default::default()
    };
    write_ui(ctx, state);
    let state = read_ui(ctx);
    enqueue(
        ctx,
        &state,
        FileAction::Reveal {
            key: Passkey {
                name: "owned".into(),
                kind: "inline".into(),
                path: "owned-private-file".into(),
            },
            editing: true,
        },
    );
    take_file_requests(ctx).pop().unwrap()
}
fn type_then_clear(ctx: &egui::Context) {
    let mut state = read_ui(ctx);
    state.kform.value = "replacement draft".into();
    write_ui(ctx, state);
    let mut state = read_ui(ctx);
    state.kform.value.clear();
    write_ui(ctx, state);
}
fn success(ctx: &egui::Context, request: &FileRequest) {
    accept_file_result(ctx, request, Ok(FileValue::Revealed("OLD-CONTENT".into())));
}
#[test]
fn type_then_clear_refuses_old_reveal() {
    let ctx = egui::Context::default();
    let request = open(&ctx);
    type_then_clear(&ctx);
    success(&ctx, &request);
    assert!(
        read_ui(&ctx).kform.value.is_empty(),
        "old receipt overwrote explicitly cleared draft"
    );
}
#[test]
fn cancel_reopen_refuses_old_but_accepts_own_reveal() {
    let ctx = egui::Context::default();
    let request = open(&ctx);
    let mut state = read_ui(&ctx);
    state.passkey_view = Sub::List;
    write_ui(&ctx, state);
    let fresh = open(&ctx);
    success(&ctx, &request);
    assert!(
        read_ui(&ctx).kform.value.is_empty(),
        "cancelled edit filled replacement edit"
    );
    accept_file_result(
        &ctx,
        &fresh,
        Ok(FileValue::Revealed("FRESH-CONTENT".into())),
    );
    assert_eq!(read_ui(&ctx).kform.value, "FRESH-CONTENT");
}
#[test]
fn original_read_failure_is_visible_without_replacing_draft() {
    let ctx = egui::Context::default();
    let request = open(&ctx);
    accept_file_result(
        &ctx,
        &request,
        Err("could not read credential file: entity not found".into()),
    );
    let state = read_ui(&ctx);
    assert_eq!(
        state.kerr.as_deref(),
        Some("could not read credential file: entity not found")
    );
    assert!(state.kform.value.is_empty());
    assert_eq!(state.passkey_view, Sub::Form);
}
#[test]
fn untouched_original_accepts_reveal() {
    let ctx = egui::Context::default();
    let request = open(&ctx);
    success(&ctx, &request);
    assert_eq!(read_ui(&ctx).kform.value, "OLD-CONTENT");
}
#[test]
fn nonempty_newer_value_survives_reveal() {
    let ctx = egui::Context::default();
    let request = open(&ctx);
    let mut state = read_ui(&ctx);
    state.kform.value = "NEW-CONTENT".into();
    write_ui(&ctx, state);
    success(&ctx, &request);
    assert_eq!(read_ui(&ctx).kform.value, "NEW-CONTENT");
}
#[test]
fn new_popup_refuses_old_success_and_error() {
    let ctx = egui::Context::default();
    let request = open(&ctx);
    clear_ui(&ctx);
    let fresh = open(&ctx);
    success(&ctx, &request);
    accept_file_result(
        &ctx,
        &request,
        Err("could not read credential file: permission denied".into()),
    );
    assert!(read_ui(&ctx).kform.value.is_empty());
    assert!(read_ui(&ctx).kerr.is_none());
    success(&ctx, &fresh);
    assert_eq!(read_ui(&ctx).kform.value, "OLD-CONTENT");
}
#[test]
fn changed_draft_refuses_old_read_failure() {
    let ctx = egui::Context::default();
    let request = open(&ctx);
    type_then_clear(&ctx);
    accept_file_result(
        &ctx,
        &request,
        Err("could not read credential file: entity not found".into()),
    );
    assert!(read_ui(&ctx).kerr.is_none());
    assert!(read_ui(&ctx).kform.value.is_empty());
}

fn list_request(ctx: &egui::Context) -> FileRequest {
    let mut state = read_ui(ctx);
    state.passkey_view = Sub::List;
    state.revealed.insert("owned".into());
    state.reveal_owners.insert("owned".into(), Arc::default());
    write_ui(ctx, state);
    let state = read_ui(ctx);
    enqueue(
        ctx,
        &state,
        FileAction::Reveal {
            key: Passkey {
                name: "owned".into(),
                kind: "inline".into(),
                path: "owned-private-file".into(),
            },
            editing: false,
        },
    );
    take_file_requests(ctx).pop().unwrap()
}
#[test]
fn active_list_reveal_receives_sanitized_failure() {
    let ctx = egui::Context::default();
    let request = list_request(&ctx);
    accept_file_result(
        &ctx,
        &request,
        Err("could not read credential file: entity not found".into()),
    );
    assert_eq!(
        read_ui(&ctx).kerr.as_deref(),
        Some("could not read credential file: entity not found")
    );
    assert!(read_ui(&ctx).revealed_values.is_empty());
}
#[test]
fn reactivated_list_refuses_old_error_but_accepts_current_receipt() {
    let ctx = egui::Context::default();
    let request = list_request(&ctx);
    let mut state = read_ui(&ctx);
    state.revealed.remove("owned");
    state.reveal_owners.remove("owned");
    write_ui(&ctx, state);
    let fresh = list_request(&ctx);
    accept_file_result(
        &ctx,
        &request,
        Err("could not read credential file: permission denied".into()),
    );
    assert!(read_ui(&ctx).kerr.is_none());
    success(&ctx, &fresh);
    assert_eq!(read_ui(&ctx).revealed_values["owned"].1, "OLD-CONTENT");
}
#[test]
fn retired_list_failure_cannot_appear_in_an_edit_or_returned_list() {
    let ctx = egui::Context::default();
    let request = list_request(&ctx);
    let _fresh = open(&ctx);
    let mut state = read_ui(&ctx);
    state.passkey_view = Sub::List;
    write_ui(&ctx, state);
    accept_file_result(
        &ctx,
        &request,
        Err("could not read credential file: entity not found".into()),
    );
    assert!(read_ui(&ctx).kerr.is_none());
}
