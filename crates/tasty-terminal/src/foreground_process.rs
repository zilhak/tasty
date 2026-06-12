//! Foreground process detection for PTY child processes.
//!
//! Returns (process_name, pid) of the foreground process group leader
//! for the controlling terminal of the given shell PID.

/// Info about the foreground process.
#[derive(Debug, Clone)]
pub struct ForegroundProcessInfo {
    pub name: String,
    pub pid: u32,
}

/// Whether `name` matches a recognised interactive shell. Used to classify a
/// terminal as idle when its foreground process is the shell itself rather
/// than a user-launched program.
pub fn is_known_shell_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    matches!(
        stem,
        "bash"
            | "zsh"
            | "fish"
            | "sh"
            | "dash"
            | "ksh"
            | "tcsh"
            | "csh"
            | "nu"
            | "xonsh"
            | "elvish"
            | "pwsh"
            | "powershell"
            | "cmd"
    )
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
        windows_foreground_process(shell_pid)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _shell_pid = shell_pid;
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

#[cfg(windows)]
fn windows_foreground_process(shell_pid: u32) -> Option<ForegroundProcessInfo> {
    // Windows has no Unix-style tcgetpgrp; ConPTY does not expose a foreground
    // process group. Walk the process tree and return the deepest leaf descendant
    // of the shell. If the shell has no descendants, return the shell's own info
    // so the caller still receives a name (matching Linux/macOS semantics where
    // tpgid points at the shell itself when idle).

    use std::collections::HashMap;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            // SAFETY: CloseHandle on a HANDLE obtained from
            // CreateToolhelp32Snapshot is the documented release path.
            // self.0이 INVALID_HANDLE_VALUE이거나 이미 닫혔어도 CloseHandle은
            // Err만 반환하고 UB를 일으키지 않는다 (Win32 보장).
            unsafe {
                // CloseHandle은 invalid handle에 대해 ERROR_INVALID_HANDLE을 반환할 뿐
                // UB를 일으키지 않는다. Drop 경로에서 추가로 로그를 남길 가치 없음.
                if let Err(e) = CloseHandle(self.0) {
                    tracing::trace!("ToolHelp snapshot CloseHandle: {e}");
                }
            }
        }
    }

    // SAFETY: 본 블록은 ToolHelp snapshot 위에서 Process32FirstW/NextW를
    // 순차 호출하는 표준 패턴. 모든 호출은 PTY worker thread에서 단일
    // 흐름으로 실행되며 snapshot HANDLE은 HandleGuard로 Drop-시 close된다.
    // PROCESSENTRY32W는 dwSize를 명시한 뒤 zeroed로 초기화 — Win32 API 요구사항.
    // 반환된 entry.szExeFile은 함수 호출 후 entry가 살아있는 동안만 valid.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let _guard = HandleGuard(snapshot);

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..std::mem::zeroed()
        };

        // (pid, parent_pid, name)
        let mut entries: Vec<(u32, u32, String)> = Vec::new();
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name_len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
                entries.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        // parent_pid -> Vec<entry index>
        let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, (_, ppid, _)) in entries.iter().enumerate() {
            children.entry(*ppid).or_default().push(i);
        }

        // DFS from shell_pid; track the deepest leaf descendant.
        let mut stack: Vec<(u32, u32)> = vec![(shell_pid, 0)];
        let mut best_leaf: Option<(u32, String, u32)> = None; // (pid, name, depth)
        let mut found_descendant = false;

        while let Some((pid, depth)) = stack.pop() {
            let Some(child_idxs) = children.get(&pid) else {
                continue;
            };
            for &idx in child_idxs {
                let (cpid, _, cname) = &entries[idx];
                found_descendant = true;
                let has_grandchildren = children.get(cpid).map_or(false, |v| !v.is_empty());
                if has_grandchildren {
                    stack.push((*cpid, depth + 1));
                } else {
                    let cdepth = depth + 1;
                    if best_leaf.as_ref().is_none_or(|(_, _, d)| cdepth > *d) {
                        best_leaf = Some((*cpid, cname.clone(), cdepth));
                    }
                }
            }
        }

        let (pid, raw_name) = if found_descendant {
            let (pid, name, _) = best_leaf?;
            (pid, name)
        } else {
            // Shell is its own foreground process — return its name for parity
            // with Linux/macOS where tpgid resolves to the shell when idle.
            let (pid, _, name) = entries.iter().find(|(p, _, _)| *p == shell_pid)?;
            (*pid, name.clone())
        };

        let name = raw_name
            .strip_suffix(".exe")
            .or_else(|| raw_name.strip_suffix(".EXE"))
            .unwrap_or(&raw_name)
            .to_string();

        if name.is_empty() {
            return None;
        }
        Some(ForegroundProcessInfo { name, pid })
    }
}

#[cfg(all(test, windows))]
mod windows_smoke {
    use super::*;

    // Manual smoke test — environment dependent. Run with:
    //   cargo test -p tasty-terminal --lib windows_smoke -- --nocapture --ignored
    #[test]
    #[ignore]
    fn current_process_resolves() {
        let pid = std::process::id();
        let info = get_foreground_process(pid);
        eprintln!(
            "foreground for self pid {pid}: {:?}",
            info.map(|i| (i.name, i.pid))
        );
    }
}
