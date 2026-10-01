//! Popup browse/create execution. UI owns only a request ID and copied progress values.
use std::{collections::HashMap,sync::{Arc,mpsc},time::{Instant,Duration}};
use crate::{outbound::{Remote,AttemptToken},browse::RemoteWorkspace};
pub type BrowserId=u64;
pub enum BrowserUpdate {Listed(Vec<RemoteWorkspace>),Created(u32),Failed {creating:bool,message:String}}
struct Session {attempt:Option<AttemptToken>,port:Option<u16>,tunnel:Option<tasty_ssh::SshTunnel>,started:Instant,creating:bool}
enum ResultValue {Listed {port:u16,tunnel:Option<tasty_ssh::SshTunnel>,rows:Vec<RemoteWorkspace>},Created(u32)}
struct Outcome {id:BrowserId,attempt:AttemptToken,result:Result<ResultValue,String>}
pub(crate) struct Browsers {sessions:HashMap<BrowserId,Session>,tx:mpsc::SyncSender<Outcome>,rx:mpsc::Receiver<Outcome>}
impl Default for Browsers {fn default()->Self {let (tx,rx)=mpsc::sync_channel(64);Self {sessions:HashMap::new(),tx,rx}}}
impl Remote {
    pub fn begin_browser(&mut self,id:BrowserId,profile:String,wake:Arc<dyn Fn()+Send+Sync>)->Result<(),String> {
        self.cancel_browser(id);
        if self.browsers.sessions.len()>=64 {return Err("remote browser capacity exhausted".into());}
        let attempt=self.begin_attempt(None,None).map_err(str::to_owned)?;
        let cancel=tasty_ssh::SshCancel::new();attempt.register_ssh(cancel.clone())?;
        let token=attempt.clone();let tx=self.browsers.tx.clone();
        self.browsers.sessions.insert(id,Session {attempt:Some(attempt.clone()),port:None,tunnel:None,started:Instant::now(),creating:false});
        let result=self.spawn_attempt(attempt,move || {
            let _scope=cancel.scope();
            let result=(||->anyhow::Result<ResultValue> {
                let (target,tasty,mode,file)=crate::browse::resolve_connection_spec(Some(&profile),None,"","")?;
                let (tunnel,port)=crate::browse::resolve_endpoint(&target,&tasty,&mode,file.as_deref())?;
                if !token.is_active() {anyhow::bail!("remote browser cancelled");}
                let rows=crate::browse::browse_via_port_bound(port,Some(&token))?;
                Ok(ResultValue::Listed {port,tunnel,rows})
            })().map_err(|error|error.to_string());
            crate::outbound::send_attempt_result(&tx,&token,Outcome {id,attempt:token.clone(),result});
            wake();
        });
        if result.is_err() {self.cancel_browser(id);}result
    }
    pub fn create_in_browser(&mut self,id:BrowserId,wake:Arc<dyn Fn()+Send+Sync>)->Result<(),String> {
        let session=self.browsers.sessions.get(&id).ok_or("remote browser not found")?;
        if session.attempt.is_some() {return Err("remote browser operation in progress".into());}
        let port=session.port.ok_or("remote browser has no endpoint")?;
        let attempt=self.begin_attempt(None,None).map_err(str::to_owned)?;let token=attempt.clone();let tx=self.browsers.tx.clone();
        if let Some(session)=self.browsers.sessions.get_mut(&id) {session.attempt=Some(attempt.clone());session.started=Instant::now();session.creating=true;}
        let result=self.spawn_attempt(attempt,move || {
            let result=crate::browse::probe_method_bound(port,"workspace.create",serde_json::json!({}),Some(&token)).map_err(|error|error.to_string()).and_then(|value|value.get("id").and_then(|id|id.as_u64()).and_then(|id|u32::try_from(id).ok()).map(ResultValue::Created).ok_or_else(||"remote workspace.create returned no ID".into()));
            crate::outbound::send_attempt_result(&tx,&token,Outcome {id,attempt:token.clone(),result});wake();
        });
        if result.is_err() && let Some(session)=self.browsers.sessions.get_mut(&id) {session.attempt=None;}result
    }
    pub fn cancel_browser(&mut self,id:BrowserId) {
        if let Some(session)=self.browsers.sessions.remove(&id) && let Some(attempt)=session.attempt {self.cancel_attempt(&attempt);}
    }
    pub fn take_browser_connection(&mut self,id:BrowserId)->Result<(u16,Option<tasty_ssh::SshTunnel>),String> {
        let session=self.browsers.sessions.get(&id).ok_or("remote browser not found")?;
        if session.attempt.is_some() {return Err("remote browser operation in progress".into());}
        let port=session.port.ok_or("remote browser has no endpoint")?;
        let session=self.browsers.sessions.remove(&id).ok_or("remote browser disappeared")?;
        Ok((port,session.tunnel))
    }
    pub fn poll_browsers(&mut self)->Vec<(BrowserId,BrowserUpdate)> {
        self.reap_attempts();let mut updates=Vec::new();
        for _ in 0..64 {
            let Ok(outcome)=self.browsers.rx.try_recv() else {break;};
            let valid=self.browsers.sessions.get(&outcome.id).and_then(|session|session.attempt.as_ref()).is_some_and(|attempt|*attempt==outcome.attempt);
            if !valid {continue;}
            if self.finish_attempt(&outcome.attempt).is_none() {continue;}
            let Some(session)=self.browsers.sessions.get_mut(&outcome.id) else {continue;};session.attempt=None;
            let update=match outcome.result {
                Ok(ResultValue::Listed {port,tunnel,rows})=>{session.port=Some(port);session.tunnel=tunnel;BrowserUpdate::Listed(rows)},
                Ok(ResultValue::Created(id))=>BrowserUpdate::Created(id),
                Err(message)=>BrowserUpdate::Failed {creating:session.creating,message},
            };updates.push((outcome.id,update));
        }
        let expired:Vec<_>=self.browsers.sessions.iter().filter(|(_,session)|session.attempt.is_some()&&session.started.elapsed()>=Duration::from_secs(if session.creating {10}else{20})).map(|(id,session)|(*id,session.creating)).collect();
        for (id,creating) in expired {
            if let Some(attempt)=self.browsers.sessions.get_mut(&id).and_then(|session|session.attempt.take()) {self.cancel_attempt(&attempt);}
            updates.push((id,BrowserUpdate::Failed {creating,message:tasty_i18n::t(if creating {"remote_attach.create_timeout"}else{"remote_attach.timeout"}).replace("{secs}",if creating {"10"}else{"20"})}));
        }updates
    }
    pub(crate) fn shutdown_browsers(&mut self) {let ids:Vec<_>=self.browsers.sessions.keys().copied().collect();for id in ids {self.cancel_browser(id);}while let Ok(outcome)=self.browsers.rx.try_recv() {drop(outcome);}}
}
