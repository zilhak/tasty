//! Charge observer copies separately from the common worker input/result slots.
use super::FileAction;
use crate::app::file_writer::command::value_bytes;
use std::sync::{Arc, Mutex};
use tasty_utils::batch_writer::Limits;
#[derive(Default)]
struct Usage {
    items: usize,
    bytes: usize,
}
#[derive(Default)]
pub(super) struct Budget {
    usage: Arc<Mutex<Usage>>,
    limits: Limits,
}
pub(super) struct Reservation {
    usage: Arc<Mutex<Usage>>,
    bytes: usize,
}
impl Budget {
    pub(super) fn reserve(&self, bytes: usize) -> Result<Reservation, &'static str> {
        let mut usage = self.usage.lock().unwrap();
        let items = usage
            .items
            .checked_add(1)
            .filter(|n| *n <= self.limits.queue_items);
        let total = usage
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.queue_bytes);
        let (Some(items), Some(total)) = (items, total) else {
            return Err("remote file continuation capacity exhausted");
        };
        usage.items = items;
        usage.bytes = total;
        Ok(Reservation {
            usage: self.usage.clone(),
            bytes,
        })
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut usage = self.usage.lock().unwrap();
        usage.items -= 1;
        usage.bytes -= self.bytes;
    }
}
fn strings(values: &[&str]) -> usize {
    values
        .iter()
        .fold(0usize, |sum, value| sum.saturating_add(value.len()))
}
pub(super) fn weight(action: &FileAction) -> usize {
    let owned = match action {
        FileAction::SaveProfile(f) => strings(&[
            &f.kind,
            &f.name,
            &f.label,
            &f.host,
            &f.user,
            &f.port,
            &f.shell,
            &f.passkey_ref,
        ])
        .saturating_add(f.fields.iter().fold(0usize, |sum, (key, value)| {
            sum.saturating_add(key.len())
                .saturating_add(value.len())
                .saturating_add(48)
        }))
        .saturating_add(f.editing_original.as_ref().map_or(0, String::len))
        .saturating_add(f.original.as_ref().map_or(0, value_bytes)),
        FileAction::SaveAttach(f) => strings(&[
            &f.name,
            &f.label,
            &f.ssh_ref,
            &f.host,
            &f.user,
            &f.port,
            &f.shell,
            &f.passkey_ref,
            &f.remote_tasty,
            &f.port_mode,
            &f.port_file,
        ])
        .saturating_add(f.editing_original.as_ref().map_or(0, String::len))
        .saturating_add(f.original.as_ref().map_or(0, value_bytes)),
        FileAction::SavePasskey(f) => strings(&[&f.name, &f.kind, &f.value])
            .saturating_add(f.editing_original.as_ref().map_or(0, String::len)),
        FileAction::DeleteProfile(name)
        | FileAction::DeletePasskey(name)
        | FileAction::Detect(name) => name.len(),
        FileAction::Reveal { key, .. } => value_bytes(key),
    };
    owned
        .saturating_add(std::mem::size_of::<super::Waiting>())
        .saturating_add(256)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn budget() -> Budget {
        Budget {
            limits: Limits {
                queue_items: 2,
                queue_bytes: 10,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    #[test]
    fn empty_budget_refuses_oversize_and_arithmetic_overflow() {
        let budget = budget();
        assert!(budget.reserve(11).is_err());
        assert!(budget.reserve(usize::MAX).is_err());
        assert_eq!(budget.usage.lock().unwrap().items, 0);
        assert_eq!(budget.usage.lock().unwrap().bytes, 0);
    }
    #[test]
    fn aggregate_bytes_remain_charged_until_the_retained_owner_drops() {
        let budget = budget();
        let first = budget.reserve(6).expect("within bound");
        assert!(budget.reserve(5).is_err());
        let second = budget.reserve(4).expect("exact bound");
        assert!(budget.reserve(0).is_err());
        drop(first);
        assert_eq!(budget.usage.lock().unwrap().bytes, 4);
        let next = budget
            .reserve(6)
            .expect("released reservation can be reused");
        drop((second, next));
        assert_eq!(budget.usage.lock().unwrap().items, 0);
        assert_eq!(budget.usage.lock().unwrap().bytes, 0);
    }
    #[test]
    fn inline_form_estimate_includes_secret_and_original_name_without_logging_them() {
        let short = super::super::PasskeyForm {
            name: "owned".into(),
            kind: "inline".into(),
            ..Default::default()
        };
        let mut long = short.clone();
        long.value = "x".repeat(1024);
        long.editing_original = Some("previous".into());
        assert_eq!(
            weight(&FileAction::SavePasskey(long)) - weight(&FileAction::SavePasskey(short)),
            1032
        );
    }
}
