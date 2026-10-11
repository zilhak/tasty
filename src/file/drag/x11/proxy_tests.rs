//! Opt-in protocol regression cases on an owned Xvfb, with separate recipient connections.
use super::*;

struct Peer {
    conn: RustConnection,
    window: Window,
}
impl Peer {
    fn new(x: i16, mapped: bool) -> Self {
        let (conn, screen) = x11rb::connect(None).unwrap();
        let window = conn.generate_id().unwrap();
        conn.create_window(
            0,
            window,
            conn.setup().roots[screen].root,
            x,
            50,
            300,
            200,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
        if mapped {
            conn.map_window(window).unwrap().check().unwrap();
        }
        Self { conn, window }
    }
    fn set(&self, atom: Atom, ty: AtomEnum, value: u32) {
        self.conn
            .change_property32(PropMode::REPLACE, self.window, atom, ty, &[value])
            .unwrap()
            .check()
            .unwrap();
    }
    fn messages(&self) -> Vec<ClientMessageEvent> {
        self.conn.get_input_focus().unwrap().reply().unwrap();
        let mut messages = Vec::new();
        while let Some(event) = self.conn.poll_for_event().unwrap() {
            if let Event::ClientMessage(event) = event {
                messages.push(event);
            }
        }
        messages
    }
}

struct Fixture {
    session: Session,
    target: Peer,
    proxy: Peer,
    _origin: Peer,
}
impl Fixture {
    fn new(original_version: Option<u32>, proxy_version: Option<u32>) -> Self {
        assert_eq!(
            std::env::var("DISPLAY").unwrap(),
            std::env::var("TASTY_DRAG_FIXTURE_DISPLAY").expect("owned fixture opt-in")
        );
        let origin = Peer::new(50, false);
        let target = Peer::new(600, true);
        let proxy = Peer::new(0, false);
        let session = Session::start(origin.window, b"file:///owned-source\r\n".to_vec()).unwrap();
        let a = &session.atoms;
        target.set(a.XdndProxy, AtomEnum::WINDOW, proxy.window);
        proxy.set(a.XdndProxy, AtomEnum::WINDOW, proxy.window);
        if let Some(version) = original_version {
            target.set(a.XdndAware, AtomEnum::ATOM, version);
        }
        if let Some(version) = proxy_version {
            proxy.set(a.XdndAware, AtomEnum::ATOM, version);
        }
        Self {
            session,
            target,
            proxy,
            _origin: origin,
        }
    }
    fn enter(&mut self, via_proxy: bool, version: u32) {
        let recipient = if via_proxy {
            self.proxy.window
        } else {
            self.target.window
        };
        assert_eq!(
            self.session.target_at(650, 100).unwrap(),
            Some(Target {
                window: self.target.window,
                recipient,
                version,
            })
        );
        // Origin is at (50, 50); forwarded coordinates land inside the mapped target.
        self.session.motion(600, 50).unwrap();
        self.session
            .conn
            .get_input_focus()
            .unwrap()
            .reply()
            .unwrap();
        let target_messages = self.target.messages();
        let proxy_messages = self.proxy.messages();
        let (received, unused) = if via_proxy {
            (proxy_messages, target_messages)
        } else {
            (target_messages, proxy_messages)
        };
        assert!(unused.is_empty(), "messages went to the wrong recipient");
        let enter = received
            .iter()
            .find(|e| e.type_ == self.session.atoms.XdndEnter)
            .expect("recipient must receive Enter");
        assert_eq!(enter.window, self.target.window, "original target identity");
        assert_eq!(enter.data.as_data32()[0], self.session.source);
        assert_eq!(enter.data.as_data32()[1] >> 24, version);
    }
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn proxy_only_target_receives_enter() {
    Fixture::new(None, Some(4)).enter(true, 4);
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn proxy_version_overrides_original_in_both_directions() {
    Fixture::new(Some(5), Some(4)).enter(true, 4);
    Fixture::new(Some(3), Some(5)).enter(true, 5);
    Fixture::new(Some(4), Some(6)).enter(true, 5);
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn invalid_proxy_self_reference_falls_back_to_original() {
    let mut f = Fixture::new(Some(5), Some(4));
    f.proxy
        .set(f.session.atoms.XdndProxy, AtomEnum::WINDOW, f.target.window);
    f.enter(false, 5);
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn missing_proxy_self_reference_falls_back_to_original() {
    let mut f = Fixture::new(Some(5), Some(4));
    f.proxy
        .conn
        .delete_property(f.proxy.window, f.session.atoms.XdndProxy)
        .unwrap()
        .check()
        .unwrap();
    f.enter(false, 5);
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn destroyed_proxy_falls_back_to_original() {
    let mut f = Fixture::new(Some(5), Some(4));
    f.proxy
        .conn
        .destroy_window(f.proxy.window)
        .unwrap()
        .check()
        .unwrap();
    f.enter(false, 5);
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn direct_target_receives_enter() {
    let mut f = Fixture::new(Some(5), Some(4));
    f.target
        .conn
        .delete_property(f.target.window, f.session.atoms.XdndProxy)
        .unwrap()
        .check()
        .unwrap();
    f.enter(false, 5);
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn valid_unaware_proxy_does_not_use_original_awareness() {
    let f = Fixture::new(Some(5), None);
    assert_eq!(f.session.target_at(650, 100).unwrap(), None);
}

#[test]
#[ignore = "requires owned Xvfb and TASTY_DRAG_FIXTURE_DISPLAY=DISPLAY; run serially"]
fn unrelated_proxy_property_error_propagates() {
    let mut f = Fixture::new(Some(5), Some(4));
    f.session.atoms.XdndAware = u32::MAX;
    let error = f.session.target(f.target.window).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ReplyError>(),
        Some(ReplyError::X11Error(error)) if error.error_kind == ErrorKind::Atom
    ));
}
