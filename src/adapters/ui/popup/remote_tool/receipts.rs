//! File results update only their original popup; pending ownership outlives retired Views.
use super::*;

const FILE_REQUESTS: &str = "remote_tool.file_requests";
pub(super) fn enqueue(ctx: &egui::Context, state: &UiState, action: FileAction) {
    ctx.memory_mut(|memory| {
        let queue = memory
            .data
            .get_temp_mut_or_default::<Vec<FileRequest>>(egui::Id::new(FILE_REQUESTS));
        if queue.len() >= 64 {
            tracing::warn!("remote tool request capacity exhausted");
            return;
        }
        queue.push(FileRequest {
            popup: Arc::downgrade(&state.identity),
            view: state.view.clone(),
            action,
            _pending: PendingToken::admit(state.pending.clone()),
        });
    });
}
pub(crate) fn take_file_requests(ctx: &egui::Context) -> Vec<FileRequest> {
    ctx.memory_mut(|memory| {
        std::mem::take(
            memory
                .data
                .get_temp_mut_or_default::<Vec<FileRequest>>(egui::Id::new(FILE_REQUESTS)),
        )
    })
}
pub(crate) fn accept_file_result(
    ctx: &egui::Context,
    request: &FileRequest,
    result: Result<FileValue, String>,
) -> bool {
    let mut state = read_ui(ctx);
    if !request.popup.ptr_eq(&Arc::downgrade(&state.identity)) {
        return false;
    }
    match (&request.action, result) {
        (FileAction::SaveProfile(form), result) if form == &state.pform => match result {
            Ok(value) => {
                state.perr = None;
                state.profile_view = Sub::List;
                if let FileValue::Detection(id, name) = value {
                    state.detecting = Some(DetectJob { id, name });
                }
            }
            Err(error) => state.perr = Some(error),
        },
        (FileAction::SaveAttach(form), result) if form == &state.aform => match result {
            Ok(_) => {
                state.aerr = None;
                state.attach_view = Sub::List;
            }
            Err(error) => state.aerr = Some(error),
        },
        (FileAction::SavePasskey(form), result) if form == &state.kform => match result {
            Ok(_) => {
                state.kerr = None;
                state.passkey_view = Sub::List;
                state.revealed.clear();
                state.revealed_values.clear();
            }
            Err(error) => state.kerr = Some(error),
        },
        (FileAction::Detect(_), Ok(FileValue::Detection(id, name))) => {
            state.detecting = Some(DetectJob { id, name })
        }
        (FileAction::Reveal { key, editing }, Ok(FileValue::Revealed(value))) => {
            reveal(&mut state, key, *editing, value)
        }
        (FileAction::DeleteProfile(name), result) => delete_profile(&mut state, name, result),
        (FileAction::DeletePasskey(name), result)
            if state.passkey_view == Sub::ConfirmDelete(name.clone()) =>
        {
            match result {
                Ok(_) => {
                    state.passkey_view = Sub::List;
                    state.kerr = None;
                }
                Err(error) => state.kerr = Some(error),
            }
        }
        (_, Err(error)) => tracing::warn!(%error,"remote tool request failed"),
        _ => {}
    }
    write_ui(ctx, state);
    ctx.request_repaint();
    true
}

