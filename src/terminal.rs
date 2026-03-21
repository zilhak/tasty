use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::escape::parser::Parser;
use termwiz::escape::Action;
use termwiz::surface::{Change, Surface};

pub struct Terminal {
    surface: Surface,
    parser: Parser,
    pty_writer: Box<dyn Write + Send>,
    action_rx: mpsc::Receiver<Vec<u8>>,
    _reader_thread: thread::JoinHandle<()>,
    cols: usize,
    rows: usize,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Result<Self> {
        let pty_system = NativePtySystem::default();

        let pair = pty_system.openpty(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = Self::default_shell();
        let mut cmd = CommandBuilder::new(&shell);
        cmd.env("TERM", "xterm-256color");

        let _child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let pty_writer = pair.master.take_writer()?;
        let mut pty_reader = pair.master.try_clone_reader()?;

        let (tx, rx) = mpsc::channel();

        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match pty_reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let surface = Surface::new(cols, rows);
        let parser = Parser::new();

        Ok(Self {
            surface,
            parser,
            pty_writer,
            action_rx: rx,
            _reader_thread: reader_thread,
            cols,
            rows,
        })
    }

    /// Process pending PTY output. Returns true if surface changed.
    pub fn process(&mut self) -> bool {
        let mut changed = false;

        while let Ok(data) = self.action_rx.try_recv() {
            let actions = self.parser.parse_as_vec(&data);
            let changes: Vec<Change> = actions
                .into_iter()
                .filter_map(|action| match action {
                    Action::Print(c) => Some(Change::Text(c.to_string())),
                    Action::PrintString(s) => Some(Change::Text(s)),
                    Action::Control(code) => {
                        use termwiz::escape::ControlCode;
                        match code {
                            ControlCode::LineFeed => Some(Change::Text("\n".to_string())),
                            ControlCode::CarriageReturn => {
                                Some(Change::Text("\r".to_string()))
                            }
                            ControlCode::Backspace => Some(Change::Text("\x08".to_string())),
                            ControlCode::Bell => None, // TODO: notification
                            ControlCode::HorizontalTab => {
                                Some(Change::Text("\t".to_string()))
                            }
                            _ => None,
                        }
                    }
                    Action::CSI(csi) => {
                        use termwiz::escape::csi::CSI;
                        match csi {
                            CSI::Sgr(_sgr) => Some(Change::AllAttributes(
                                termwiz::cell::CellAttributes::default(),
                            )),
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .collect();

            if !changes.is_empty() {
                for change in changes {
                    self.surface.add_change(change);
                }
                changed = true;
            }
        }

        changed
    }

    /// Send keyboard input to PTY
    pub fn send_key(&mut self, text: &str) {
        let _ = self.pty_writer.write_all(text.as_bytes());
        let _ = self.pty_writer.flush();
    }

    /// Send raw bytes to PTY
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        let _ = self.pty_writer.write_all(bytes);
        let _ = self.pty_writer.flush();
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.surface.resize(cols, rows);
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    fn default_shell() -> String {
        #[cfg(windows)]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }
        #[cfg(not(windows))]
        {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        }
    }
}
