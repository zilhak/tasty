//! Event Bus 라우팅: 호스트 발화 envelope, plugin publish → fan-out, pre/post hook
//! dispatch, throttle 처리, surface event flush.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::plugin::manifest::EventHookDecl;
use crate::plugin::protocol::{self, PluginRequest};

use super::{PendingRequestKind, PluginManager};

impl PluginManager {
    pub fn publish_host_event(&mut self, envelope: tasty_plugin_protocol::EventEnvelope) {
        let event_key = envelope.key.clone();
        let payload = envelope.payload.clone();
        let dispatches = self.event_bus.publish_from_host(envelope);
        self.send_event_dispatches(dispatches);
        self.fire_popup_triggers(&event_key, &payload);
    }

    /// `[[contributes.popup]] trigger.kind = "event"`로 선언된 popup 중 방금 발화된
    /// 이벤트 key에 매칭되는 것을 자동으로 연다. plugin process가 살아 있어야 한다.
    /// payload는 popup.open IPC의 `context`로 그대로 전달된다.
    pub(super) fn fire_popup_triggers(&mut self, event_key: &str, payload: &serde_json::Value) {
        let matches: Vec<(String, String)> = self
            .packages
            .iter()
            .filter(|pkg| self.processes.contains_key(&pkg.manifest.id))
            .flat_map(|pkg| {
                let plugin_id = pkg.manifest.id.clone();
                pkg.manifest.contributes.popup.iter().filter_map(move |p| {
                    if let crate::plugin::manifest::PopupTrigger::Event { event_key: ek } =
                        &p.trigger
                    {
                        if ek == event_key {
                            return Some((plugin_id.clone(), p.id.clone()));
                        }
                    }
                    None
                })
            })
            .collect();
        for (plugin_id, popup_id) in matches {
            self.open_popup_instance(&plugin_id, &popup_id, payload.clone());
        }
    }

    /// `EventScope`/origin/trace_id를 호스트 기본값으로 채워 envelope을 만든다.
    pub fn build_host_envelope<P: serde::Serialize>(
        &self,
        key: &str,
        payload: &P,
        scope: tasty_plugin_protocol::EventScope,
    ) -> tasty_plugin_protocol::EventEnvelope {
        let trace_seq = self.event_trace_seq.fetch_add(1, Ordering::Relaxed);
        tasty_plugin_protocol::EventEnvelope {
            key: key.to_string(),
            payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
            meta: tasty_plugin_protocol::EventMeta {
                trace_id: format!("h{trace_seq:x}"),
                hop: 0,
                origin: tasty_plugin_protocol::EventOrigin::Host,
                scope,
            },
        }
    }

    /// 호스트가 직접 한 줄로 발화. envelope을 만드는 호출자가 거의 모든 곳이라
    /// 편의 헬퍼.
    pub fn emit_host_event<P: serde::Serialize>(
        &mut self,
        key: &str,
        payload: &P,
        scope: tasty_plugin_protocol::EventScope,
    ) {
        let envelope = self.build_host_envelope(key, payload, scope);
        self.publish_host_event(envelope);
    }

    /// owner unicast 발화 — envelope를 정확히 한 plugin에만 전달. 호스트가 명시적으로
    /// 보내는 메시지이므로 구독 등록 여부와 무관하다. `command.invoked`처럼
    /// "broadcast 아님"으로 지정된 이벤트에 사용. plugin이 실행 중이 아니면 조용히 폐기.
    pub fn emit_host_event_to_plugin<P: serde::Serialize>(
        &mut self,
        plugin_id: &str,
        key: &str,
        payload: &P,
        scope: tasty_plugin_protocol::EventScope,
    ) {
        if !self.processes.contains_key(plugin_id) {
            tracing::trace!(
                "emit_host_event_to_plugin: plugin '{}' not running, dropping '{}'",
                plugin_id,
                key
            );
            return;
        }
        let envelope = self.build_host_envelope(key, payload, scope);
        let dispatch = self.event_bus.unicast_to_plugin(plugin_id, envelope);
        self.send_event_dispatches(vec![dispatch]);
    }

    /// plugin이 보낸 publish를 라우팅. 권한/origin/hop 검사 실패 시 경고 로그.
    /// 활성 extension이 있고 pre_event hook이 매칭되면 hook을 먼저 dispatch한 뒤
    /// 응답에 따라 fan-out 진행 (PR 6).
    pub(super) fn route_plugin_event_publish(
        &mut self,
        plugin_id: &str,
        envelope: tasty_plugin_protocol::EventEnvelope,
    ) {
        // hook이 적용되는 publisher인지 먼저 검사. caller가 extension 자신이면 self-loop 방지.
        let hooks = self.find_active_event_hooks(plugin_id, &envelope.key);
        match hooks {
            Some((ext_id, pre_opt, post_opt)) => {
                let pre = pre_opt.filter(|h| !self.is_hook_in_backoff(&ext_id, &h.event));
                let post = post_opt.filter(|h| !self.is_hook_in_backoff(&ext_id, &h.event));
                if let Some(pre) = pre {
                    self.dispatch_pre_event_hook(plugin_id, ext_id, envelope, pre, post);
                } else if post.is_some() {
                    self.fan_out_then_post(plugin_id, envelope, ext_id, post);
                } else {
                    self.publish_and_dispatch(plugin_id, envelope);
                }
            }
            None => self.publish_and_dispatch(plugin_id, envelope),
        }
    }

