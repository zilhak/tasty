//! Engine resource lifetimes and the committed journal execution boundary.
//! The storage worker owns canonical journal models; App publishes committed descriptor projections.
mod child_observation;
pub(crate) mod command_executor;
pub(crate) mod effect_runner;
pub(crate) mod engine_access;
pub(crate) mod engine_runtime;
pub(crate) mod engine_session;
pub(crate) mod journal;
pub(crate) mod journal_product;
pub(crate) mod surface_restorer;
mod terminal_access;
mod terminal_activity;
mod terminal_output;

#[cfg(test)]
mod tests;

pub(crate) mod agent;

pub(crate) mod child_terminal;

pub(crate) mod terminal_store;

pub(crate) mod terminal_spawn;

pub(crate) mod output_observer;

pub(crate) mod surface_registry;

pub(crate) mod egui_mesh_surface;

pub(crate) mod resource_retirement;

pub(crate) mod surface_cleanup;

pub(crate) mod host_events;

pub(crate) mod surface_capture;

pub(crate) mod id_reservations;

pub(crate) mod counters;

pub(crate) mod preset_plan;

pub(crate) mod journal_payload;
pub(crate) mod restored_presentation;

mod structure_observation;

pub(crate) mod pending_submit;

pub(crate) mod engine_read;

pub(crate) mod kind_catalog;

pub(crate) mod file_catalog;

pub(crate) mod registries;

mod pty;

#[cfg(feature = "gui")]
pub(crate) mod html_script;
#[cfg(feature = "gui")]
pub(crate) mod surface_binding;
