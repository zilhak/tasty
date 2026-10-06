//! 홈의 인스턴스 파일 `tasty.instance`. 잠금을 얻은 인스턴스가 PID·프로세스 시작 시각·IPC 포트를 적는다.
//!
//! 두 번째 프로세스가 실행 중인 인스턴스를 찾을 때 읽는다. 잠금을 가졌다고 파일이 최신인 것은 아니다.
//! 이전 소유자가 비정상 종료하면 파일이 남으므로, 읽는 쪽은 PID가 살아 있고 그 프로세스의 시작
//! 시각이 파일 값과 같은지 확인한다(PID 재사용도 이 검증으로 걸러진다). 쓰기는 임시 파일 + rename이다.
//! 잠금 직후에는 포트가 없고, IPC 서버가 열린 뒤 포트를 넣어 다시 쓴다. Windows는 잠긴 파일을 다른
//! 프로세스가 읽을 수 없으므로 잠금 파일과 따로 둔다.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "tasty.instance";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstanceRecord {
    pub(crate) pid: u32,
    /// OS가 보고한 프로세스 시작 시각. 단위는 OS마다 다르며 같은 OS 안에서 비교만 한다.
    pub(crate) start_time: u64,
    /// IPC 포트. 아직 IPC 서버를 열지 않았으면 없다.
    pub(crate) port: Option<u16>,
}

/// 두 번째 프로세스가 본 실행 중 인스턴스의 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstanceState {
    /// 파일이 있고 PID·시작 시각이 맞으며 포트도 있다.
    Live(InstanceRecord),
    /// 아직 부팅 중이다. 파일이 없거나, 포트가 없거나, 낡은 기록이다(새 소유자가 아직 쓰지 않았다).
    Booting(BootingReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BootingReason {
    NoFile,
    NoPort(InstanceRecord),
    /// 기록한 PID가 없거나 시작 시각이 다르다.
    Stale {
        record: InstanceRecord,
        observed_start: Option<u64>,
    },
    Unreadable(String),
}

pub(crate) fn path(home: &Path) -> PathBuf {
    home.join(FILE_NAME)
}

/// 읽은 기록과 그 PID의 실제 시작 시각으로 상태를 정한다.
pub(crate) fn classify(record: InstanceRecord, observed_start: Option<u64>) -> InstanceState {
    if observed_start != Some(record.start_time) {
        return InstanceState::Booting(BootingReason::Stale {
            record,
            observed_start,
        });
    }
    if record.port.is_none() {
        return InstanceState::Booting(BootingReason::NoPort(record));
    }
    InstanceState::Live(record)
}

pub(crate) fn read_state(home: &Path) -> InstanceState {
    let bytes = match std::fs::read(path(home)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return InstanceState::Booting(BootingReason::NoFile);
        }
        Err(e) => return InstanceState::Booting(BootingReason::Unreadable(e.to_string())),
    };
    match serde_json::from_slice::<InstanceRecord>(&bytes) {
        Ok(record) => classify(record, process_start_time(record.pid)),
        Err(e) => InstanceState::Booting(BootingReason::Unreadable(e.to_string())),
    }
}

fn write(home: &Path, record: &InstanceRecord) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    let mut temporary = tempfile::NamedTempFile::new_in(home)?;
    std::io::Write::write_all(&mut temporary, &bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path(home)).map_err(|e| e.error)?;
    Ok(())
}

/// 이 프로세스가 쓴 기록. 포트를 넣어 다시 쓰고 종료할 때 지우는 데 쓴다.
static OWNED: Mutex<Option<(PathBuf, InstanceRecord)>> = Mutex::new(None);

fn with_owned(f: impl FnOnce(&mut Option<(PathBuf, InstanceRecord)>)) {
    match OWNED.lock() {
        Ok(mut owned) => f(&mut owned),
        Err(e) => tracing::warn!("instance file state poisoned: {e}"),
    }
}

/// 잠금을 얻은 직후 포트 없는 기록을 쓴다. 실패는 로그만 남긴다. 두 번째 프로세스는 부팅 중으로 보고 기다린다.
pub(crate) fn publish_started(home: &Path) {
    let pid = std::process::id();
    let Some(start_time) = process_start_time(pid) else {
        tracing::warn!("instance file: cannot read this process start time; not publishing");
        return;
    };
    let record = InstanceRecord {
        pid,
        start_time,
        port: None,
    };
    if let Err(e) = write(home, &record) {
        tracing::warn!("instance file: write {} failed: {e}", path(home).display());
        return;
    }
    with_owned(|owned| *owned = Some((home.to_path_buf(), record)));
}

