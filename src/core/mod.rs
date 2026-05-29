//! `Core` — 도메인 본체 + 단일 mutate 진입점 + 외부 자원 (port) 주입.
//!
//! ```text
//! handler   ──read──>   &CoreState        (외부 read-only 노출)
//! handler   ──enqueue─> Intent            (HandlerCtx.intents)
//! dispatch  ──drain──>  Core::apply(...)  (Core 만이 mutate)
//! Core::apply ──mutate self.state          (도메인 일관성 보장)
//! ```
//!
//! Phase D 진행 중. 본 Core 는 11 outbound port + preset_store 직속 보유만.
//! 도메인 데이터 (`CoreState`) 는 `crate::engine_state` 에 — App.engine_state
//! 가 main owner. D.3.C 의 도메인 마이그레이션으로 점진 흡수 예정.

pub(crate) mod builder;
pub(crate) mod intent;

use std::sync::{Arc, Mutex};

use intent::{CoreEvent, CoreIntent};
use tasty_memory::MemoryStorage;
use tasty_presets::{PresetStorage, PresetStore};
use tasty_settings::SettingsStorage;
use tasty_themes::ThemeStorage;

use crate::ports::clipboard::ClipboardSystem;
use crate::ports::clock::Clock;
use crate::ports::fs::FileSystem;
use crate::ports::home::HomeDirectory;
use crate::ports::process::ProcessSpawner;
use crate::ports::pty::{PtyService, TerminalWaker};

/// 도메인 본체. 11 outbound port (7 external + 4 internal) + preset_store 직속.
///
/// 도메인 데이터 (`crate::engine_state::CoreState`) 는 본 struct 가 아닌
/// `App.engine_state` 가 main owner — Phase D 진행 중의 *공존 layer*. D.3.C
/// 에서 점진 흡수.
#[allow(dead_code)]
pub(crate) struct Core {
    // ─── External ports (bin 안 정의, src/ports/) ───
    pty: Arc<dyn PtyService>,
    waker: Arc<dyn TerminalWaker>,
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    clipboard: Arc<dyn ClipboardSystem>,
    process: Arc<dyn ProcessSpawner>,
    home: Arc<dyn HomeDirectory>,

    // ─── Internal crate trait ports ───
    /// `Sync` 아님 (SQLite Connection 의 `RefCell` 캐시) — `Mutex` 보호.
    memory: Arc<Mutex<dyn MemoryStorage>>,
    themes: Arc<dyn ThemeStorage>,
    /// `&mut self` 메서드 (save/delete/rename) 가 있어 `Mutex` 보호.
    /// `preset_store` 와 *같은 allocation* (coerce 된 trait Arc).
    presets: Arc<Mutex<dyn PresetStorage>>,
    settings_storage: Arc<dyn SettingsStorage>,

    /// Layout preset 디스크 캐시. 구체 Arc — MainWindow / PresetWindow 에 clone
    /// 으로 전달해 *공유 owner* 가 된다. `presets` (trait Arc) 와 같은 allocation.
    pub(crate) preset_store: Arc<Mutex<PresetStore>>,
}

impl Core {
    // ─── Domain wrapper methods ───
    //
    // sync 결과 반환이 필요한 mutate 는 본 wrapper 들로 노출 (handler 가 직접
    // 호출). Phase D 진행 중에는 wrapper 가 `engine` 도 함께 받아 그쪽을 mutate
    // 한다 — 도메인 데이터의 Core 흡수가 완료되면 `engine` 인자 제거 예정.

    /// Surface message 전송. 옛 `engine.send_message` 의 Core 진입점.
    pub(crate) fn send_surface_message(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        from: u32,
        to: u32,
        content: String,
    ) -> u32 {
        engine.send_message(from, to, content)
    }

    /// Surface message 큐 read (peek/consume). 옛 `engine.read_messages` 의 Core 진입점.
    pub(crate) fn read_surface_messages(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        sid: u32,
        from: Option<u32>,
        peek: bool,
    ) -> Vec<crate::state::SurfaceMessage> {
        engine.read_messages(sid, from, peek)
    }

    /// Surface message 큐 clear. 옛 `engine.clear_messages` 의 Core 진입점.
    pub(crate) fn clear_surface_messages(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        sid: u32,
    ) {
        engine.clear_messages(sid);
    }

    // ─── Hooks ───

