//! Form values become explicit field deltas; no disk read or write occurs here.
use crate::adapters::ui::popup::remote_tool::{self, AttachForm, PasskeyForm, ProfileForm};
use crate::app::remote_file_commands::Request;
use crate::i18n::t;
use tasty_remote_profiles::{
    PasskeyCommand, Passkeys, ProfileCommand, ProfilePatch, RemoteProfile, is_valid_passkey_name,
    is_valid_port_mode, is_valid_shell,
};
const ATTACH_KIND: &str = "tasty-attach";

pub(super) fn profile(form: ProfileForm) -> Result<Request, String> {
    let (value, detect) = profile_value(&form, true)?;
    let command = if let Some(name) = &form.editing_original {
        let original = form
            .original
            .as_ref()
            .ok_or("original profile form is unavailable")?;
        let before = remote_tool::form_from_profile(original, &Passkeys::default());
        let (before, _) = profile_value(&before, false)?;
        let mut command = edit(name.clone(), before, value);
        if !detect
            && form
                .original
                .as_ref()
                .is_some_and(|original| original.fields.contains_key("detect_failed"))
            && let ProfileCommand::Rename { patch, .. } = &mut command
        {
            patch.fields.insert("detect_failed".into(), None);
        }
        command
    } else {
        ProfileCommand::Insert(value)
    };
    Ok(Request::Profile {
        command: Box::new(command),
        linked: None,
        detect,
    })
}
pub(super) fn attach(form: AttachForm) -> Result<Request, String> {
    let value = attach_value(&form, true)?;
    let command = if let Some(name) = &form.editing_original {
        let original = form
            .original
            .as_ref()
            .ok_or("original attach form is unavailable")?;
        let before = remote_tool::form_from_attach(original);
        edit(name.clone(), attach_value(&before, false)?, value)
    } else {
        ProfileCommand::Insert(value)
    };
    Ok(Request::Profile {
        command: Box::new(command),
        linked: None,
        detect: false,
    })
}
fn edit(original: String, before: RemoteProfile, after: RemoteProfile) -> ProfileCommand {
    let mut patch = ProfilePatch {
        kind: (before.kind != after.kind).then(|| after.kind.clone()),
        label: (before.label != after.label).then(|| after.label.clone()),
        passkey_ref: (before.passkey_ref != after.passkey_ref).then(|| after.passkey_ref.clone()),
        ..Default::default()
    };
    let keys: std::collections::BTreeSet<_> = before
        .fields
        .keys()
        .chain(after.fields.keys())
        .cloned()
        .collect();
    for key in keys {
        if before.fields.get(&key) != after.fields.get(&key) {
            patch
                .fields
                .insert(key.clone(), after.fields.get(&key).cloned());
        }
    }
    ProfileCommand::Rename {
        original,
        name: after.name,
        patch,
    }
}
pub(super) fn key(form: PasskeyForm) -> Result<Request, String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_owned());
    }
    if !is_valid_passkey_name(name) {
        return Err(t("remote_tool.err_name_format").to_owned());
    }
    if form.value.trim().is_empty() {
        return Err(t("remote_tool.err_value_empty").to_owned());
    }
    let replacement = if form.kind == "inline" {
        PasskeyCommand::Inline {
            name: name.to_owned(),
            secret: form.value,
        }
    } else {
        PasskeyCommand::Path {
            name: name.to_owned(),
            path: form.value.trim().to_owned(),
        }
    };
    Ok(match form.editing_original {
        Some(original) => Request::RenamePasskey {
            original,
            replacement,
        },
        None => Request::InsertPasskey(replacement),
    })
}

