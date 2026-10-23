//! Match standalone CLI's conditional status clear after a known explicit-shell save.
use super::Failure;
use tasty_remote_profiles::{
    ProfileCommand, ProfileOutcome, ProfilePatch, RemoteFileStore, shell_to_port_mode,
};
pub(super) fn clear_explicit(
    store: &RemoteFileStore,
    saved: &mut ProfileOutcome,
) -> Result<(), Failure> {
    let Some(original) = saved.profile.as_ref() else {
        return Ok(());
    };
    let explicit = original
        .as_ssh()
        .is_some_and(|ssh| shell_to_port_mode(ssh.shell()).is_some());
    if !explicit || !original.fields.contains_key("detect_failed") {
        return Ok(());
    }
    let mut patch = ProfilePatch {
        expected: Some(original.clone()),
        ..Default::default()
    };
    patch.fields.insert("detect_failed".into(), None);
    let cleared = store
        .profile(
            ProfileCommand::Patch {
                name: original.name.clone(),
                patch,
            },
            None,
        )
        .map_err(|source| failure(source, saved))?;
    saved.profile = cleared.profile;
    Ok(())
}
fn failure(source: tasty_remote_profiles::MutationFailure, saved: &ProfileOutcome) -> Failure {
    Failure::Status {
        source,
        secret_written: saved
            .passkey
            .as_ref()
            .is_some_and(|key| key.secret_written.is_some()),
        passkeys_saved: saved.passkey.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_conditional_status_clear_reports_the_already_committed_profile() {
        let root = tempfile::tempdir().unwrap();
        let store = RemoteFileStore::at(root.path());
        let original = tasty_remote_profiles::RemoteProfile::new("owned", "ssh")
            .with_field("host", "first.invalid")
            .with_field("shell", "bash")
            .with_field("detect_failed", "true");
        let mut saved = store
            .profile(ProfileCommand::Insert(original), None)
            .unwrap();
        let mut external = ProfilePatch::default();
        external
            .fields
            .insert("host".into(), Some("latest.invalid".into()));
        store
            .profile(
                ProfileCommand::Patch {
                    name: "owned".into(),
                    patch: external,
                },
                None,
            )
            .unwrap();
        let error =
            clear_explicit(&store, &mut saved).expect_err("external edit must not be overwritten");
        assert!(error.partial());
        assert!(!error.unknown());
        let response = error.response(serde_json::json!(883));
        let data = response.error.unwrap().data.unwrap();
        assert_eq!(data["saved"], false);
        assert_eq!(data["profile_saved"], true);
        assert_eq!(data["partial"], true);
        assert_eq!(data["stage"], "DetectionStatus");
        let profiles: tasty_remote_profiles::RemoteProfiles = toml::from_str(
            &std::fs::read_to_string(root.path().join("remote-profiles.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            profiles
                .get("owned")
                .unwrap()
                .fields
                .get("host")
                .and_then(|value| value.as_str()),
            Some("latest.invalid")
        );
    }
}