    /// `publish_from_plugin` 호출 + 결과 dispatch. hook 없는 경로의 helper.
    pub(super) fn publish_and_dispatch(
        &mut self,
        plugin_id: &str,
        envelope: tasty_plugin_protocol::EventEnvelope,
    ) {
        let key_for_log = envelope.key.clone();
        let payload = envelope.payload.clone();
        match self.event_bus.publish_from_plugin(plugin_id, envelope) {
            Ok(dispatches) => {
                self.send_event_dispatches(dispatches);
                self.fire_popup_triggers(&key_for_log, &payload);
            }
            Err(e) => {
                tracing::warn!("plugin '{plugin_id}' publish '{key_for_log}' rejected: {e}");
            }
        }
    }

    /// 활성 extension이 있고 publisher가 그 extension의 target이며 매칭 event hook이
    /// 있으면 (ext_id, pre_hook, post_hook)을 반환. caller가 extension 자신이면 None.
    pub(super) fn find_active_event_hooks(
        &self,
        publisher_plugin_id: &str,
        event_key: &str,
    ) -> Option<(String, Option<EventHookDecl>, Option<EventHookDecl>)> {
        let ext_id = self
            .extensions
            .active_extension_for_target(publisher_plugin_id)?
            .to_string();
        if ext_id == publisher_plugin_id {
            return None;
        }
        let pkg = self.packages.iter().find(|p| p.manifest.id == ext_id)?;
        let extends = pkg.manifest.extends.as_ref()?;
        let pre = extends
            .pre_event
            .iter()
            .find(|h| h.event == event_key)
            .cloned();
        let post = extends
            .post_event
            .iter()
            .find(|h| h.event == event_key)
            .cloned();
        if pre.is_none() && post.is_none() {
            None
        } else {
            Some((ext_id, pre, post))
        }
    }

    pub(super) fn dispatch_pre_event_hook(
        &mut self,
        publisher_plugin_id: &str,
        ext_id: String,
        envelope: tasty_plugin_protocol::EventEnvelope,
        pre: EventHookDecl,
        post: Option<EventHookDecl>,
    ) {
        let payload = envelope.payload.clone();
        let deadline = Instant::now() + Duration::from_millis(pre.timeout_ms as u64);
        match self.send_extension_invoke_hook(
            &ext_id,
            tasty_plugin_protocol::ExtensionHookKind::Event,
            tasty_plugin_protocol::ExtensionHookPhase::Pre,
            pre.mode,
            &envelope.key,
            payload,
        ) {
            Ok(req_id) => {
                self.pending_requests.insert(
                    req_id,
                    PendingRequestKind::ExtensionPreEventHook {
                        publisher_plugin_id: publisher_plugin_id.to_string(),
                        extension_plugin_id: ext_id,
                        envelope,
                        pre_hook_mode: pre.mode,
                        post_hook: post,
                        deadline,
                    },
                );
            }
            Err(msg) => {
                tracing::warn!("pre-event-hook dispatch failed: {msg}; bypassing");
                self.fan_out_then_post(publisher_plugin_id, envelope, ext_id, post);
            }
        }
    }

    /// fan-out 실행 후 post_event hook이 있으면 dispatch.
    pub(super) fn fan_out_then_post(
        &mut self,
        publisher_plugin_id: &str,
        envelope: tasty_plugin_protocol::EventEnvelope,
        ext_id: String,
        post: Option<EventHookDecl>,
    ) {
        let event_key = envelope.key.clone();
        let payload = envelope.payload.clone();
        self.publish_and_dispatch(publisher_plugin_id, envelope);
        if let Some(post) = post {
            if self.is_hook_in_backoff(&ext_id, &event_key) {
                return;
            }
            let deadline = Instant::now() + Duration::from_millis(post.timeout_ms as u64);
            match self.send_extension_invoke_hook(
                &ext_id,
                tasty_plugin_protocol::ExtensionHookKind::Event,
                tasty_plugin_protocol::ExtensionHookPhase::Post,
                post.mode,
                &event_key,
                payload,
            ) {
                Ok(req_id) => {
                    self.pending_requests.insert(
                        req_id,
                        PendingRequestKind::ExtensionPostEventHook {
                            extension_plugin_id: ext_id,
                            event_key,
                            deadline,
                        },
                    );
                }
                Err(msg) => {
                    tracing::warn!("post-event-hook dispatch failed: {msg}; ignoring");
                }
            }
        }
    }

    pub(super) fn send_event_dispatches(
        &mut self,
        dispatches: Vec<crate::plugin::event_bus::PluginDispatch>,
    ) {
        for d in dispatches {
            let mut req = crate::plugin::event_bus::EventBus::build_dispatch_request(&d);
            req.id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
            if let Some(proc) = self.processes.get(&d.plugin_id) {
                if let Err(e) = proc.req_tx.send(req) {
                    tracing::warn!("plugin '{}' event.dispatch send failed: {}", d.plugin_id, e);
                }
            }
        }
    }

    /// plugin이 보조 채널로 알린 dirty rect를 drain. 호스트 렌더링 레이어가 frame
    /// 합성 직전에 호출한다. 반환된 map의 value가 `None`이면 "전체 갱신" sticky.
    /// plugin이 죽었거나 보조 채널이 미연결이면 빈 map.
    pub(super) fn flush_pending_events(&mut self) {
        let surface_ids: Vec<u32> = self.surfaces.keys().copied().collect();
        for sid in surface_ids {
            let (plugin_id, events) = match self.surfaces.get(&sid) {
                Some(entry) => {
                    let events = entry
                        .handles
                        .pending_events
                        .lock()
                        .map(|mut v| std::mem::take(&mut *v))
                        .unwrap_or_default();
                    (entry.plugin_id.clone(), events)
                }
                None => continue,
            };
            for ev in events {
                self.send_surface_request(
                    &plugin_id,
                    protocol::METHOD_SURFACE_EVENT,
                    json!({
                        "surface_id": sid,
                        "event": ev,
                    }),
                    PendingRequestKind::SurfaceEvent { surface_id: sid },
                );
            }
        }
    }
}
