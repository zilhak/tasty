//! SessionStart 훅의 처리: 시작 상태(입력 대기면 idle)와 프로필 복원 계획.

use std::path::Path;

use tasty_plugin_agent_common::host_call::HostCall as HostCallSink;
use tasty_plugin_sdk::i18n::Translator;

use crate::hook::{HostCall, RESTORE_COMMAND_META_KEY};
use crate::profile_attach::{self, AttachRecord};

/// 입력을 기다리는 화면으로 시작한 세션인가. `compact` 는 턴 도중 자동 압축에서도 오므로 제외하고,
/// 값이 없거나 모르는 값이면 판단하지 않는다.
fn session_start_waits_for_input(source: Option<&str>) -> bool {
    matches!(source, Some("startup" | "resume" | "clear" | "fork"))
}

/// surface meta 키 — 플러그인이 프롬프트를 실어 Claude 를 실행했다는 표시. 실행 기록이 쓰고 지우며,
/// 그 실행의 SessionStart 가 읽고 지운다. 그 세션은 시작하자마자 프롬프트를 처리하므로 idle 로 두지 않는다.
pub(crate) const LAUNCH_PROMPT_META_KEY: &str = "claude-launch-prompt";

/// 입력 대기로 시작한 세션은 첫 Stop 전에도 idle 로 기록한다. 그래야 기존 세션 agent task 처럼
/// idle 을 기다리는 소비자가 사용자 턴 없이 진행한다. 완료 훅(`claude-idle`)·화면 알림·
/// telemetry 는 턴이 끝난 것이 아니므로 만들지 않는다. 상태 값만 바꾼다.
/// 프롬프트를 실어 실행한 세션은 곧 그 턴이 시작되므로 active 로 둔다. idle 을 거치면
/// spawn 노드처럼 idle 을 완료로 보는 소비자가 프롬프트를 처리하기 전에 끝난다.
pub(crate) fn session_start_state<H: HostCallSink>(
    calls: &mut Vec<HostCall>,
    host: &H,
    surface_id: u32,
    source: Option<&str>,
) {
    if !session_start_waits_for_input(source) {
        return;
    }
    if crate::stop_pairing::meta_value(host, surface_id, LAUNCH_PROMPT_META_KEY).is_some() {
        calls.push(HostCall::MetaUnset {
            surface_id,
            key: LAUNCH_PROMPT_META_KEY,
        });
        return;
    }
    for call in calls.iter_mut() {
        if let HostCall::SetState { state, .. } = call
            && *state == "active"
        {
            *state = "idle";
        }
    }
}

/// session-start 가 복원할 프로필 계획.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SessionStartProfile {
    /// 저장 기록에서 복구했을 때 메타데이터를 되돌릴 호출 목록.
    pub meta_calls: Vec<HostCall>,
    /// 이번 세션에 실제로 실을 프로필 파일 경로. `None` 이면 프로필 없이 진행한다.
    pub profile_file: Option<String>,
    /// 프로필을 확인한 뒤 다시 저장할 기록.
    pub restamp: Option<AttachRecord>,
}

/// surface 메타데이터를 먼저 보고 없으면 저장된 프로필 기록을 읽는다.
/// 등록 이름은 매번 다시 해석해 변경된 내용을 반영한다.
/// 실패하면 경고 후 프로필 없이 복원 명령을 남긴다. 프로필 문제로 세션 복원까지 막지 않기 위해서다.
pub(crate) fn plan_session_start_profile(
    session_id: &str,
    meta: &crate::reboot::AttachedProfile,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> SessionStartProfile {
    let (attached, from_record) = match (&meta.names, &meta.path) {
        (Some(names), _) => (Some(AttachRecord::Names(names.clone())), false),
        (None, Some(path)) => (Some(AttachRecord::Path(path.clone())), false),
        (None, None) => (profile_attach::load(data_dir, session_id), true),
    };
    let Some(record) = attached else {
        return SessionStartProfile::default();
    };

    let resolved = match &record {
        AttachRecord::Names(names) => {
            match crate::profile::resolve_names(data_dir, names, tr) {
                Ok(path) => Some(path.to_string_lossy().into_owned()),
                Err(e) => {
                    // 부착 후 unregister 됐거나 파일이 깨진 경우.
                    tracing::warn!(
                        "claude hook session-start {session_id}: profile names {names:?} no longer resolve ({e:?}) — restoring without a profile"
                    );
                    None
                }
            }
        }
        AttachRecord::Path(path) => match crate::reboot::validate_profile_file(path, tr) {
            Ok(()) => Some(path.clone()),
            Err(e) => {
                tracing::warn!(
                    "claude hook session-start {session_id}: attached profile file {path:?} is unusable ({}) — restoring without a profile",
                    e.message
                );
                None
            }
        },
    };

    let Some(profile_file) = resolved else {
        // 실패 시 기존 메타데이터와 기록은 유지해 다음 session-start에서 다시 시도할 수 있게 한다.
        return SessionStartProfile::default();
    };

    let meta_calls = if from_record {
        vec![match &record {
            AttachRecord::Names(names) => HostCall::MetaSet {
                surface_id: 0,
                key: crate::reboot::PROFILE_NAMES_META_KEY,
                value: names.clone(),
            },
            AttachRecord::Path(path) => HostCall::MetaSet {
                surface_id: 0,
                key: crate::reboot::PROFILE_META_KEY,
                value: path.clone(),
            },
        }]
    } else {
        Vec::new()
    };

    SessionStartProfile {
        meta_calls,
        profile_file: Some(profile_file),
        restamp: Some(record),
    }
}

/// 복원 명령에 settings를 반영하고 기록에서 복구할 메타데이터 호출을 덧붙인다.
pub(crate) fn apply_session_start_profile(
    calls: &mut Vec<HostCall>,
    surface_id: u32,
    session_id: &str,
    plan: &SessionStartProfile,
) {
    if let Some(path) = &plan.profile_file {
        for call in calls.iter_mut() {
            if let HostCall::MetaSet { key, value, .. } = call
                && *key == RESTORE_COMMAND_META_KEY
            {
                *value = crate::reboot::resume_command_line(session_id, Some(path));
            }
        }
    }
    calls.extend(plan.meta_calls.iter().map(|call| match call {
        // 프로필 선택 단계에서 알 수 없던 대상 surface를 채운다.
        HostCall::MetaSet { key, value, .. } => HostCall::MetaSet {
            surface_id,
            key,
            value: value.clone(),
        },
        other => other.clone(),
    }));
}
