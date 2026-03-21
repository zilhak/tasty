use std::collections::HashSet;
use std::process::Command;

pub type HookId = u64;

#[derive(Clone, Debug)]
pub struct SurfaceHook {
    pub id: HookId,
    pub surface_id: u32,
    pub event: HookEvent,
    pub command: String,
    pub once: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookEvent {
    ProcessExit,
    OutputMatch(String),
    Bell,
    Notification,
    IdleTimeout(u64),
}

impl HookEvent {
    fn matches(&self, other: &HookEvent) -> bool {
        match (self, other) {
            (HookEvent::ProcessExit, HookEvent::ProcessExit) => true,
            (HookEvent::Bell, HookEvent::Bell) => true,
            (HookEvent::Notification, HookEvent::Notification) => true,
            (HookEvent::OutputMatch(pattern), HookEvent::OutputMatch(text)) => {
                // pattern is a regex, text is the actual output
                regex::Regex::new(pattern)
                    .map(|re| re.is_match(text))
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Parse a hook event from a CLI string like "process-exit", "bell", "output-match:pattern".
    pub fn parse(s: &str) -> Option<Self> {
        if s == "process-exit" {
            Some(HookEvent::ProcessExit)
        } else if s == "bell" {
            Some(HookEvent::Bell)
        } else if s == "notification" {
            Some(HookEvent::Notification)
        } else if let Some(pattern) = s.strip_prefix("output-match:") {
            Some(HookEvent::OutputMatch(pattern.to_string()))
        } else if let Some(secs) = s.strip_prefix("idle-timeout:") {
            secs.parse::<u64>().ok().map(HookEvent::IdleTimeout)
        } else {
            None
        }
    }

    /// Serialize to a display string.
    pub fn to_display_string(&self) -> String {
        match self {
            HookEvent::ProcessExit => "process-exit".to_string(),
            HookEvent::Bell => "bell".to_string(),
            HookEvent::Notification => "notification".to_string(),
            HookEvent::OutputMatch(pattern) => format!("output-match:{}", pattern),
            HookEvent::IdleTimeout(secs) => format!("idle-timeout:{}", secs),
        }
    }
}

pub struct HookManager {
    hooks: Vec<SurfaceHook>,
    next_id: HookId,
}

impl HookManager {
    pub fn new() -> Self {
        Self {
            hooks: Vec::new(),
            next_id: 1,
        }
    }

    pub fn add_hook(
        &mut self,
        surface_id: u32,
        event: HookEvent,
        command: String,
        once: bool,
    ) -> HookId {
        let id = self.next_id;
        self.next_id += 1;
        self.hooks.push(SurfaceHook {
            id,
            surface_id,
            event,
            command,
            once,
        });
        id
    }

    pub fn remove_hook(&mut self, hook_id: HookId) -> bool {
        let len_before = self.hooks.len();
        self.hooks.retain(|h| h.id != hook_id);
        self.hooks.len() < len_before
    }

    pub fn list_hooks(&self, surface_id: Option<u32>) -> Vec<&SurfaceHook> {
        self.hooks
            .iter()
            .filter(|h| surface_id.map_or(true, |id| h.surface_id == id))
            .collect()
    }

    /// Check events and fire matching hooks. Returns fired hook IDs.
    pub fn check_and_fire(&mut self, surface_id: u32, events: &[HookEvent]) -> Vec<HookId> {
        let mut fired = Vec::new();

        for hook in &self.hooks {
            if hook.surface_id != surface_id {
                continue;
            }
            for event in events {
                if hook.event.matches(event) {
                    // Fire the hook command in background
                    let cmd = hook.command.clone();
                    std::thread::spawn(move || {
                        let _ = if cfg!(windows) {
                            Command::new("cmd").args(["/C", &cmd]).output()
                        } else {
                            Command::new("sh").args(["-c", &cmd]).output()
                        };
                    });
                    fired.push(hook.id);
                }
            }
        }

        // Remove once-hooks that fired
        let fired_set: HashSet<HookId> = fired.iter().copied().collect();
        self.hooks
            .retain(|h| !h.once || !fired_set.contains(&h.id));

        fired
    }
}
