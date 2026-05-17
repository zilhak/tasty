//! macOS port enumeration via `lsof`.
//!
//! Strategy: run `lsof -iTCP -sTCP:LISTEN -nP -p <pid1>,<pid2>,...`
//! and parse each row. This avoids linking against libproc and is good enough
//! for the typical case (a handful of pids per surface).

use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::process::Command;

use crate::ListeningPort;

pub fn scan(pids: &HashSet<u32>) -> io::Result<Vec<ListeningPort>> {
    if pids.is_empty() {
        return Ok(Vec::new());
    }
    let pid_list: Vec<String> = pids.iter().map(|p| p.to_string()).collect();
    let pid_arg = pid_list.join(",");

    // -F pPnT — fields: p (pid), n (name e.g. *:8080), T (TCP state).
    // We use the default human-readable output for simplicity; structured `-F` parsing
    // is fiddly because each record is on a separate line.
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-p", &pid_arg])
        .output()?;
    // lsof returns 1 when *some* pids have no matching descriptors; treat
    // non-empty stdout as success regardless of exit code.
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_lsof(&text))
}

/// Parse human-readable lsof output. Each line looks like:
///   `node 1234 user 22u IPv4 ... TCP *:8080 (LISTEN)`
fn parse_lsof(text: &str) -> Vec<ListeningPort> {
    let mut out = Vec::new();
    for line in text.lines() {
        // skip header line: "COMMAND   PID USER ..."
        if line.starts_with("COMMAND") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 9 {
            continue;
        }
        // Sanity: must end with "(LISTEN)"
        if !line.contains("(LISTEN)") {
            continue;
        }
        let pid: u32 = match fields[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        // The `name` field looks like `*:8080`, `127.0.0.1:3000`, `[::1]:8080`, or `[::]:8080`.
        // It's the 9th column when -nP is used.
        let name = fields[8];
        if let Some((addr, port)) = parse_lsof_addr(name) {
            out.push(ListeningPort { pid, port, addr });
        }
    }
    out
}

fn parse_lsof_addr(s: &str) -> Option<(IpAddr, u16)> {
    // IPv6: "[<addr>]:<port>" or "[::]:port"
    if let Some(rest) = s.strip_prefix('[') {
        let (addr_part, port_part) = rest.split_once("]:")?;
        let port: u16 = port_part.parse().ok()?;
        if addr_part == "::" {
            return Some((IpAddr::V6(Ipv6Addr::UNSPECIFIED), port));
        }
        let addr: Ipv6Addr = addr_part.parse().ok()?;
        return Some((IpAddr::V6(addr), port));
    }
    // IPv4 or wildcard: "*:8080" or "127.0.0.1:8080"
    let (addr_part, port_part) = s.rsplit_once(':')?;
    let port: u16 = port_part.parse().ok()?;
    if addr_part == "*" {
        return Some((IpAddr::V4(Ipv4Addr::UNSPECIFIED), port));
    }
    let addr: Ipv4Addr = addr_part.parse().ok()?;
    Some((IpAddr::V4(addr), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wildcard_v4() {
        let (addr, port) = parse_lsof_addr("*:8080").unwrap();
        assert_eq!(port, 8080);
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn parse_loopback_v4() {
        let (addr, port) = parse_lsof_addr("127.0.0.1:3000").unwrap();
        assert_eq!(port, 3000);
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn parse_wildcard_v6() {
        let (addr, port) = parse_lsof_addr("[::]:9090").unwrap();
        assert_eq!(port, 9090);
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
    }

    #[test]
    fn parse_loopback_v6() {
        let (addr, port) = parse_lsof_addr("[::1]:8080").unwrap();
        assert_eq!(port, 8080);
        assert_eq!(addr, IpAddr::V6(Ipv6Addr::LOCALHOST));
    }

    #[test]
    fn parse_lsof_full_output() {
        let sample = "\
COMMAND   PID  USER  FD   TYPE             DEVICE SIZE/OFF NODE NAME
node    12345  ljh   22u  IPv4 0x1234567890abcdef      0t0  TCP *:3000 (LISTEN)
node    12345  ljh   23u  IPv6 0x1234567890abcdef      0t0  TCP [::1]:8080 (LISTEN)
ssh     54321  ljh   3u   IPv4 0x1234                  0t0  TCP 1.2.3.4:22->5.6.7.8:55 (ESTABLISHED)
";
        let results = parse_lsof(sample);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].pid, 12345);
        assert_eq!(results[0].port, 3000);
        assert_eq!(results[1].port, 8080);
    }
}
