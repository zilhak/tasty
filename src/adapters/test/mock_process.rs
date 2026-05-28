//! MockProcessSpawner — process spawn 을 *기록만* 함. 외부 process 띄우지 않음.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::ports::process::{ExitStatus, ProcessChild, ProcessSpawner};

#[derive(Debug, Clone)]
pub struct SpawnRecord {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct MockProcessSpawner {
    pub spawns: Mutex<Vec<SpawnRecord>>,
}

impl MockProcessSpawner {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProcessSpawner for MockProcessSpawner {
    fn spawn(
        &self,
        command: &str,
        args: &[&str],
        env: &[(String, String)],
        cwd: Option<&Path>,
    ) -> anyhow::Result<Box<dyn ProcessChild>> {
        self.spawns
            .lock()
            .expect("MockProcessSpawner poisoned")
            .push(SpawnRecord {
                command: command.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                env: env.to_vec(),
                cwd: cwd.map(|p| p.to_path_buf()),
            });
        Ok(Box::new(MockProcessChild { pid: 1 }))
    }
}

pub struct MockProcessChild {
    pid: u32,
}

impl ProcessChild for MockProcessChild {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> anyhow::Result<Option<ExitStatus>> {
        Ok(Some(ExitStatus::Exited(0)))
    }

    fn kill(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
