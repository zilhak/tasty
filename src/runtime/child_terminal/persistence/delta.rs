//! Field deltas never restore an unchanged counter from an older queued cache.
use super::super::{ChildEntry, ChildTerminalRegistry as Registry};
use std::{collections::HashMap, hash::Hash};

#[derive(Default)]
pub(super) struct Delta {
    parent: Vec<(u32, Option<u32>)>,
    external_parent: Vec<(u32, Option<String>)>,
    idle: Vec<(u32, Option<bool>)>,
    needs_input: Vec<(u32, Option<bool>)>,
    reported: Vec<(u32, Option<u64>)>,
    slots: Vec<(u32, Option<u32>)>,
    children: Vec<Relation<u32>>,
    external_children: Vec<Relation<String>>,
    next: Vec<(u32, Option<u32>, Option<u32>)>,
    external_next: Vec<(String, Option<u32>, Option<u32>)>,
    empty_parents: Vec<(u32, bool)>,
    removed: HashMap<String, u32>,
}

struct Relation<K> {
    parent: K,
    child: u32,
    before: Option<ChildEntry>,
    after: Option<ChildEntry>,
}

impl Delta {
    pub(super) fn between(before: &Registry, after: &Registry) -> Self {
        let owned = |id: &u32| after.owned.contains(id);
        Self {
            parent: changes(&before.parent_of, &after.parent_of, owned),
            external_parent: changes(&before.external_parent_of, &after.external_parent_of, owned),
            idle: state_changes(before, after, true),
            needs_input: state_changes(before, after, false),
            reported: changes(
                &before.last_state_report_at,
                &after.last_state_report_at,
                owned,
            ),
            slots: changes(&before.slot_of, &after.slot_of, owned),
            children: relations(&before.children, &after.children, owned),
            external_children: relations(
                &before.external_children,
                &after.external_children,
                owned,
            ),
            next: counters(&before.next_index, &after.next_index, owned),
            external_next: counters(
                &before.external_next_index,
                &after.external_next_index,
                |_| true,
            ),
            empty_parents: before
                .children
                .keys()
                .chain(after.children.keys())
                .filter(|id| owned(id))
                .filter(|id| before.children.contains_key(id) != after.children.contains_key(id))
                .map(|id| (*id, after.children.contains_key(id)))
                .collect(),
            removed: after.removed_external_top.clone(),
        }
    }

    pub(super) fn apply(&self, disk: &mut Registry) {
        let mut bounds = self.removed.clone();
        // Include known removed indices from earlier saves, never a cached next-index
        // that may include a reservation whose child has not registered yet.
        for (parent, list) in &disk.external_children {
            if let Some(bound) = bounds.get_mut(parent) {
                for child in list {
                    *bound = (*bound).max(child.index.saturating_add(1));
                }
            }
        }
        apply_map(&mut disk.parent_of, &self.parent);
        apply_map(&mut disk.external_parent_of, &self.external_parent);
        apply_map(&mut disk.idle, &self.idle);
        apply_map(&mut disk.needs_input, &self.needs_input);
        apply_map(&mut disk.last_state_report_at, &self.reported);
        apply_map(&mut disk.slot_of, &self.slots);
        apply_relations(&mut disk.children, &self.children);
        apply_relations(&mut disk.external_children, &self.external_children);
        for (parent, present) in &self.empty_parents {
            if *present {
                disk.children.entry(*parent).or_default();
            } else if disk.children.get(parent).is_some_and(Vec::is_empty) {
                disk.children.remove(parent);
            }
        }
        disk.external_children.retain(|_, list| !list.is_empty());
        apply_counters(&mut disk.next_index, &self.next);
        let increases: Vec<_> = self
            .external_next
            .iter()
            .filter(|(_, _, next)| next.is_some())
            .cloned()
            .collect();
        apply_counters(&mut disk.external_next_index, &increases);
        for (parent, bound) in &bounds {
            if !disk.external_children.contains_key(parent)
                && !disk
                    .external_next_index
                    .get(parent)
                    .is_some_and(|next| next > bound)
            {
                disk.external_next_index.remove(parent);
            }
        }
        bounds.retain(|parent, _| disk.external_children.contains_key(parent));
        disk.removed_external_top = bounds;
    }
}

