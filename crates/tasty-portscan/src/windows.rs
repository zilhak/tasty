//! Windows port enumeration via `GetExtendedTcpTable`.

use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use crate::ListeningPort;

pub fn scan(pids: &HashSet<u32>) -> io::Result<Vec<ListeningPort>> {
    let mut out = Vec::new();
    collect_v4(pids, &mut out)?;
    collect_v6(pids, &mut out)?;
    Ok(out)
}

fn collect_v4(pids: &HashSet<u32>, out: &mut Vec<ListeningPort>) -> io::Result<()> {
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
            out.push(ListeningPort {
                pid,
                port,
                addr: IpAddr::V4(addr),
            });
        }
    }
    Ok(())
}

fn collect_v6(pids: &HashSet<u32>, out: &mut Vec<ListeningPort>) -> io::Result<()> {
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
            out.push(ListeningPort {
                pid,
                port,
                addr: IpAddr::V6(addr),
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
            TCP_TABLE_OWNER_PID_LISTENER,
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
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if rc != NO_ERROR {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    buffer.truncate(size as usize);
    Ok(buffer)
}
