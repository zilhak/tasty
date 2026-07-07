use super::SurfaceId;
use super::surface_trait::Surface;

/// attach 의 `client_id`. 단계 1 `StreamClientId`(adapters)와 값·의미가 같다
/// (둘 다 u32). 모델 crate 는 adapters 에 의존하지 않으므로 여기서 별칭만 둔다.
pub type AttachClientId = u32;

/// attach 가 성립한 세션의 *양쪽 표현*을 담는 surface marker.
///
/// - 서버측: `Placeholder` — PTY 는 백그라운드 생존, 내용 숨김(렌더는 단계 4).
/// - client측: `Mirror` — 원격 grid 를 재생하는 mirror Terminal(step2 `new_detached`)을
///   가리키는 marker. 실제 grid 는 `TerminalStore` 의 detached Terminal 이 보유.
///
/// 본 struct 자체는 grid 를 들지 않는 *트리 leaf marker*. 권위 점유 상태는
/// `OccupancyRegistry`(엔진)에 있고, 본 타입은 트리에서 "이 자리는 attached" 임을 표시.
/// 트리 leaf 실제 교체·렌더 분기는 단계 4 — 단계 3 은 타입 정의 + registry 등록만.
pub struct AttachedSurface {
    pub id: SurfaceId,
    pub role: AttachRole,
}

/// attach 방향. 같은 논리 세션의 서버측/ client측 두 표현을 구분한다.
pub enum AttachRole {
    /// 서버측: 다른 client 가 점유 중. holder = 점유 client.
    Placeholder { holder: AttachClientId },
    /// client측: 원격 세션 mirror. `remote_surface_id` 는 원격 서버의 surface_id,
    /// `session` 은 client 의 attach 세션 식별(원격↔로컬 ID 재매핑 소유자, 단계 4+).
    Mirror {
        remote_surface_id: SurfaceId,
        session: u32, // AttachSessionId — 단계 4 에서 재매핑 테이블 키로 확장
    },
}

impl AttachedSurface {
    /// 서버측 placeholder marker.
    pub fn placeholder(id: SurfaceId, holder: AttachClientId) -> Self {
        Self {
            id,
            role: AttachRole::Placeholder { holder },
        }
    }

    /// client측 mirror marker.
    pub fn mirror(id: SurfaceId, remote_surface_id: SurfaceId, session: u32) -> Self {
        Self {
            id,
            role: AttachRole::Mirror {
                remote_surface_id,
                session,
            },
        }
    }

    pub fn is_placeholder(&self) -> bool {
        matches!(self.role, AttachRole::Placeholder { .. })
    }

    pub fn is_mirror(&self) -> bool {
        matches!(self.role, AttachRole::Mirror { .. })
    }

    /// 서버측 placeholder 의 점유 client. mirror 면 None.
    pub fn holder(&self) -> Option<AttachClientId> {
        match self.role {
            AttachRole::Placeholder { holder } => Some(holder),
            AttachRole::Mirror { .. } => None,
        }
    }
}

impl Surface for AttachedSurface {
    crate::impl_surface_any!();

    /// 렌더/입력 경로의 attach 분기는 `kind()=="attached"` 한 줄로 판별(design §1.2).
    fn kind(&self) -> &'static str {
        "attached"
    }

    fn type_name(&self) -> &'static str {
        match self.role {
            AttachRole::Placeholder { .. } => "Attached (held)",
            AttachRole::Mirror { .. } => "Attached (mirror)",
        }
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    /// attached marker 는 cwd 를 들지 않는다(서버측은 내부 Terminal 이, client측은
    /// mirror Terminal 이 별도 보유). Surface cwd invariant —
    /// `docs/architecture/invariants/surface-cwd.md`.
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "Attached",
            "kind": "attached",
            "id": self.id,
            "role": match self.role {
                AttachRole::Placeholder { holder } =>
                    serde_json::json!({ "placeholder": { "holder": holder } }),
                AttachRole::Mirror { remote_surface_id, session } =>
                    serde_json::json!({ "mirror": {
                        "remote_surface_id": remote_surface_id,
                        "session": session,
                    } }),
            },
        })
    }
}
