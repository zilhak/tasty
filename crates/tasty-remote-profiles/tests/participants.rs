use std::{
    fs,
    process::{Command, Stdio},
};
use tasty_remote_profiles::{
    ManagedCleanup, MutationStage, PasskeyCommand, ProfileCommand, ProfilePatch, RemoteFileStore,
    RemoteProfile,
};
fn put(name: &str) -> ProfileCommand {
    ProfileCommand::Put(RemoteProfile::new(name, "ssh").with_field("host", "example.invalid"))
}
fn patch(name: &str, field: &str, value: &str) -> ProfileCommand {
    let mut patch = ProfilePatch::default();
    patch.fields.insert(field.into(), Some(value.into()));
    ProfileCommand::Patch {
        name: name.into(),
        patch,
    }
}
#[test]
fn participant_child() {
    let Some(root) = std::env::var_os("TASTY_PARTICIPANT_FIXTURE") else {
        return;
    };
    let name = std::env::var("TASTY_PARTICIPANT_NAME").unwrap();
    let store = RemoteFileStore::at(root);
    for n in 0..12 {
        store.profile(put(&format!("{name}-{n}")), None).unwrap();
        store
            .passkey(PasskeyCommand::Path {
                name: format!("{name}-{n}"),
                path: "unused-fixture-key".into(),
            })
            .unwrap();
        store
            .profile(patch("shared", &format!("{name}-{n}"), "kept"), None)
            .unwrap();
    }
}
#[test]
fn two_process_participants_preserve_unrelated_and_same_profile_deltas() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    store.profile(put("shared"), None).unwrap();
    let spawn = |name: &str| {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "participant_child", "--nocapture"])
            .env("TASTY_PARTICIPANT_FIXTURE", dir.path())
            .env("TASTY_PARTICIPANT_NAME", name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut cli = spawn("cli");
    let mut host = spawn("host");
    assert!(cli.wait().unwrap().success());
    assert!(host.wait().unwrap().success());
    let p: toml::Value =
        toml::from_str(&fs::read_to_string(dir.path().join("remote-profiles.toml")).unwrap())
            .unwrap();
    assert_eq!(p["profile"].as_array().unwrap().len(), 25);
    assert_eq!(p["profile"][0]["fields"].as_table().unwrap().len(), 25);
    let k: toml::Value =
        toml::from_str(&fs::read_to_string(dir.path().join("passkeys.toml")).unwrap()).unwrap();
    assert_eq!(k["passkey"].as_array().unwrap().len(), 24);
}
#[test]
fn unknown_invalid_fields_foreign_sections_comments_and_latest_external_edit_survive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote-profiles.toml");
    let original = "# header\nversion = 5\n[foreign]\nkeep = true\n[[profile]]\nname = 'odd'\nkind = 'future'\nextra = 17\n[profile.fields]\nport = false # invalid port retained\nfuture = { nested = 4 }\n[[profile]]\nname = 'ssh'\nkind = 'ssh'\n[profile.fields]\nhost = 'before'\n";
    fs::write(&path, original).unwrap();
    let store = RemoteFileStore::at(dir.path());
    store.profile(patch("odd", "user", "alice"), None).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# header"));
    assert!(text.contains("# invalid port retained"));
    let raw: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(raw["foreign"]["keep"].as_bool(), Some(true));
    assert_eq!(raw["profile"][0]["fields"]["port"].as_bool(), Some(false));
    assert_eq!(
        raw["profile"][0]["fields"]["future"]["nested"].as_integer(),
        Some(4)
    );
    assert_eq!(raw["profile"][0]["extra"].as_integer(), Some(17));
    fs::write(&path, text.replace("'before'", "'externally-edited'")).unwrap();
    store
        .profile(patch("ssh", "user", "new-user"), None)
        .unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("externally-edited")
    );
}
#[test]
fn corrupt_unreadable_and_ambiguous_originals_are_not_overwritten() {
    for bytes in [
        b"broken = [".as_slice(),
        b"\xff",
        b"[[profile]]\nname='x'\nkind='ssh'\n[[profile]]\nname='x'\nkind='ssh'\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote-profiles.toml");
        fs::write(&path, bytes).unwrap();
        assert!(
            RemoteFileStore::at(dir.path())
                .profile(put("x"), None)
                .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote-profiles.toml");
    fs::create_dir(&path).unwrap();
    assert!(
        RemoteFileStore::at(dir.path())
            .profile(put("x"), None)
            .is_err()
    );
    assert!(path.is_dir());
}
#[test]
fn delayed_detection_conflict_cannot_restore_removed_or_edited_profile() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    let original = store.profile(put("x"), None).unwrap().profile.unwrap();
    store.profile(patch("x", "user", "another"), None).unwrap();
    let mut delta = ProfilePatch {
        expected: Some(original),
        ..Default::default()
    };
    delta
        .fields
        .insert("detect_failed".into(), Some("true".into()));
    assert!(
        store
            .profile(
                ProfileCommand::Patch {
                    name: "x".into(),
                    patch: delta.clone()
                },
                None
            )
            .is_err()
    );
    store
        .profile(ProfileCommand::Remove { name: "x".into() }, None)
        .unwrap();
    assert!(
        store
            .profile(
                ProfileCommand::Patch {
                    name: "x".into(),
                    patch: delta
                },
                None
            )
            .is_err()
    );
}
#[test]
fn list_commit_precedes_managed_cleanup_and_path_files_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    let user = dir.path().join("user-key");
    fs::write(&user, "fixture").unwrap();
    store
        .passkey(PasskeyCommand::Path {
            name: "user".into(),
            path: user.to_string_lossy().into(),
        })
        .unwrap();
    let result = store
        .passkey(PasskeyCommand::Remove {
            name: "user".into(),
        })
        .unwrap();
    assert_eq!(result.cleanup, ManagedCleanup::NotManaged);
    assert!(user.exists());
    store
        .passkey(PasskeyCommand::Inline {
            name: "managed".into(),
            secret: "fixture".into(),
        })
        .unwrap();
    assert_eq!(
        store
            .passkey(PasskeyCommand::Remove {
                name: "managed".into()
            })
            .unwrap()
            .cleanup,
        ManagedCleanup::Removed
    );
    // A managed entry pointing outside its owned name/directory must not delete that file.
    fs::write(
        dir.path().join("passkeys.toml"),
        format!(
            "[[passkey]]\nname='outside'\nkind='inline'\npath={}\n",
            toml::Value::String(user.to_string_lossy().into())
        ),
    )
    .unwrap();
    assert!(matches!(
        store
            .passkey(PasskeyCommand::Remove {
                name: "outside".into()
            })
            .unwrap()
            .cleanup,
        ManagedCleanup::Failed(_)
    ));
    assert!(user.exists());
}
#[test]
fn corrupt_list_prevents_secret_side_effect_and_reference_publication() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    let path = dir.path().join("passkeys.toml");
    fs::write(&path, "broken = [").unwrap();
    let error = store
        .profile(
            put("x"),
            Some(PasskeyCommand::Inline {
                name: "key".into(),
                secret: "fixture".into(),
            }),
        )
        .unwrap_err();
    assert_eq!(error.stage, MutationStage::PasskeyRead);
    assert!(!error.passkeys_saved);
    assert!(error.secret_written.is_none());
    assert!(!dir.path().join("passkeys/key").exists());
    assert!(!dir.path().join("remote-profiles.toml").exists());
    assert_eq!(fs::read_to_string(path).unwrap(), "broken = [");
}
#[test]
fn managed_cleanup_failure_is_distinct_from_saved_removal() {
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    store
        .passkey(PasskeyCommand::Inline {
            name: "managed".into(),
            secret: "fixture".into(),
        })
        .unwrap();
    fs::remove_file(dir.path().join("passkeys/managed")).unwrap();
    fs::create_dir(dir.path().join("passkeys/managed")).unwrap();
    let result = store
        .passkey(PasskeyCommand::Remove {
            name: "managed".into(),
        })
        .unwrap();
    assert!(result.removed);
    assert!(matches!(result.cleanup, ManagedCleanup::Failed(_)));
    assert!(
        !fs::read_to_string(dir.path().join("passkeys.toml"))
            .unwrap()
            .contains("managed")
    );
}
#[cfg(unix)]
#[test]
fn registry_symlink_is_retained_and_inline_symlink_never_touches_user_file() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("actual");
    fs::write(&target, "version=1\n").unwrap();
    symlink(&target, dir.path().join("remote-profiles.toml")).unwrap();
    RemoteFileStore::at(dir.path())
        .profile(put("x"), None)
        .unwrap();
    assert!(
        fs::symlink_metadata(dir.path().join("remote-profiles.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(fs::read_to_string(&target).unwrap().contains("x"));
    let secrets = dir.path().join("passkeys");
    fs::create_dir(&secrets).unwrap();
    symlink(&target, secrets.join("key")).unwrap();
    let original = fs::read(&target).unwrap();
    assert!(
        RemoteFileStore::at(dir.path())
            .passkey(PasskeyCommand::Inline {
                name: "key".into(),
                secret: "fixture".into()
            })
            .is_err()
    );
    assert_eq!(fs::read(target).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn readonly_registry_and_list_failure_preserve_bytes_and_managed_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    let profile = dir.path().join("remote-profiles.toml");
    store.profile(put("x"), None).unwrap();
    let original = fs::read(&profile).unwrap();
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(store.profile(patch("x", "host", "changed"), None).is_err());
    assert_eq!(fs::read(&profile).unwrap(), original);
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o600)).unwrap();
    store
        .passkey(PasskeyCommand::Inline {
            name: "key".into(),
            secret: "fixture".into(),
        })
        .unwrap();
    let list = dir.path().join("passkeys.toml");
    let before = fs::read(&list).unwrap();
    fs::set_permissions(&list, fs::Permissions::from_mode(0o400)).unwrap();
    let e = store
        .passkey(PasskeyCommand::Remove { name: "key".into() })
        .unwrap_err();
    assert_eq!(e.stage, MutationStage::PasskeySave);
    assert_eq!(fs::read(&list).unwrap(), before);
    assert!(dir.path().join("passkeys/key").exists());
    let e = store
        .passkey(PasskeyCommand::Inline {
            name: "orphan".into(),
            secret: "fixture".into(),
        })
        .unwrap_err();
    assert_eq!(e.stage, MutationStage::PasskeySave);
    assert!(e.secret_written.is_some());
    assert!(!e.passkeys_saved);
    assert!(dir.path().join("passkeys/orphan").exists());
    fs::set_permissions(&list, fs::Permissions::from_mode(0o600)).unwrap();
}
#[cfg(unix)]
#[test]
fn compound_profile_failure_reports_saved_key_and_never_claims_transaction() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    store.profile(put("x"), None).unwrap();
    let path = dir.path().join("remote-profiles.toml");
    let before = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    let mut p = RemoteProfile::new("x", "ssh");
    p.passkey_ref = Some("key".into());
    let e = store
        .profile(
            ProfileCommand::Put(p),
            Some(PasskeyCommand::Inline {
                name: "key".into(),
                secret: "fixture".into(),
            }),
        )
        .unwrap_err();
    assert_eq!(e.stage, MutationStage::ProfileSave);
    assert!(e.passkeys_saved);
    assert!(e.secret_written.is_some());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        fs::read_to_string(dir.path().join("passkeys.toml"))
            .unwrap()
            .contains("key")
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(unix)]
#[test]
fn aliased_compound_registry_lock_is_rejected_before_second_acquisition() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    store.profile(put("x"), None).unwrap();
    symlink(
        dir.path().join("remote-profiles.toml"),
        dir.path().join("passkeys.toml"),
    )
    .unwrap();
    let mut p = RemoteProfile::new("x", "ssh");
    p.passkey_ref = Some("key".into());
    let e = store
        .profile(
            ProfileCommand::Put(p),
            Some(PasskeyCommand::Path {
                name: "key".into(),
                path: "fixture".into(),
            }),
        )
        .unwrap_err();
    assert_eq!(e.stage, MutationStage::PasskeyRead);
}
#[test]
fn empty_legacy_array_is_a_valid_starting_registry() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("remote-profiles.toml"),
        "version=1\nprofile=[]\n",
    )
    .unwrap();
    fs::write(dir.path().join("passkeys.toml"), "version=1\npasskey=[]\n").unwrap();
    let store = RemoteFileStore::at(dir.path());
    store.profile(put("x"), None).unwrap();
    store
        .passkey(PasskeyCommand::Path {
            name: "key".into(),
            path: "fixture".into(),
        })
        .unwrap();
}

