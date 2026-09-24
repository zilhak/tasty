use super::SurfaceId;
pub use super::surface_layout::{SurfaceLayout, SurfaceRegion};
use super::surface_trait::Surface;

/// 트리에서 터미널 ID만 보관하는 marker. PTY·스크롤백은 호스트 TerminalStore가 소유한다.
pub struct TerminalSurface {
    pub id: SurfaceId,
}

/// EmptySurface의 Deferred::Terminal에 보관할 PTY 생성 정보.
#[derive(Clone)]
pub struct DeferredSpawn {
    pub shell: Option<String>,
    pub shell_args: Vec<String>,
    /// 자식 셸에 추가로 심을 환경변수(docs/features/terminal-output/index.md#명령-인덱싱-osc-133
    /// 참고). `shell_args` 와 마찬가지로 spawn
    /// 시점까지 owned 로 들고 있다가, 실제 PTY spawn 순간 `TerminalConfig` 로 넘긴다.
    pub extra_env: Vec<(String, String)>,
    pub cols: usize,
    pub rows: usize,
    pub waker: tasty_terminal::Waker,
    pub working_dir: Option<std::path::PathBuf>,
    /// PTY 첫 입력으로 보낼 복원 명령. 생성 경로가 끝에 CR을 붙인다.
    /// 호출자는 줄바꿈을 넣지 않는다. TUI 세션 재개에 사용한다.
    pub restore_command: Option<String>,
    /// 복원 시 layout.json 의 scrollback_ref 를 그대로 들고 있다가, PTY 가
    /// 실제로 spawn 되는 순간 `TerminalStore::set_scrollback_persist_id` 로 이관된다.
    pub scrollback_persist_id: Option<String>,
}

/// kind 등록을 기다리는 플러그인의 복원 정보. 등록 뒤 호스트의 restore 콜백에 전달한다.
#[derive(Clone)]
pub struct DeferredPlugin {
    /// surface kind 식별자(예: `"markdown"`). registry 등록을 기다리는 대상.
    pub kind: String,
    /// 해당 kind 의 `restore` 콜백이 받을 JSON. reify 까지 owned 로 보관한다.
    pub snapshot: serde_json::Value,
}

/// 지연 생성 대상. 한 placeholder가 Terminal과 Plugin 정보를 동시에 갖지 못하도록 구분한다.
#[derive(Clone)]
pub enum Deferred {
    /// PTY 가 아직 안 뜬 터미널 자리.
    Terminal(DeferredSpawn),
    /// plugin kind 가 아직 registry 에 없는 non-terminal surface 자리.
    Plugin(DeferredPlugin),
}

impl Surface for TerminalSurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "terminal"
    }

    fn type_name(&self) -> &'static str {
        "Terminal"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    /// Terminal 의 cwd 는 `engine.terminals.get(id).get_cwd()` 로 store 경유 —
    /// trait 는 None 반환. caller(host 의 `CoreState::surface_cwd`)가 분기 처리. Surface cwd
    /// invariant — `docs/design/policies/cwd.md#surface-cwd-invariant`.
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn to_tree_json(&self) -> serde_json::Value {
        // cols/rows 는 caller 가 engine.terminals.get(id) 로 enrichment.
        serde_json::json!({
            "type": "Terminal",
            "id": self.id,
            "cols": 0,
            "rows": 0,
        })
    }
}
