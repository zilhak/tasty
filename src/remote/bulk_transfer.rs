//! 파일 전송 청크를 (client_id, transfer_id)별로 누적한다.
//! 프레임 해석은 stream_hub, 권한 확인·저장은 attach_runtime이 맡는다.
//! 연결 종료 시 호출자가 clear_client로 미완료 버퍼를 회수해야 한다.

use std::collections::HashMap;

use super::transfer_spool::{Spool,TransferOwner};
struct BulkPartial {filename:String,total_size:u64,next_seq:u32,spool:Spool,owner:TransferOwner}
#[derive(Default)]
pub(crate) struct BulkTransferRegistry {transfers:HashMap<(u32,u64),BulkPartial>}
impl BulkTransferRegistry {
    pub(crate) fn new()->Self {Self::default()}
    pub(crate) fn begin(&mut self,client:u32,id:u64,filename:String,total_size:u64,owner:TransferOwner)->Result<(),String> {
        self.transfers.remove(&(client,id));
        if self.transfers.len()>=256 {return Err("pending transfer capacity exhausted".into());}
        let spool=Spool::new()?;
        self.transfers.insert((client,id),BulkPartial {filename,total_size,next_seq:0,spool,owner});Ok(())
    }
    pub(crate) fn append(&mut self,client:u32,id:u64,seq:u32,bytes:&[u8],owner:&TransferOwner)->bool {
        let accepted=self.transfers.get_mut(&(client,id)).is_some_and(|entry| {
            if !entry.owner.same(owner) || seq!=entry.next_seq || entry.spool.len().saturating_add(bytes.len() as u64)>entry.total_size {return false;}
            let Some(next)=entry.next_seq.checked_add(1) else {return false;};
            if let Err(error)=entry.spool.append(bytes) {tracing::warn!(%error,"bulk spool write failed");return false;}
            entry.next_seq=next;true
        });
        if !accepted {self.transfers.remove(&(client,id));}accepted
    }
    pub(crate) fn take(&mut self,client:u32,id:u64)->Option<(String,TransferOwner,Spool)> {
        let entry=self.transfers.remove(&(client,id))?;
        (entry.spool.len()==entry.total_size).then_some((entry.filename,entry.owner,entry.spool))
    }
    pub(crate) fn clear_client(&mut self,client:u32) {self.transfers.retain(|(id,_),_|*id!=client);}
}
