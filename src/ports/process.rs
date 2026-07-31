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

// 이유(TODO 04 재확인): enum 자체는 아니다 — std_process.rs/mock_process.rs 가 실제로
// construct 하지만, ProcessSpawner 소비 경로(`self.process.*`)가 아직 없어 그 결과를
// match/read 하는 곳이 없다. `Exited(i32)`/`Signaled(i32)` 의 payload 필드가
// "never read" 로 걸린다(타입 자체가 아니라 필드 단위 dead_code).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum ExitStatus {
    Exited(i32),
    Signaled(i32),
}
