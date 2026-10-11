//! XDND source on a dedicated connection. Only winit supplies gesture coordinates/buttons.
//! This connection neither grabs nor queries the pointer and never owns CLIPBOARD.
use super::Outcome;
use std::time::{Duration, Instant};
use x11rb::{
    connection::Connection,
    errors::ReplyError,
    protocol::{ErrorKind, Event, xproto::*},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
x11rb::atom_manager! {
    pub(super) Atoms: AtomsCookie {
        XdndAware, XdndProxy, XdndEnter, XdndPosition, XdndStatus, XdndLeave, XdndDrop,
        XdndFinished, XdndSelection, XdndActionCopy, XdndActionList,
        TARGETS, TIMESTAMP, INCR, _NET_WM_PID, _TASTY_DRAG_CLOCK,
        uri: b"text/uri-list",
    }
}
const PEER_TIMEOUT: Duration = Duration::from_secs(10);
const CHUNK: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Target {
    window: Window,
    recipient: Window,
    version: u32,
}
struct Transfer {
    window: Window,
    property: Atom,
    offset: usize,
}

pub(super) struct Session {
    conn: RustConnection,
    atoms: Atoms,
    source: Window,
    origin: Window,
    root: Window,
    bytes: Vec<u8>,
    target: Option<Target>,
    accepted: bool,
    awaiting_status: bool,
    queued: Option<(i16, i16)>,
    released: bool,
    dropped: bool,
    deadline: Option<Instant>,
    outcome: Option<Outcome>,
    timestamp: Timestamp,
    transfers: Vec<Transfer>,
}
impl Session {
    pub(super) fn start(origin: Window, bytes: Vec<u8>) -> Result<Self> {
        let (conn, _) = x11rb::connect(None)?;
        let root = conn.get_geometry(origin)?.reply()?.root;
        let atoms = Atoms::new(&conn)?.reply()?;
        let source = conn.generate_id()?;
        conn.create_window(
            0,
            source,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?
        .check()?;
        conn.change_property32(
            PropMode::REPLACE,
            source,
            atoms.XdndActionList,
            AtomEnum::ATOM,
            &[atoms.XdndActionCopy],
        )?;
        // PropertyNotify supplies an actual server timestamp without sampling any input device.
        conn.change_property8(
            PropMode::REPLACE,
            source,
            atoms._TASTY_DRAG_CLOCK,
            AtomEnum::INTEGER,
            &[0],
        )?;
        conn.set_selection_owner(source, atoms.XdndSelection, x11rb::CURRENT_TIME)?
            .check()?;
        if conn
            .get_selection_owner(atoms.XdndSelection)?
            .reply()?
            .owner
            != source
        {
            return Err("could not acquire XdndSelection".into());
        }
        conn.flush()?;
        Ok(Self {
            conn,
            atoms,
            source,
            origin,
            root,
            bytes,
            target: None,
            accepted: false,
            awaiting_status: false,
            queued: None,
            released: false,
            dropped: false,
            deadline: None,
            outcome: None,
            timestamp: 0,
            transfers: Vec::new(),
        })
    }
    fn property(
        &self,
        window: Window,
        atom: Atom,
        ty: Atom,
    ) -> std::result::Result<Option<u32>, ReplyError> {
        let reply = self
            .conn
            .get_property(false, window, atom, ty, 0, 1)?
            .reply()?;
        Ok(reply.value32().and_then(|mut values| values.next()))
    }
    fn target(&self, window: Window) -> Result<Option<Target>> {
        if let Some(proxy) = self.property(window, self.atoms.XdndProxy, AtomEnum::WINDOW.into())? {
            let awareness = (|| {
                if self.property(proxy, self.atoms.XdndProxy, AtomEnum::WINDOW.into())?
                    != Some(proxy)
                {
                    return Ok(None);
                }
                self.property(proxy, self.atoms.XdndAware, AtomEnum::ATOM.into())
                    .map(Some)
            })();
            match awareness {
                Ok(Some(version)) => {
                    return Ok(version.map(|version| Target {
                        window,
                        recipient: proxy,
                        version: version.min(5),
                    }));
                }
                // XDND requires ignoring a stale proxy, including one lost during validation.
                Err(ReplyError::X11Error(error)) if error.error_kind == ErrorKind::Window => {}
                Err(error) => return Err(error.into()),
                Ok(None) => {}
            }
        }
        Ok(self
            .property(window, self.atoms.XdndAware, AtomEnum::ATOM.into())?
            .map(|version| Target {
                window,
                recipient: window,
                version: version.min(5),
            }))
    }
    fn target_at(&self, x: i16, y: i16) -> Result<Option<Target>> {
        // Walk the windows at these forwarded coordinates, never XQueryPointer.
        let mut window = self.root;
        let mut candidate = None;
        for _ in 0..64 {
            if window == self.origin
                || self.property(window, self.atoms._NET_WM_PID, AtomEnum::CARDINAL.into())?
                    == Some(std::process::id())
            {
                return Ok(None);
            }
            if let Some(target) = self.target(window)? {
                candidate = Some(target);
            }
            let child = self
                .conn
                .translate_coordinates(self.root, window, x, y)?
                .reply()?
                .child;
            if child == x11rb::NONE {
                break;
            }
            window = child;
        }
        Ok(candidate)
    }
    fn send(&self, target: Target, ty: Atom, data: [u32; 5]) -> Result<()> {
        self.conn.send_event(
            false,
            target.recipient,
            EventMask::NO_EVENT,
            ClientMessageEvent::new(32, target.window, ty, data),
        )?;
        self.conn.flush()?;
        Ok(())
    }
    fn leave(&mut self) -> Result<()> {
        if let Some(target) = self.target.take() {
            self.send(target, self.atoms.XdndLeave, [self.source, 0, 0, 0, 0])?;
        }
        self.accepted = false;
        self.awaiting_status = false;
        Ok(())
    }
    pub(super) fn motion(&mut self, x: i16, y: i16) -> Result<()> {
        if self.released || self.outcome.is_some() {
            return Ok(());
        }
        let point = self
            .conn
            .translate_coordinates(self.origin, self.root, x, y)?
            .reply()?;
        let next = self.target_at(point.dst_x, point.dst_y)?;
        if self.target != next {
            self.leave()?;
            self.queued = None;
            self.target = next;
            if let Some(target) = next {
                self.send(
                    target,
                    self.atoms.XdndEnter,
                    [self.source, target.version << 24, self.atoms.uri, 0, 0],
                )?;
            }
        }
        self.queued = Some((point.dst_x, point.dst_y));
        self.accepted = false;
        self.send_position()
    }
    fn send_position(&mut self) -> Result<()> {
        if self.awaiting_status || self.timestamp == 0 {
            return Ok(());
        }
        if let (Some(target), Some((x, y))) = (self.target, self.queued.take()) {
            self.send(
                target,
                self.atoms.XdndPosition,
                [
                    self.source,
                    0,
                    ((x as u16 as u32) << 16) | y as u16 as u32,
                    self.timestamp,
                    self.atoms.XdndActionCopy,
                ],
            )?;
            self.awaiting_status = true;
            self.deadline = Some(Instant::now() + PEER_TIMEOUT);
        }
        Ok(())
    }
    pub(super) fn release(&mut self) -> Result<()> {
        if !self.released {
            self.released = true;
            self.deadline = Some(Instant::now() + PEER_TIMEOUT);
            self.try_drop()?;
        }
        Ok(())
    }
    fn try_drop(&mut self) -> Result<()> {
        if !self.released || self.awaiting_status || self.dropped {
            return Ok(());
        }
        if self.accepted
            && let Some(target) = self.target
        {
            self.send(
                target,
                self.atoms.XdndDrop,
                [self.source, 0, self.timestamp, 0, 0],
            )?;
            self.dropped = true;
        } else {
            self.leave()?;
            self.outcome = Some(Outcome::Cancelled);
        }
        Ok(())
    }
    pub(super) fn accepted(&self) -> bool {
        self.accepted && !self.awaiting_status && self.queued.is_none()
    }
    pub(super) fn poll(&mut self) -> Result<Option<Outcome>> {
        while let Some(event) = self.conn.poll_for_event()? {
            match event {
                Event::PropertyNotify(e)
                    if e.window == self.source && e.atom == self.atoms._TASTY_DRAG_CLOCK =>
                {
                    self.timestamp = e.time;
                    self.send_position()?;
                }
                Event::ClientMessage(e) if e.window == self.source && e.format == 32 => {
                    let d = e.data.as_data32();
                    if self.target.is_none_or(|t| t.window != d[0]) {
                        continue;
                    }
                    if e.type_ == self.atoms.XdndStatus && self.awaiting_status && !self.dropped {
                        self.awaiting_status = false;
                        if !self.released {
                            self.deadline = None;
                        }
                        self.accepted = d[1] & 1 != 0 && d[4] == self.atoms.XdndActionCopy;
                        if self.queued.is_some() {
                            self.accepted = false;
                            self.send_position()?;
                        }
                        self.try_drop()?;
                    } else if e.type_ == self.atoms.XdndFinished && self.dropped {
                        self.outcome = Some(if self.target.is_some_and(|t| t.version >= 5) {
                            if d[1] & 1 != 0 && d[2] == self.atoms.XdndActionCopy {
                                Outcome::Copied
                            } else {
                                Outcome::Cancelled
                            }
                        } else {
                            Outcome::Unknown
                        });
                    }
                }
                Event::SelectionRequest(e) => self.selection(e)?,
                Event::PropertyNotify(e) if e.state == Property::DELETE => {
                    self.increment(e.window, e.atom)?
                }
                Event::SelectionClear(e) if e.selection == self.atoms.XdndSelection => {
                    self.outcome = Some(Outcome::Cancelled)
                }
                Event::Error(e) => return Err(format!("XDND protocol error: {e:?}").into()),
                _ => {}
            }
        }
        if self.deadline.is_some_and(|d| Instant::now() >= d) && self.outcome.is_none() {
            self.outcome = Some(if self.dropped {
                Outcome::Unknown
            } else {
                Outcome::Cancelled
            });
        }
        Ok(self.outcome)
    }
    fn selection(&mut self, e: SelectionRequestEvent) -> Result<()> {
        let property = if e.property == x11rb::NONE {
            e.target
        } else {
            e.property
        };
        let valid = e.selection == self.atoms.XdndSelection
            && (e.time == 0 || e.time.wrapping_sub(self.timestamp) < (1 << 31));
        let mut supplied = false;
        if valid && e.target == self.atoms.TARGETS {
            self.conn.change_property32(
                PropMode::REPLACE,
                e.requestor,
                property,
                AtomEnum::ATOM,
                &[self.atoms.uri, self.atoms.TARGETS, self.atoms.TIMESTAMP],
            )?;
            supplied = true;
        } else if valid && e.target == self.atoms.TIMESTAMP {
            self.conn.change_property32(
                PropMode::REPLACE,
                e.requestor,
                property,
                AtomEnum::INTEGER,
                &[self.timestamp],
            )?;
            supplied = true;
        } else if valid && e.target == self.atoms.uri && self.transfers.len() < 8 {
            if self.bytes.len() > CHUNK {
                self.conn.change_window_attributes(
                    e.requestor,
                    &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                )?;
                self.conn.change_property32(
                    PropMode::REPLACE,
                    e.requestor,
                    property,
                    self.atoms.INCR,
                    &[self.bytes.len() as u32],
                )?;
                self.transfers.push(Transfer {
                    window: e.requestor,
                    property,
                    offset: 0,
                });
            } else {
                self.conn.change_property8(
                    PropMode::REPLACE,
                    e.requestor,
                    property,
                    self.atoms.uri,
                    &self.bytes,
                )?;
            }
            supplied = true;
        }
        self.conn.send_event(
            false,
            e.requestor,
            EventMask::NO_EVENT,
            SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: 0,
                time: e.time,
                requestor: e.requestor,
                selection: e.selection,
                target: e.target,
                property: if supplied { property } else { x11rb::NONE },
            },
        )?;
        self.conn.flush()?;
        Ok(())
    }
    fn increment(&mut self, window: Window, property: Atom) -> Result<()> {
        if let Some(index) = self
            .transfers
            .iter()
            .position(|t| t.window == window && t.property == property)
        {
            let transfer = &mut self.transfers[index];
            let end = (transfer.offset + CHUNK).min(self.bytes.len());
            self.conn.change_property8(
                PropMode::REPLACE,
                window,
                property,
                self.atoms.uri,
                &self.bytes[transfer.offset..end],
            )?;
            if transfer.offset == self.bytes.len() {
                self.transfers.swap_remove(index);
            } else {
                transfer.offset = end;
            }
            self.deadline = Some(Instant::now() + PEER_TIMEOUT);
            self.conn.flush()?;
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if !self.dropped
            && let Err(error) = self.leave()
        {
            tracing::debug!(%error, "XDND leave during cleanup");
        }
        // Disconnecting releases our selection, properties and source window, never a later owner's selection.
    }
}

#[cfg(test)]
#[path = "x11/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "x11/proxy_tests.rs"]
mod proxy_tests;