fn state_changes(before: &Registry, after: &Registry, idle: bool) -> Vec<(u32, Option<bool>)> {
    let (old, new) = if idle {
        (&before.idle, &after.idle)
    } else {
        (&before.needs_input, &after.needs_input)
    };
    let mut delta = changes(old, new, |id| after.owned.contains(id));
    let (old_intents, new_intents) = if idle {
        (&before.idle_intents, &after.idle_intents)
    } else {
        (&before.needs_input_intents, &after.needs_input_intents)
    };
    for (child, intent) in new_intents {
        if old_intents.get(child) == Some(intent) || !after.owned.contains(child) {
            continue;
        }
        delta.retain(|(id, _)| id != child);
        delta.push((*child, Some(intent.1)));
    }
    delta
}

fn changes<K: Clone + Eq + Hash, V: Clone + Eq>(
    before: &HashMap<K, V>,
    after: &HashMap<K, V>,
    owned: impl Fn(&K) -> bool,
) -> Vec<(K, Option<V>)> {
    let keys: std::collections::HashSet<_> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .filter(|k| owned(k) && before.get(k) != after.get(k))
        .map(|k| (k.clone(), after.get(k).cloned()))
        .collect()
}
fn apply_map<K: Clone + Eq + Hash, V: Clone>(disk: &mut HashMap<K, V>, delta: &[(K, Option<V>)]) {
    for (key, value) in delta {
        match value {
            Some(value) => {
                disk.insert(key.clone(), value.clone());
            }
            None => {
                disk.remove(key);
            }
        }
    }
}
fn counters<K: Clone + Eq + Hash>(
    before: &HashMap<K, u32>,
    after: &HashMap<K, u32>,
    owned: impl Fn(&K) -> bool,
) -> Vec<(K, Option<u32>, Option<u32>)> {
    changes(before, after, owned)
        .into_iter()
        .map(|(key, next)| {
            let previous = before.get(&key).copied();
            (key, previous, next)
        })
        .collect()
}
fn apply_counters<K: Clone + Eq + Hash>(
    disk: &mut HashMap<K, u32>,
    delta: &[(K, Option<u32>, Option<u32>)],
) {
    for (key, previous, next) in delta {
        match next {
            Some(next) => {
                let value = disk.entry(key.clone()).or_default();
                *value = (*value).max(*next);
            }
            None => {
                if disk
                    .get(key)
                    .is_some_and(|current| Some(current) <= previous.as_ref())
                {
                    disk.remove(key);
                }
            }
        }
    }
}
fn relations<K: Clone + Eq + Hash>(
    before: &HashMap<K, Vec<ChildEntry>>,
    after: &HashMap<K, Vec<ChildEntry>>,
    owned: impl Fn(&u32) -> bool,
) -> Vec<Relation<K>> {
    let flatten = |map: &HashMap<K, Vec<ChildEntry>>| -> HashMap<(K, u32), ChildEntry> {
        let mut entries = HashMap::new();
        for (parent, list) in map {
            for child in list {
                if owned(&child.child_surface_id) {
                    entries.insert((parent.clone(), child.child_surface_id), child.clone());
                }
            }
        }
        entries
    };
    let (before, after) = (flatten(before), flatten(after));
    changes(&before, &after, |_| true)
        .into_iter()
        .map(|((parent, child), entry)| Relation {
            before: before.get(&(parent.clone(), child)).cloned(),
            parent,
            child,
            after: entry,
        })
        .collect()
}
fn apply_relations<K: Clone + Eq + Hash>(
    disk: &mut HashMap<K, Vec<ChildEntry>>,
    delta: &[Relation<K>],
) {
    for change in delta {
        let list = disk.entry(change.parent.clone()).or_default();
        match &change.after {
            None => list.retain(|c| c.child_surface_id != change.child),
            Some(after) => match (
                list.iter_mut().find(|c| c.child_surface_id == change.child),
                &change.before,
            ) {
                (Some(current), Some(before)) => {
                    merge_entry(current, before, after);
                }
                (Some(current), None) => *current = after.clone(),
                (None, None) => list.push(after.clone()),
                // A field edit cannot recreate a relationship deleted by a newer participant.
                (None, Some(_)) => {}
            },
        }
        list.sort_by_key(|c| c.index);
    }
}

fn merge_entry(current: &mut ChildEntry, before: &ChildEntry, after: &ChildEntry) {
    if before.index != after.index {
        current.index = after.index;
    }
    if before.cwd != after.cwd {
        current.cwd = after.cwd.clone();
    }
    if before.role != after.role {
        current.role = after.role.clone();
    }
    if before.nickname != after.nickname {
        current.nickname = after.nickname.clone();
    }
}
