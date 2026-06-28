//! Windows port enumeration via `GetExtendedTcpTable`.

use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use std::collections::HashMap;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP_STATE_CLOSE_WAIT, MIB_TCP_STATE_CLOSED, MIB_TCP_STATE_CLOSING,
    MIB_TCP_STATE_ESTAB, MIB_TCP_STATE_FIN_WAIT1, MIB_TCP_STATE_FIN_WAIT2, MIB_TCP_STATE_LAST_ACK,
    MIB_TCP_STATE_LISTEN, MIB_TCP_STATE_SYN_RCVD, MIB_TCP_STATE_SYN_SENT, MIB_TCP_STATE_TIME_WAIT,
    MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
    TCP_TABLE_OWNER_PID_ALL,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use crate::{ListeningPort, PortState, SystemListeningPort};

/// Map a Windows `MIB_TCP_STATE` (`dwState`) value to [`PortState`].
fn state_from_windows(dw_state: u32) -> PortState {
    match dw_state as i32 {
        MIB_TCP_STATE_LISTEN => PortState::Listen,
        MIB_TCP_STATE_ESTAB => PortState::Established,
        MIB_TCP_STATE_SYN_SENT => PortState::SynSent,
        MIB_TCP_STATE_SYN_RCVD => PortState::SynRecv,
        MIB_TCP_STATE_FIN_WAIT1 => PortState::FinWait1,
        MIB_TCP_STATE_FIN_WAIT2 => PortState::FinWait2,
        MIB_TCP_STATE_TIME_WAIT => PortState::TimeWait,
        MIB_TCP_STATE_CLOSED => PortState::Closed,
        MIB_TCP_STATE_CLOSE_WAIT => PortState::CloseWait,
        MIB_TCP_STATE_LAST_ACK => PortState::LastAck,
        MIB_TCP_STATE_CLOSING => PortState::Closing,
        _ => PortState::Unknown,
    }
}

pub fn scan(pids: &HashSet<u32>) -> io::Result<Vec<ListeningPort>> {
    let mut out = Vec::new();
    let mut name_cache: HashMap<u32, Option<String>> = HashMap::new();
    collect_v4(pids, &mut out, &mut name_cache)?;
    collect_v6(pids, &mut out, &mut name_cache)?;
    Ok(out)
}

/// System-wide scan: no pid filter.
pub fn scan_all() -> io::Result<Vec<SystemListeningPort>> {
    let mut out = Vec::new();
    let mut name_cache: HashMap<u32, Option<String>> = HashMap::new();
    collect_v4_all(&mut out, &mut name_cache)?;
    collect_v6_all(&mut out, &mut name_cache)?;
    Ok(out)
}

fn process_name_for(pid: u32, cache: &mut HashMap<u32, Option<String>>) -> Option<String> {
    if let Some(v) = cache.get(&pid) {
        return v.clone();
    }
    let name = query_process_name(pid);
    cache.insert(pid, name.clone());
    name
}

fn query_process_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // SAFETY: OpenProcess returns a valid handle or null; null is checked.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let mut size: u32 = buf.len() as u32;
    // SAFETY: buf is large enough; size is updated to actual length.
    let rc = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
    // SAFETY: handle is valid (checked above).
    unsafe {
        CloseHandle(handle);
    }
    if rc == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..size as usize]);
    // Take the file name component.
    let leaf = path.rsplit(['\\', '/']).next().unwrap_or(&path);
    if leaf.is_empty() {
        None
    } else {
        Some(leaf.to_string())
    }
}

fn collect_v4(
    pids: &HashSet<u32>,
    out: &mut Vec<ListeningPort>,
    name_cache: &mut HashMap<u32, Option<String>>,
) -> io::Result<()> {
    let buffer = query_table(AF_INET as u32)?;
    if buffer.len() < std::mem::size_of::<u32>() {
        return Ok(());
    }
    // SAFETY: the returned buffer is a MIB_TCPTABLE_OWNER_PID; we read the
    // dwNumEntries header then iterate over the trailing flexible array.
    unsafe {
        let table = buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
        let num = (*table).dwNumEntries as usize;
        let rows = std::ptr::addr_of!((*table).table) as *const MIB_TCPROW_OWNER_PID;
        for i in 0..num {
            let row = &*rows.add(i);
            let pid = row.dwOwningPid;
            if !pids.contains(&pid) {
                continue;
            }
            // dwLocalPort is in network byte order (only low 16 bits used).
            let port = u16::from_be(row.dwLocalPort as u16);
            // dwLocalAddr is a u32 in network byte order.
            let addr_bytes = row.dwLocalAddr.to_le_bytes();
            let addr = Ipv4Addr::new(addr_bytes[0], addr_bytes[1], addr_bytes[2], addr_bytes[3]);
            let process_name = process_name_for(pid, name_cache);
            out.push(ListeningPort {
                pid,
                port,
                addr: IpAddr::V4(addr),
                process_name,
                state: state_from_windows(row.dwState),
            });
        }
    }
    Ok(())
}