fn profile_value(f: &ProfileForm, validate: bool) -> Result<(RemoteProfile, bool), String> {
    let kind = f.kind.trim();
    if validate && kind.is_empty() {
        return Err(t("remote_tool.err_type_empty").to_string());
    }
    let name = f.name.trim();
    if validate && name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    let is_ssh = kind == "ssh";
    if is_ssh {
        if validate && f.host.trim().is_empty() {
            return Err(t("remote_tool.err_host_empty").to_string());
        }
        if validate && !f.port.trim().is_empty() && f.port.trim().parse::<u16>().is_err() {
            return Err(t("remote_tool.err_port_invalid").to_string());
        }
    }
    let mut p = RemoteProfile::new(name, kind);
    if !f.label.trim().is_empty() {
        p.label = Some(f.label.trim().to_string());
    }
    if !f.passkey_ref.is_empty() {
        p.passkey_ref = Some(f.passkey_ref.clone());
    }
    let mut needs_detect = false;
    if is_ssh {
        p.set_field("host", f.host.trim().to_string());
        if !f.user.trim().is_empty() {
            p.set_field("user", f.user.trim().to_string());
        }
        if !f.port.trim().is_empty() {
            p.set_field("port", f.port.trim().to_string());
        }
        let shell = if is_valid_shell(&f.shell) {
            f.shell.clone()
        } else {
            "auto".into()
        };
        p.set_field("shell", shell.clone());
        // 명시 셸 → port_mode 즉시 도출, auto → 저장 후 워커 감지.
        needs_detect = tasty_remote_profiles::shell_to_port_mode(&shell).is_none();
        if let Some(mode) = tasty_remote_profiles::shell_to_port_mode(&shell) {
            p.set_field("port_mode", mode);
        }
    } else {
        for (k, v) in &f.fields {
            if !k.trim().is_empty() {
                p.set_field(k.trim().to_string(), v.clone());
            }
        }
    }

    Ok((p, needs_detect))
}

