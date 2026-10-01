//! A queued frame belongs to exactly one connection, never to a replaceable sender slot.
use std::sync::{Arc,atomic::{AtomicBool,Ordering},mpsc};
use super::client_session::OutFrame;
#[derive(Clone)]
pub(crate) struct ConnectionEpoch(Arc<AtomicBool>);
impl ConnectionEpoch {
    fn new()->Self {Self(Arc::new(AtomicBool::new(true)))}
    pub(crate) fn is_active(&self)->bool {self.0.load(Ordering::Acquire)}
    pub(crate) fn retire(&self) {self.0.store(false,Ordering::Release);}
    pub(crate) fn same(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}
}
pub(crate) struct QueuedFrame {
    pub(crate) epoch:ConnectionEpoch,
    pub(crate) frame:OutFrame,
}
pub(crate) struct ConnectionSender {
    epoch:ConnectionEpoch,
    sender:mpsc::Sender<QueuedFrame>,
}
impl ConnectionSender {
    pub(crate) fn epoch(&self)->ConnectionEpoch {self.epoch.clone()}
    pub(crate) fn retire(&self) {self.epoch.retire();}
    pub(crate) fn send(&self,frame:OutFrame)->Result<(),mpsc::SendError<OutFrame>> {
        if !self.epoch.is_active() {return Err(mpsc::SendError(frame));}
        self.sender.send(QueuedFrame {epoch:self.epoch.clone(),frame}).map_err(|error|mpsc::SendError(error.0.frame))
    }
}
pub(crate) fn channel()->(Arc<ConnectionSender>,mpsc::Receiver<QueuedFrame>) {
    let (sender,receiver)=mpsc::channel();
    (Arc::new(ConnectionSender {epoch:ConnectionEpoch::new(),sender}),receiver)
}
