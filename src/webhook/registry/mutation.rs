//! Prepare one mutation from the last settled state; no tentative section can escape a rollback.
use super::*;

pub(super) enum Operation {
    Register(WebhookEntry),
    Remove(String),
    Sweep,
    Persist,
    Match {
        id: String,
        method: String,
        evidence: Option<(Arc<()>, bool)>,
    },
}
impl Operation {
    pub(super) fn bytes(&self) -> usize {
        match self {
            Self::Register(entry) => entry_bytes(entry),
            Self::Remove(id) => id.len().saturating_add(128),
            Self::Match { id, method, .. } => {
                id.len().saturating_add(method.len()).saturating_add(128)
            }
            Self::Sweep | Self::Persist => 128,
        }
    }
}
pub(super) fn entry_bytes(entry: &WebhookEntry) -> usize {
    command::value_bytes(&(
        &entry.id,
        &entry.methods,
        entry.handler_id.as_ref().map(|h| &h.0),
        &entry.calls,
        &entry.auth,
    ))
    .saturating_add(256)
}
pub(super) enum Change {
    None,
    Insert(Entry),
    Remove(Vec<(String, Arc<()>)>),
    Lifetime(String, Arc<()>, Lifetime),
}
pub(super) struct Prepared {
    pub(super) change: Change,
    pub(super) value: Value,
    pub(super) persistent: bool,
    pub(super) continue_on_failure: bool,
}
impl Prepared {
    fn unchanged(value: Value) -> Self {
        Self {
            change: Change::None,
            value,
            persistent: false,
            continue_on_failure: false,
        }
    }
    pub(super) fn apply(&mut self, s: &mut State) {
        match std::mem::replace(&mut self.change, Change::None) {
            Change::None => {}
            Change::Insert(entry) => {
                s.entries.entry(entry.id.clone()).or_insert(entry);
            }
            Change::Remove(ids) => {
                for (id, identity) in ids {
                    if s.entries
                        .get(&id)
                        .is_some_and(|e| Arc::ptr_eq(&e.identity, &identity))
                    {
                        s.entries.remove(&id);
                    }
                }
            }
            Change::Lifetime(id, identity, lifetime) => {
                if let Some(e) = s.entries.get_mut(&id)
                    && Arc::ptr_eq(&e.identity, &identity)
                {
                    e.value.lifetime = lifetime;
                }
            }
        }
    }
    fn retain(&self, id: &str) -> bool {
        !matches!(&self.change,Change::Remove(ids) if ids.iter().any(|(gone,_)|gone==id))
    }
    pub(super) fn section_bytes(&self, s: &State) -> usize {
        let mut bytes = s
            .entries
            .values()
            .filter(|e| e.lifetime.is_persistent() && self.retain(&e.id))
            .fold(128usize, |n, e| n.saturating_add(entry_bytes(e)));
        if let Change::Insert(e) = &self.change
            && e.lifetime.is_persistent()
        {
            bytes = bytes.saturating_add(entry_bytes(e));
        }
        bytes
    }
    pub(super) fn section(&self, s: &State) -> Vec<super::super::persist::PersistedWebhook> {
        let mut items: Vec<_> = s
            .entries
            .values()
            .filter(|e| e.lifetime.is_persistent() && self.retain(&e.id))
            .map(|e| {
                let mut value = super::super::persist::to_persisted(e);
                if let Change::Lifetime(id, _, lifetime) = &self.change
                    && id == &e.id
                {
                    value.limit = super::super::persist::PersistedLimit::from_limit(lifetime.limit);
                }
                value
            })
            .collect();
        if let Change::Insert(e) = &self.change
            && e.lifetime.is_persistent()
        {
            items.push(super::super::persist::to_persisted(e));
        }
        items
    }
}

pub(super) fn prepare(s: &State, op: Operation, reply: &Reply) -> Result<Prepared, Failure> {
    match op {
        Operation::Register(entry) => {
            let value = Value::Registered(RegisterOutcome {
                id: entry.id.clone(),
                url: build_url(s, &entry.id),
            });
            let persistent = entry.lifetime.is_persistent();
            Ok(Prepared {
                change: Change::Insert(Entry {
                    value: entry,
                    identity: Arc::new(()),
                }),
                value,
                persistent,
                continue_on_failure: false,
            })
        }
        Operation::Remove(id) => match s.entries.get(&id) {
            None => Ok(Prepared::unchanged(Value::Removed(false))),
            Some(e) => Ok(Prepared {
                change: Change::Remove(vec![(id, e.identity.clone())]),
                value: Value::Removed(true),
                persistent: e.lifetime.is_persistent(),
                continue_on_failure: false,
            }),
        },
        Operation::Sweep => {
            let now = now_unix();
            let bytes = s
                .entries
                .values()
                .filter(|e| e.lifetime.is_expired(now))
                .fold(128usize, |n, e| {
                    n.saturating_add(e.id.len().saturating_mul(2))
                        .saturating_add(128)
                });
            reply.grow(bytes)?;
            let ids: Vec<_> = s
                .entries
                .values()
                .filter(|e| e.lifetime.is_expired(now))
                .map(|e| (e.id.clone(), e.identity.clone()))
                .collect();
            let persistent = ids
                .iter()
                .any(|(id, _)| s.entries[id].lifetime.is_persistent());
            Ok(Prepared {
                value: Value::Swept(ids.iter().map(|(id, _)| id.clone()).collect()),
                change: Change::Remove(ids),
                persistent,
                continue_on_failure: true,
            })
        }
        Operation::Persist => Ok(Prepared {
            change: Change::None,
            value: Value::Persisted,
            persistent: true,
            continue_on_failure: true,
        }),
        Operation::Match {
            id,
            method,
            evidence,
        } => match_entry(s, id, method, evidence, reply),
    }
}
fn match_entry(
    s: &State,
    id: String,
    method: String,
    evidence: Option<(Arc<()>, bool)>,
    reply: &Reply,
) -> Result<Prepared, Failure> {
    let unmatched = |result| Ok(Prepared::unchanged(Value::Matched(result)));
    let Some(entry) = s.entries.get(&id) else {
        return unmatched(MatchResult::NotFound);
    };
    let Some((identity, authorized)) = evidence else {
        return unmatched(MatchResult::NotFound);
    };
    if !Arc::ptr_eq(&entry.identity, &identity) {
        return unmatched(MatchResult::NotFound);
    }
    let persistent = entry.lifetime.is_persistent();
    if entry.lifetime.is_expired(now_unix()) {
        return Ok(Prepared {
            change: Change::Remove(vec![(id, identity)]),
            value: Value::Matched(MatchResult::Expired),
            persistent,
            continue_on_failure: true,
        });
    }
    if !entry.methods.iter().any(|m| m == &method) {
        return unmatched(MatchResult::MethodNotAllowed);
    }
    if !authorized {
        return unmatched(MatchResult::Unauthorized);
    }
    reply.grow(entry_bytes(entry))?;
    let mut lifetime = entry.lifetime;
    let exhausted = lifetime.consume();
    let value = Value::Matched(MatchResult::Matched {
        calls: entry.calls.clone(),
        injector: s.injector.clone(),
    });
    Ok(Prepared {
        change: if exhausted {
            Change::Remove(vec![(id, identity)])
        } else {
            Change::Lifetime(id, identity, lifetime)
        },
        value,
        persistent,
        continue_on_failure: true,
    })
}
