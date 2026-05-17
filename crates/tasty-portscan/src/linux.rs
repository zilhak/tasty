//! Linux port enumeration via `/proc`.
//!
//! Strategy:
//! 1. Parse `/proc/net/tcp` + `/proc/net/tcp6` for LISTEN sockets (state == 0x0A).
//!    Each row gives (local_addr, local_port, inode).
//! 2. For each PID of interest, walk `/proc/{pid}/fd/*` symlinks. Each socket
//!    fd is `socket:[<inode>]`. Match inode → port.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::ListeningPort;

/// State value used by /proc/net/tcp* for LISTEN.
const TCP_LISTEN: u32 = 0x0A;

pub fn scan(pids: &HashSet<u32>) -> io::Result<Vec<ListeningPort>> {
    // inode → (addr, port)
    let mut inode_map: HashMap<u64, (IpAddr, u16)> = HashMap::new();
    if let Ok(text) = std::fs::read_to_string("/proc/net/tcp") {
        parse_proc_net_tcp(&text, false, &mut inode_map);
    }
    if let Ok(text) = std::fs::read_to_string("/proc/net/tcp6") {
        parse_proc_net_tcp(&text, true, &mut inode_map);
    }
    if inode_map.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for &pid in pids {
        let fd_dir = format!("/proc/{pid}/fd");
        let entries = match std::fs::read_dir(&fd_dir) {
            Ok(e) => e,
            Err(_) => continue, // process might have exited or we lack permission
        };
        for entry in entries.flatten() {
            let target = match std::fs::read_link(entry.path()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let s = match target.to_str() {
                Some(s) => s,
                None => continue,
            };
            if let Some(inode_str) = s.strip_prefix("socket:[").and_then(|r| r.strip_suffix(']')) {
                if let Ok(inode) = inode_str.parse::<u64>() {
                    if let Some(&(addr, port)) = inode_map.get(&inode) {
                        out.push(ListeningPort { pid, port, addr });
                    }
                }
            }
        }
    }
    Ok(out)
}

fn parse_proc_net_tcp(text: &str, ipv6: bool, out: &mut HashMap<u64, (IpAddr, u16)>) {
    for line in text.lines().skip(1) {
        // Format (whitespace-separated):
        //   sl  local_address  rem_address  st  ...  inode ...
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        let local = fields[1];
        let state = fields[3];

        let state_val = match u32::from_str_radix(state, 16) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if state_val != TCP_LISTEN {
            continue;
        }

        let (addr, port) = match parse_hex_endpoint(local, ipv6) {
            Some(a) => a,
            None => continue,
        };

        // The inode field is index 9.
        let inode: u64 = match fields[9].parse() {
            Ok(i) => i,
            Err(_) => continue,
        };
        out.insert(inode, (addr, port));
    }
}

/// Parse `<addr_hex>:<port_hex>` where `addr_hex` is 8 hex chars (v4) or 32 hex chars (v6).
fn parse_hex_endpoint(s: &str, ipv6: bool) -> Option<(IpAddr, u16)> {
    let (addr_part, port_part) = s.split_once(':')?;
    let port = u16::from_str_radix(port_part, 16).ok()?;
    if ipv6 {
        let bytes = parse_hex_bytes(addr_part)?;
        if bytes.len() != 16 {
            return None;
        }
        // /proc/net/tcp6 stores each 32-bit word in little-endian, but the
        // 4 words appear in network order. So we reverse within each 4-byte chunk.
        let mut buf = [0u8; 16];
        for (chunk_idx, chunk) in bytes.chunks(4).enumerate() {
            for (i, b) in chunk.iter().rev().enumerate() {
                buf[chunk_idx * 4 + i] = *b;
            }
        }
        Some((IpAddr::V6(Ipv6Addr::from(buf)), port))
    } else {
        let bytes = parse_hex_bytes(addr_part)?;
        if bytes.len() != 4 {
            return None;
        }
        // /proc/net/tcp v4 address is little-endian.
        let ip = Ipv4Addr::new(bytes[3], bytes[2], bytes[1], bytes[0]);
        Some((IpAddr::V4(ip), port))
    }
}

fn parse_hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        out.push(u8::from_str_radix(&s[i..i + 2], 16).ok()?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_v4_loopback_8080() {
        // 127.0.0.1:8080 → 0100007F:1F90
        let (addr, port) = parse_hex_endpoint("0100007F:1F90", false).unwrap();
        assert_eq!(port, 8080);
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn parse_v4_any_3000() {
        let (addr, port) = parse_hex_endpoint("00000000:0BB8", false).unwrap();
        assert_eq!(port, 3000);
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn parse_v6_loopback() {
        // ::1
        let s = "00000000000000000000000001000000:1F90";
        let (addr, port) = parse_hex_endpoint(s, true).unwrap();
        assert_eq!(port, 8080);
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::LOCALHOST));
    }

    #[test]
    fn parse_proc_filters_to_listen() {
        let sample = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345
   1: 00000000:0050 0A0A0A0A:1234 01 00000000:00000000 00:00000000 00000000     0        0 99999
";
        let mut map = HashMap::new();
        parse_proc_net_tcp(sample, false, &mut map);
        assert_eq!(map.len(), 1);
        let (addr, port) = map[&12345];
        assert_eq!(port, 8080);
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }
}
