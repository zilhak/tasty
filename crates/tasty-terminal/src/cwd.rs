use std::path::PathBuf;

/// Get the current working directory of a process by PID.
/// Returns None if the PID is invalid or the cwd cannot be determined.
///
/// Windows에서는 항상 None을 반환한다. 다른 프로세스의 CWD를 얻는 표준 API가
/// 없고, WMI/PowerShell 호출은 콘솔창을 띄우는 무거운 동작이라 폴링에 부적합하다.
/// Windows에서는 OSC 7 시퀀스에만 의존한다 (git bash, PowerShell 7+ 등은 송신함).
pub fn get_cwd_of_pid(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{}/cwd", pid)).ok()
    }

    #[cfg(target_os = "macos")]
    {
        macos_proc_cwd(pid)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_proc_cwd(pid: u32) -> Option<PathBuf> {
    use std::process::Command;
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string(), "-Fn", "-a", "-d", "cwd"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix('n') {
            return Some(PathBuf::from(path));
        }
    }
    None
}
