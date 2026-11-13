//! 플러그인 입력칸에 보낸 편집 요청(복사·잘라내기·붙여넣기)과 그 결과 회신을 짝짓는다.
//!
//! 호스트는 회신을 약속한 플러그인(hello 의 `edit_outcome`)에만 요청을 보낸다. 요청마다 결과는 정확히
//! 한 번 나온다: 플러그인 회신, 플러그인 종료(`Gone`), 대기 상한 초과(`Unknown`) 중 먼저 온 것이다. 계약은
//! `docs/dev-guide/plugin-development.md`의 "편집 결과 회신" 절에 있다.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::Weak;

use tasty_plugin_protocol::{CopyTargetWire, EditResultWire};

use super::{PluginCopyTarget, PluginManager};

/// 결과를 기다리는 요청의 최대 수. 넘으면 가장 오래된 요청을 모름으로 끝낸다.
pub(super) const EDIT_REQUEST_LIMIT: usize = 64;

/// 꺼내지 않은 결과의 최대 수. 넘으면 가장 오래된 결과부터 버린다. 버린 요청의 보류는 시간 초과로 끝나며
/// 실행됐을 수 있으므로 재생하지 않는다.
pub(super) const EDIT_OUTCOME_QUEUE_LIMIT: usize = 64;

/// 플러그인에 보낸 편집 요청 하나. 보낸 쪽이 보류와 함께 들고 있다가 결과와 맞춰 본다.
#[derive(Debug, Clone)]
pub struct PluginEditRequest {
    pub edit_id: u64,
    /// 요청을 받은 플러그인.
    pub plugin_id: String,
    pub target: PluginCopyTarget,
    /// 요청을 받은 실행. 다시 시작한 플러그인은 다른 실행이다.
    run: Weak<()>,
}

impl PluginEditRequest {
    fn same_run(&self, run: &Weak<()>) -> bool {
        self.run.ptr_eq(run)
    }
}

/// 편집 요청의 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginEditResult {
    /// 편집이 일어났다. 원래 키를 소비한다.
    Acted,
    /// 편집이 일어나지 않았다. 원래 키를 넘긴다.
    NotActed,
    /// 실행 여부를 모른다(플러그인이 모름으로 회신했거나 대기 상한을 넘었다). 키를 소비하고 재생하지 않는다.
    Unknown,
    /// 결과가 오기 전에 플러그인 실행이 끝났다. 실행 여부를 모르므로 재생하지 않는다.
    Gone,
}

/// 요청 하나의 결과. `copied`는 Copy·Cut 이 복사한 글자다.
#[derive(Debug, Clone)]
pub struct PluginEditOutcome {
    pub request: PluginEditRequest,
    pub result: PluginEditResult,
    pub copied: Option<String>,
}

/// 수신 스레드에서 모은 회신. 채널의 플러그인 id 를 담는다.
#[derive(Debug)]
pub(super) struct ReceivedEditOutcome {
    pub(super) channel: String,
    pub(super) target: CopyTargetWire,
    pub(super) edit_id: u64,
    pub(super) result: EditResultWire,
    pub(super) copied: Option<String>,
}

/// 편집 요청과 결과의 상태.
#[derive(Debug, Default)]
pub(crate) struct EditOutcomes {
    /// hello 로 회신을 약속한 실행 중 플러그인.
    capable: HashSet<String>,
    /// 결과를 기다리는 요청. id 는 늘어나기만 하므로 첫 항목이 가장 오래됐다.
    pending: BTreeMap<u64, PluginEditRequest>,
    outcomes: VecDeque<PluginEditOutcome>,
    last_id: u64,
}

impl EditOutcomes {
    fn push(&mut self, outcome: PluginEditOutcome) {
        self.outcomes.push_back(outcome);
        while self.outcomes.len() > EDIT_OUTCOME_QUEUE_LIMIT {
            if let Some(dropped) = self.outcomes.pop_front() {
                tracing::warn!(
                    "plugin '{}' edit outcome {} dropped: queue limit {} reached",
                    dropped.request.plugin_id,
                    dropped.request.edit_id,
                    EDIT_OUTCOME_QUEUE_LIMIT
                );
            }
        }
    }

    fn end(&mut self, request: PluginEditRequest, result: PluginEditResult) {
        self.push(PluginEditOutcome {
            request,
            result,
            copied: None,
        });
    }
}

impl PluginManager {
    /// 이 플러그인이 편집 결과를 회신하는지. false 면 그 대상의 편집 키는 넘긴다.
    pub fn supports_edit_outcome(&self, plugin_id: &str) -> bool {
        self.edit_outcomes.capable.contains(plugin_id) && self.processes.contains_key(plugin_id)
    }

