//! Process tree traversal — collect descendant PIDs of a root process.

use std::collections::{HashMap, HashSet, VecDeque};

/// Returns `root` and all transitive descendants, deduplicated.
///
/// On enumeration error returns just `{root}` (best-effort).
pub fn collect_descendant_pids(root: u32) -> HashSet<u32> {
    let mut out = HashSet::new();
    out.insert(root);

    let table = match build_ppid_table() {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("portscan tree: failed to build ppid table: {e}");
            return out;
        }
    };

    // BFS through the table.
    let mut queue: VecDeque<u32> = VecDeque::new();
    queue.push_back(root);
    while let Some(parent) = queue.pop_front() {
        if let Some(children) = table.get(&parent) {
            for &child in children {
                if out.insert(child) {
                    queue.push_back(child);
                }
            }
        }
    }
    out
}

/// Map of parent PID → list of child PIDs.
type PpidTable = HashMap<u32, Vec<u32>>;

#[cfg(target_os = "linux")]
fn build_ppid_table() -> std::io::Result<PpidTable> {
    let mut table: PpidTable = HashMap::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        let pid: u32 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        // /proc/<pid>/stat — field 4 is ppid (after `comm`).
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let after_comm = match stat.rfind(')') {
            Some(idx) if idx + 2 < stat.len() => idx + 2,
            _ => continue,
        };
        let fields: Vec<&str> = stat[after_comm..].split_whitespace().collect();
        // fields: [0]=state [1]=ppid
        let ppid: u32 = match fields.get(1).and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        table.entry(ppid).or_default().push(pid);
    }
    Ok(table)
}

#[cfg(target_os = "macos")]
fn build_ppid_table() -> std::io::Result<PpidTable> {
    // `ps -A -o pid=,ppid=` is portable and avoids a libproc binding.
    use std::process::Command;
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid="])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("ps exited with {}", output.status),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut table: PpidTable = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let pid: u32 = match parts.next().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        let ppid: u32 = match parts.next().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        table.entry(ppid).or_default().push(pid);
    }
    Ok(table)
}

#[cfg(windows)]
fn build_ppid_table() -> std::io::Result<PpidTable> {
    use std::mem::MaybeUninit;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut table: PpidTable = HashMap::new();

    // SAFETY: CreateToolhelp32Snapshot returns INVALID_HANDLE_VALUE (-1) on error;
    // we check before use.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot as isize == -1 {
        return Err(std::io::Error::last_os_error());
    }

    let mut entry: MaybeUninit<PROCESSENTRY32W> = MaybeUninit::zeroed();
    // SAFETY: write only the dwSize field of the zeroed struct, as required by Process32FirstW.
    unsafe {
        let ptr = entry.as_mut_ptr();
        (*ptr).dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    }

    // SAFETY: snapshot is valid; entry is zero-initialised except dwSize which Win32 requires.
    let first = unsafe { Process32FirstW(snapshot, entry.as_mut_ptr()) };
    if first != 0 {
        loop {
            // SAFETY: Process32FirstW/NextW returned nonzero, so entry is fully initialised.
            let e = unsafe { entry.assume_init_ref() };
            table
                .entry(e.th32ParentProcessID)
                .or_default()
                .push(e.th32ProcessID);
            // SAFETY: snapshot and entry are valid; Process32NextW returns 0 when no more entries.
            let more = unsafe { Process32NextW(snapshot, entry.as_mut_ptr()) };
            if more == 0 {
                break;
            }
        }
    }

    // SAFETY: snapshot was successfully created and not yet closed.
    unsafe { CloseHandle(snapshot) };
    Ok(table)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn build_ppid_table() -> std::io::Result<PpidTable> {
    Ok(HashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_always_included() {
        // No reasonable assertion about descendants without a fixture, but the
        // root itself must be present even when the table build fails.
        let pids = collect_descendant_pids(u32::MAX);
        assert!(pids.contains(&u32::MAX));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn self_pid_has_known_ancestor() {
        let me = std::process::id();
        let table = build_ppid_table().expect("ppid table");
        let mut found_self = false;
        for children in table.values() {
            if children.contains(&me) {
                found_self = true;
                break;
            }
        }
        assert!(
            found_self,
            "ps/proc must list our own pid as a child of some parent"
        );
    }
}
