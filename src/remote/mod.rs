//! Remote occupancy, transport queues and mirror presentation resources.
pub(crate) mod state;

pub(crate) mod server;

pub(crate) mod structure_sync;

#[cfg(feature="gui")]
pub(crate) mod readonly;

#[cfg(feature="gui")]
pub(crate) mod mesh_frames;

pub(crate) mod mesh_mirror;

pub(crate) mod capture_upload;

pub(crate) mod bulk_transfer;

#[cfg(feature="gui")]
pub(crate) mod client_session;
#[cfg(feature="gui")]
pub(crate) mod outbound;

#[cfg(feature="gui")]
pub(crate) mod transport;

#[cfg(feature="gui")]
pub(crate) mod connection;
