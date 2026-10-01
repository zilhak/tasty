//! Runtime consumption of already durable ID ranges. This module never issues IDs or opens SQLite.
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tasty_core::IdKind;

static REFILL_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
const KINDS: [IdKind; 4] = [
    IdKind::Workspace,
    IdKind::Pane,
    IdKind::Tab,
    IdKind::Surface,
];
#[derive(Debug)]
#[cfg(feature = "gui")]
pub(crate) enum ReservationError {
    Pending,
    Unavailable(String),
}
#[cfg(feature = "gui")]
impl std::fmt::Display for ReservationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => f.write_str("durable ID reservation is pending"),
            Self::Unavailable(error) => f.write_str(error),
        }
    }
}
#[cfg(feature = "gui")]
impl std::error::Error for ReservationError {}
#[cfg(feature = "gui")]
impl From<&str> for ReservationError {
    fn from(error: &str) -> Self {
        Self::Unavailable(error.into())
    }
}
#[derive(Default)]
struct Bank {
    failure: Option<String>,
    available: BTreeMap<IdKind, VecDeque<std::ops::Range<u64>>>,
    wanted: BTreeMap<IdKind, u64>,
}
#[derive(Clone, Default)]
pub(crate) struct IdReservations(Arc<Mutex<Bank>>);
#[cfg(feature = "gui")]
pub(crate) struct ReservedIds {
    ranges: Mutex<BTreeMap<IdKind, VecDeque<std::ops::Range<u64>>>>,
    bank: std::sync::Weak<Mutex<Bank>>,
}
#[cfg(feature = "gui")]
impl Drop for ReservedIds {
    fn drop(&mut self) {
        let (Some(bank), Ok(ranges)) = (self.bank.upgrade(), self.ranges.get_mut()) else {
            return;
        };
        let Ok(mut bank) = bank.lock() else {
            return;
        };
        // Only unissued suffixes return. Every ID handed to a constructor has already advanced
        // the cursor and remains burned even if that construction fails or is cancelled.
        for (kind, unused) in std::mem::take(ranges) {
            bank.available.entry(kind).or_default().extend(unused);
        }
    }
}
impl IdReservations {
    pub(crate) fn needed(&self) -> Result<Vec<(IdKind, u32)>, String> {
        let bank = self.0.lock().map_err(|_| "ID reservation bank poisoned")?;
        if bank.failure.is_some() {
            return Ok(Vec::new());
        }
        KINDS
            .into_iter()
            .filter_map(|kind| {
                let available = bank.available.get(&kind).map_or(0, |ranges| {
                    ranges.iter().map(|range| range.end - range.start).sum()
                });
                let wanted = bank.wanted.get(&kind).copied().unwrap_or(0).max(512);
                (available < 64 || available < wanted && bank.wanted.contains_key(&kind)).then(
                    || {
                        u32::try_from(wanted.saturating_sub(available))
                            .map(|count| (kind, count))
                            .map_err(|_| "ID reservation request exhausted".into())
                    },
                )
            })
            .collect()
    }
    pub(crate) fn supply(&self, ranges: Vec<tasty_event_store::IdRange>) -> Result<(), String> {
        let mut bank = self.0.lock().map_err(|_| "ID reservation bank poisoned")?;
        for range in ranges {
            let kind = KINDS
                .into_iter()
                .find(|kind| kind.label() == range.kind)
                .ok_or("unexpected auxiliary ID kind")?;
            let upper = if kind == IdKind::Surface {
                u64::from(crate::runtime::terminal_store::PTY_ID_BASE)
            } else {
                u64::from(u32::MAX) + 1
            };
            if range.start == 0 || range.start >= range.end || range.end > upper {
                return Err("invalid durable ID range".into());
            }
            bank.available
                .entry(kind)
                .or_default()
                .push_back(range.start..range.end);
        }
        let supplied: Vec<_> = bank
            .wanted
            .iter()
            .filter_map(|(kind, wanted)| {
                let available = bank.available.get(kind).map_or(0, |ranges| {
                    ranges
                        .iter()
                        .map(|range| range.end - range.start)
                        .sum::<u64>()
                });
                (available >= *wanted).then_some(*kind)
            })
            .collect();
        for kind in supplied {
            bank.wanted.remove(&kind);
        }
        Ok(())
    }
    pub(crate) fn refuse_refill(&self, reason: String) {
        match self.0.lock() {
            Ok(mut bank) => bank.failure = Some(reason),
            Err(error) => {
                // Multi-kind leasing may have advanced only part of a reservation. Do not
                // recover or clear poison: every subsequent ID operation must still fail.
                if !REFILL_POISON_REPORTED.swap(true, Ordering::Relaxed) {
                    tracing::error!(%error, %reason,
                        "cannot record ID refill refusal; poisoned reservation bank remains unavailable");
                }
            }
        }
    }
    #[cfg(feature = "gui")]
    pub(crate) fn ensure(&self, needed: &[(IdKind, u32)]) -> Result<(), ReservationError> {
        let mut bank = self
            .0
            .lock()
            .map_err(|_| ReservationError::from("ID reservation bank poisoned"))?;
        if let Some(error) = &bank.failure {
            return Err(ReservationError::Unavailable(error.clone()));
        }
        let mut pending = false;
        for (kind, count) in needed {
            let available = bank.available.get(kind).map_or(0, |ranges| {
                ranges
                    .iter()
                    .map(|range| range.end - range.start)
                    .sum::<u64>()
            });
            if available < u64::from(*count) {
                bank.wanted.insert(*kind, u64::from(*count));
                pending = true;
            }
        }
        if pending {
            Err(ReservationError::Pending)
        } else {
            Ok(())
        }
    }
    /// Preflight and consume all ranges together, before installing any mirror resources.
    #[cfg(feature = "gui")]
    pub(crate) fn lease(&self, needed: &[(IdKind, u32)]) -> Result<ReservedIds, ReservationError> {
        let mut bank = self.0.lock().map_err(|_| "ID reservation bank poisoned")?;
        if let Some(error) = &bank.failure {
            return Err(ReservationError::Unavailable(error.clone()));
        }
        let mut insufficient = false;
        for (kind, count) in needed {
            let available = bank.available.get(kind).map_or(0, |ranges| {
                ranges
                    .iter()
                    .map(|range| range.end - range.start)
                    .sum::<u64>()
            });
            if available < u64::from(*count) {
                bank.wanted.insert(*kind, u64::from(*count));
                insufficient = true;
            }
        }
        if insufficient {
            return Err(ReservationError::Pending);
        }
        let mut leased = BTreeMap::new();
        for (kind, count) in needed {
            let ranges = bank.available.entry(*kind).or_default();
            let mut remaining = u64::from(*count);
            let out = leased.entry(*kind).or_insert_with(VecDeque::new);
            while remaining > 0 {
                let range = ranges.front_mut().ok_or("preflight ID capacity changed")?;
                let take = remaining.min(range.end - range.start);
                out.push_back(range.start..range.start + take);
                range.start += take;
                remaining -= take;
                if range.start == range.end {
                    ranges.pop_front();
                }
            }
        }
        Ok(ReservedIds {
            ranges: Mutex::new(leased),
            bank: Arc::downgrade(&self.0),
        })
    }
}
#[cfg(feature = "gui")]
impl ReservedIds {
    fn next(&self, kind: IdKind) -> anyhow::Result<u32> {
        let mut ranges = self
            .ranges
            .lock()
            .map_err(|_| anyhow::anyhow!("reserved ID lease poisoned"))?;
        let ranges = ranges
            .get_mut(&kind)
            .ok_or_else(|| anyhow::anyhow!("no {} identity reserved", kind.label()))?;
        let range = ranges
            .front_mut()
            .ok_or_else(|| anyhow::anyhow!("reserved {} identities exhausted", kind.label()))?;
        let value = range.start;
        range.start += 1;
        if range.start == range.end {
            ranges.pop_front();
        }
        Ok(u32::try_from(value)?)
    }
    pub(crate) fn next_workspace(&self) -> anyhow::Result<u32> {
        self.next(IdKind::Workspace)
    }
    pub(crate) fn next_pane(&self) -> anyhow::Result<u32> {
        self.next(IdKind::Pane)
    }
    pub(crate) fn next_tab(&self) -> anyhow::Result<u32> {
        self.next(IdKind::Tab)
    }
    pub(crate) fn next_surface(&self) -> anyhow::Result<u32> {
        self.next(IdKind::Surface)
    }
}
