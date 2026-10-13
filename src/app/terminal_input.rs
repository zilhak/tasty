//! terminal.tell·broadcast·respawn의 입력 실행과 지연 실행 대상 고정.
//! wire 해석과 JSON-RPC 응답 변환은 IPC 진입 봉투가 맡고, 여기서는 typed 값만 받는다.
//! 세 경로의 입력 방식은 서로 다르다. tell은 본문 ack 뒤 CR을 따로 제출하고,
//! broadcast와 respawn은 요청 처리 안에서 바로 쓴다.

use std::collections::BTreeMap;

use crate::app::command::SendPayload;
use crate::app::services::AppServices;
use crate::app::services::impl_attach::SurfaceSendOutcome;
use crate::runtime::child_terminal::{ChildEntry, ParentKey};
use crate::runtime::engine_access::{EngineMut, EngineRef};
use crate::runtime::pending_submit::SubmitRejected;
use tasty_terminal::ResourceGeneration;

/// 대상 surface별로 고정 시점의 터미널 세대. 세대가 없으면 그 대상은 실행하지 않는다.
pub(crate) type Bindings = BTreeMap<u32, Option<ResourceGeneration>>;

/// 지연 실행 동안 같은 child인지 확인하는 근거. surface 번호만으로 다시 찾지 않는다.
#[derive(Clone)]
pub(crate) struct ChildTarget {
    pub surface: u32,
    parent: ParentKey,
    index: u32,
    mirror: Option<(u32, std::sync::Weak<()>)>,
}

impl ChildTarget {
    /// mirror 워크스페이스의 child는 현재 투영 연결까지 함께 고정한다.
    pub(crate) fn fix(engine: &EngineRef<'_>, parent: ParentKey, entry: &ChildEntry) -> Self {
        let surface = entry.child_surface_id;
        let mirror = engine
            .find_workspace_index_for_surface(surface)
            .and_then(|(index, _)| engine.workspace_at(index))
            .filter(|workspace| workspace.mirror)
            .and_then(|workspace| {
                engine
                    .mirror_projection_token(workspace.id)
                    .map(|token| (workspace.id, token))
            });
        Self {
            surface,
            parent,
            index: entry.index,
            mirror,
        }
    }

    fn still_bound(&self, engine: &EngineMut<'_>, bindings: &Bindings) -> bool {
        engine
            .runtime
            .child_terminals
            .find_child(&self.parent, self.index)
            .is_some_and(|entry| entry.child_surface_id == self.surface)
            && self.mirror.as_ref().is_none_or(|(workspace, token)| {
                engine.matches_mirror_projection(*workspace, token)
            })
            && bindings
                .get(&self.surface)
                .copied()
                .flatten()
                .is_some_and(|generation| {
                    engine
                        .runtime
                        .terminals
                        .matches_generation(self.surface, generation)
                })
    }
}

/// 활성화를 기다리는 동안 확정해 둔 입력. 대상과 본문·옵션을 함께 보유하므로
/// 실행할 때 원 요청을 다시 해석하지 않는다. 원 요청은 인증·응답 봉투로만 남는다.
pub(crate) enum PreparedTerminalInput {
    /// 대상 surface의 세대 검사는 재개 봉투가 실행 전에 한다.
    Tell { surface: u32, text: String },
    Broadcast {
        targets: Vec<ChildTarget>,
        text: String,
    },
    Respawn {
        target: ChildTarget,
        options: RespawnOptions,
    },
}

/// respawn이 바꾸는 값. 문자열이 아닌 wire 값은 지정하지 않은 것으로 읽는다.
#[derive(Clone, Default)]
pub(crate) struct RespawnOptions {
    pub command: Option<String>,
    pub role: Option<String>,
    pub nickname: Option<String>,
}

impl RespawnOptions {
    fn heap_bytes(&self) -> usize {
        [&self.command, &self.role, &self.nickname]
            .into_iter()
            .flatten()
            .map(String::capacity)
            .sum()
    }
}

