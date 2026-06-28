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

/// Resolve the foreground process for many shell PIDs at once.
///
/// On Windows the per-PID lookup snapshots *every* system process
/// (`CreateToolhelp32Snapshot`, ≈ several ms with a few hundred processes).
/// Calling [`get_foreground_process`] once per surface therefore put
/// `O(surfaces × processes)` on the main thread every busy-poll tick. This
/// batch path takes **one** snapshot and resolves all PIDs against it.
///
/// On Linux/macOS the per-PID lookup is already a single `/proc` read or
/// libproc syscall, so this just maps [`get_foreground_process`] over the slice.
///
/// The returned vector is index-aligned with `shell_pids`.
pub fn resolve_foreground_many(shell_pids: &[u32]) -> Vec<Option<ForegroundProcessInfo>> {
    #[cfg(windows)]
    {
        let snapshot = WindowsProcessSnapshot::capture();
        shell_pids
            .iter()
            .map(|&pid| snapshot.as_ref().and_then(|s| s.resolve(pid)))
            .collect()
    }

    #[cfg(not(windows))]
    {
        shell_pids
            .iter()
            .map(|&pid| get_foreground_process(pid))
            .collect()
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
    // Use the proc_pidinfo libproc syscall instead of forking `ps`. The old path
    // forked `ps` twice per call (~ms each in posix_spawn); at 1Hz over every live
    // surface that blocked the main thread enough to stall workspace switching.
    // proc_bsdinfo carries both the controlling-terminal foreground pgid (e_tpgid)
    // and the process name, so two syscalls (µs each) replace the two forks.
    // Same approach as cwd.rs (lsof fork -> proc_pidinfo).
    let bsd = macos_proc_bsdinfo(shell_pid)?;
    let tpgid = bsd.e_tpgid;
    if tpgid == 0 {
        return None;
    }
    // Name of the foreground process-group leader (PID == PGID for the leader).
    let leader = macos_proc_bsdinfo(tpgid)?;
    let name = cstr_field_to_string(&leader.pbi_name)
        .or_else(|| cstr_field_to_string(&leader.pbi_comm))?;
    if name.is_empty() {
        return None;
    }
    Some(ForegroundProcessInfo { name, pid: tpgid })
}

/// Query `proc_bsdinfo` for `pid` via the libproc `proc_pidinfo` syscall.
#[cfg(target_os = "macos")]
fn macos_proc_bsdinfo(pid: u32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: proc_bsdinfo is a plain-old-data C struct; an all-zero bit pattern is
    // a valid initial value (every field is an integer/array). proc_pidinfo overwrites it.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: proc_pidinfo is the darwin libproc syscall, thread-safe (Apple docs).
    // PROC_PIDTBSDINFO fills a proc_bsdinfo of exactly `size` bytes. Same pattern as
    // cwd.rs::macos_proc_cwd. A full write returns `size`; anything else is a miss.
    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    (ret == size).then_some(info)
}

/// Read a NUL-terminated C-char field (e.g. `pbi_name`) into an owned `String`.
#[cfg(target_os = "macos")]
fn cstr_field_to_string(buf: &[libc::c_char]) -> Option<String> {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    if bytes.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(windows)]
fn windows_foreground_process(shell_pid: u32) -> Option<ForegroundProcessInfo> {
    WindowsProcessSnapshot::capture()?.resolve(shell_pid)
}

/// One system-wide process snapshot, reusable for resolving the foreground
/// program of many shell PIDs in a single tick.
///
/// Windows has no Unix-style `tcgetpgrp`; ConPTY does not expose a foreground
/// process group, so each shell's foreground must be found by walking the
/// process tree. The expensive part — `CreateToolhelp32Snapshot` plus building
/// the parent→child map — is identical for every shell in one poll, so it is
/// captured once here and reused by [`resolve`](Self::resolve).
#[cfg(windows)]
struct WindowsProcessSnapshot {
    /// `(pid, parent_pid, name)` for every process in the system.
    entries: Vec<(u32, u32, String)>,
    /// `parent_pid` → indices into `entries`.
    children: std::collections::HashMap<u32, Vec<usize>>,
}

#[cfg(windows)]
impl WindowsProcessSnapshot {
    /// Take a single `TH32CS_SNAPPROCESS` snapshot and build the child map.
    /// Returns `None` if the snapshot could not be created.
    fn capture() -> Option<Self> {
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
        // 순차 호출하는 표준 패턴. 모든 호출은 단일 흐름으로 실행되며 snapshot
        // HANDLE은 HandleGuard로 Drop-시 close된다. PROCESSENTRY32W는 dwSize를
        // 명시한 뒤 zeroed로 초기화 — Win32 API 요구사항. 반환된 entry.szExeFile은
        // 함수 호출 후 entry가 살아있는 동안만 valid.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let _guard = HandleGuard(snapshot);

            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..std::mem::zeroed()
            };

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

            let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
            for (i, (_, ppid, _)) in entries.iter().enumerate() {
                children.entry(*ppid).or_default().push(i);
            }

            Some(Self { entries, children })
        }
    }

    /// Resolve the foreground program for `shell_pid`: the **shallowest
    /// non-shell descendant** of the shell, breaking ties at the same depth by
    /// lowest PID for a deterministic, stable choice.
    ///
    /// Windows has no Unix-style `tcgetpgrp` and ConPTY exposes no foreground
    /// process group, so this is a heuristic approximation of the tty foreground
    /// program, not the real thing. Interactive agents (claude/node) constantly
    /// spawn and reap short-lived helpers (bash/git/rg/…); picking the *deepest*
    /// leaf made the displayed name flicker frame-to-frame as whichever transient
    /// helper happened to be alive got chosen. The shallowest non-shell
    /// descendant is the outermost user-launched program (e.g. `node`) — stable,
    /// and what the user perceives as running.
    ///
    /// Nested shells are walked through but never chosen (`pwsh → cmd → node`
    /// resolves to `node`). Fallbacks, in order: if the shell has no non-shell
    /// descendant, return the deepest leaf (an inner idle shell still shows a
    /// name); if there is no descendant at all, return the shell's own info —
    /// matching Linux/macOS where tpgid resolves to the shell itself when idle.
    fn resolve(&self, shell_pid: u32) -> Option<ForegroundProcessInfo> {
        use std::collections::VecDeque;
        // BFS level-by-level from shell_pid. The explicit (depth, pid) comparison
        // below makes the choice independent of `children` iteration order, so
        // the result is deterministic even with multiple same-depth candidates.
        let mut queue: VecDeque<(u32, u32)> = VecDeque::new(); // (pid, depth)
        queue.push_back((shell_pid, 0));
        // (pid, name, depth) of the shallowest non-shell descendant so far.
        let mut shallowest_non_shell: Option<(u32, String, u32)> = None;
        // Fallback: deepest leaf, used only when no non-shell descendant exists.
        let mut best_leaf: Option<(u32, String, u32)> = None; // (pid, name, depth)
        let mut found_descendant = false;

        while let Some((pid, depth)) = queue.pop_front() {
            let Some(child_idxs) = self.children.get(&pid) else {
                continue;
            };
            for &idx in child_idxs {
                let (cpid, _, cname) = &self.entries[idx];
                found_descendant = true;
                let cdepth = depth + 1;
                if !is_known_shell_name(cname) {
                    // Shallowest wins; ties at the same depth → lowest PID.
                    let better = shallowest_non_shell
                        .as_ref()
                        .is_none_or(|(bpid, _, bd)| cdepth < *bd || (cdepth == *bd && cpid < bpid));
                    if better {
                        shallowest_non_shell = Some((*cpid, cname.clone(), cdepth));
                    }
                }
                // Track the deepest leaf in parallel for the no-non-shell fallback.
                let has_children = self.children.get(cpid).is_some_and(|v| !v.is_empty());
                if has_children {
                    queue.push_back((*cpid, cdepth));
                } else if best_leaf.as_ref().is_none_or(|(_, _, d)| cdepth > *d) {
                    best_leaf = Some((*cpid, cname.clone(), cdepth));
                }
            }
        }

        let (pid, raw_name) = if let Some((pid, name, _)) = shallowest_non_shell {
            (pid, name)
        } else if found_descendant {
            // Only nested/leaf shells under this shell — show the deepest one.
            let (pid, name, _) = best_leaf?;
            (pid, name)
        } else {
            // Shell is its own foreground process — return its name for parity
            // with Linux/macOS where tpgid resolves to the shell when idle.
            let (pid, _, name) = self.entries.iter().find(|(p, _, _)| *p == shell_pid)?;
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

#[cfg(all(test, windows))]
mod windows_tree {
    //! Synthetic process-tree tests for [`WindowsProcessSnapshot::resolve`].
    //! These build the snapshot's `entries`/`children` directly (same module,
    //! so the private fields are reachable) and make no OS calls — fully
    //! deterministic under headless CI.
    use super::*;
    use std::collections::HashMap;

    /// Build a snapshot from `(pid, parent_pid, name)` triples.
    fn snapshot(rows: &[(u32, u32, &str)]) -> WindowsProcessSnapshot {
        let entries: Vec<(u32, u32, String)> = rows
            .iter()
            .map(|(pid, ppid, name)| (*pid, *ppid, name.to_string()))
            .collect();
        let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, (_, ppid, _)) in entries.iter().enumerate() {
            children.entry(*ppid).or_default().push(i);
        }
        WindowsProcessSnapshot { entries, children }
    }

    // shell(100) → node(200) → {bash(300), git(301), rg(302)}.
    // node is the shallowest non-shell descendant; the deep transient helpers
    // (which used to win as the deepest leaf and caused the flicker) are ignored.
    #[test]
    fn picks_shallowest_non_shell_over_deep_helpers() {
        let snap = snapshot(&[
            (100, 1, "bash.exe"),
            (200, 100, "node.exe"),
            (300, 200, "bash.exe"),
            (301, 200, "git.exe"),
            (302, 200, "rg.exe"),
        ]);
        let info = snap.resolve(100).expect("resolve");
        assert_eq!(info.name, "node");
        assert_eq!(info.pid, 200);
    }

    // pwsh(100) → cmd(200, a shell) → node(300): nested shell is walked through
    // but never chosen; node is selected.
    #[test]
    fn walks_through_nested_shell() {
        let snap = snapshot(&[
            (100, 1, "pwsh.exe"),
            (200, 100, "cmd.exe"),
            (300, 200, "node.exe"),
        ]);
        let info = snap.resolve(100).expect("resolve");
        assert_eq!(info.name, "node");
        assert_eq!(info.pid, 300);
    }

    // shell(100) with no descendants → fall back to the shell itself.
    #[test]
    fn falls_back_to_shell_when_no_descendant() {
        let snap = snapshot(&[(100, 1, "bash.exe")]);
        let info = snap.resolve(100).expect("resolve");
        assert_eq!(info.name, "bash");
        assert_eq!(info.pid, 100);
    }

    // Two non-shell children at the same (shallowest) depth → lowest PID wins.
    // node is listed after cargo on purpose so insertion order can't pass by luck.
    #[test]
    fn ties_break_by_lowest_pid() {
        let snap = snapshot(&[
            (100, 1, "bash.exe"),
            (301, 100, "cargo.exe"),
            (300, 100, "node.exe"),
        ]);
        let info = snap.resolve(100).expect("resolve");
        assert_eq!(info.pid, 300);
        assert_eq!(info.name, "node");
    }
}
