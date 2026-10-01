//! Profile probes own SSH cancellation and worker joins independently of their GUI observers.
use std::thread::JoinHandle;
use tasty_remote_profiles::{RemoteProfile, RemoteProfiles, Passkeys};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct DetectionId(pub u64);
pub struct DetectionUpdate { pub id:DetectionId, pub name:String, pub result:Result<String,String> }
struct Job { id:DetectionId, original:RemoteProfile, cancel:tasty_ssh::SshCancel, worker:JoinHandle<Result<String,String>> }
#[derive(Default)]
pub struct ProfileDetections { jobs:Vec<Job>, next:u64, stopping:bool }
impl ProfileDetections {
    pub fn enqueue(&mut self,name:String)->Result<DetectionId,String> {
        if self.stopping {return Err("remote profile detection is shutting down".into());}
        if self.jobs.len()>=8 {return Err("remote profile detection capacity exhausted".into());}
        let original=RemoteProfiles::load().get(&name).cloned().ok_or("remote profile not found")?;
        let passkeys=Passkeys::load();
        let id=DetectionId(self.next.checked_add(1).ok_or("remote profile detection ID exhausted")?);
        self.next=id.0;
        // A replacement probe invalidates the earlier result, but retains its worker until join.
        for job in &self.jobs {if job.original.name==name {job.cancel.request_cancel();}}
        let cancel=tasty_ssh::SshCancel::new();
        let worker_cancel=cancel.clone();let input=original.clone();
        let worker=std::thread::Builder::new().name("remote-profile-detect".into()).spawn(move || {
            let _scope=worker_cancel.scope();
            tasty_ssh::detect_for_profile(&input,&passkeys).map(|mode|mode.as_str().to_owned()).map_err(|error|error.to_string())
        }).map_err(|error|error.to_string())?;
        self.jobs.push(Job {id,original,cancel,worker});Ok(id)
    }
    pub fn poll(&mut self)->Vec<DetectionUpdate> {
        let mut updates=Vec::new();let mut running=Vec::new();
        for job in std::mem::take(&mut self.jobs) {
            if !job.worker.is_finished() {running.push(job);continue;}
            let result=job.worker.join().unwrap_or_else(|_|Err("remote profile detection worker panicked".into()));
            let result=if job.cancel.is_cancelled() || self.stopping {Err("remote profile detection cancelled".into())} else {
                // Re-read the file only at commit: an old probe must not replace an edited profile
                // or restore a deleted one, nor overwrite unrelated entries from its start snapshot.
                let mut profiles=RemoteProfiles::load();
                if profiles.get(&job.original.name)!=Some(&job.original) {Err("remote profile changed during detection".into())} else {
                    let mut profile=job.original.clone();
                    if result.is_ok() {profile.remove_field("detect_failed");} else {profile.set_field("detect_failed","true");}
                    profiles.upsert(profile);
                    match profiles.save() {Ok(())=>result,Err(error)=>Err(format!("save: {error}"))}
                }
            };
            match &result {Ok(mode)=>tracing::info!(profile=%job.original.name,%mode,"remote profile detection completed"),Err(error)=>tracing::warn!(profile=%job.original.name,%error,"remote profile detection did not complete")}
            updates.push(DetectionUpdate {id:job.id,name:job.original.name,result});
        }
        self.jobs=running;updates
    }
    pub fn has_pending(&self)->bool { !self.jobs.is_empty() }
    pub fn begin_shutdown(&mut self) {self.stopping=true;for job in &self.jobs {job.cancel.request_cancel();}}
    /// The returned number counts actual unjoined workers, never UI timeouts.
    pub fn poll_shutdown(&mut self)->usize {self.begin_shutdown();self.poll();self.jobs.len()}
}
impl Drop for ProfileDetections {
    fn drop(&mut self) {
        let remaining=self.poll_shutdown();
        if remaining!=0 {tracing::warn!(remaining,"remote profile workers still unjoined at owner drop");}
    }
}
