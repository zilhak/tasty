//! 실행 파일과 그 런처가 사용할 PATH를 호출 시점에 고정한다. 프로세스 전역 환경은 바꾸지 않는다.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    pub executable: PathBuf,
    pub search_path: OsString,
}

impl ExecutionContext {
    /// 현재 환경에서 찾는다. 셸 alias나 function을 실행하지 않는다.
    pub fn capture(name: &str) -> io::Result<Self> {
        let path = std::env::var_os("PATH")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "PATH is not set"))?;
        Self::resolve(name, &path, &std::env::current_dir()?)
    }

    /// 상대 PATH 항목은 원래 cwd 기준으로 고정한다. symlink는 유지하여 설치 관리자의 경로를 쓴다.
    pub fn resolve(name: &str, path: &OsStr, cwd: &Path) -> io::Result<Self> {
        let dirs: Vec<_> = std::env::split_paths(path)
            .map(|p| if p.is_absolute() { p } else { cwd.join(p) })
            .collect();
        let search_path = std::env::join_paths(&dirs)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        #[cfg(windows)]
        let names = [
            format!("{name}.exe"),
            format!("{name}.cmd"),
            format!("{name}.bat"),
            format!("{name}.com"),
        ];
        #[cfg(not(windows))]
        let names = [name.to_owned()];
        for dir in dirs {
            for name in &names {
                let executable = dir.join(name);
                if executable_file(&executable) {
                    return Self::from_parts(executable, search_path);
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{name} executable was not found in the execution PATH"),
        ))
    }

    /// IPC·저장 기록을 복원한다. 상대 경로를 plugin cwd로 다시 해석하지 않는다.
    pub fn from_parts(executable: PathBuf, search_path: OsString) -> io::Result<Self> {
        if !executable.is_absolute()
            || search_path.is_empty()
            || std::env::split_paths(&search_path).any(|p| !p.is_absolute())
            || executable.to_string_lossy().contains('\0')
            || search_path.to_string_lossy().contains('\0')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "execution context requires absolute executable and PATH entries without NUL",
            ));
        }
        Ok(Self {
            executable,
            search_path,
        })
    }

    /// PATH는 이 자식에만 적용한다. npm의 /usr/bin/env node 런처도 같은 환경에서 해석된다.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.env("PATH", &self.search_path);
        command
    }
}

fn executable_file(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests;
