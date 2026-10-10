//! X11 CLIPBOARD 선택으로 파일 목록을 주고받는다. Wayland 세션에서는 XWayland 가 Wayland 클립보드와 잇는다.
//!
//! 쓰기는 별도 연결과 1×1 창으로 선택을 소유하고, 소유를 잃을 때까지(다른 앱이 복사하거나
//! [`clear`]) 스레드 하나가 요청에 답한다. 읽기는 호출마다 연결을 새로 열어 TARGETS 를 묻고
//! GNOME 형식을 먼저, 없으면 `text/uri-list` 와 KDE 잘라내기 표지를 읽는다. 읽기 한 번 전체를
//! [`READ_TIMEOUT`] 안에 끝내지 못하면 소유자가 응답하지 않은 것([`OsClip::Unreadable`])으로 본다.

use std::time::{Duration, Instant};

use x11rb::{
    COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE,
    connection::{Connection, RequestConnection},
    protocol::{
        Event,
        xproto::{
            AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, Property,
            SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use super::format;
use super::{OsClip, OsFiles};

x11rb::atom_manager! {
    Atoms: AtomCookies {
        CLIPBOARD,
        TARGETS,
        INCR,
        UTF8_STRING,
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
        URI_LIST: b"text/uri-list",
        GNOME_COPIED_FILES: b"x-special/gnome-copied-files",
        KDE_CUT_SELECTION: b"application/x-kde-cutselection",
        TASTY_FILES_REPLY,
    }
}

/// 읽기 한 번 전체(TARGETS·형식들·INCR 조각 모두)의 한도. 읽기는 UI 스레드에서 붙여넣기·메뉴를
/// 열 때 일어나므로 짧게 둔다. 조각을 천천히 보내는 소유자도 이 한도를 넘기면 읽기 실패다.
const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// 연결과 응답을 받을 창.
struct Session {
    conn: RustConnection,
    win: u32,
    atoms: Atoms,
}

fn open(event_mask: EventMask) -> Result<Session, String> {
    let (conn, screen_num) =
        RustConnection::connect(None).map_err(|e| format!("X11 connect failed: {e}"))?;
    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or("X11: no screen")?
        .root;
    let win = conn.generate_id().map_err(|e| e.to_string())?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        COPY_FROM_PARENT,
        &CreateWindowAux::new().event_mask(event_mask),
    )
    .map_err(|e| e.to_string())?;
    let atoms = Atoms::new(&conn)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    Ok(Session { conn, win, atoms })
}

pub(super) fn write(files: &OsFiles) -> Result<(), String> {
    let s = open(EventMask::NO_EVENT)?;
    let a = &s.atoms;
    let text = files
        .paths
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    let offers = vec![
        (
            a.GNOME_COPIED_FILES,
            format::gnome_copied_files(files).into_bytes(),
        ),
        (a.URI_LIST, format::uri_list(&files.paths).into_bytes()),
        (
            a.KDE_CUT_SELECTION,
            if files.cut {
                b"1".to_vec()
            } else {
                b"0".to_vec()
            },
        ),
        (a.UTF8_STRING, text.clone().into_bytes()),
        (a.TEXT_PLAIN_UTF8, text.into_bytes()),
    ];
    s.conn
        .set_selection_owner(s.win, a.CLIPBOARD, CURRENT_TIME)
        .map_err(|e| e.to_string())?;
    let owner = s
        .conn
        .get_selection_owner(a.CLIPBOARD)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .owner;
    if owner != s.win {
        return Err("X11: could not take the clipboard".into());
    }
    std::thread::Builder::new()
        .name("file-clipboard-owner".into())
        .spawn(move || serve(&s, &offers))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 선택을 잃을 때까지 요청에 답한다.
fn serve(s: &Session, offers: &[(u32, Vec<u8>)]) {
    loop {
        match s.conn.wait_for_event() {
            Ok(Event::SelectionRequest(req)) => {
                if let Err(e) = answer(s, offers, &req) {
                    tracing::warn!("file clipboard request failed: {e}");
                }
            }
            Ok(Event::SelectionClear(ev)) if ev.selection == s.atoms.CLIPBOARD => return,
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("file clipboard owner connection closed: {e}");
                return;
            }
        }
    }
}

fn answer(
    s: &Session,
    offers: &[(u32, Vec<u8>)],
    req: &SelectionRequestEvent,
) -> Result<(), String> {
    let a = &s.atoms;
    // 옛 클라이언트는 property 없이 묻는다. 그때는 target 이름으로 답한다(ICCCM 2.2).
    let property = if req.property == NONE {
        req.target
    } else {
        req.property
    };
    // 한 요청에 담을 수 있는 크기를 넘으면 거절한다(INCR 로 나눠 보내지 않는다).
    let limit = s.conn.maximum_request_bytes().saturating_sub(64);
    let answered = if req.selection != a.CLIPBOARD {
        false
    } else if req.target == a.TARGETS {
        let mut targets = vec![a.TARGETS];
        targets.extend(offers.iter().map(|(t, _)| *t));
        s.conn
            .change_property32(
                PropMode::REPLACE,
                req.requestor,
                property,
                AtomEnum::ATOM,
                &targets,
            )
            .map_err(|e| e.to_string())?;
        true
    } else if let Some((target, bytes)) = offers.iter().find(|(t, _)| *t == req.target) {
        if bytes.len() > limit {
            tracing::warn!(
                "file clipboard offer of {} bytes is larger than one X11 request",
                bytes.len()
            );
            false
        } else {
            s.conn
                .change_property8(PropMode::REPLACE, req.requestor, property, *target, bytes)
                .map_err(|e| e.to_string())?;
            true
        }
    } else {
        false
    };
    let notify = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: req.time,
        requestor: req.requestor,
        selection: req.selection,
        target: req.target,
        property: if answered { property } else { NONE },
    };
    s.conn
        .send_event(false, req.requestor, EventMask::NO_EVENT, notify)
        .map_err(|e| e.to_string())?;
    s.conn.flush().map_err(|e| e.to_string())
}

pub(super) fn read() -> Result<OsClip, String> {
    read_within(READ_TIMEOUT)
}

/// X 서버에 접속하지 못하면 Err 다. 소유자가 있는데 `timeout` 안에 내용을 받지 못하면
/// [`OsClip::Unreadable`] 이다.
fn read_within(timeout: Duration) -> Result<OsClip, String> {
    let s = open(EventMask::PROPERTY_CHANGE)?;
    let owner = s
        .conn
        .get_selection_owner(s.atoms.CLIPBOARD)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .owner;
    if owner == NONE {
        return Ok(OsClip::Other);
    }
    match read_owned(&s, Instant::now() + timeout) {
        Ok(clip) => Ok(clip),
        Err(e) => {
            tracing::debug!("file clipboard owner {owner:#x} did not answer: {e}");
            Ok(OsClip::Unreadable { owner })
        }
    }
}

fn read_owned(s: &Session, deadline: Instant) -> Result<OsClip, String> {
    let a = &s.atoms;
    let Some(targets) = convert(s, a.TARGETS, deadline)? else {
        return Ok(OsClip::Other);
    };
    let targets: Vec<u32> = targets
        .chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if targets.contains(&a.GNOME_COPIED_FILES)
        && let Some(bytes) = convert(s, a.GNOME_COPIED_FILES, deadline)?
    {
        return Ok(format::parse_gnome_copied_files(&String::from_utf8_lossy(
            &bytes,
        )));
    }
    if !targets.contains(&a.URI_LIST) {
        return Ok(OsClip::Other);
    }
    let Some(list) = convert(s, a.URI_LIST, deadline)? else {
        return Ok(OsClip::Other);
    };
    let kde = if targets.contains(&a.KDE_CUT_SELECTION) {
        convert(s, a.KDE_CUT_SELECTION, deadline)?
    } else {
        None
    };
    Ok(format::parse_uri_list(
        &String::from_utf8_lossy(&list),
        kde.as_deref(),
    ))
}

pub(super) fn clear() -> Result<(), String> {
    let s = open(EventMask::NO_EVENT)?;
    s.conn
        .set_selection_owner(NONE, s.atoms.CLIPBOARD, CURRENT_TIME)
        .map_err(|e| e.to_string())?;
    s.conn.sync().map_err(|e| e.to_string())
}

/// CLIPBOARD 에 형식 하나를 묻고 응답(INCR 조각 포함)을 `deadline` 까지 모은다. 소유자가 거절하면 None.
fn convert(s: &Session, target: u32, deadline: Instant) -> Result<Option<Vec<u8>>, String> {
    let a = &s.atoms;
    s.conn
        .delete_property(s.win, a.TASTY_FILES_REPLY)
        .map_err(|e| e.to_string())?;
    s.conn
        .convert_selection(
            s.win,
            a.CLIPBOARD,
            target,
            a.TASTY_FILES_REPLY,
            CURRENT_TIME,
        )
        .map_err(|e| e.to_string())?;
    s.conn.sync().map_err(|e| e.to_string())?;

    let mut incr: Option<Vec<u8>> = None;
    loop {
        if Instant::now() > deadline {
            return Err("X11: clipboard owner did not answer in time".into());
        }
        let Some(event) = s.conn.poll_for_event().map_err(|e| e.to_string())? else {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        };
        match event {
            Event::SelectionNotify(ev) if incr.is_none() => {
                if ev.property == NONE {
                    return Ok(None);
                }
                let reply = take_reply(s)?;
                if reply.type_ == a.INCR {
                    incr = Some(Vec::new());
                    continue;
                }
                return Ok(Some(reply.value));
            }
            Event::PropertyNotify(ev)
                if ev.atom == a.TASTY_FILES_REPLY && ev.state == Property::NEW_VALUE =>
            {
                let Some(data) = incr.as_mut() else {
                    continue;
                };
                let reply = take_reply(s)?;
                if reply.value_len == 0 {
                    return Ok(incr.take());
                }
                data.extend(reply.value);
            }
            _ => {}
        }
    }
}

fn take_reply(s: &Session) -> Result<x11rb::protocol::xproto::GetPropertyReply, String> {
    s.conn
        .get_property(
            true,
            s.win,
            s.atoms.TASTY_FILES_REPLY,
            AtomEnum::ANY,
            0,
            u32::MAX / 4,
        )
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// `text/uri-list` 를 INCR 로, 조각마다 `gap` 을 쉬며 `chunks` 개 보내는 시험용 소유자.
    /// 선택을 잡은 뒤 돌아오고, 소유자는 다른 스레드에서 돈다.
    fn slow_incr_owner(chunks: usize, gap: Duration) -> std::thread::JoinHandle<()> {
        let s = open(EventMask::NO_EVENT).expect("owner connection");
        s.conn
            .set_selection_owner(s.win, s.atoms.CLIPBOARD, CURRENT_TIME)
            .expect("own");
        s.conn.sync().expect("sync");
        std::thread::spawn(move || {
            let a = &s.atoms;
            let mut sending: Option<(u32, u32, usize)> = None;
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(10) {
                let Ok(Some(event)) = s.conn.poll_for_event() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                match event {
                    Event::SelectionRequest(req) => {
                        if req.target == a.TARGETS {
                            s.conn
                                .change_property32(
                                    PropMode::REPLACE,
                                    req.requestor,
                                    req.property,
                                    AtomEnum::ATOM,
                                    &[a.TARGETS, a.URI_LIST],
                                )
                                .expect("targets");
                        } else {
                            s.conn
                                .change_window_attributes(
                                    req.requestor,
                                    &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                                        .event_mask(EventMask::PROPERTY_CHANGE),
                                )
                                .expect("watch requestor");
                            s.conn
                                .change_property32(
                                    PropMode::REPLACE,
                                    req.requestor,
                                    req.property,
                                    a.INCR,
                                    &[1024],
                                )
                                .expect("incr");
                            sending = Some((req.requestor, req.property, 0));
                        }
                        let notify = SelectionNotifyEvent {
                            response_type: SELECTION_NOTIFY_EVENT,
                            sequence: 0,
                            time: req.time,
                            requestor: req.requestor,
                            selection: req.selection,
                            target: req.target,
                            property: req.property,
                        };
                        s.conn
                            .send_event(false, req.requestor, EventMask::NO_EVENT, notify)
                            .expect("notify");
                        s.conn.flush().expect("flush");
                    }
                    Event::PropertyNotify(ev) if ev.state == Property::DELETE => {
                        let Some((win, prop, sent)) = sending else {
                            continue;
                        };
                        if ev.window != win || ev.atom != prop {
                            continue;
                        }
                        std::thread::sleep(gap);
                        let chunk: &[u8] = if sent < chunks {
                            b"file:///tmp/slow\r\n"
                        } else {
                            b""
                        };
                        // 요청자가 이미 떠났으면 실패한다. 시험은 읽는 쪽만 본다.
                        if s.conn
                            .change_property8(PropMode::REPLACE, win, prop, a.URI_LIST, chunk)
                            .is_err()
                            || s.conn.flush().is_err()
                        {
                            return;
                        }
                        if sent >= chunks {
                            // 끝 조각을 읽는 쪽이 가져갈 때까지 연결을 둔다. 바로 닫으면 소유자가
                            // 사라져 읽는 쪽이 끝 조각을 받기 전에 실패할 수 있다.
                            linger_until_taken(&s, win, prop);
                            return;
                        }
                        sending = Some((win, prop, sent + 1));
                    }
                    Event::SelectionClear(_) => return,
                    _ => {}
                }
            }
        })
    }

    /// 읽는 쪽이 `prop` 을 지울 때(끝 조각을 가져갈 때)까지, 길어도 300ms 기다린다.
    fn linger_until_taken(s: &Session, win: u32, prop: u32) {
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            match s.conn.poll_for_event() {
                Ok(Some(Event::PropertyNotify(ev)))
                    if ev.state == Property::DELETE && ev.window == win && ev.atom == prop =>
                {
                    return;
                }
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => return,
            }
        }
    }

    /// 조각을 천천히 보내는 소유자도 읽기 한 번의 한도 안에서 끝난다. 실제 X 서버가 필요하다:
    /// `DISPLAY=:N cargo test -- --ignored a_slow_incr_owner`.
    #[test]
    #[ignore = "takes the CLIPBOARD selection; run on an isolated display"]
    fn a_slow_incr_owner_is_unreadable_within_the_read_timeout() {
        let _clipboard = super::super::os_clipboard_test_lock();
        // 10 조각 × 150ms = 1.5초. 조각마다 한도를 다시 잡으면 다 받아 Files 가 된다.
        let owner = slow_incr_owner(10, Duration::from_millis(150));
        let started = Instant::now();
        let clip = read_within(Duration::from_millis(500)).expect("read");
        let took = started.elapsed();
        assert!(matches!(clip, OsClip::Unreadable { .. }), "{clip:?}");
        assert!(took < Duration::from_millis(900), "read took {took:?}");
        // 읽는 쪽이 떠났으므로 소유자는 다음 삭제 알림을 받지 못한다. 기다리지 않는다.
        drop(owner);
    }

    /// 같은 소유자라도 한도 안에 다 보내면 읽는다(시험용 소유자 자체의 확인).
    #[test]
    #[ignore = "takes the CLIPBOARD selection; run on an isolated display"]
    fn a_quick_incr_owner_is_read() {
        let _clipboard = super::super::os_clipboard_test_lock();
        let owner = slow_incr_owner(3, Duration::from_millis(10));
        let clip = read_within(Duration::from_secs(2)).expect("read");
        let OsClip::Files(files) = clip else {
            panic!("{clip:?}");
        };
        assert_eq!(files.paths, vec![PathBuf::from("/tmp/slow"); 3]);
        if let Err(e) = owner.join() {
            std::panic::resume_unwind(e);
        }
    }
}
