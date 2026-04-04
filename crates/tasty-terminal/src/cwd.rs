use std::path::PathBuf;

/// Get the current working directory of a process by PID.
/// Returns None if the PID is invalid or the cwd cannot be determined.
pub fn get_cwd_of_pid(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{}/cwd", pid)).ok()
    }

    #[cfg(target_os = "macos")]
    {
        macos_proc_cwd(pid)
    }

    #[cfg(windows)]
    {
        windows_proc_cwd(pid)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_proc_cwd(pid: u32) -> Option<PathBuf> {
    use std::process::Command;
    // Use lsof to get the cwd of the process
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

#[cfg(windows)]
fn windows_proc_cwd(pid: u32) -> Option<PathBuf> {
    use std::process::Command;
    // Use PowerShell to query the process's working directory via CIM
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(Get-CimInstance Win32_Process -Filter \"ProcessId={}\" | Select-Object -ExpandProperty ExecutablePath | Split-Path -Parent)",
                pid
            ),
        ])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}
