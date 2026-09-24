use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone)]
pub enum HookCondition {
    /// tick에서 경과 시간을 확인해 반복한다. 정확한 시각의 실행을 예약하는 OS 타이머는 아니다.
    Interval(Duration),
    Once(Duration),
    /// tick에서 파일 mtime 변경을 확인한다.
    File(PathBuf),
}

impl HookCondition {
    /// interval:SECS, once:SECS, file:PATH를 해석한다. file: 뒤의 추가 콜론은 경로에 남긴다.
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

    pub fn to_display_string(&self) -> String {
        match self {
            HookCondition::Interval(d) => format!("interval:{}", d.as_secs_f64()),
            HookCondition::Once(d) => format!("once:{}", d.as_secs_f64()),
            HookCondition::File(p) => format!("file:{}", p.display()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GlobalHook {
    pub id: u32,
    pub condition: HookCondition,
    pub command: String,
    pub label: Option<String>,
}

pub struct GlobalHookManager {
    hooks: HashMap<u32, GlobalHook>,
    /// 여러 engine이 같은 카운터를 사용해 hook ID 충돌을 피한다.
    next_id: Arc<AtomicU32>,
    last_fired: HashMap<u32, Instant>,
    created_at: HashMap<u32, Instant>,
    fired_once: Vec<u32>,
    /// 마지막 mtime. None은 부재뿐 아니라 metadata·수정 시각 조회 실패도 포함한다.
    last_mtime: HashMap<u32, Option<SystemTime>>,
}

impl GlobalHookManager {
    /// 검사 전용 독립 카운터. 제품 생성 경로는 공유 발급기를 받는다.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU32::new(0)))
    }

    /// 다른 engine과 공유할 ID 카운터를 받는다.
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

    pub fn remove(&mut self, id: u32) -> bool {
        self.last_fired.remove(&id);
        self.created_at.remove(&id);
        self.last_mtime.remove(&id);
        self.hooks.remove(&id).is_some()
    }

    pub fn list(&self) -> Vec<&GlobalHook> {
        self.hooks.values().collect()
    }

    #[allow(dead_code)]
    pub fn get(&self, id: u32) -> Option<&GlobalHook> {
        self.hooks.get(&id)
    }

    /// 지금 조건을 만족한 요청 목록을 만든다. 실행 전에 시간·once·mtime 기록을 갱신한다.
    /// 실제 명령 실행 실패를 자동 재시도하지는 않는다.
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
                    // 크기 제한 없는 파일을 매번 전부 읽지 않도록 mtime만 본다.
                    // 같은 mtime으로 내용이 바뀌거나 시각이 복원되면 변경을 놓칠 수 있다.
                    let current = file_mtime(path);
                    // 조회 실패는 기존 mtime을 유지한다. 같은 mtime으로 다시 생긴 파일도 구분하지 못한다.
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

        for (id, _) in &to_fire {
            if let Some(hook) = self.hooks.get(id)
                && matches!(hook.condition, HookCondition::Interval(_))
            {
                self.last_fired.insert(*id, now);
            }
        }

        let to_remove: Vec<u32> = self.fired_once.drain(..).collect();
        for id in to_remove {
            self.remove(id);
        }

        for (id, mtime) in file_mtime_updates {
            self.last_mtime.insert(id, mtime);
        }

        to_fire
    }

    /// 셸 프로세스 생성을 요청한다. 실패는 로그에 남기며 자식 완료·종료 코드는 기다리지 않는다.
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

/// 심볼릭 링크를 따라 metadata의 수정 시각을 읽는다. 실패는 None이다.
/// 디렉터리 경로도 읽을 수 있지만 내부 파일 내용의 변경까지 재귀 추적하지는 않는다.
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
        assert!(mgr.tick().is_empty());
    }

    #[test]
    fn parse_accepts_file_condition() {
        assert!(matches!(
            HookCondition::parse("file:/tmp/foo.txt"),
            Some(HookCondition::File(p)) if p == std::path::PathBuf::from("/tmp/foo.txt")
        ));
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