fn reveal(state: &mut UiState, key: &Passkey, editing: bool, value: String) {
    if editing {
        if state.passkey_view == Sub::Form
            && state.kform.editing_original.as_deref() == Some(key.name.as_str())
            && state.kform.kind == key.kind
            && state.kform.value.is_empty()
        {
            state.kform.value = value;
        }
    } else if state.revealed.contains(&key.name) {
        state
            .revealed_values
            .insert(key.name.clone(), (key.clone(), value));
    }
}
fn delete_profile(state: &mut UiState, name: &str, result: Result<FileValue, String>) {
    let mut observed = false;
    for (view, error) in [
        (&mut state.profile_view, &mut state.perr),
        (&mut state.attach_view, &mut state.aerr),
    ] {
        if *view == Sub::ConfirmDelete(name.to_owned()) {
            observed = true;
            match &result {
                Ok(_) => {
                    *view = Sub::List;
                    *error = None;
                }
                Err(message) => *error = Some(message.clone()),
            }
        }
    }
    if !observed && let Err(error) = result {
        tracing::warn!(%error,"remote profile deletion failed after its confirmation changed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_form_keeps_failure_draft_until_final_request_owner_releases_it() {
        let ctx = egui::Context::default();
        let state = UiState {
            profile_view: Sub::Form,
            pform: ProfileForm {
                name: "draft".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        write_ui(&ctx, state.clone());
        enqueue(&ctx, &state, FileAction::SaveProfile(state.pform.clone()));
        let request = take_file_requests(&ctx).pop().unwrap();
        let retained = request.clone();
        assert_eq!(state.pending.load(Ordering::Relaxed), 1);
        assert!(accept_file_result(
            &ctx,
            &request,
            Err("storage refused".into())
        ));
        let current = read_ui(&ctx);
        assert_eq!(current.pform, state.pform);
        assert_eq!(current.profile_view, Sub::Form);
        assert_eq!(current.perr.as_deref(), Some("storage refused"));
        drop(request);
        assert_eq!(state.pending.load(Ordering::Relaxed), 1);
        drop(retained);
        assert_eq!(state.pending.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn newer_form_edits_survive_an_older_success_receipt() {
        let ctx = egui::Context::default();
        let state = UiState {
            profile_view: Sub::Form,
            ..Default::default()
        };
        enqueue(&ctx, &state, FileAction::SaveProfile(state.pform.clone()));
        let request = take_file_requests(&ctx).pop().unwrap();
        let mut current = state.clone();
        current.pform.name = "newer edit".into();
        write_ui(&ctx, current.clone());
        assert!(accept_file_result(&ctx, &request, Ok(FileValue::Saved)));
        assert_eq!(read_ui(&ctx).pform, current.pform);
        assert_eq!(read_ui(&ctx).profile_view, Sub::Form);
    }
    #[test]
    fn retired_popup_receipt_cannot_change_a_new_observer() {
        let ctx = egui::Context::default();
        let original = UiState::default();
        enqueue(
            &ctx,
            &original,
            FileAction::SaveProfile(original.pform.clone()),
        );
        let request = take_file_requests(&ctx).pop().unwrap();
        let next = UiState {
            profile_view: Sub::Form,
            ..Default::default()
        };
        write_ui(&ctx, next.clone());
        assert!(!accept_file_result(&ctx, &request, Ok(FileValue::Saved)));
        assert_eq!(read_ui(&ctx).profile_view, Sub::Form);
        assert!(Arc::ptr_eq(&read_ui(&ctx).identity, &next.identity));
        drop(request);
        assert_eq!(original.pending.load(Ordering::Relaxed), 0);
        assert_eq!(next.pending.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn delete_stays_in_confirmation_on_failure_and_leaves_only_after_success() {
        let ctx = egui::Context::default();
        let state = UiState {
            passkey_view: Sub::ConfirmDelete("owned".into()),
            ..Default::default()
        };
        write_ui(&ctx, state.clone());
        enqueue(&ctx, &state, FileAction::DeletePasskey("owned".into()));
        let request = take_file_requests(&ctx).pop().unwrap();
        assert!(accept_file_result(
            &ctx,
            &request,
            Err("cleanup failed".into())
        ));
        assert_eq!(read_ui(&ctx).passkey_view, state.passkey_view);
        assert_eq!(read_ui(&ctx).kerr.as_deref(), Some("cleanup failed"));
        drop(request);
        let current = read_ui(&ctx);
        enqueue(&ctx, &current, FileAction::DeletePasskey("owned".into()));
        let retry = take_file_requests(&ctx).pop().unwrap();
        assert!(accept_file_result(&ctx, &retry, Ok(FileValue::Saved)));
        assert_eq!(read_ui(&ctx).passkey_view, Sub::List);
        assert!(read_ui(&ctx).kerr.is_none());
    }
    #[test]
    fn late_reveal_does_not_fill_a_new_credential_kind() {
        let ctx = egui::Context::default();
        let original = UiState {
            passkey_view: Sub::Form,
            kform: PasskeyForm {
                editing_original: Some("owned".into()),
                kind: "path".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let key = Passkey {
            name: "owned".into(),
            kind: "path".into(),
            path: "owned-old-reference".into(),
        };
        enqueue(&ctx, &original, FileAction::Reveal { key, editing: true });
        let request = take_file_requests(&ctx).pop().unwrap();
        let mut current = original;
        current.kform.kind = "inline".into();
        write_ui(&ctx, current);
        assert!(accept_file_result(
            &ctx,
            &request,
            Ok(FileValue::Revealed("owned-old-reference".into()))
        ));
        assert!(read_ui(&ctx).kform.value.is_empty());
        assert_eq!(read_ui(&ctx).kform.kind, "inline");
    }
}
