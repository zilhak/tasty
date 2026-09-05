use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Condition that triggers a global hook.
#[derive(Debug, Clone)]
pub enum HookCondition {
    /// Fires repeatedly every `Duration`.
    Interval(Duration),
    /// Fires once after `Duration` has elapsed since the hook was added.
    Once(Duration),
    /// Fires whenever the file at this path changes mtime (1Hz poll,
    /// `GlobalHookManager::tick` 편승 — 별도 watcher 불필요).
    File(PathBuf),
}

impl HookCondition {
    /// Parse a condition string of the form:
    /// - `"interval:SECS"` → Interval
    /// - `"once:SECS"` → Once
    /// - `"file:PATH"` → File. `file:` 뒤 나머지 전체를 경로로 그대로 받는다 —
    ///   추가로 `:` 를 기준 분리하지 않으므로 Windows 드라이브 문자
    ///   (`file:C:\Users\...`)도 별도 처리 없이 올바르게 `C:\Users\...` 로 파싱된다.
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(rest) = s.strip_prefix("interval:") {
            let secs: f64 = rest.parse().ok()?;
            Some(HookCondition::Interval(Duration::from_secs_f64(secs)))
        } else if let Some(rest) = s.strip_prefix("once:") {
            let secs: f64 = rest.parse().ok()?;
            Some(HookCondition::Once(Duration::from_secs_f64(secs)))
        } else if let Some(rest) = s.strip_prefix("file:") {
            if rest.is_empty() {
                None
            } else {
                Some(HookCondition::File(PathBuf::from(rest)))
            }
        } else {
            None
        }
    }

    /// Human-readable description of the condition.
    pub fn to_display_string(&self) -> String {
        match self {
            HookCondition::Interval(d) => format!("interval:{}", d.as_secs_f64()),
            HookCondition::Once(d) => format!("once:{}", d.as_secs_f64()),
            HookCondition::File(p) => format!("file:{}", p.display()),
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
    /// id 카운터. **engine 사이에서 공유한다** — `HookManager` 와 같은 이유이고, 여기는
    /// 한 겹 더 나쁘다: global hook 은 라우팅이 창을 건너 풀지도 않아(`Kind` 에 없다)
    /// 포커스된 창의 것만 답한다. 카운터까지 engine 마다면 비포커스 창의 훅은 **존재하는데
    /// 어떤 요청으로도 닿지 않는다**(실측: `unset global-hook --hook 1` 이 두 번째 호출에서
    /// `removed: false` 를 내고 창1 의 것은 그대로 남는다).
    next_id: Arc<AtomicU32>,
    /// When each Interval/Once hook last fired (or was created).
    last_fired: HashMap<u32, Instant>,
    /// Creation time for Once hooks, to measure elapsed time.
    created_at: HashMap<u32, Instant>,
    /// Set of Once hook IDs that have already fired and should be removed.
    fired_once: Vec<u32>,
    /// Last observed mtime of each File hook's target path. `None` 이면 마지막
    /// 관찰 시점에 파일이 존재하지 않았음(등록 시 미존재 포함) — 이후 파일이
    /// 나타나면 "변경"으로 간주해 발화한다.
    last_mtime: HashMap<u32, Option<SystemTime>>,
}

impl GlobalHookManager {
    /// 자기 카운터로 만든다 — **테스트 전용**이다. production 경로에 이것이 남아 있으면
    /// engine 마다 1 부터 도는 카운터가 되살아나므로 cfg 로 못 박는다.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU32::new(0)))
    }

    /// engine 들이 공유하는 카운터로 만든다 — production 경로는 이쪽이다.
    pub fn with_counter(next_id: Arc<AtomicU32>) -> Self {
        Self {
            hooks: HashMap::new(),
            next_id,
            last_fired: HashMap::new(),
            created_at: HashMap::new(),
            fired_once: Vec::new(),
            last_mtime: HashMap::new(),
        }
    }

    /// Add a new hook. Returns the assigned hook ID.
    pub fn add(&mut self, condition: HookCondition, command: String, label: Option<String>) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let now = Instant::now();

        match &condition {
            HookCondition::Interval(_) => {
                self.last_fired.insert(id, now);
            }
            HookCondition::Once(_) => {
                self.created_at.insert(id, now);
            }
            HookCondition::File(path) => {
                // 등록 시점의 mtime 을 기준선으로만 기록 — 즉시 발화하지 않는다.
                self.last_mtime.insert(id, file_mtime(path));
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
        self.last_mtime.remove(&id);
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
    /// executed right now. `CoreState::poll_global_hooks` 가 `Tick::Busy`
    /// 1Hz cadence 에 편승해 호출한다.
    pub fn tick(&mut self) -> Vec<(u32, String)> {
        let now = Instant::now();
        let mut to_fire: Vec<(u32, String)> = Vec::new();
        let mut file_mtime_updates: Vec<(u32, Option<SystemTime>)> = Vec::new();

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
                HookCondition::File(path) => {
                    let current = file_mtime(path);
                    // 파일이 사라진 경우(metadata 에러)는 "변경 없음"으로 취급 —
                    // interval/once 와의 일관성(외부 요인으로 훅이 조용히 사라지지
                    // 않음) 유지. 마지막 관찰값도 갱신하지 않고 그대로 둔다.
                    if let Some(mtime) = current {
                        let last = self.last_mtime.get(id).copied().unwrap_or(None);
                        if last != Some(mtime) {
                            to_fire.push((*id, hook.command.clone()));
                            file_mtime_updates.push((*id, Some(mtime)));
                        }
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

        // Record newly observed mtimes for File hooks that fired.
        for (id, mtime) in file_mtime_updates {
            self.last_mtime.insert(id, mtime);
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

/// 파일 mtime 조회. 심볼릭 링크는 `std::fs::metadata`(링크를 따라감)로 대상
/// 파일의 mtime을 관찰한다 — 별도 처리 없이 자연스럽게 동작. 디렉토리 경로도
/// `metadata`가 그대로 mtime을 반환하므로 디렉토리 자체의 변경(항목 추가/삭제
/// 등으로 갱신되는 디렉토리 엔트리의 mtime)을 감지하는 용도로도 동작하지만,
/// 파일 하나만 공식 지원 범위다(디렉토리 감지는 스코프 밖 — 문서 참고).
/// 조회 실패(파일 없음 등)는 `None`.
fn file_mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
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

    #[test]
    fn parse_accepts_file_condition() {
        assert!(matches!(
            HookCondition::parse("file:/tmp/foo.txt"),
            Some(HookCondition::File(p)) if p == std::path::PathBuf::from("/tmp/foo.txt")
        ));
        // Windows 드라이브 문자 — "file:" 뒤 나머지 전체를 그대로 경로로 받으므로
        // 추가 콜론 분리 없이 올바르게 파싱된다.
        assert!(matches!(
            HookCondition::parse(r"file:C:\Users\foo\bar.txt"),
            Some(HookCondition::File(p)) if p == std::path::PathBuf::from(r"C:\Users\foo\bar.txt")
        ));
        assert!(HookCondition::parse("file:").is_none());
    }

    #[test]
    fn tick_does_not_fire_file_hook_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("watched.txt");
        std::fs::write(&path, "v1").unwrap();
        let mut mgr = GlobalHookManager::new();
        mgr.add(
            HookCondition::File(path.clone()),
            "echo x".to_string(),
            None,
        );
        assert!(mgr.tick().is_empty(), "등록 직후 발화 없음");
        assert!(mgr.tick().is_empty(), "변경 없으면 계속 발화 없음");
    }

    #[test]
    fn tick_fires_file_hook_repeatedly_on_each_mtime_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("watched.txt");
        std::fs::write(&path, "v1").unwrap();
        let mut mgr = GlobalHookManager::new();
        let id = mgr.add(
            HookCondition::File(path.clone()),
            "echo x".to_string(),
            None,
        );
        assert!(mgr.tick().is_empty());

        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&path, "v2").unwrap();
        assert_eq!(mgr.tick(), vec![(id, "echo x".to_string())]);
        assert!(mgr.tick().is_empty(), "같은 변경으로 두 번 발화하지 않음");

        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&path, "v3").unwrap();
        // interval 처럼 반복 발화, 훅 자체는 사라지지 않는다.
        assert_eq!(mgr.tick(), vec![(id, "echo x".to_string())]);
        assert_eq!(mgr.list().len(), 1);
    }

    #[test]
    fn tick_fires_file_hook_when_file_appears_after_being_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-yet.txt");
        let mut mgr = GlobalHookManager::new();
        let id = mgr.add(
            HookCondition::File(path.clone()),
            "echo x".to_string(),
            None,
        );
        // 파일이 아직 없으므로 발화하지 않는다.
        assert!(mgr.tick().is_empty());

        std::fs::write(&path, "v1").unwrap();
        assert_eq!(
            mgr.tick(),
            vec![(id, "echo x".to_string())],
            "등록 후 파일이 새로 생기면 변경으로 간주해 발화"
        );
        assert!(mgr.tick().is_empty());
    }

    #[test]
    fn tick_treats_deleted_file_as_no_change_and_keeps_hook() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("watched.txt");
        std::fs::write(&path, "v1").unwrap();
        let mut mgr = GlobalHookManager::new();
        let id = mgr.add(
            HookCondition::File(path.clone()),
            "echo x".to_string(),
            None,
        );
        assert!(mgr.tick().is_empty());

        std::fs::remove_file(&path).unwrap();
        assert!(
            mgr.tick().is_empty(),
            "파일 삭제는 변경으로 취급하지 않는다"
        );
        assert_eq!(mgr.list().len(), 1, "훅이 자동 제거되지 않는다");

        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&path, "v2").unwrap();
        assert_eq!(
            mgr.tick(),
            vec![(id, "echo x".to_string())],
            "파일이 다시 생기면 재감지한다"
        );
    }
}