#[test]
fn insert_conflict_and_missing_patch_have_stable_owned_error_reasons() {
    use tasty_remote_profiles::MutationReason;
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    let profile = RemoteProfile::new("x", "ssh");
    store
        .profile(ProfileCommand::Insert(profile.clone()), None)
        .unwrap();
    let before = fs::read(dir.path().join("remote-profiles.toml")).unwrap();
    assert_eq!(
        store
            .profile(ProfileCommand::Insert(profile), None)
            .unwrap_err()
            .reason,
        MutationReason::AlreadyExists
    );
    assert_eq!(
        store
            .profile(patch("missing", "host", "no"), None)
            .unwrap_err()
            .reason,
        MutationReason::NotFound
    );
    assert_eq!(
        fs::read(dir.path().join("remote-profiles.toml")).unwrap(),
        before
    );
}

#[cfg(unix)]
#[test]
fn registry_aliasing_inline_file_never_takes_the_same_lock_twice() {
    use std::os::unix::fs::symlink;
    for registry in ["remote-profiles.toml", "passkeys.toml"] {
        let dir = tempfile::tempdir().unwrap();
        let store = RemoteFileStore::at(dir.path());
        fs::create_dir(dir.path().join("passkeys")).unwrap();
        let key = dir.path().join("passkeys/key");
        fs::write(&key, "version=1\n").unwrap();
        symlink(&key, dir.path().join(registry)).unwrap();
        let command = PasskeyCommand::Inline {
            name: "key".into(),
            secret: "fixture".into(),
        };
        if registry == "passkeys.toml" {
            assert!(store.passkey(command).is_err());
        } else {
            let mut p = RemoteProfile::new("x", "ssh");
            p.passkey_ref = Some("key".into());
            assert!(
                store
                    .profile(ProfileCommand::Put(p), Some(command))
                    .is_err()
            );
        }
        assert_eq!(fs::read_to_string(key).unwrap(), "version=1\n");
    }
}

