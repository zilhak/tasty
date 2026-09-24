//! Core에 저장소와 외부 기능 구현을 주입한다. 실제 앱 조립은 boot::wiring에서 맡는다.

use std::sync::{Arc, Mutex, OnceLock};

use tasty_memory::MemoryStorage;
use tasty_presets::{PresetStorage, PresetStore};
use tasty_settings::SettingsStorage;
use tasty_themes::ThemeStorage;

use super::Core;
use crate::ports::clipboard::ClipboardSystem;
use crate::ports::clock::Clock;
use crate::ports::fs::FileSystem;
use crate::ports::home::HomeDirectory;
use crate::ports::notification_sound::NotificationSoundPlayer;
use crate::ports::process::ProcessSpawner;

/// 필수 port가 모두 있어야 build가 성공한다. presets와 preset_store는 같은 Arc를 공유한다.
pub(crate) struct CoreBuilder {
    fs: Option<Arc<dyn FileSystem>>,
    clock: Option<Arc<dyn Clock>>,
    clipboard: Option<Arc<dyn ClipboardSystem>>,
    process: Option<Arc<dyn ProcessSpawner>>,
    home: Option<Arc<dyn HomeDirectory>>,
    sound_player: Option<Arc<dyn NotificationSoundPlayer>>,
    memory: Option<Arc<Mutex<dyn MemoryStorage>>>,
    themes: Option<Arc<dyn ThemeStorage>>,
    preset_store: Option<Arc<Mutex<PresetStore>>>,
    settings_storage: Option<Arc<dyn SettingsStorage>>,
    /// 저장소 진단 정보는 선택 사항이며 없어도 build할 수 있다.
    db_latency: Option<Arc<tasty_memory::DbLatencyStats>>,
    memory_pragmas: Option<tasty_memory::pragma::AppliedPragmas>,
    memory_init_fallback: Option<tasty_memory::InitFallback>,
}

impl CoreBuilder {
    pub(crate) fn new() -> Self {
        Self {
            fs: None,
            clock: None,
            clipboard: None,
            process: None,
            home: None,
            sound_player: None,
            memory: None,
            themes: None,
            preset_store: None,
            settings_storage: None,
            db_latency: None,
            memory_pragmas: None,
            memory_init_fallback: None,
        }
    }

    pub(crate) fn with_fs(mut self, fs: Arc<dyn FileSystem>) -> Self {
        self.fs = Some(fs);
        self
    }
    pub(crate) fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }
    pub(crate) fn with_clipboard(mut self, clipboard: Arc<dyn ClipboardSystem>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }
    pub(crate) fn with_process(mut self, process: Arc<dyn ProcessSpawner>) -> Self {
        self.process = Some(process);
        self
    }
    pub(crate) fn with_home(mut self, home: Arc<dyn HomeDirectory>) -> Self {
        self.home = Some(home);
        self
    }
    pub(crate) fn with_sound_player(
        mut self,
        sound_player: Arc<dyn NotificationSoundPlayer>,
    ) -> Self {
        self.sound_player = Some(sound_player);
        self
    }
    pub(crate) fn with_memory(mut self, memory: Arc<Mutex<dyn MemoryStorage>>) -> Self {
        self.memory = Some(memory);
        self
    }
    pub(crate) fn with_themes(mut self, themes: Arc<dyn ThemeStorage>) -> Self {
        self.themes = Some(themes);
        self
    }
    pub(crate) fn with_preset_store(mut self, preset_store: Arc<Mutex<PresetStore>>) -> Self {
        self.preset_store = Some(preset_store);
        self
    }
    /// 저장소의 실제 지연 계측기를 연결한다. 생략하면 새 빈 계측기를 쓴다.
    pub(crate) fn with_db_latency(mut self, stats: Arc<tasty_memory::DbLatencyStats>) -> Self {
        self.db_latency = Some(stats);
        self
    }
    /// 저장소를 열 때 읽은 pragma 결과. 생략하면 진단은 null이다.
    pub(crate) fn with_memory_pragmas(
        mut self,
        pragmas: tasty_memory::pragma::AppliedPragmas,
    ) -> Self {
        self.memory_pragmas = Some(pragmas);
        self
    }
    /// 저장소 초기화 실패 후 대체 저장소를 쓴 이유. 생략했다고 실제 저장소의 내구성을 검증한 것은 아니다.
    pub(crate) fn with_memory_init_fallback(
        mut self,
        fallback: Option<tasty_memory::InitFallback>,
    ) -> Self {
        self.memory_init_fallback = fallback;
        self
    }
    pub(crate) fn with_settings_storage(mut self, settings: Arc<dyn SettingsStorage>) -> Self {
        self.settings_storage = Some(settings);
        self
    }

    pub(crate) fn build(self) -> anyhow::Result<Core> {
        let preset_store = self
            .preset_store
            .ok_or_else(|| anyhow::anyhow!("PresetStore missing"))?;
        let presets: Arc<Mutex<dyn PresetStorage>> = preset_store.clone();
        Ok(Core {
            fs: self
                .fs
                .ok_or_else(|| anyhow::anyhow!("FileSystem missing"))?,
            clock: self.clock.ok_or_else(|| anyhow::anyhow!("Clock missing"))?,
            clipboard: self
                .clipboard
                .ok_or_else(|| anyhow::anyhow!("ClipboardSystem missing"))?,
            process: self
                .process
                .ok_or_else(|| anyhow::anyhow!("ProcessSpawner missing"))?,
            home: self
                .home
                .ok_or_else(|| anyhow::anyhow!("HomeDirectory missing"))?,
            sound_player: self
                .sound_player
                .ok_or_else(|| anyhow::anyhow!("NotificationSoundPlayer missing"))?,
            memory: self
                .memory
                .ok_or_else(|| anyhow::anyhow!("MemoryStorage missing"))?,
            themes: self
                .themes
                .ok_or_else(|| anyhow::anyhow!("ThemeStorage missing"))?,
            presets,
            settings_storage: self
                .settings_storage
                .ok_or_else(|| anyhow::anyhow!("SettingsStorage missing"))?,
            preset_store,
            host_ipc_injector: Arc::new(OnceLock::new()),
            runner_registry: Arc::new(crate::core::agent::runner_thread::RunnerRegistry::new()),
            hook_task_waits: Arc::new(crate::core::agent::hook_wait::HookTaskWaits::new()),
            pressure: tasty_telemetry::PressureStats::default(),
            gate: tasty_telemetry::GateStats::default(),
            plugin_wait: std::sync::Arc::new(tasty_telemetry::PluginWaitStats::default()),
            slow_requests: std::sync::Arc::new(tasty_telemetry::SlowRequestLog::default()),
            db_latency: self.db_latency.unwrap_or_default(),
            memory_pragmas: self.memory_pragmas,
            memory_init_fallback: self.memory_init_fallback,
            // IPC 서버는 Core 이후에 시작하므로 여기서 만든 계측기를 부팅 과정에서 서버에도 넘긴다.
            connections: std::sync::Arc::new(tasty_telemetry::ConnectionStats::default()),
            dispatch: std::sync::Arc::new(tasty_ipc::dispatch::DispatchStats::default()),
        })
    }
}

impl Default for CoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
