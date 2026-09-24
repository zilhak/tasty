//! workspace의 원격 attach 대상. 저장 프로필 참조 또는 인라인 SSH 대상을 보관한다.
//! 프로필 해석과 터널 연결은 호스트·CLI가 맡으며 이 모델은 SSH 구현에 의존하지 않는다.
//! 호스트가 SavedWorkspace.attach_mapping으로 저장하고 활성화 때 재연결에 사용한다.

use serde::{Deserialize, Serialize};

/// 워크스페이스가 attach 할 원격 대상. 저장 프로필 name 참조 또는 즉석 인라인.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceAttachTarget {
    /// 저장된 tasty-attach 프로필 name 참조 → 자동 attach 시 `remote-profiles.toml`
    /// 에서 resolve(ref/inline · remote_tasty/port_mode/port_file 는 그 프로필이 소유).
    Profile { name: String },
    /// 1회성 인라인 타깃(저장 프로필 없이). `host` = ssh destination(`user@host` | alias).
    Inline {
        host: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_tasty: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port_mode: Option<String>,
        /// 원격 port 파일의 명시 경로(비표준 위치). None 이면 port_mode 관례 체인.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port_file: Option<String>,
    },
}

/// 한 워크스페이스의 attach 매핑(대상 + 원격 workspace id).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAttachMapping {
    /// 원격 대상(프로필 참조 또는 인라인).
    pub target: WorkspaceAttachTarget,
    /// 원격 workspace ID. None이면 연결할 때 지정해야 하며 자동 attach는 건너뛴다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_workspace: Option<u32>,
}

impl WorkspaceAttachMapping {
    /// 저장 프로필 매핑.
    pub fn profile(name: impl Into<String>, remote_workspace: Option<u32>) -> Self {
        Self {
            target: WorkspaceAttachTarget::Profile { name: name.into() },
            remote_workspace,
        }
    }

    /// 1회성 인라인 매핑.
    pub fn inline(
        host: impl Into<String>,
        remote_tasty: Option<String>,
        port_mode: Option<String>,
        port_file: Option<String>,
        remote_workspace: Option<u32>,
    ) -> Self {
        Self {
            target: WorkspaceAttachTarget::Inline {
                host: host.into(),
                remote_tasty,
                port_mode,
                port_file,
            },
            remote_workspace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_mapping_roundtrip() {
        let m = WorkspaceAttachMapping::profile("gx10", Some(1));
        let json = serde_json::to_string(&m).unwrap();
        let back: WorkspaceAttachMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
        match back.target {
            WorkspaceAttachTarget::Profile { name } => assert_eq!(name, "gx10"),
            _ => panic!("expected Profile"),
        }
        assert_eq!(back.remote_workspace, Some(1));
    }

    #[test]
    fn inline_mapping_roundtrip() {
        let m = WorkspaceAttachMapping::inline(
            "user@host",
            Some("/usr/local/bin/tasty".into()),
            Some("subcommand".into()),
            Some("/data/tasty.port".into()),
            None,
        );
        let json = serde_json::to_string(&m).unwrap();
        let back: WorkspaceAttachMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
        assert!(back.remote_workspace.is_none());
    }
}