fn attach_value(f: &AttachForm, validate: bool) -> Result<RemoteProfile, String> {
    let name = f.name.trim();
    if validate && name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    if f.mode_ref {
        if validate && f.ssh_ref.is_empty() {
            return Err(t("remote_tool.err_ssh_ref_empty").to_string());
        }
    } else {
        if validate && f.host.trim().is_empty() {
            return Err(t("remote_tool.err_host_empty").to_string());
        }
        if validate && !f.port.trim().is_empty() && f.port.trim().parse::<u16>().is_err() {
            return Err(t("remote_tool.err_port_invalid").to_string());
        }
    }
    let mut p = RemoteProfile::new(name, ATTACH_KIND);
    if !f.label.trim().is_empty() {
        p.label = Some(f.label.trim().to_string());
    }
    if f.mode_ref {
        p.set_field("ssh_ref", f.ssh_ref.clone());
    } else {
        p.set_field("host", f.host.trim().to_string());
        if !f.user.trim().is_empty() {
            p.set_field("user", f.user.trim().to_string());
        }
        if !f.port.trim().is_empty() {
            p.set_field("port", f.port.trim().to_string());
        }
        let shell = if is_valid_shell(&f.shell) {
            f.shell.clone()
        } else {
            "auto".into()
        };
        // "auto" 는 AttachView 기본값 — 파일을 깨끗하게 유지하려 기본값은 쓰지 않는다.
        if shell != "auto" {
            p.set_field("shell", shell);
        }
        if !f.passkey_ref.is_empty() {
            p.passkey_ref = Some(f.passkey_ref.clone());
        }
    }
    let rt = f.remote_tasty.trim();
    if !rt.is_empty() && rt != "tasty" {
        p.set_field("remote_tasty", rt.to_string());
    }
    if is_valid_port_mode(&f.port_mode) && f.port_mode != "auto" {
        p.set_field("port_mode", f.port_mode.clone());
    }
    if !f.port_file.trim().is_empty() {
        p.set_field("port_file", f.port_file.trim().to_string());
    }

    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_remote_profiles::{FieldValue, RemoteFileStore, RemoteProfiles};
    #[test]
    fn late_form_save_keeps_latest_unedited_fields_and_list_values() {
        let dir = tempfile::tempdir().unwrap();
        let store = RemoteFileStore::at(dir.path());
        let original = RemoteProfile::new("owned", "ssh")
            .with_field("host", "owned.invalid")
            .with_field("user", "old-user")
            .with_field("shell", "bash")
            .with_field(
                "extra_options",
                FieldValue::List(vec!["ProxyCommand".into(), "owned-fixture".into()]),
            );
        store
            .profile(ProfileCommand::Insert(original.clone()), None)
            .unwrap();
        let mut form = remote_tool::form_from_profile(&original, &Passkeys::default());
        form.label = "new label".into();
        let mut external = ProfilePatch::default();
        external
            .fields
            .insert("user".into(), Some("latest-user".into()));
        store
            .profile(
                ProfileCommand::Patch {
                    name: "owned".into(),
                    patch: external,
                },
                None,
            )
            .unwrap();
        let Request::Profile { command, .. } = profile(form).unwrap() else {
            panic!("profile command");
        };
        store.profile(*command, None).unwrap();
        let result: RemoteProfiles = toml::from_str(
            &std::fs::read_to_string(dir.path().join("remote-profiles.toml")).unwrap(),
        )
        .unwrap();
        let actual = result.get("owned").unwrap();
        assert_eq!(actual.label.as_deref(), Some("new label"));
        assert_eq!(
            actual.fields.get("user").and_then(FieldValue::as_str),
            Some("latest-user")
        );
        assert_eq!(
            actual.fields.get("extra_options"),
            original.fields.get("extra_options")
        );
    }
    #[test]
    fn renamed_form_clears_only_a_deliberately_removed_visible_field() {
        let original = RemoteProfile::new("owned", "ssh")
            .with_field("host", "owned.invalid")
            .with_field("user", "old-user")
            .with_field("shell", "bash");
        let mut form = remote_tool::form_from_profile(&original, &Passkeys::default());
        form.name = "renamed".into();
        form.user.clear();
        let Request::Profile { command, .. } = profile(form).unwrap() else {
            panic!("profile command");
        };
        let ProfileCommand::Rename {
            original,
            name,
            patch,
        } = *command
        else {
            panic!("atomic rename");
        };
        assert_eq!(original, "owned");
        assert_eq!(name, "renamed");
        assert_eq!(patch.fields.len(), 1);
        assert_eq!(patch.fields.get("user"), Some(&None));
        assert!(patch.label.is_none());
        assert!(patch.passkey_ref.is_none());
    }
    #[test]
    fn attach_mode_edit_removes_previous_reference_without_rewriting_unrendered_values() {
        let original = RemoteProfile::new("owned", ATTACH_KIND)
            .with_field("ssh_ref", "prior")
            .with_field("foreign", "keep");
        let mut form = remote_tool::form_from_attach(&original);
        form.mode_ref = false;
        form.host = "owned.invalid".into();
        let Request::Profile { command, .. } = attach(form).unwrap() else {
            panic!("profile command");
        };
        let ProfileCommand::Rename { patch, .. } = *command else {
            panic!("atomic edit");
        };
        assert_eq!(patch.fields.get("ssh_ref"), Some(&None));
        assert_eq!(
            patch.fields.get("host"),
            Some(&Some("owned.invalid".into()))
        );
        assert!(!patch.fields.contains_key("foreign"));
    }
}

#[cfg(test)]
mod explicit_status_tests {
    use super::*;
    #[test]
    fn explicit_shell_form_clears_previous_failure_in_its_first_patch() {
        let root = tempfile::tempdir().unwrap();
        let store = tasty_remote_profiles::RemoteFileStore::at(root.path());
        let original = RemoteProfile::new("owned", "ssh")
            .with_field("host", "owned.invalid")
            .with_field("shell", "bash")
            .with_field("detect_failed", "true");
        store
            .profile(ProfileCommand::Insert(original.clone()), None)
            .unwrap();
        let mut form = remote_tool::form_from_profile(&original, &Passkeys::default());
        form.label = "edited label".into();
        let Request::Profile {
            command, detect, ..
        } = profile(form).unwrap()
        else {
            panic!("profile edit");
        };
        assert!(!detect);
        let saved = store.profile(*command, None).unwrap().profile.unwrap();
        assert!(!saved.fields.contains_key("detect_failed"));
        assert_eq!(saved.label.as_deref(), Some("edited label"));
    }
}
