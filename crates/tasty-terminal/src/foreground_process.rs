/// Foreground process detection for PTY child processes.
///
/// Returns (process_name, pid) of the foreground process group leader
/// for the controlling terminal of the given shell PID.

/// Info about the foreground process.
pub struct ForegroundProcessInfo {
    pub name: String,
    pub pid: u32,
}

/// Get the foreground process info for a given shell PID.
pub fn get_foreground_process(shell_pid: u32) -> Option<ForegroundProcessInfo> {
    #[cfg(target_os = "linux")]
    {
        linux_foreground_process(shell_pid)
    }

    #[cfg(target_os = "macos")]
    {
        macos_foreground_process(shell_pid)
    }

    #[cfg(windows)]
    {
        let _ = shell_pid;
        None
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = shell_pid;
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_foreground_process(shell_pid: u32) -> Option<ForegroundProcessInfo> {
    // Read /proc/<pid>/stat to get tpgid (foreground process group ID)
    // Format: pid (comm) state ppid pgrp session tty_nr tpgid ...
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", shell_pid)).ok()?;
    let after_comm = stat.rfind(')')? + 2;
    let fields: Vec<&str> = stat[after_comm..].split_whitespace().collect();
    // fields: [0]=state [1]=ppid [2]=pgrp [3]=session [4]=tty_nr [5]=tpgid
    let tpgid: u32 = fields.get(5)?.parse().ok()?;
    if tpgid == 0 {
        return None;
    }
    // Get process name from /proc/<tpgid>/comm
    let name = std::fs::read_to_string(format!("/proc/{}/comm", tpgid))
        .ok()?
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    Some(ForegroundProcessInfo { name, pid: tpgid })
}

#[cfg(target_os = "macos")]
fn macos_foreground_process(shell_pid: u32) -> Option<ForegroundProcessInfo> {
    use std::process::Command;
    // Get tpgid (foreground process group ID) for the shell's terminal
    let output = Command::new("ps")
        .args(["-o", "tpgid=", "-p", &shell_pid.to_string()])
        .output()
        .ok()?;
    let tpgid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let tpgid: u32 = tpgid_str.parse().ok()?;
    if tpgid == 0 {
        return None;
    }
    // Get the process name of the PGID leader (PID == PGID for the leader)
    let output = Command::new("ps")
        .args(["-o", "comm=", "-p", &tpgid.to_string()])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        return None;
    }
    // Extract binary name from full path (e.g., /usr/bin/zsh -> zsh)
    let name = name.rsplit('/').next().unwrap_or(&name).to_string();
    Some(ForegroundProcessInfo { name, pid: tpgid })
}