    /// surface hook 등록. 반환: 새 hook id.
    pub(crate) fn register_surface_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        event: tasty_hooks::HookEvent,
        command: String,
        once: bool,
    ) -> u64 {
        engine
            .hook_manager
            .add_hook(surface_id, event, command, once)
    }

    /// surface hook 해제. 반환: 실제 제거 여부.
    pub(crate) fn unregister_surface_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        hook_id: u64,
    ) -> bool {
        engine.hook_manager.remove_hook(hook_id)
    }

    /// global hook 등록. 반환: 새 hook id.
    pub(crate) fn register_global_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        condition: crate::global_hooks::HookCondition,
        command: String,
        label: Option<String>,
    ) -> u32 {
        engine.global_hook_manager.add(condition, command, label)
    }

    /// global hook 해제. 반환: 실제 제거 여부.
    pub(crate) fn unregister_global_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        hook_id: u32,
    ) -> bool {
        engine.global_hook_manager.remove(hook_id)
    }

    /// surface 의 hook 들 중 event 매칭 시 fire — 발사된 hook id 들 반환.
    /// AppState 의 enqueue_host_event 는 호출처 (handler) 에서 처리.
    pub(crate) fn fire_surface_hooks(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        events: &[tasty_hooks::HookEvent],
    ) -> Vec<u64> {
        engine.hook_manager.check_and_fire(surface_id, events)
    }

    // ─── Approval (휴먼 핸드오프) ───

    /// approval 요청 생성. 옛 `engine.approval_store.request` 의 Core 진입점.
    pub(crate) fn request_approval(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        req: tasty_approval::ApprovalRequest,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.request(req)
    }

    /// approval 응답 적용. 옛 `engine.approval_store.respond` 의 Core 진입점.
    pub(crate) fn respond_approval(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        req_id: &tasty_approval::ApprovalId,
        choice: String,
        by: tasty_approval::Responder,
        comment: Option<String>,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.respond(req_id, choice, by, comment)
    }

    /// approval 취소. 옛 `engine.approval_store.cancel` 의 Core 진입점.
    pub(crate) fn cancel_approval(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        req_id: &tasty_approval::ApprovalId,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.cancel(req_id)
    }

    // ─── Memory (영속 store) ───

    /// Memory port 의 Arc clone. AppState 등 *Core 인자가 cascade 로 도달하지
    /// 못하는 표면* (UI popup draw_fn, state cleanup 등) 에 inject 해 동일
    /// allocation 의 port 를 공유시킨다.
    pub(crate) fn memory_arc(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        self.memory.clone()
    }

    /// Memory store 의 lock 안에서 함수를 실행한다. Mutex poisoning 시
    /// poison 해제 후 inner 사용 (host 부팅이 store 의 Arc 를 항상 inject
    /// 하므로 None 반환 분기는 없다 — 호출처는 `Result<R, _>` 만 처리하면 된다).
    pub(crate) fn with_memory<R>(
        &self,
        f: impl FnOnce(&mut dyn tasty_memory::MemoryStorage) -> R,
    ) -> R {
        let mut guard = match self.memory.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut *guard)
    }

    // ─── Layout persistence (~/.tasty/layout.json) ───

    /// 현재 layout 을 디스크에 저장한다. debounce / shutdown flush 의 진입점.
    /// 호출자는 (engine, active_workspace_index) 를 넘긴다.
    pub(crate) fn save_layout(
        &self,
        engine: &mut crate::engine_state::CoreState,
        active_workspace: usize,
    ) {
        crate::engine::layout_persistence::save_to_disk(engine, active_workspace);
    }

    /// 저장된 layout 을 live engine 으로 복원한다. plugin 이 register 한 surface
    /// kind 가 준비된 후에만 호출해야 한다 — engine boot 시점이 아닌, 첫 plugin
    /// pump 이후 호출.
    pub(crate) fn restore_layout(
        &self,
        engine: &mut crate::engine_state::CoreState,
        saved: crate::engine::layout_persistence::SavedLayout,
    ) -> bool {
        saved.restore(engine)
    }

    // ─── Clipboard (외부 시스템 clipboard) ───

    /// 시스템 clipboard 에 text 쓰기. 옛 `arboard::Clipboard::new().set_text` 의 Core 진입점.
    pub(crate) fn clipboard_write_text(&self, text: &str) -> anyhow::Result<()> {
        self.clipboard.write_text(text)
    }

    /// 시스템 clipboard 에 image 쓰기. 옛 `arboard::Clipboard::new().set_image` 의 Core 진입점.
    pub(crate) fn clipboard_write_image(
        &self,
        image: &crate::ports::clipboard::ClipboardImage,
    ) -> anyhow::Result<()> {
        self.clipboard.write_image(image)
    }

    /// 도메인 변경의 단일 진입점. handler 가 발행한 `CoreIntent` 를 받아
    /// 결과 이벤트 목록을 반환. Phase D 진행 중 — variant 추가 시 본 match 도 채움.
    #[allow(dead_code)]
    pub(crate) fn apply(&mut self, intent: CoreIntent) -> anyhow::Result<Vec<CoreEvent>> {
        match intent {
            CoreIntent::UpdateSettings(new_settings) => {
                // Phase D 진행 중 — 본 stub 은 *이벤트만 발행*. cascade
                // (Theme apply / Scrollback limit / clipboard max / notification
                // coalesce) 는 후속 sub-step (호출처 전환과 함께) 에서 통합.
                Ok(vec![CoreEvent::SettingsUpdated(new_settings)])
            }
            CoreIntent::PushNotification {
                ws_id,
                surface_id,
                title,
                body,
                source,
            } => Ok(vec![CoreEvent::NotificationPushRequested {
                ws_id,
                surface_id,
                title,
                body,
                source,
            }]),
            CoreIntent::SurfaceCwdChanged { surface_id } => {
                Ok(vec![CoreEvent::SurfaceCwdChanged { surface_id }])
            }
            CoreIntent::SetTerminalMark { surface_id } => {
                Ok(vec![CoreEvent::TerminalMarkSet { surface_id }])
            }
        }
    }
}
