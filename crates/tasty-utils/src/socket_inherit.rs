//! 소켓 핸들을 자식 프로세스가 물려받지 않게 하는 함수.
//!
//! Windows에서 std는 소켓을 `WSA_FLAG_NO_HANDLE_INHERIT`로 만들고 복제하지만, 돌려받은 핸들이
//! 상속 가능으로 남는 경우가 관측됐다. `accept`한 소켓은 std가 상속을 막지 않는다. std
//! `Command`는 상속 가능한 핸들을 모두 자식에게 넘기므로, 그 순간 std `Command`로 뜬 자식(플러그인·
//! ssh 등)이 연결을 쥔 채 남을 수 있다. 그래서 만든 직후 핸들의 상속 플래그를 직접 끈다.
//! 이것이 막는 것은 끈 뒤에 띄우는 자식의 상속이다. 생성·복제·accept가 돌아온 뒤 끄기 전까지의 짧은
//! 구간에 다른 스레드가 띄운 자식은 여전히 핸들을 물려받을 수 있고, std `Command`의 핸들 목록 제한
//! 없이는 이 구간을 닫을 수 없다.
//! 다른 OS의 std는 소켓을 이미 close-on-exec로 만들므로 아무것도 하지 않는다.

use std::io;
use std::net::TcpStream;

/// 이 소켓 핸들의 상속을 끈다. 실패하면 OS 오류를 돌려준다. Windows 밖에서는 아무것도 하지 않는다.
#[cfg(windows)]
pub fn disable_inherit(stream: &TcpStream) -> io::Result<()> {
    windows::clear_inherit_flag(stream)
}

/// 이 소켓 핸들의 상속을 끈다. 실패하면 OS 오류를 돌려준다. Windows 밖에서는 아무것도 하지 않는다.
#[cfg(not(windows))]
pub fn disable_inherit(_stream: &TcpStream) -> io::Result<()> {
    Ok(())
}

/// `try_clone`한 뒤 그 복제의 상속을 끈다. 끄지 못하면 복제를 닫고 오류를 돌려준다. 복제가 돌아온 뒤
/// 끄기 전의 짧은 구간에 다른 스레드가 띄운 자식은 그 복제를 물려받을 수 있다(모듈 설명).
pub fn try_clone_uninherited(stream: &TcpStream) -> io::Result<TcpStream> {
    let clone = stream.try_clone()?;
    disable_inherit(&clone)?;
    Ok(clone)
}

/// 시험 전용: 이 소켓 핸들이 상속 가능한지(`HANDLE_FLAG_INHERIT`)를 읽는다.
#[cfg(all(windows, any(test, feature = "test-support")))]
pub fn is_inheritable(stream: &TcpStream) -> io::Result<bool> {
    windows::read_inherit_flag(stream)
}

/// 시험 전용: 상속을 일부러 켠다. 끄는 호출이 빠진 자리를 std가 이미 꺼 둔 핸들로는 가를 수 없어서 쓴다.
#[cfg(all(windows, any(test, feature = "test-support")))]
pub fn make_inheritable(stream: &TcpStream) -> io::Result<()> {
    windows::set_inherit_flag(stream)
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "std has no API to change a socket handle's inherit flag; each FFI call borrows the socket for one synchronous call"
)]
mod windows {
    use std::io;
    use std::net::TcpStream;
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation};

    fn handle(stream: &TcpStream) -> HANDLE {
        stream.as_raw_socket() as usize as HANDLE
    }

    pub(super) fn clear_inherit_flag(stream: &TcpStream) -> io::Result<()> {
        // SAFETY: the borrow keeps the socket open for this synchronous call. A SOCKET is a
        // kernel handle and is passed at pointer width. The call neither takes nor closes it.
        let ok = unsafe { SetHandleInformation(handle(stream), HANDLE_FLAG_INHERIT, 0) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn set_inherit_flag(stream: &TcpStream) -> io::Result<()> {
        // SAFETY: as in `clear_inherit_flag`.
        let ok = unsafe {
            SetHandleInformation(handle(stream), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT)
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn read_inherit_flag(stream: &TcpStream) -> io::Result<bool> {
        use windows_sys::Win32::Foundation::GetHandleInformation;
        let mut flags = 0u32;
        // SAFETY: as above; `flags` outlives the call that writes it.
        let ok = unsafe { GetHandleInformation(handle(stream), &mut flags) };
        if ok != 0 {
            Ok(flags & HANDLE_FLAG_INHERIT != 0)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{disable_inherit, is_inheritable, make_inheritable, try_clone_uninherited};
    use std::net::{TcpListener, TcpStream};

    /// 상속을 일부러 켠 핸들도 끈다. std가 이미 꺼 둔 핸들만 보면 아무것도 하지 않는 구현도 통과한다.
    #[test]
    fn an_inheritable_handle_is_made_uninheritable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (accepted, _) = listener.accept().expect("accept");
        for (name, socket) in [("client", &client), ("accepted", &accepted)] {
            make_inheritable(socket).expect(name);
            assert!(
                is_inheritable(socket).expect(name),
                "{name}: flag was not set"
            );
            disable_inherit(socket).expect(name);
            assert!(!is_inheritable(socket).expect(name), "{name}: flag remains");
        }
        make_inheritable(&client).expect("client again");
        let clone = try_clone_uninherited(&client).expect("clone");
        assert!(
            !is_inheritable(&clone).expect("clone"),
            "clone: flag remains"
        );
    }

    /// 연결 원본·accept한 소켓·복제 모두 처리한 뒤에는 상속되지 않는다.
    #[test]
    fn processed_socket_handles_are_not_inheritable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (accepted, _) = listener.accept().expect("accept");
        disable_inherit(&client).expect("client");
        disable_inherit(&accepted).expect("accepted");
        let client_clone = try_clone_uninherited(&client).expect("client clone");
        let accepted_clone = try_clone_uninherited(&accepted).expect("accepted clone");
        let flags: Vec<(&str, bool)> = [
            ("client", &client),
            ("accepted", &accepted),
            ("client clone", &client_clone),
            ("accepted clone", &accepted_clone),
        ]
        .into_iter()
        .map(|(name, socket)| (name, is_inheritable(socket).expect(name)))
        .collect();
        assert!(
            flags.iter().all(|(_, inheritable)| !inheritable),
            "inheritable handles remain: {flags:?}"
        );
    }
}
