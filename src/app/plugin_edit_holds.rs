//! 플러그인 입력칸에 보낸 편집 키의 보류. 편집 요청을 보낸 원래 키를 그 창에 붙잡아 두고, pump 직후 꺼낸
//! 플러그인 회신으로 소비·재생을 정한다. 보류 중에 그 창에 온 키·IME·포인터 입력은 받은 순서대로 줄을 선다.
//! 계약: `docs/design/policies/key-mapping.md`의 "플러그인 입력칸" 절.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use winit::event::WindowEvent;
use winit::window::WindowId;

use super::App;
use super::edit_key_replay::{HoldSite, ReplayStep, replay_steps};
use crate::adapters::ui::input::edit_key::{EditKey, EditKeyHold, Outcome};
use crate::gfx::gpu::edit_keys::HeldEvent;
use crate::plugin::manager::{PluginEditOutcome, PluginEditRequest, PluginEditResult};

/// 회신을 기다리는 상한. 넘으면 실행됐을 수 있으므로 원래 키를 소비하고 재생하지 않는다.
const PLUGIN_HOLD_LIMIT: Duration = Duration::from_secs(1);

struct PluginEditHold {
    request: PluginEditRequest,
    hold: EditKeyHold<WindowEvent>,
    since: Instant,
}

/// 창마다 하나인 플러그인 편집 보류. 보류 중의 다음 편집 키는 줄을 서므로 창마다 하나다.
#[derive(Default)]
pub(crate) struct PluginEditHolds {
    holds: HashMap<WindowId, PluginEditHold>,
}

/// 풀린 보류. `steps`는 재생할 입력, `copied`는 소비한 편집이 복사한 글자다.
pub(crate) struct ResolvedPluginEdit {
    pub(crate) window: WindowId,
    pub(crate) steps: Vec<ReplayStep<WindowEvent>>,
    pub(crate) edit: EditKey,
    pub(crate) result: Option<PluginEditResult>,
    pub(crate) copied: Option<String>,
}

impl PluginEditHolds {
    /// 플러그인에 편집 요청을 보낸 원래 키를 붙잡는다. 회신이 없으면 재생하지 않는다(실행됐을 수 있다).
    /// 이미 보류가 있는 창에서는 부르지 않는다. 그 창의 키는 [`Self::queue`]로 줄을 선다.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the plugin popup and surface edit paths create these holds"
        )
    )]
    pub(crate) fn hold(
        &mut self,
        window: WindowId,
        request: PluginEditRequest,
        original: WindowEvent,
        edit: EditKey,
        now: Instant,
    ) {
        let hold = PluginEditHold {
            request,
            hold: EditKeyHold::new(original, edit, false),
            since: now,
        };
        if let Some(earlier) = self.holds.insert(window, hold) {
            tracing::warn!(
                edit_id = earlier.request.edit_id,
                "a plugin edit hold replaced an earlier one of the same window"
            );
        }
    }

    /// 보류 중인 창의 키·IME·modifier·포인터 입력을 줄 세운다. 줄을 섰으면 true 이고 호출자는 처리하지 않는다.
    pub(crate) fn queue(&mut self, window: WindowId, event: &WindowEvent) -> bool {
        let Some(held) = self.holds.get_mut(&window) else {
            return false;
        };
        if !(event.queues_behind_hold() || event.is_pointer()) {
            return false;
        }
        held.hold.queue(event.clone());
        true
    }

    /// 회신으로 보류를 푼다. 보류가 없는 회신(창이 닫혔거나 이미 시간이 지났다)은 버린다.
    pub(crate) fn resolve(&mut self, outcomes: Vec<PluginEditOutcome>) -> Vec<ResolvedPluginEdit> {
        let mut resolved = Vec::new();
        for outcome in outcomes {
            let window = self
                .holds
                .iter()
                .find(|(_, h)| h.request.edit_id == outcome.request.edit_id)
                .map(|(w, _)| *w);
            let Some((window, held)) = window.and_then(|w| self.holds.remove_entry(&w)) else {
                tracing::debug!(
                    edit_id = outcome.request.edit_id,
                    "plugin edit outcome without a hold (dropped)"
                );
                continue;
            };
            let key_outcome = match outcome.result {
                PluginEditResult::Acted | PluginEditResult::Unknown => Outcome::Acted,
                PluginEditResult::NotActed => Outcome::NotActed,
                PluginEditResult::Gone => Outcome::TargetGone,
            };
            let edit = held.hold.edit();
            resolved.push(ResolvedPluginEdit {
                window,
                steps: plugin_replay(held.hold.resolve(key_outcome)),
                edit,
                result: Some(outcome.result),
                copied: outcome.copied,
            });
        }
        resolved
    }

    /// 상한까지 회신이 없는 보류를 시간 초과로 푼다. 돌려준 요청 id 는 더 기다리지 않는다.
    pub(crate) fn expire(&mut self, now: Instant) -> (Vec<ResolvedPluginEdit>, Vec<u64>) {
        let late: Vec<WindowId> = self
            .holds
            .iter()
            .filter(|(_, h)| now.saturating_duration_since(h.since) >= PLUGIN_HOLD_LIMIT)
            .map(|(w, _)| *w)
            .collect();
        let mut resolved = Vec::new();
        let mut cancelled = Vec::new();
        for window in late {
            let Some(held) = self.holds.remove(&window) else {
                continue;
            };
            cancelled.push(held.request.edit_id);
            let edit = held.hold.edit();
            resolved.push(ResolvedPluginEdit {
                window,
                steps: plugin_replay(held.hold.resolve(Outcome::TimedOut)),
                edit,
                result: None,
                copied: None,
            });
        }
        (resolved, cancelled)
    }

    /// 가장 이른 시간 초과 시각. 이벤트가 없어도 이때 깨어 보류를 푼다.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.holds
            .values()
            .map(|h| h.since + PLUGIN_HOLD_LIMIT)
            .min()
    }
}

