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
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;

/// 이 소켓 핸들(연결·listener, std·tokio)의 상속을 끈다. 실패하면 OS 오류를 돌려준다. Windows 밖에서는
/// 아무것도 하지 않는다.
#[cfg(windows)]
pub fn disable_inherit<S: AsRawSocket + ?Sized>(socket: &S) -> io::Result<()> {
    windows::clear_inherit_flag(socket)
}

/// 이 소켓 핸들(연결·listener, std·tokio)의 상속을 끈다. 실패하면 OS 오류를 돌려준다. Windows 밖에서는
/// 아무것도 하지 않는다.
#[cfg(not(windows))]
pub fn disable_inherit<S: ?Sized>(_socket: &S) -> io::Result<()> {
    Ok(())
}

/// `try_clone`한 뒤 그 복제의 상속을 끈다. 끄지 못하면 복제를 닫고 오류를 돌려준다. 복제가 돌아온 뒤
/// 끄기 전의 짧은 구간에 다른 스레드가 띄운 자식은 그 복제를 물려받을 수 있다(모듈 설명).
pub fn try_clone_uninherited(stream: &TcpStream) -> io::Result<TcpStream> {
    let clone = stream.try_clone()?;
    #[cfg(all(windows, any(test, feature = "test-support")))]
    seam::created(
        seam::CLONE,
        &clone,
        clone.local_addr().ok().map(|addr| addr.port()),
    );
    disable_inherit(&clone)?;
    Ok(clone)
}

/// 시험 전용: 이 소켓 핸들이 상속 가능한지(`HANDLE_FLAG_INHERIT`)를 읽는다.
#[cfg(all(windows, any(test, feature = "test-support")))]
pub fn is_inheritable<S: AsRawSocket + ?Sized>(socket: &S) -> io::Result<bool> {
    windows::read_inherit_flag(socket)
}

/// 시험 전용: 상속을 일부러 켠다. 끄는 호출이 빠진 자리를 std가 이미 꺼 둔 핸들로는 가를 수 없어서 쓴다.
#[cfg(all(windows, any(test, feature = "test-support")))]
pub fn make_inheritable<S: AsRawSocket + ?Sized>(socket: &S) -> io::Result<()> {
    windows::set_inherit_flag(socket)
}

/// 시험 전용 이음새. 제품 자리가 만든 직후 [`seam::created`]를, 끈 뒤 [`seam::cleared`]를 부른다.
/// 시험이 그 자리 이름을 [`seam::arm`]으로 켜 두면 `created`가 상속을 일부러 켜고 그 뒤의 플래그를,
/// `cleared`가 끈 뒤의 플래그를 남긴다. std가 이미 꺼 둔 핸들로는 끄는 호출이 빠진 자리를 가를 수 없어서
/// 쓴다. 켜는 범위는 시험 스레드([`seam::Scope::Thread`])나 연결의 로컬 포트([`seam::Scope::Port`])다.
/// [`try_clone_uninherited`]는 복제 직후 [`seam::CLONE`] 자리로 `created`를 부른다.
#[cfg(all(windows, any(test, feature = "test-support")))]
pub mod seam {
    use std::os::windows::io::AsRawSocket;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
    use std::thread::ThreadId;

    /// [`super::try_clone_uninherited`]가 복제 직후 부르는 자리 이름.
    pub const CLONE: &str = "socket_inherit.try_clone_uninherited";

    /// 켜는 범위.
    #[derive(Clone, Copy, Debug)]
    pub enum Scope {
        /// `arm`을 부른 스레드에서 만든 소켓.
        Thread,
        /// 로컬 포트가 이 값인 소켓(accept한 연결은 listener 포트, 그 복제도 같다).
        Port(u16),
    }