/// 입력 실행 결과. IPC가 기존 응답과 오류 코드·문구로 바꾼다.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TerminalInputResult {
    /// 본문을 쓰고 CR 제출을 예약했다. CR 완료를 뜻하지 않는다.
    Told {
        surface: u32,
    },
    Rejected {
        surface: u32,
        reason: SubmitRejected,
    },
    /// 고정 대상 전체. 쓰지 못한 child도 포함한다.
    Broadcast {
        children: Vec<u32>,
    },
    Respawned {
        surface: u32,
        index: u32,
    },
    ChildMissing {
        parent: ParentKey,
        index: u32,
    },
    InterruptFailed {
        surface: u32,
        outcome: SurfaceSendOutcome,
    },
    CommandFailed {
        surface: u32,
        outcome: SurfaceSendOutcome,
    },
    /// 기다리는 동안 child·mirror 연결·터미널 세대가 바뀌었다.
    TargetChanged,
}

impl PreparedTerminalInput {
    pub(crate) fn targets(&self) -> Vec<u32> {
        match self {
            Self::Tell { surface, .. } => vec![*surface],
            Self::Broadcast { targets, .. } => {
                targets.iter().map(|target| target.surface).collect()
            }
            Self::Respawn { target, .. } => vec![target.surface],
        }
    }

    /// 원 요청과 별도로 보유하는 heap 크기. 본문 사본도 대기 예산에 포함한다.
    pub(crate) fn weight(&self) -> usize {
        match self {
            Self::Tell { text, .. } => text.capacity(),
            Self::Broadcast { targets, text } => {
                targets.capacity() * std::mem::size_of::<ChildTarget>() + text.capacity()
            }
            Self::Respawn { options, .. } => {
                std::mem::size_of::<ChildTarget>() + options.heap_bytes()
            }
        }
    }

    pub(crate) fn permits_partial(&self) -> bool {
        matches!(self, Self::Broadcast { .. })
    }

    /// child 등록과 mirror 연결로 대상을 다시 확인하는 입력. tell은 surface 세대만 본다.
    pub(crate) fn binds_children(&self) -> bool {
        !matches!(self, Self::Tell { .. })
    }

    pub(crate) fn execute(
        &self,
        core: &mut AppServices,
        engine: &mut EngineMut<'_>,
        bindings: &Bindings,
    ) -> TerminalInputResult {
        match self {
            Self::Tell { surface, text } => tell(engine, *surface, text),
            Self::Broadcast { targets, text } => broadcast_where(
                core,
                engine,
                targets.iter().map(|target| target.surface).collect(),
                text,
                |position, engine| targets[position].still_bound(engine, bindings),
            ),
            Self::Respawn { target, options } => {
                if !target.still_bound(engine, bindings) {
                    return TerminalInputResult::TargetChanged;
                }
                respawn(core, engine, &target.parent, target.index, options)
            }
        }
    }
}

/// 멀티라인 본문을 bracketed paste로 감싼다. 제출용 CR은 호출자가 따로 보낸다.
pub(crate) fn tell_payload(message: &str) -> String {
    if message.contains('\n') {
        format!("\u{1b}[200~{message}\u{1b}[201~")
    } else {
        message.to_string()
    }
}

/// 끝의 CR을 본문에서 분리해 별도 전송하게 한다. CR이 없으면 제출하지 않는다.
pub(crate) fn broadcast_payload(text: &str) -> (String, bool) {
    let submit = text.ends_with('\r');
    let body = text.trim_end_matches('\r');
    (tell_payload(body), submit)
}

/// 본문을 ack와 함께 쓰고, ack 또는 5초 뒤 settle을 거쳐 CR을 제출하도록 예약한다.
pub(crate) fn tell(engine: &mut EngineMut<'_>, surface: u32, text: &str) -> TerminalInputResult {
    if let Err(reason) = engine.write_then_submit(surface, &tell_payload(text)) {
        return TerminalInputResult::Rejected { surface, reason };
    }
    if engine
        .runtime
        .child_terminals
        .clear_idle_for_new_prompt(surface)
    {
        engine.runtime.child_terminals.save();
    }
    TerminalInputResult::Told { surface }
}

/// 각 child에 본문을 바로 쓰고, 끝에 CR이 있었으면 즉시 한 번 더 쓴다.
pub(crate) fn broadcast(
    core: &mut AppServices,
    engine: &mut EngineMut<'_>,
    children: Vec<u32>,
    text: &str,
) -> TerminalInputResult {
    broadcast_where(core, engine, children, text, |_, _| true)
}

