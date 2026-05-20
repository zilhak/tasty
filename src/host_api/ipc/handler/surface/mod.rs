//! `surface.*` IPC 핸들러 도메인 sub-module 모음.

mod close;
mod commands;
mod list;
mod mark;
mod query;
mod send;

pub(crate) use close::{handle_surface_close, handle_surface_close_self};
pub(crate) use commands::{handle_command_at, handle_commands, handle_last_command};
pub(crate) use list::handle_surface_list;
pub(crate) use mark::{handle_parse_since_mark, handle_read_since_mark, handle_set_mark};
pub(crate) use query::{
    handle_cursor_position, handle_foreground_process, handle_screen_text, handle_surface_locate,
    handle_surface_respawn_terminal,
};
pub(crate) use send::{
    handle_surface_send, handle_surface_send_combo, handle_surface_send_key,
    handle_surface_send_to, handle_surface_wake,
};

pub(super) use super::{caller_surface_id, require_surface_id};
