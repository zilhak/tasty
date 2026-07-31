use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Condition that triggers a global hook.
#[derive(Debug, Clone)]
pub enum HookCondition {
    /// Fires repeatedly every `Duration`.
    Interval(Duration),
    /// Fires once after `Duration` has elapsed since the hook was added.
    Once(Duration),
}

impl HookCondition {
    /// Parse a condition string of the form:
    /// - `"interval:SECS"` → Interval
    /// - `"once:SECS"` → Once
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(rest) = s.strip_prefix("interval:") {
            let secs: f64 = rest.parse().ok()?;
            Some(HookCondition::Interval(Duration::from_secs_f64(secs)))
        } else if let Some(rest) = s.strip_prefix("once:") {
            let secs: f64 = rest.parse().ok()?;
            Some(HookCondition::Once(Duration::from_secs_f64(secs)))
        } else {
            None
        }
    }

    /// Human-readable description of the condition.
    pub fn to_display_string(&self) -> String {
        match self {
            HookCondition::Interval(d) => format!("interval:{}", d.as_secs_f64()),
            HookCondition::Once(d) => format!("once:{}", d.as_secs_f64()),
        }
    }
}

/// A single global hook entry.
#[derive(Debug, Clone)]
pub struct GlobalHook {
    pub id: u32,
    pub condition: HookCondition,
    pub command: String,
    pub label: Option<String>,
}

/// Manages global (non-surface-bound) hooks driven by timers.
pub struct GlobalHookManager {
    hooks: HashMap<u32, GlobalHook>,
    next_id: u32,
    /// When each Interval/Once hook last fired (or was created).
    last_fired: HashMap<u32, Instant>,
    /// Creation time for Once hooks, to measure elapsed time.
    created_at: HashMap<u32, Instant>,
    /// Set of Once hook IDs that have already fired and should be removed.
    fired_once: Vec<u32>,
}

impl GlobalHookManager {
    pub fn new() -> Self {
        Self {
            hooks: HashMap::new(),
            next_id: 0,
            last_fired: HashMap::new(),
            created_at: HashMap::new(),
            fired_once: Vec::new(),
        }
    }

    /// Add a new hook. Returns the assigned hook ID.
    pub fn add(&mut self, condition: HookCondition, command: String, label: Option<String>) -> u32 {
        self.next_id += 1;
        let id = self.next_id;
        let now = Instant::now();

        match &condition {
            HookCondition::Interval(_) => {
                self.last_fired.insert(id, now);
            }
            HookCondition::Once(_) => {
                self.created_at.insert(id, now);
            }
        }

        self.hooks.insert(
            id,
            GlobalHook {
                id,
                condition,
                command,
                label,
            },
        );
        id
    }

    /// Remove a hook by ID. Returns `true` if it existed.
    pub fn remove(&mut self, id: u32) -> bool {
        self.last_fired.remove(&id);
        self.created_at.remove(&id);
        self.hooks.remove(&id).is_some()
    }

    /// List all registered hooks.
    pub fn list(&self) -> Vec<&GlobalHook> {
        self.hooks.values().collect()
    }

    /// Get a single hook by ID.
    ///
    /// IPC handler 가 hook 메타 조회 통합 후 사용 예정. 현재는 add/remove 만
    /// 사용 — 공개 API 유지.
    #[allow(dead_code)]
    pub fn get(&self, id: u32) -> Option<&GlobalHook> {
        self.hooks.get(&id)
    }

    /// Check all hooks and return `(hook_id, command)` pairs that should be
    /// executed right now. `CoreState::poll_global_hooks` 가 `AppEvent::BusyPoll`
    /// 1Hz cadence 에 편승해 호출한다(TODO12).
    pub fn tick(&mut self) -> Vec<(u32, String)> {
        let now = Instant::now();
        let mut to_fire: Vec<(u32, String)> = Vec::new();

        for (id, hook) in &self.hooks {
            match &hook.condition {
                HookCondition::Interval(period) => {
                    let last = self.last_fired.get(id).copied().unwrap_or(now);
                    if now.duration_since(last) >= *period {
                        to_fire.push((*id, hook.command.clone()));
                    }
                }
                HookCondition::Once(delay) => {
                    let created = self.created_at.get(id).copied().unwrap_or(now);
                    if now.duration_since(created) >= *delay {
                        to_fire.push((*id, hook.command.clone()));
                        self.fired_once.push(*id);
                    }
                }
            }
        }

        // Update last_fired for interval hooks that just fired.
        for (id, _) in &to_fire {
            if let Some(hook) = self.hooks.get(id)
                && matches!(hook.condition, HookCondition::Interval(_))
            {
                self.last_fired.insert(*id, now);
            }
        }

        // Remove once-hooks that fired.
        let to_remove: Vec<u32> = self.fired_once.drain(..).collect();
        for id in to_remove {
            self.remove(id);
        }

        to_fire
    }

    /// Execute a shell command in a fire-and-forget fashion.
    /// Spawn 실패는 사용자 hook이 발동했다고 보이지만 실제로는 자식이 안 뜬 상태라
    /// 디버깅이 어렵다. warn으로 흔적을 남긴다.
    pub fn execute_command(command: &str) {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", command]);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", command]);
            c
        };
        let result = tasty_utils::process::hide_console(&mut cmd).spawn();
        if let Err(e) = result {
            tracing::warn!("global hook command spawn failed: {e}; cmd: {command}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_interval_and_once() {
        assert!(matches!(
            HookCondition::parse("interval:5"),
            Some(HookCondition::Interval(d)) if d == Duration::from_secs(5)
        ));
        assert!(matches!(
            HookCondition::parse("once:1.5"),
            Some(HookCondition::Once(d)) if d == Duration::from_secs_f64(1.5)
        ));
        assert!(HookCondition::parse("garbage:1").is_none());
    }

    #[test]
    fn tick_does_not_fire_interval_hook_before_period_elapses() {
        let mut mgr = GlobalHookManager::new();
        mgr.add(
            HookCondition::Interval(Duration::from_secs(60)),
            "echo x".to_string(),
            None,
        );
        assert!(mgr.tick().is_empty());
    }

    #[test]
    fn tick_fires_interval_hook_repeatedly_after_each_period() {
        let mut mgr = GlobalHookManager::new();
        let id = mgr.add(
            HookCondition::Interval(Duration::from_millis(10)),
            "echo x".to_string(),
            None,
        );
        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(mgr.tick(), vec![(id, "echo x".to_string())]);
        // Interval 훅은 발화 후에도 남아 있어야 다음 주기에 다시 발화한다.
        assert_eq!(mgr.list().len(), 1);
        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(mgr.tick(), vec![(id, "echo x".to_string())]);
    }

    #[test]
    fn tick_fires_once_hook_exactly_once_then_removes_it() {
        let mut mgr = GlobalHookManager::new();
        let id = mgr.add(
            HookCondition::Once(Duration::from_millis(10)),
            "echo once".to_string(),
            None,
        );
        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(mgr.tick(), vec![(id, "echo once".to_string())]);
        assert!(mgr.list().is_empty(), "once 훅은 발화 후 제거되어야 한다");
        // 이미 제거됐으니 이후 tick 에서 다시 발화하면 안 된다.
        assert!(mgr.tick().is_empty());
    }
}
