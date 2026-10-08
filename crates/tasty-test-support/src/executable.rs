//! 시험이 만드는 실행 파일(스텁 도구·가짜 CLI)을 쓴다.
//!
//! 시험 프로세스가 실행 파일을 직접 쓰기로 열면, 그동안 다른 시험 스레드가 fork 한 자식이 그 쓰기
//! fd 를 물려받는다. 그 자식이 exec 해 fd 를 닫기 전까지 커널은 파일을 쓰기 중으로 보고 실행을
//! `ETXTBSY`(셸 종료코드 126)로 거부한다. `O_CLOEXEC` 와 이름 바꾸기로는 막을 수 없어, 쓰기는
//! 자식 셸이 하고 이 프로세스는 파일을 쓰기로 열지 않는다.
//! 근거와 다른 처방: `docs/dev-guide/unit-test-isolation.md` §7 형태 E.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `path` 에 `body` 를 쓰고 실행 권한(0o755)을 준다. 이미 있으면 덮어쓴다.
pub fn write_executable(path: &Path, body: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut writer = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()?;
    let written = match writer.stdin.take() {
        // 다 쓰면 파이프를 닫아 cat 에 EOF 를 준다.
        Some(mut stdin) => stdin.write_all(body.as_ref()),
        None => Err(std::io::Error::other("the writer shell has no stdin")),
    };
    let status = writer.wait()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "writing the executable {} failed: {status}",
            path.display()
        )));
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// 다른 스레드가 프로세스를 계속 띄우는 동안 쓰고 곧바로 실행해도 `ETXTBSY` 가 나지 않는다.
    /// 이 프로세스가 직접 쓰는 방식은 같은 조건에서 200 번에 수십 번 거부된다.
    #[test]
    fn an_executable_written_beside_spawning_threads_runs_without_etxtbsy() {
        let stop = Arc::new(AtomicBool::new(false));
        let spawners: Vec<_> = (0..12)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if let Err(e) = Command::new("/bin/true").status() {
                            panic!("spawner: {e}");
                        }
                    }
                })
            })
            .collect();
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let mut busy = 0;
        for i in 0..200 {
            let bin = dir.path().join(format!("stub{}", i % 4));
            write_executable(&bin, "#!/bin/sh\nexit 0\n").expect("스텁 쓰기");
            match Command::new(&bin).status() {
                Ok(status) => assert!(status.success(), "{status}"),
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => busy += 1,
                Err(e) => panic!("스텁 실행: {e}"),
            }
        }
        stop.store(true, Ordering::Relaxed);
        for s in spawners {
            s.join().expect("spawner");
        }
        assert_eq!(busy, 0, "200 번 중 {busy} 번 ETXTBSY 로 거부됐다");
    }

    #[test]
    fn the_written_file_has_the_body_and_runs() {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let bin = dir.path().join("tool");
        write_executable(&bin, "#!/bin/sh\nexit 7\n").expect("쓰기");
        assert_eq!(
            std::fs::read_to_string(&bin).unwrap(),
            "#!/bin/sh\nexit 7\n"
        );
        assert_eq!(Command::new(&bin).status().unwrap().code(), Some(7));
        // 덮어쓴다.
        write_executable(&bin, "#!/bin/sh\nexit 0\n").expect("덮어쓰기");
        assert!(Command::new(&bin).status().unwrap().success());
        // 쓸 수 없는 경로는 오류다.
        assert!(write_executable(&dir.path().join("no/such/dir/tool"), "x").is_err());
    }
}
