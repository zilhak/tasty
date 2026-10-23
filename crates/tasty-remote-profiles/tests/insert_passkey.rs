//! New-name creation must not replace a concurrent participant's entry or private payload.
use std::{
    fs,
    sync::{Arc, Barrier},
};
use tasty_remote_profiles::{
    MutationReason, MutationStage, PasskeyCommand, Passkeys, RemoteFileStore,
};
use tasty_utils::file_replace::{FileOperationMetrics, observe_operations};
fn inline(name: &str, secret: &str) -> PasskeyCommand {
    PasskeyCommand::Inline {
        name: name.into(),
        secret: secret.into(),
    }
}
#[test]
fn duplicate_names_refuse_before_materialization_and_preserve_the_existing_entry() {
    for managed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = RemoteFileStore::at(dir.path());
        let existing = if managed {
            inline("owned", "existing-private-fixture")
        } else {
            PasskeyCommand::Path {
                name: "owned".into(),
                path: "owned-user-reference".into(),
            }
        };
        store.passkey(existing).unwrap();
        let list = dir.path().join("passkeys.toml");
        let before = fs::read(&list).unwrap();
        let counts = Arc::new(FileOperationMetrics::default());
        let _scope = observe_operations(counts.clone());
        let failure = store
            .insert_passkey(inline("owned", "replacement-private-fixture"))
            .unwrap_err();
        assert_eq!(failure.reason, MutationReason::AlreadyExists);
        assert_eq!(failure.stage, MutationStage::PasskeyRead);
        assert!(failure.secret_written.is_none());
        assert!(!failure.passkeys_saved);
        assert_eq!(fs::read(&list).unwrap(), before);
        let secret = dir.path().join("passkeys/owned");
        if managed {
            assert_eq!(
                fs::read_to_string(secret).unwrap(),
                "existing-private-fixture"
            );
        } else {
            assert!(!secret.exists());
        }
        assert_eq!(counts.snapshot().replace.attempts, 0);
        assert_eq!(counts.snapshot().delete.attempts, 0);
    }
}
#[test]
fn new_inline_and_path_creation_use_existing_owned_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    let first = store
        .insert_passkey(inline("owned", "private-fixture"))
        .unwrap();
    assert!(!first.replaced);
    assert!(!first.removed);
    assert!(first.secret_written.is_some());
    let second = store
        .insert_passkey(PasskeyCommand::Path {
            name: "reference".into(),
            path: "owned-user-file".into(),
        })
        .unwrap();
    assert!(!second.replaced);
    assert!(second.secret_written.is_none());
    let keys: Passkeys =
        toml::from_str(&fs::read_to_string(dir.path().join("passkeys.toml")).unwrap()).unwrap();
    assert_eq!(keys.passkeys.len(), 2);
    assert_eq!(keys.get("reference").unwrap().path, "owned-user-file");
    assert_eq!(
        fs::read_to_string(dir.path().join("passkeys/owned")).unwrap(),
        "private-fixture"
    );
}
#[test]
fn remove_and_invalid_names_are_refused_before_opening_a_list() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    assert!(
        store
            .insert_passkey(PasskeyCommand::Remove {
                name: "owned".into()
            })
            .is_err()
    );
    assert!(
        store
            .insert_passkey(inline("../invalid", "private-fixture"))
            .is_err()
    );
    assert!(!dir.path().join("passkeys.toml").exists());
    assert!(!dir.path().join("passkeys.toml.lock").exists());
    assert!(!dir.path().join("passkeys").exists());
}
#[test]
fn concurrent_new_name_has_one_winner_and_no_losing_secret_side_effect() {
    let dir = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = ["first-private-fixture", "second-private-fixture"]
        .into_iter()
        .map(|secret| {
            let barrier = barrier.clone();
            let root = dir.path().to_owned();
            std::thread::spawn(move || {
                barrier.wait();
                (
                    secret,
                    RemoteFileStore::at(root).insert_passkey(inline("owned", secret)),
                )
            })
        })
        .collect();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, outcome)| outcome.is_ok())
            .count(),
        1
    );
    let (winner, _) = outcomes
        .iter()
        .find(|(_, outcome)| outcome.is_ok())
        .unwrap();
    let failure = outcomes
        .iter()
        .find_map(|(_, outcome)| outcome.as_ref().err())
        .unwrap();
    assert_eq!(failure.reason, MutationReason::AlreadyExists);
    assert!(failure.secret_written.is_none());
    assert!(!failure.passkeys_saved);
    assert_eq!(
        fs::read_to_string(dir.path().join("passkeys/owned")).unwrap(),
        *winner
    );
    let keys: Passkeys =
        toml::from_str(&fs::read_to_string(dir.path().join("passkeys.toml")).unwrap()).unwrap();
    assert_eq!(keys.passkeys.len(), 1);
    assert_eq!(keys.passkeys[0].name, "owned");
}