    /// 한 번 지나간 기록. `inheritable`은 그 때 읽은 상속 플래그다.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Seen {
        pub port: Option<u16>,
        pub inheritable: bool,
    }

    #[derive(Clone, Copy)]
    enum Bound {
        Thread(ThreadId),
        Port(u16),
    }

    impl Bound {
        fn covers(self, port: Option<u16>) -> bool {
            match self {
                Bound::Thread(id) => std::thread::current().id() == id,
                Bound::Port(wanted) => port == Some(wanted),
            }
        }
    }

    type Log = Arc<Mutex<Vec<Seen>>>;

    struct Entry {
        site: &'static str,
        bound: Bound,
        created: Log,
        cleared: Log,
    }

    static ARMED: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

    fn armed() -> MutexGuard<'static, Vec<Entry>> {
        ARMED.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn read(log: &Log) -> Vec<Seen> {
        log.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// 켠 자리. 놓으면 그 자리를 끈다.
    pub struct Armed {
        created: Log,
        cleared: Log,
    }

    impl Armed {
        /// 켠 뒤 `created`가 남긴 기록(상속을 켠 뒤 읽은 값)을 지나간 순서대로 돌려준다.
        pub fn created(&self) -> Vec<Seen> {
            read(&self.created)
        }

        /// 켠 뒤 `cleared`가 남긴 기록(끈 뒤 읽은 값)을 지나간 순서대로 돌려준다.
        pub fn cleared(&self) -> Vec<Seen> {
            read(&self.cleared)
        }
    }

    impl Drop for Armed {
        fn drop(&mut self) {
            armed().retain(|entry| !Arc::ptr_eq(&entry.created, &self.created));
        }
    }

    /// `site`를 `scope` 범위로 켠다.
    pub fn arm(site: &'static str, scope: Scope) -> Armed {
        let bound = match scope {
            Scope::Thread => Bound::Thread(std::thread::current().id()),
            Scope::Port(port) => Bound::Port(port),
        };
        let (created, cleared) = (Log::default(), Log::default());
        armed().push(Entry {
            site,
            bound,
            created: Arc::clone(&created),
            cleared: Arc::clone(&cleared),
        });
        Armed { created, cleared }
    }

    fn logs(site: &'static str, port: Option<u16>, pick: fn(&Entry) -> &Log) -> Vec<Log> {
        armed()
            .iter()
            .filter(|entry| entry.site == site && entry.bound.covers(port))
            .map(|entry| Arc::clone(pick(entry)))
            .collect()
    }

    fn push(logs: Vec<Log>, seen: Seen) {
        for log in logs {
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(seen);
        }
    }

    /// 제품 자리가 소켓을 만든 직후 부른다. `port`는 그 소켓의 로컬 포트다. 켠 범위면 상속을 켜고 그 뒤
    /// 플래그를 남긴다. 읽지 못하면 상속 불가로 남긴다(켜기가 확인되지 않은 기록이다).
    pub fn created<S: AsRawSocket + ?Sized>(site: &'static str, socket: &S, port: Option<u16>) {
        let logs = logs(site, port, |entry| &entry.created);
        if logs.is_empty() {
            return;
        }
        super::make_inheritable(socket).expect("test seam could not set the inherit flag");
        let inheritable = super::is_inheritable(socket).unwrap_or(false);
        push(logs, Seen { port, inheritable });
    }

    /// 제품 자리가 상속을 끈 뒤 부른다. 켠 범위면 그 플래그를 남긴다. 읽지 못하면 상속 가능으로 남긴다.
    pub fn cleared<S: AsRawSocket + ?Sized>(site: &'static str, socket: &S, port: Option<u16>) {
        let logs = logs(site, port, |entry| &entry.cleared);
        if logs.is_empty() {
            return;
        }
        let inheritable = super::is_inheritable(socket).unwrap_or(true);
        push(logs, Seen { port, inheritable });
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "std has no API to change a socket handle's inherit flag; each FFI call borrows the socket for one synchronous call"
)]
mod windows {
    use std::io;
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation};

    fn handle<S: AsRawSocket + ?Sized>(socket: &S) -> HANDLE {
        socket.as_raw_socket() as usize as HANDLE
    }

    pub(super) fn clear_inherit_flag<S: AsRawSocket + ?Sized>(stream: &S) -> io::Result<()> {
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
    pub(super) fn set_inherit_flag<S: AsRawSocket + ?Sized>(stream: &S) -> io::Result<()> {
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
    pub(super) fn read_inherit_flag<S: AsRawSocket + ?Sized>(stream: &S) -> io::Result<bool> {
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

    /// listener 핸들도 같은 함수로 켜고 끈다.
    #[test]
    fn an_inheritable_listener_is_made_uninheritable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        make_inheritable(&listener).expect("make");
        assert!(is_inheritable(&listener).expect("read"), "flag was not set");
        disable_inherit(&listener).expect("disable");
        assert!(!is_inheritable(&listener).expect("read"), "flag remains");
    }

    /// 켠 자리·범위에서만 만든 직후 상속이 켜지고 기록이 남는다. 놓은 뒤에는 건드리지 않는다.
    #[test]
    fn an_armed_seam_sets_the_flag_only_in_its_scope() {
        use super::seam::{self, Scope, Seen};
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let other = TcpListener::bind("127.0.0.1:0").expect("bind other");
        let other_port = other.local_addr().expect("addr other").port();
        let by_port = seam::arm("socket_inherit.tests.port", Scope::Port(port));
        seam::created("socket_inherit.tests.port", &other, Some(other_port));
        seam::created("socket_inherit.tests.port", &listener, Some(port));
        assert!(!is_inheritable(&other).expect("other"), "other port set");
        disable_inherit(&listener).expect("disable");
        seam::cleared("socket_inherit.tests.port", &listener, Some(port));
        let seen = |inheritable| Seen {
            port: Some(port),
            inheritable,
        };
        assert_eq!(by_port.created(), vec![seen(true)]);
        assert_eq!(by_port.cleared(), vec![seen(false)]);

        let by_thread = seam::arm("socket_inherit.tests.thread", Scope::Thread);
        std::thread::scope(|scope| {
            scope.spawn(|| seam::created("socket_inherit.tests.thread", &other, None));
        });
        assert!(!is_inheritable(&other).expect("other"), "other thread set");
        assert!(by_thread.created().is_empty());
        drop(by_port);
        seam::created("socket_inherit.tests.port", &other, Some(port));
        assert!(!is_inheritable(&other).expect("other"), "released site set");
    }

    /// 복제는 [`seam::CLONE`] 자리에서 끄기 전에 켜진다. 끄는 호출이 빠지면 복제가 상속 가능으로 남는다.
    #[test]
    fn a_clone_is_set_at_the_clone_seam_before_it_is_cleared() {
        use super::seam::{self, Scope};
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let armed = seam::arm(seam::CLONE, Scope::Thread);
        let clone = try_clone_uninherited(&client).expect("clone");
        let created = armed.created();
        assert!(
            !created.is_empty() && created.iter().all(|seen| seen.inheritable),
            "the clone seam did not set the flag: {created:?}"
        );
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
