//! Owned editable buffers and their original metadata for field-delta saves.
use tasty_remote_profiles::RemoteProfile;

/// 프로필 폼 버퍼. ssh 는 전용 필드, 그 외는 generic key-value(`fields`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProfileForm {
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) label: String,
    // ssh 전용
    pub(crate) host: String,
    pub(crate) user: String,
    pub(crate) port: String,
    pub(crate) shell: String,
    // 공통
    pub(crate) passkey_ref: String,
    pub(crate) fields: Vec<(String, String)>, // generic(비-ssh)
    pub(crate) editing_original: Option<String>,
    pub(crate) original: Option<RemoteProfile>,
}

/// Passkey 폼 버퍼.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct PasskeyForm {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) value: String,
    pub(crate) editing_original: Option<String>,
}

/// Attach 폼 버퍼. Connection 은 ref(ssh 프로필 참조) ↔ inline(자체 연결정보) 토글.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AttachForm {
    pub(crate) name: String,
    pub(crate) label: String,
    /// true = ref 모드(`ssh_ref`), false = inline 모드(host/user/…).
    pub(crate) mode_ref: bool,
    pub(crate) ssh_ref: String,
    // inline 전용
    pub(crate) host: String,
    pub(crate) user: String,
    pub(crate) port: String,
    pub(crate) shell: String,
    pub(crate) passkey_ref: String,
    // Remote tasty 그룹 (모드 무관 공통)
    pub(crate) remote_tasty: String,
    pub(crate) port_mode: String,
    pub(crate) port_file: String,
    pub(crate) editing_original: Option<String>,
    pub(crate) original: Option<RemoteProfile>,
}