    /// 플러그인 `target`에 보낼 편집 요청을 만든다. 회신을 약속하지 않았거나 실행 중이 아니면 `None`이고,
    /// 호출자는 요청을 보내지 않고 키를 넘긴다. 돌려받은 `edit_id`를 `EditRequest` 입력 이벤트에 싣는다.
    pub fn begin_edit_request(
        &mut self,
        plugin_id: &str,
        target: PluginCopyTarget,
    ) -> Option<PluginEditRequest> {
        if !self.supports_edit_outcome(plugin_id) {
            return None;
        }
        let run = self.processes.get(plugin_id)?.reply_binding();
        let state = &mut self.edit_outcomes;
        state.last_id += 1;
        let request = PluginEditRequest {
            edit_id: state.last_id,
            plugin_id: plugin_id.to_string(),
            target,
            run,
        };
        state.pending.insert(request.edit_id, request.clone());
        while state.pending.len() > EDIT_REQUEST_LIMIT {
            if let Some((_, oldest)) = state.pending.pop_first() {
                tracing::warn!(
                    "plugin '{}' edit request {} ended unknown: {} requests wait",
                    oldest.plugin_id,
                    oldest.edit_id,
                    EDIT_REQUEST_LIMIT
                );
                state.end(oldest, PluginEditResult::Unknown);
            }
        }
        Some(request)
    }

    /// 보내지 못했거나 더 기다리지 않는 요청을 지운다. 그 뒤 오는 회신은 버린다.
    pub fn cancel_edit_request(&mut self, edit_id: u64) {
        self.edit_outcomes.pending.remove(&edit_id);
    }

    /// 받은 순서대로 편집 결과를 모두 꺼낸다. pump 뒤 같은 tick 에서 한 번 부른다. 어느 창의 보류인지는
    /// 호출자가 `edit_id`로 정한다.
    pub fn take_edit_outcomes(&mut self) -> Vec<PluginEditOutcome> {
        self.edit_outcomes.outcomes.drain(..).collect()
    }

    /// pump 가 모은 hello 기능과 회신을 반영한다. 같은 배치에서 연결이 끊긴 플러그인의 것은 넣지 않는다.
    pub(super) fn apply_edit_events(
        &mut self,
        capable: Vec<String>,
        received: Vec<ReceivedEditOutcome>,
        disconnected: &[String],
    ) {
        for plugin_id in capable {
            if !disconnected.contains(&plugin_id) {
                self.edit_outcomes.capable.insert(plugin_id);
            }
        }
        for item in received {
            if disconnected.contains(&item.channel) {
                continue;
            }
            self.accept_edit_outcome(item);
        }
    }

    /// 회신 하나를 그 요청의 결과로 넣는다. 기다리는 요청이 없거나(중복·늦은 회신·모르는 id) 대상이나 회신이 온
    /// 채널의 실행이 요청과 다르면 버린다.
    fn accept_edit_outcome(&mut self, item: ReceivedEditOutcome) {
        let run = self.processes.get(&item.channel).map(|p| p.reply_binding());
        let state = &mut self.edit_outcomes;
        // 실행은 채널 프로세스의 것이므로 실행이 같으면 플러그인도 같다.
        let matches = state.pending.get(&item.edit_id).is_some_and(|request| {
            PluginCopyTarget::from_wire(item.target) == Some(request.target)
                && run.as_ref().is_some_and(|run| request.same_run(run))
        });
        if !matches {
            tracing::debug!(
                "[plugin {}] edit outcome {} matches no waiting request (dropped)",
                item.channel,
                item.edit_id
            );
            return;
        }
        let Some(request) = state.pending.remove(&item.edit_id) else {
            return;
        };
        let result = match item.result {
            EditResultWire::Acted => PluginEditResult::Acted,
            EditResultWire::NotActed => PluginEditResult::NotActed,
            EditResultWire::Unknown => PluginEditResult::Unknown,
        };
        let copied = item.copied.filter(|_| result != PluginEditResult::NotActed);
        state.push(PluginEditOutcome {
            request,
            result,
            copied,
        });
    }

    /// 이 플러그인 실행의 편집 상태를 끝낸다. 결과를 기다리던 요청은 `Gone`이 된다. 이미 받은 결과는 그 실행이
    /// 보낸 유효한 결과이므로 그대로 둔다. 종료·비활성화·재시작·연결 끊김 때 부른다.
    pub(super) fn forget_edit_requests(&mut self, plugin_id: &str) {
        let state = &mut self.edit_outcomes;
        state.capable.remove(plugin_id);
        let waiting: Vec<u64> = state
            .pending
            .values()
            .filter(|r| r.plugin_id == plugin_id)
            .map(|r| r.edit_id)
            .collect();
        for edit_id in waiting {
            if let Some(request) = state.pending.remove(&edit_id) {
                state.end(request, PluginEditResult::Gone);
            }
        }
    }
}

#[cfg(test)]
#[path = "edit_outcome_tests.rs"]
mod tests;
