//! Engine-owned attach receivers. No forwarding thread outlives its grant or physical terminal.
use crate::runtime::engine_access::EngineMut;
use std::sync::Weak;
use tasty_ipc::stream::{StreamControl,StreamFrame,StreamTag,encode_mux};
use tasty_ipc::stream_hub::{StreamHub,PushResult};

pub(crate) struct Subscription {
    surface:u32,client:u32,grant:u64,generation:tasty_terminal::ResourceGeneration,
    binding:Weak<()>,hub:StreamHub,mux:bool,events:tasty_terminal::AttachEventReceiver,
}
impl EngineMut<'_> {
    pub(crate) fn subscribe_terminal(&mut self,surface:u32,client:u32,hub:&StreamHub,mux:bool,announce:bool) {
        let Some(binding)=hub.client_binding(client) else {return;};
        let Some((_,grant))=self.live.occupancy.locks_snapshot().into_iter().find(|(id,grant)|*id==surface&&grant.holder==client&&grant.ready) else {return;};
        if self.remote.attach_subscriptions.len()>=1024 && !self.remote.attach_subscriptions.contains_key(&(surface,client)) {
            tracing::warn!(surface,client,"attach subscription capacity exhausted");hub.unregister_bound(client,&binding);return;
        }
        let Some(terminal)=self.runtime.terminals.get_mut(surface) else {return;};
        let generation=terminal.resource_generation();
        let tasty_terminal::AttachStreamSubscription {snapshot,cols,rows,events}=terminal.snapshot_and_stream();
        if announce {
            let descriptor=serde_json::json!({"event":"attached","surface_id":surface,"cols":cols,"rows":rows});
            if hub.push_bound(client,&binding,StreamFrame::new(StreamTag::Control,serde_json::to_vec(&descriptor).unwrap_or_default()))!=PushResult::Sent {hub.unregister_bound(client,&binding);return;}
        }
        let payload=if mux {encode_mux(surface,&snapshot)} else {snapshot};
        if hub.push_bound(client,&binding,StreamFrame::new(StreamTag::Data,payload))!=PushResult::Sent {hub.unregister_bound(client,&binding);return;}
        self.remote.attach_subscriptions.insert((surface,client),Subscription {surface,client,grant:grant.granted_seq,generation,binding,hub:hub.clone(),mux,events});
    }
    pub(crate) fn poll_attach_subscriptions(&mut self) {
        let grants:std::collections::HashMap<_,_>=self.live.occupancy.locks_snapshot().into_iter().collect();
        for (key,mut subscription) in std::mem::take(&mut self.remote.attach_subscriptions) {
            let current=grants.get(&subscription.surface).is_some_and(|grant|grant.ready && grant.holder==subscription.client && grant.granted_seq==subscription.grant)
                && self.runtime.terminals.matches_generation(subscription.surface,subscription.generation)
                && subscription.hub.matches_client_binding(subscription.client,&subscription.binding);
            if !current {continue;}
            let mut keep=true;let mut used=0;
            while used<128 {
                let event=match subscription.events.try_recv() {
                    Ok(event)=>event,
                    Err(std::sync::mpsc::TryRecvError::Empty)=>break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected)=>{keep=false;break;},
                };used+=1;
                let frame=match event {
                    tasty_terminal::AttachEvent::Output(bytes)=>StreamFrame::new(StreamTag::Data,if subscription.mux {encode_mux(subscription.surface,&bytes)} else {bytes}),
                    tasty_terminal::AttachEvent::Resize {cols,rows}=>StreamFrame::new(StreamTag::Control,serde_json::to_vec(&StreamControl::Resize {surface_id:subscription.surface,cols,rows}).unwrap_or_default()),
                    tasty_terminal::AttachEvent::Loss=>{
                        // Loss-aware clients resnapshot; older clients still observe the forced EOF.
                        subscription.hub.push_bound(subscription.client,&subscription.binding,StreamFrame::new(StreamTag::Control,serde_json::to_vec(&StreamControl::Loss {frames:1}).unwrap_or_default()));
                        subscription.hub.unregister_bound(subscription.client,&subscription.binding);keep=false;break;
                    },
                };
                if matches!(subscription.hub.push_bound(subscription.client,&subscription.binding,frame),PushResult::Unknown|PushResult::Disconnected) {keep=false;break;}
            }
            if keep {
                self.remote.attach_subscriptions.insert(key,subscription);
                if used==128 {(self.runtime.waker)();}
            }
        }
    }
}
