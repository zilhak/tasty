//! 워크스페이스 ↔ 컴퓨터(SSH) attach 매핑 (attach/detach 단계 7).
//!
//! "워크스페이스1 = a컴퓨터, 워크스페이스2 = b컴퓨터" 를 표현하는 데이터. model 은
//! **데이터만 보관**하고 프로필 "해석"(→SSH 터널 수립, attach)은 호스트/CLI 가 한다
//! (tasty-model deps-free 유지 — `tasty-ssh-profiles` 에 의존하지 않는다).
//!
//! - 저장 프로필 참조([`WorkspaceAttachTarget::Profile`]) 또는 1회성 인라인
//!   ([`WorkspaceAttachTarget::Inline`]) 둘 다 지원(decisions 10).
//! - 매핑은 `Workspace.attach_mapping` 으로 들고, `SavedWorkspace.attach_mapping` 으로
//!   layout.json 에 영속한다(재시작 후 활성화 시 자동 재attach).

use serde::{Deserialize, Serialize};

/// 워크스페이스가 attach 할 원격 대상. 저장 프로필 name 참조 또는 즉석 인라인.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceAttachTarget {
    /// 저장된 프로필 name 참조 → 자동 attach 시 `ssh-profiles.toml` 에서 resolve.
    Profile { name: String },
    /// 1회성 인라인 타깃(저장 프로필 없이). `host` = ssh destination(`user@host` | alias).
    Inline {
        host: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_tasty: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port_mode: Option<String>,
    },
}

/// 한 워크스페이스의 attach 매핑(대상 + 원격 workspace id).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAttachMapping {
    /// 원격 대상(프로필 참조 또는 인라인).
    pub target: WorkspaceAttachTarget,
    /// 원격 tasty 의 attach 대상 workspace_id(원칙 3 — ID 명시). None 이면 attach 시
    /// 명시 필요(자동 attach 는 None 일 때 skip — 호스트가 안내).
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
        remote_workspace: Option<u32>,
    ) -> Self {
        Self {
            target: WorkspaceAttachTarget::Inline {
                host: host.into(),
                remote_tasty,
                port_mode,
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
            None,
        );
        let json = serde_json::to_string(&m).unwrap();
        let back: WorkspaceAttachMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
        assert!(back.remote_workspace.is_none());
    }
}
