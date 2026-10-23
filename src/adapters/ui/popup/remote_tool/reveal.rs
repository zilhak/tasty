//! A reveal belongs to one edit or list activation, and an edit never becomes untouched again.
use super::*;

pub(super) fn begin_form(state: &mut UiState) {
    state.key_edit.store(true, Ordering::Relaxed);
    state.key_edit = Arc::default();
}
/// Also catches programmatic draft transitions; widget changed flags cover type/clear in one frame.
pub(super) fn retire_changed(previous: &UiState, next: &mut UiState) {
    if previous.passkey_view == Sub::Form && next.passkey_view != Sub::Form
        || Arc::ptr_eq(&previous.key_edit, &next.key_edit) && previous.kform != next.kform
    {
        previous.key_edit.store(true, Ordering::Relaxed);
    }
    if previous.passkey_view != Sub::Form
        && next.passkey_view == Sub::Form
        && next.key_edit.load(Ordering::Relaxed)
    {
        next.key_edit = Arc::default();
    }
    if previous.tab != next.tab
        || previous.passkey_view == Sub::List && next.passkey_view != Sub::List
    {
        for owner in previous.reveal_owners.values() {
            owner.store(true, Ordering::Relaxed);
        }
    }
}
pub(super) fn capture(state: &UiState, action: &FileAction) -> Option<Weak<AtomicBool>> {
    match action {
        FileAction::Reveal { editing: true, .. } => Some(Arc::downgrade(&state.key_edit)),
        FileAction::Reveal {
            key,
            editing: false,
        } => state.reveal_owners.get(&key.name).map(Arc::downgrade),
        _ => None,
    }
}
fn matches(state: &UiState, request: &FileRequest, key: &Passkey, editing: bool) -> bool {
    let Some(owner) = request.reveal_owner.as_ref().and_then(Weak::upgrade) else {
        return false;
    };
    if owner.load(Ordering::Relaxed) {
        return false;
    }
    if editing {
        Arc::ptr_eq(&owner, &state.key_edit)
            && state.passkey_view == Sub::Form
            && state.kform.editing_original.as_deref() == Some(key.name.as_str())
            && state.kform.kind == key.kind
            && state.kform.value.is_empty()
    } else {
        state.passkey_view == Sub::List
            && state.revealed.contains(&key.name)
            && state
                .reveal_owners
                .get(&key.name)
                .is_some_and(|current| Arc::ptr_eq(&owner, current))
    }
}
pub(super) fn settle(
    state: &mut UiState,
    request: &FileRequest,
    key: &Passkey,
    editing: bool,
    result: Result<FileValue, String>,
) {
    if !matches(state, request, key, editing) {
        if let Err(error) = result {
            tracing::warn!(%error, "credential preload failed after its observer changed");
        }
        return;
    }
    match result {
        Ok(FileValue::Revealed(value)) if editing => {
            state.kform.value = value;
            state.kerr = None;
        }
        Ok(FileValue::Revealed(value)) => {
            state
                .revealed_values
                .insert(key.name.clone(), (key.clone(), value));
        }
        Err(error) => state.kerr = Some(error),
        _ => {}
    }
}
