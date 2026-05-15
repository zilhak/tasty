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
        let _pid = pid;
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_proc_cwd(pid: u32) -> Option<PathBuf> {
    // Use proc_pidinfo syscall instead of lsof subprocess.
    // lsof fork+exec takes 10-54ms per call; proc_pidinfo is microseconds.
    //
    // Struct sizes verified with C sizeof():
    //   vinfo_stat=136, vnode_info=152, vnode_info_path=1176, proc_vnodepathinfo=2352
    use std::mem::MaybeUninit;

    const PROC_PIDVNODEPATHINFO: libc::c_int = 9;

    #[repr(C)]
    struct VInfoStat {
        _pad: [u8; 136],
    }

    #[repr(C)]
    struct VnodeInfo {
        _stat: VInfoStat,
        _type: libc::c_int,
        _padding: [u8; 12], // alignment padding to reach 152 bytes total
    }

    #[repr(C)]
    struct VnodeInfoPath {
        vi: VnodeInfo,
        path: [u8; libc::PATH_MAX as usize],
    }

    #[repr(C)]
    struct ProcVnodePathInfo {
        cdir: VnodeInfoPath,
        rdir: VnodeInfoPath,
    }

    // Compile-time size check
    const _: () = assert!(std::mem::size_of::<ProcVnodePathInfo>() == 2352);

    let mut info = MaybeUninit::<ProcVnodePathInfo>::zeroed();
    // SAFETY: proc_pidinfo는 darwin libproc 시스템콜로 thread-safe (Apple 문서).
    // info는 MaybeUninit::zeroed로 alloc, ptr+size를 정확히 sizeof(ProcVnodePathInfo)=2352
    // 만큼 넘긴다 (위 const assert로 컴파일 타임 검증). PTY worker thread에서 호출되어도
    // 동시 호출은 다른 pid 대상이므로 race 없음.
    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr() as *mut libc::c_void,
            std::mem::size_of::<ProcVnodePathInfo>() as libc::c_int,
        )
    };
    if ret <= 0 {
        return None;
    }
    // SAFETY: ret > 0면 proc_pidinfo가 info를 전체 채웠다 (size만큼 write 보장).
    // assume_init은 zeroed 초기화 후 syscall write로 모든 바이트가 valid 상태.
    let info = unsafe { info.assume_init() };
    let path_bytes = &info.cdir.path;
    let len = path_bytes.iter().position(|&b| b == 0).unwrap_or(path_bytes.len());
    if len == 0 {
        return None;
    }
    let path_str = std::str::from_utf8(&path_bytes[..len]).ok()?;
    Some(PathBuf::from(path_str))
}
