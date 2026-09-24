//! NDJSON 한 줄 송신.
//!
//! 호스트 ↔ plugin 메인 채널(TCP)의 양 끝이 메시지를 쓰는 자리는 전부 이 함수를 거친다.

use std::io::{self, Write};

/// 본문과 개행을 한 버퍼로 묶어 write_all한 뒤 flush한다.
/// 작은 쓰기를 나누어 보낼 때 생기는 지연을 줄이기 위한 처리이며,
/// write_all 내부의 실제 write 횟수나 TCP 세그먼트 수를 보장하지는 않는다.
/// docs/dev-guide/attach-behavior.md의 프레임 전송 지연 절 참조.
pub fn write_line<W: Write + ?Sized>(w: &mut W, line: &str) -> io::Result<()> {
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    w.write_all(&buf)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 전체 버퍼를 한 번에 받는 writer에는 write를 한 번만 호출하는지 확인한다.
    #[test]
    fn write_line_emits_one_write_call() {
        struct CountingWriter {
            writes: usize,
            buf: Vec<u8>,
        }
        impl Write for CountingWriter {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                self.writes += 1;
                self.buf.extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        for line in ["", "{\"id\":1}", &"x".repeat(4096)] {
            let mut w = CountingWriter {
                writes: 0,
                buf: Vec::new(),
            };
            write_line(&mut w, line).unwrap();
            assert_eq!(
                w.writes,
                1,
                "{} 바이트 줄이 {} 번의 write 로 쪼개졌다",
                line.len(),
                w.writes
            );
            assert_eq!(w.buf, format!("{line}\n").into_bytes());
        }
    }
}