/// IPC 서버가 열린 뒤 포트를 넣어 다시 쓴다. 먼저 쓴 기록이 없으면 아무것도 하지 않는다.
pub(crate) fn publish_port(port: u16) {
    with_owned(|owned| {
        let Some((home, record)) = owned.as_mut() else {
            return;
        };
        record.port = Some(port);
        if let Err(e) = write(home, record) {
            tracing::warn!("instance file: port update failed: {e}");
        }
    });
}

/// 정상 종료할 때 저널 잠금을 놓기 전에 지운다.
pub(crate) fn remove_owned() {
    with_owned(|owned| {
        if let Some((home, _)) = owned.take()
            && let Err(e) = std::fs::remove_file(path(&home))
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("instance file: remove failed: {e}");
        }
    });
}

/// PID의 프로세스 시작 시각. 프로세스가 없거나 읽을 수 없으면 None.
pub(crate) fn process_start_time(pid: u32) -> Option<u64> {
    imp::process_start_time(pid)
}

#[cfg(target_os = "linux")]
mod imp {
    /// `/proc/<pid>/stat`의 22번째 필드(부팅 이후 clock tick).
    pub(super) fn process_start_time(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        parse_start_time(&stat)
    }

    /// 2번째 필드(comm)는 공백·괄호를 담을 수 있으므로 마지막 `)` 뒤부터 센다.
    pub(super) fn parse_start_time(stat: &str) -> Option<u64> {
        let (_, rest) = stat.rsplit_once(')')?;
        rest.split_whitespace().nth(19)?.parse().ok()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    /// `proc_pidinfo(PROC_PIDTBSDINFO)`의 시작 시각(마이크로초).
    pub(super) fn process_start_time(pid: u32) -> Option<u64> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: info는 proc_bsdinfo 크기의 쓰기 가능한 버퍼이고 size가 그 크기다.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (written == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// `GetProcessTimes`의 생성 시각(100ns FILETIME).
    pub(super) fn process_start_time(pid: u32) -> Option<u64> {
        // SAFETY: 반환한 핸들은 아래에서 닫고, FILETIME 출력 버퍼는 지역 변수다.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            let result = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
            if let Err(e) = CloseHandle(handle) {
                tracing::debug!("CloseHandle after GetProcessTimes failed: {e}");
            }
            result.ok()?;
            Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub(super) fn process_start_time(_pid: u32) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(port: Option<u16>) -> InstanceRecord {
        InstanceRecord {
            pid: 10,
            start_time: 500,
            port,
        }
    }

    #[test]
    fn a_record_without_a_port_is_still_booting() {
        assert_eq!(
            classify(record(None), Some(500)),
            InstanceState::Booting(BootingReason::NoPort(record(None)))
        );
    }

    #[test]
    fn a_matching_pid_and_start_time_with_a_port_is_live() {
        assert_eq!(
            classify(record(Some(4000)), Some(500)),
            InstanceState::Live(record(Some(4000)))
        );
    }

    #[test]
    fn a_dead_or_reused_pid_is_stale() {
        for observed in [None, Some(501)] {
            assert!(matches!(
                classify(record(Some(4000)), observed),
                InstanceState::Booting(BootingReason::Stale { .. })
            ));
        }
    }

    #[test]
    fn this_process_has_a_start_time_and_its_record_reads_back_live() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            read_state(home.path()),
            InstanceState::Booting(BootingReason::NoFile)
        );
        let pid = std::process::id();
        let start_time = process_start_time(pid).expect("own start time");
        let live = InstanceRecord {
            pid,
            start_time,
            port: Some(1234),
        };
        write(home.path(), &live).unwrap();
        assert_eq!(read_state(home.path()), InstanceState::Live(live));
        std::fs::write(path(home.path()), b"not json").unwrap();
        assert!(matches!(
            read_state(home.path()),
            InstanceState::Booting(BootingReason::Unreadable(_))
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_start_time_skips_a_command_name_with_spaces_and_parens() {
        let mut fields = vec!["S".to_string()];
        fields.extend((4..=21).map(|n| n.to_string()));
        fields.push("987654".into());
        fields.push("0".into());
        let stat = format!("42 (we (ird) name) {}", fields.join(" "));
        assert_eq!(imp::parse_start_time(&stat), Some(987654));
    }
}
