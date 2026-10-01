//! Resource ownership while a host waits for its fixed engine's durable logical ID reservation.
use super::outbound::{Remote,AttemptToken};
use super::transport::PreparedConnection;
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct ConnectionTicket(pub u64);
pub(crate) enum PendingConnection {Connecting(AttemptToken),Ready(AttemptToken,PreparedConnection),Failed(AttemptToken,String)}
pub(crate) struct ConnectionOutcome {pub ticket:ConnectionTicket,pub result:Result<PreparedConnection,String>}
impl Remote {
    pub fn queue_connection(&mut self,port:u16,workspace:u32,tunnel:Option<tasty_ssh::SshTunnel>,anchor:Option<u32>,mapping:Option<tasty_model::WorkspaceAttachMapping>,wake:std::sync::Arc<dyn Fn()+Send+Sync>,decode:fn(&[u8])->Option<super::client_session::MirrorEvent>)->Result<ConnectionTicket,String> {
        if self.pending_connections.len()>=8 {return Err("pending remote connection capacity exhausted".into());}
        let token=self.begin_attempt(anchor,mapping).map_err(str::to_owned)?;
        let ticket=ConnectionTicket(self.next_connection);
        self.next_connection=self.next_connection.checked_add(1).ok_or("connection ticket exhausted")?;
        self.pending_connections.insert(ticket,PendingConnection::Connecting(token.clone()));
        let tx=self.connection_tx.clone();let worker_wake=wake.clone();let worker_token=token.clone();
        if let Err(error)=self.spawn_attempt(token.clone(),move || {
            let result=PreparedConnection::connect(worker_token,port,workspace,tunnel,worker_wake,decode).map_err(|error|error.to_string());
            if let Err(error)=tx.send(ConnectionOutcome {ticket,result}) {drop(error);}
            wake();
        }) {self.pending_connections.remove(&ticket);return Err(error);}
        Ok(ticket)
    }
    pub fn collect_connections(&mut self) {
        self.reap_attempts();
        while let Ok(outcome)=self.connection_rx.try_recv() {
            if let Ok(prepared)=&outcome.result {self.track_workers(prepared.transport.workers.receipt());}
            let Some(PendingConnection::Connecting(token))=self.pending_connections.remove(&outcome.ticket) else {drop(outcome);continue;};
            if !token.is_active() || self.shutdown_started.is_some() {drop(outcome);continue;}
            self.pending_connections.insert(outcome.ticket,match outcome.result {Ok(prepared)=>PendingConnection::Ready(token,prepared),Err(error)=>PendingConnection::Failed(token,error)});
        }
    }
    pub fn prepared_connection(&self,ticket:ConnectionTicket)->Option<&PreparedConnection> {
        match self.pending_connections.get(&ticket) {Some(PendingConnection::Ready(token,prepared)) if token.is_active()=>Some(prepared),_=>None}
    }
    pub fn connection_error(&self,ticket:ConnectionTicket)->Option<String> {
        match self.pending_connections.get(&ticket) {
            Some(PendingConnection::Failed(_,error))=>Some(error.clone()),
            Some(PendingConnection::Connecting(token)|PendingConnection::Ready(token,_)) if !token.is_active()=>Some("connection attempt was retired".into()),
            None=>Some("connection attempt no longer exists".into()),
            _=>None,
        }
    }
    pub fn take_connection(&mut self,ticket:ConnectionTicket)->Option<PreparedConnection> {
        let PendingConnection::Ready(token,prepared)=self.pending_connections.remove(&ticket)? else {return None;};
        let active=self.finish_attempt(&token).is_some();active.then_some(prepared)
    }
    pub fn cancel_connection(&mut self,ticket:ConnectionTicket) {
        if let Some(pending)=self.pending_connections.remove(&ticket) {
            let token=match &pending {PendingConnection::Connecting(token)|PendingConnection::Ready(token,_)|PendingConnection::Failed(token,_)=>token};
            self.cancel_attempt(token);drop(pending);
        }
    }
}
