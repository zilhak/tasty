//! `CoreBuilder` — DI 패턴. production / test 별 adapter 주입.
//!
//! Production wiring 은 `src/boot/wiring.rs` 의 `build_production_core()`.

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
use crate::ports::pty::{PtyService, TerminalWaker};

/// Builder for `Core`. 모든 port 가 주입돼야 `build()` 가 성공.
///
/// `preset_store` 는 구체 Arc 로 받고, trait `presets` port 는 빌더가 내부에서
/// *같은 allocation* 으로 coerce — 호출처가 별 두 인자 주입할 필요 없음 +
/// 두 필드의 owner 일관성 자동 보장.
#[allow(dead_code)]
pub(crate) struct CoreBuilder {
    pty: Option<Arc<dyn PtyService>>,
    waker: Option<Arc<dyn TerminalWaker>>,
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
}

#[allow(dead_code)]
impl CoreBuilder {
    pub(crate) fn new() -> Self {
        Self {
            pty: None,
            waker: None,
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
        }
    }

    pub(crate) fn with_pty(mut self, pty: Arc<dyn PtyService>) -> Self {
        self.pty = Some(pty);
        self
    }
    pub(crate) fn with_waker(mut self, waker: Arc<dyn TerminalWaker>) -> Self {
        self.waker = Some(waker);
        self
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
    pub(crate) fn with_settings_storage(mut self, settings: Arc<dyn SettingsStorage>) -> Self {
        self.settings_storage = Some(settings);
        self
    }

    /// 모든 11 port 가 주입됐는지 확인 후 Core 생성.
    pub(crate) fn build(self) -> anyhow::Result<Core> {
        let preset_store = self
            .preset_store
            .ok_or_else(|| anyhow::anyhow!("PresetStore missing"))?;
        // `presets` (trait Arc) 는 `preset_store` 와 *같은 allocation* — coerce 로 type 만 변경.
        let presets: Arc<Mutex<dyn PresetStorage>> = preset_store.clone();
        Ok(Core {
            pty: self
                .pty
                .ok_or_else(|| anyhow::anyhow!("PtyService missing"))?,
            waker: self
                .waker
                .ok_or_else(|| anyhow::anyhow!("TerminalWaker missing"))?,
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
        })
    }
}

impl Default for CoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
