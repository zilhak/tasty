//! StdProcessSpawner — `std::process::Command` 기반 production ProcessSpawner.

use std::path::Path;

use crate::ports::process::{ExitStatus, ProcessChild, ProcessSpawner};

#[derive(Debug, Default)]
pub struct StdProcessSpawner;

impl ProcessSpawner for StdProcessSpawner {
    fn spawn(
        &self,
        command: &str,
        args: &[&str],
        env: &[(String, String)],
        cwd: Option<&Path>,
    ) -> anyhow::Result<Box<dyn ProcessChild>> {
        let mut cmd = std::process::Command::new(command);
        cmd.args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let child = cmd.spawn()?;
        Ok(Box::new(StdProcessChild { inner: child }))
    }
}

pub struct StdProcessChild {
    inner: std::process::Child,
}

impl ProcessChild for StdProcessChild {
    fn pid(&self) -> u32 {
        self.inner.id()
    }

    fn try_wait(&mut self) -> anyhow::Result<Option<ExitStatus>> {
        match self.inner.try_wait()? {
            None => Ok(None),
            Some(status) => Ok(Some(map_status(status))),
        }
    }

    fn kill(&mut self) -> anyhow::Result<()> {
        Ok(self.inner.kill()?)
    }
}

fn map_status(status: std::process::ExitStatus) -> ExitStatus {
    if let Some(code) = status.code() {
        ExitStatus::Exited(code)
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(sig) = status.signal() {
                return ExitStatus::Signaled(sig);
            }
        }
        ExitStatus::Exited(-1)
    }
}
