//! The application resolves its installed strategies; execution receives only their fixed values.
#[derive(Clone,Debug)]
pub enum CompletionKind {Poll(tasty_agent::PollSpec),Push {notify_via:String,timeout_ms:u64}}
#[derive(Clone,Debug)]
pub struct CompletionStrategy {pub id:String,pub kind:CompletionKind}
pub trait CompletionResolver:Send+Sync {
    fn named(&self,id:&str)->Result<CompletionStrategy,String>;
    fn default_for_method(&self,method:&str)->Option<CompletionStrategy>;
}