/// 본문 실패는 그 child만 건너뛴다. CR 실패는 경고만 남기고 성공으로 본다.
fn broadcast_where(
    core: &mut AppServices,
    engine: &mut EngineMut<'_>,
    children: Vec<u32>,
    text: &str,
    reachable: impl Fn(usize, &EngineMut<'_>) -> bool,
) -> TerminalInputResult {
    let (body, submit) = broadcast_payload(text);
    let mut idle_cleared = false;
    for (position, &surface) in children.iter().enumerate() {
        if !reachable(position, engine) {
            continue;
        }
        if !broadcast_to_child(core, engine, surface, &body, submit) {
            continue;
        }
        idle_cleared |= engine
            .runtime
            .child_terminals
            .clear_idle_for_new_prompt(surface);
    }
    if idle_cleared {
        engine.runtime.child_terminals.save();
    }
    TerminalInputResult::Broadcast { children }
}

/// 본문을 쓰지 못하면 false다. 끝 CR은 즉시 따로 쓰며 실패해도 경고만 남긴다.
fn broadcast_to_child(
    core: &mut AppServices,
    engine: &mut EngineMut<'_>,
    surface: u32,
    body: &str,
    submit: bool,
) -> bool {
    let sent = core.send_to_surface(engine, surface, SendPayload::Text(body.to_owned()));
    if sent != SurfaceSendOutcome::Sent {
        tracing::warn!("terminal.broadcast body (sid={surface}) failed: {sent:?}");
        return false;
    }
    if submit {
        let sent = core.send_to_surface(engine, surface, SendPayload::Text("\r".into()));
        if sent != SurfaceSendOutcome::Sent {
            tracing::warn!("terminal.broadcast submit (sid={surface}) failed: {sent:?}");
        }
    }
    true
}

/// 터미널 자원은 그대로 두고 Ctrl-C 다음 command를 쓴다. Ctrl-C가 종료 완료를 보장하지는 않는다.
/// cwd를 바꾸는 respawn은 구조 admission이 맡으므로 여기로 오지 않는다.
pub(crate) fn respawn(
    core: &mut AppServices,
    engine: &mut EngineMut<'_>,
    parent: &ParentKey,
    index: u32,
    options: &RespawnOptions,
) -> TerminalInputResult {
    engine.reconcile_child_terminals();
    let Some(surface) = engine
        .runtime
        .child_terminals
        .find_child(parent, index)
        .map(|entry| entry.child_surface_id)
    else {
        return TerminalInputResult::ChildMissing {
            parent: parent.clone(),
            index,
        };
    };
    let outcome = core.send_to_surface(engine, surface, SendPayload::Bytes(vec![0x03]));
    if outcome != SurfaceSendOutcome::Sent {
        return TerminalInputResult::InterruptFailed { surface, outcome };
    }
    if let Some(command) = &options.command {
        let outcome = core.send_to_surface(engine, surface, SendPayload::Text(command.clone()));
        if outcome != SurfaceSendOutcome::Sent {
            return TerminalInputResult::CommandFailed { surface, outcome };
        }
    }
    engine
        .runtime
        .child_terminals
        .update_child(parent, index, |entry| {
            if let Some(role) = &options.role {
                entry.role = Some(role.clone());
            }
            if let Some(nickname) = &options.nickname {
                entry.nickname = Some(nickname.clone());
            }
        });
    engine.runtime.child_terminals.set_idle(surface, false);
    engine.runtime.child_terminals.save();
    TerminalInputResult::Respawned { surface, index }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_payload_splits_trailing_cr_for_submit() {
        let body63 = "a".repeat(63);
        let (payload, submit) = broadcast_payload(&format!("{body63}\r"));
        assert_eq!(payload, body63, "본문에 제출 \\r 이 섞이면 안 됨");
        assert!(!payload.contains('\r'));
        assert!(submit);
    }

    #[test]
    fn broadcast_payload_no_trailing_cr_is_injection_only() {
        let (payload, submit) = broadcast_payload(&"b".repeat(80));
        assert_eq!(payload, "b".repeat(80));
        assert!(!submit);
    }

    #[test]
    fn broadcast_payload_multiline_wraps_bracketed_and_submits() {
        let (payload, submit) = broadcast_payload("line1\nline2\r");
        assert_eq!(payload, "\u{1b}[200~line1\nline2\u{1b}[201~");
        assert!(!payload.contains('\r'));
        assert!(submit);
    }
}