/// 원래 키는 view 에서 붙잡혔고, 줄 선 입력은 플러그인 단축키 단계 앞에서 붙잡혔다.
fn plugin_replay(
    release: crate::adapters::ui::input::edit_key::Release<WindowEvent>,
) -> Vec<ReplayStep<WindowEvent>> {
    replay_steps(release, HoldSite::BeforePluginShortcuts)
}

impl App {
    /// 보류 중인 창의 입력이면 줄 세운다. 줄을 섰으면 true 다.
    pub(super) fn queue_behind_plugin_edit(&mut self, id: WindowId, event: &WindowEvent) -> bool {
        self.plugin_edit_holds.queue(id, event)
    }

    /// pump 직후 플러그인 편집 회신을 꺼내 보낸 창의 보류를 풀고, 시간이 지난 보류도 푼다. 소비한 편집의 복사
    /// 글자는 그 창의 복사 문으로 쓰고, 넘길 키와 줄 선 입력은 그 창의 평소 이벤트 경로로 재생한다. 그 사이 창이
    /// 닫혔으면 복사 글자와 재생할 입력은 받을 곳이 없어 버려진다.
    pub(super) fn settle_plugin_edit_holds(&mut self, now: Instant) {
        let outcomes = self
            .plugin_manager
            .as_mut()
            .map(|mgr| mgr.take_edit_outcomes())
            .unwrap_or_default();
        let mut resolved = self.plugin_edit_holds.resolve(outcomes);
        let (late, cancelled) = self.plugin_edit_holds.expire(now);
        resolved.extend(late);
        if let Some(mgr) = self.plugin_manager.as_mut() {
            for edit_id in cancelled {
                mgr.cancel_edit_request(edit_id);
            }
        }
        for item in resolved {
            self.finish_plugin_edit(item);
        }
    }

    fn finish_plugin_edit(&mut self, item: ResolvedPluginEdit) {
        let ResolvedPluginEdit {
            window,
            steps,
            edit,
            result,
            copied,
        } = item;
        tracing::debug!(?edit, ?result, "plugin edit hold resolved");
        if let Some(text) = copied
            && let Some(main) = self
                .view
                .views
                .get_mut(&window)
                .and_then(|v| v.as_main_mut())
        {
            main.copy_text(
                &text,
                false,
                "toast.copied",
                Vec::new(),
                crate::adapters::ui::ToastScope::Window,
            );
        }
        self.replay_edit_steps(window, steps);
    }
}

#[cfg(test)]
#[path = "plugin_edit_holds_tests.rs"]
mod tests;
