//! 외부 프로세스 실행 인터페이스.

use std::path::Path;

#[allow(dead_code)] // 이유: 실행 어댑터는 있지만 제품 코드의 호출부는 없다
pub trait ProcessSpawner: Send + Sync {
    fn spawn(
        &self,
        command: &str,
        args: &[&str],
        env: &[(String, String)],
        cwd: Option<&Path>,
    ) -> anyhow::Result<Box<dyn ProcessChild>>;
}

#[allow(dead_code)] // 이유: 실행 어댑터는 있지만 제품 코드의 호출부는 없다
pub trait ProcessChild: Send {
    fn pid(&self) -> u32;
    fn try_wait(&mut self) -> anyhow::Result<Option<ExitStatus>>;
    fn kill(&mut self) -> anyhow::Result<()>;
}

// 이유: 어댑터가 결과를 만들지만 제품 코드에는 이 결과를 읽는 호출부가 없다.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum ExitStatus {
    Exited(i32),
    Signaled(i32),
}