#[test]
fn profile_rename_preserves_raw_fields_and_rejects_name_conflicts_atomically() {
    use tasty_remote_profiles::MutationReason;
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    let path = dir.path().join("remote-profiles.toml");
    fs::write(&path,"# keep\n[[profile]]\nname='old'\nkind='future'\nextra=42\n[profile.fields]\ninvalid=false\nuser='before'\n").unwrap();
    let mut delta = ProfilePatch {
        kind: Some("ssh".into()),
        ..Default::default()
    };
    delta.fields.insert("user".into(), None);
    delta.fields.insert("host".into(), Some("fixture".into()));
    let outcome = store
        .profile(
            ProfileCommand::Rename {
                original: "old".into(),
                name: "new".into(),
                patch: delta,
            },
            None,
        )
        .unwrap();
    assert_eq!(outcome.profile.unwrap().name, "new");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# keep"));
    let raw: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(raw["profile"][0]["extra"].as_integer(), Some(42));
    assert_eq!(
        raw["profile"][0]["fields"]["invalid"].as_bool(),
        Some(false)
    );
    assert!(raw["profile"][0]["fields"].get("user").is_none());
    store.profile(put("taken"), None).unwrap();
    let before = fs::read(&path).unwrap();
    let error = store
        .profile(
            ProfileCommand::Rename {
                original: "new".into(),
                name: "taken".into(),
                patch: Default::default(),
            },
            None,
        )
        .unwrap_err();
    assert_eq!(error.reason, MutationReason::AlreadyExists);
    assert_eq!(fs::read(&path).unwrap(), before);
}
#[test]
fn passkey_rename_commits_list_then_cleans_managed_original_and_keeps_user_paths() {
    use tasty_remote_profiles::MutationReason;
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    store
        .passkey(PasskeyCommand::Inline {
            name: "old".into(),
            secret: "fixture".into(),
        })
        .unwrap();
    let result = store
        .rename_passkey(
            "old".into(),
            PasskeyCommand::Inline {
                name: "new".into(),
                secret: "replacement fixture".into(),
            },
        )
        .unwrap();
    assert_eq!(result.cleanup, ManagedCleanup::Removed);
    assert!(!dir.path().join("passkeys/old").exists());
    assert!(dir.path().join("passkeys/new").exists());
    let user = dir.path().join("user-file");
    fs::write(&user, "fixture").unwrap();
    store
        .passkey(PasskeyCommand::Path {
            name: "user".into(),
            path: user.to_string_lossy().into(),
        })
        .unwrap();
    store
        .rename_passkey(
            "user".into(),
            PasskeyCommand::Path {
                name: "renamed-user".into(),
                path: user.to_string_lossy().into(),
            },
        )
        .unwrap();
    assert!(user.exists());
    let before = fs::read(dir.path().join("passkeys.toml")).unwrap();
    let error = store
        .rename_passkey(
            "new".into(),
            PasskeyCommand::Inline {
                name: "renamed-user".into(),
                secret: "fixture".into(),
            },
        )
        .unwrap_err();
    assert_eq!(error.reason, MutationReason::AlreadyExists);
    assert_eq!(fs::read(dir.path().join("passkeys.toml")).unwrap(), before);
    assert!(!dir.path().join("passkeys/renamed-user").exists());
}
#[cfg(unix)]
#[test]
fn rename_list_failure_keeps_original_and_reports_orphan_instead_of_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = RemoteFileStore::at(dir.path());
    store
        .passkey(PasskeyCommand::Inline {
            name: "old".into(),
            secret: "fixture".into(),
        })
        .unwrap();
    let list = dir.path().join("passkeys.toml");
    let before = fs::read(&list).unwrap();
    fs::set_permissions(&list, fs::Permissions::from_mode(0o400)).unwrap();
    let error = store
        .rename_passkey(
            "old".into(),
            PasskeyCommand::Inline {
                name: "new".into(),
                secret: "fixture".into(),
            },
        )
        .unwrap_err();
    assert_eq!(error.stage, MutationStage::PasskeySave);
    assert!(error.secret_written.is_some());
    assert_eq!(fs::read(&list).unwrap(), before);
    assert!(dir.path().join("passkeys/old").exists());
    assert!(dir.path().join("passkeys/new").exists());
    fs::set_permissions(list, fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(unix)]
#[test]
fn managed_cleanup_aliasing_the_registry_returns_partial_result_without_relocking() {
    use std::os::unix::fs::symlink;
    for rename in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = RemoteFileStore::at(dir.path());
        fs::create_dir(dir.path().join("passkeys")).unwrap();
        let key = dir.path().join("passkeys/key");
        fs::write(
            &key,
            format!(
                "version=1\n[[passkey]]\nname='key'\nkind='inline'\npath={}\n",
                toml::Value::String(key.to_string_lossy().into())
            ),
        )
        .unwrap();
        symlink(&key, dir.path().join("passkeys.toml")).unwrap();
        let result = if rename {
            store.rename_passkey(
                "key".into(),
                PasskeyCommand::Inline {
                    name: "new".into(),
                    secret: "fixture".into(),
                },
            )
        } else {
            store.passkey(PasskeyCommand::Remove { name: "key".into() })
        };
        assert!(matches!(result.unwrap().cleanup, ManagedCleanup::Failed(_)));
        assert!(key.exists());
        assert!(toml::from_str::<toml::Value>(&fs::read_to_string(key).unwrap()).is_ok());
    }
}
