//! Native HTTP context storage preparation and ownership boundaries.
#[cfg(target_os = "linux")]
pub(crate) mod preparation;
#[cfg(target_os = "linux")]
pub(crate) mod profile;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
