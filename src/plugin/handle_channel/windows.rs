//! Windows 전용 stub (02c — Unsupported 반환).

#![cfg(windows)]

    use std::io::{self, Read, Write};

    /// Named Pipe server-side stream의 placeholder. 02c에서 실제 HANDLE 래퍼로 교체.
    pub(super) struct PipeServerStream;

    impl Write for PipeServerStream {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle channel write not implemented on Windows yet",
            ))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Read for PipeServerStream {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle channel read not implemented on Windows yet",
            ))
        }
    }
}