fn collect_v6(
    pids: &HashSet<u32>,
    out: &mut Vec<ListeningPort>,
    name_cache: &mut HashMap<u32, Option<String>>,
) -> io::Result<()> {
    let buffer = query_table(AF_INET6 as u32)?;
    if buffer.len() < std::mem::size_of::<u32>() {
        return Ok(());
    }
    // SAFETY: the returned buffer is a MIB_TCP6TABLE_OWNER_PID; we read the
    // dwNumEntries header then iterate over the trailing flexible array.
    unsafe {
        let table = buffer.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID;
        let num = (*table).dwNumEntries as usize;
        let rows = std::ptr::addr_of!((*table).table) as *const MIB_TCP6ROW_OWNER_PID;
        for i in 0..num {
            let row = &*rows.add(i);
            let pid = row.dwOwningPid;
            if !pids.contains(&pid) {
                continue;
            }
            let port = u16::from_be(row.dwLocalPort as u16);
            let addr = Ipv6Addr::from(row.ucLocalAddr);
            let process_name = process_name_for(pid, name_cache);
            out.push(ListeningPort {
                pid,
                port,
                addr: IpAddr::V6(addr),
                process_name,
                state: state_from_windows(row.dwState),
            });
        }
    }
    Ok(())
}

fn collect_v4_all(
    out: &mut Vec<SystemListeningPort>,
    name_cache: &mut HashMap<u32, Option<String>>,
) -> io::Result<()> {
    let buffer = query_table(AF_INET as u32)?;
    if buffer.len() < std::mem::size_of::<u32>() {
        return Ok(());
    }
    // SAFETY: see collect_v4.
    unsafe {
        let table = buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
        let num = (*table).dwNumEntries as usize;
        let rows = std::ptr::addr_of!((*table).table) as *const MIB_TCPROW_OWNER_PID;
        for i in 0..num {
            let row = &*rows.add(i);
            let pid = row.dwOwningPid;
            let port = u16::from_be(row.dwLocalPort as u16);
            let addr_bytes = row.dwLocalAddr.to_le_bytes();
            let addr = Ipv4Addr::new(addr_bytes[0], addr_bytes[1], addr_bytes[2], addr_bytes[3]);
            let process_name = process_name_for(pid, name_cache);
            out.push(SystemListeningPort {
                pid: Some(pid),
                port,
                addr: IpAddr::V4(addr),
                process_name,
                state: state_from_windows(row.dwState),
            });
        }
    }
    Ok(())
}

fn collect_v6_all(
    out: &mut Vec<SystemListeningPort>,
    name_cache: &mut HashMap<u32, Option<String>>,
) -> io::Result<()> {
    let buffer = query_table(AF_INET6 as u32)?;
    if buffer.len() < std::mem::size_of::<u32>() {
        return Ok(());
    }
    // SAFETY: see collect_v6.
    unsafe {
        let table = buffer.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID;
        let num = (*table).dwNumEntries as usize;
        let rows = std::ptr::addr_of!((*table).table) as *const MIB_TCP6ROW_OWNER_PID;
        for i in 0..num {
            let row = &*rows.add(i);
            let pid = row.dwOwningPid;
            let port = u16::from_be(row.dwLocalPort as u16);
            let addr = Ipv6Addr::from(row.ucLocalAddr);
            let process_name = process_name_for(pid, name_cache);
            out.push(SystemListeningPort {
                pid: Some(pid),
                port,
                addr: IpAddr::V6(addr),
                process_name,
                state: state_from_windows(row.dwState),
            });
        }
    }
    Ok(())
}

/// Call `GetExtendedTcpTable` with a growing buffer until it fits.
fn query_table(family: u32) -> io::Result<Vec<u8>> {
    let mut size: u32 = 0;
    // First call to learn the required size. Pass null pointer.
    // SAFETY: we pass null buffer + zero-size; GetExtendedTcpTable writes to `size` only.
    let rc = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if rc != ERROR_INSUFFICIENT_BUFFER && rc != NO_ERROR {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: buffer is sized to GetExtendedTcpTable's reported requirement;
    // the call will populate it. `size` is updated to the actual filled bytes.
    let rc = unsafe {
        GetExtendedTcpTable(
            buffer.as_mut_ptr() as *mut _,
            &mut size,
            0,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if rc != NO_ERROR {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    buffer.truncate(size as usize);
    Ok(buffer)
}
