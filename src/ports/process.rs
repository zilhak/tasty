//! ProcessSpawner port — system process spawn (hook command 실행 등).

use std::path::Path;

#[allow(dead_code)] // 이유: ProcessSpawner port — std_process 어댑터 존재, hook 실행 호출 경로 배선 대기
pub trait ProcessSpawner: Send + Sync {
    fn spawn(
        &self,
        command: &str,
        args: &[&str],
        env: &[(String, String)],
        cwd: Option<&Path>,
    ) -> anyhow::Result<Box<dyn ProcessChild>>;
}

#[allow(dead_code)] // 이유: ProcessSpawner port — std_process 어댑터 존재, hook 실행 호출 경로 배선 대기
pub trait ProcessChild: Send {
    fn pid(&self) -> u32;
    fn try_wait(&mut self) -> anyhow::Result<Option<ExitStatus>>;
    fn kill(&mut self) -> anyhow::Result<()>;
}

#[allow(dead_code)]
// 이유: ProcessSpawner port — std_process 어댑터 존재, hook 실행 호출 경로 배선 대기
#[derive(Debug, Clone, Copy)]
pub enum ExitStatus {
    Exited(i32),
    Signaled(i32),
}
