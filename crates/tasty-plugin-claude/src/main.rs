#![forbid(unsafe_code)]

//! Claude Code의 기동, 훅, 오류 감시와 텔레메트리를 제공한다.
//! 자식 터미널 관리는 호스트의 `terminal.*` IPC에 맡긴다.
//! 호스트 코드에 직접 의존하지 않고 `tasty-plugin-sdk`로 통신한다.

mod auto_resume;
mod checklist;
mod error_scan;
mod gate;
mod handlers;
mod hook;
mod install;
mod notifications;
mod profile;
mod profile_attach;
mod profile_merge;
mod reboot;
mod state;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use error_scan::{ErrorScanner, scan_target_is_alive};
use handlers::*;
use serde_json::{Value, json};
use state::ClaudeState;
use tasty_plugin_sdk::{
    HostHandle, IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, SurfaceCreateCtx, SurfaceResult,
    i18n::Translator,
};

const PLUGIN_ID: &str = "com.tasty.claude";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 폴링 사이의 대기 시간. 한 회차의 IPC 처리 시간은 별도로 걸린다.
const ERROR_SCAN_INTERVAL: Duration = Duration::from_millis(800);

struct ClaudePlugin {
    /// 훅에서 사용하는 Claude 실행 시간 기록. 자식 목록은 호스트가 관리한다.
    state: ClaudeState,
    scanner: Arc<Mutex<ErrorScanner>>,
    /// API 오류 후 재개 예약. `claude-auto-resume` 스레드가 만기를 처리한다.
    resume: Arc<Mutex<auto_resume::ResumeTable>>,
    /// 그 스레드가 쓸 문구. `on_start` 에서 스레드로 넘긴다.
    resume_texts: auto_resume::Texts,
    /// reboot 시퀀스 진행 중인 surface 집합 — 같은 surface 중복 reboot 가드.
    rebooting: Arc<Mutex<HashSet<u32>>>,
    /// 프로필·게이트·라운드 상태를 저장하는 `TASTY_PLUGIN_DATA_DIR`.
    /// 없으면 다른 경로에 쓰지 않고 각 처리기에서 오류 또는 통과로 처리한다.
    plugin_data_dir: Option<PathBuf>,
    /// 기본 게이트의 번역된 본문. 시작할 때 한 번 읽는다.
    /// 사용자 게이트 본문은 별도 파일에서 훅 실행 때마다 읽는다.
    checklist_body: String,
    /// 사람이 읽는 IPC 에러/응답 문자열 번역용. `main()` 에서 1 회 로드해 재사용한다.
    translator: Translator,
}

impl ClaudePlugin {
    fn new(
        plugin_data_dir: Option<PathBuf>,
        checklist_body: String,
        translator: Translator,
    ) -> Self {
        let resume_texts = auto_resume::Texts {
            message: translator.t("claude.auto_resume.message").to_string(),
            limit_title: translator.t("claude.auto_resume.limit_title").to_string(),
            limit_body: translator.t("claude.auto_resume.limit_body").to_string(),
        };
        Self {
            state: ClaudeState::new(),
            scanner: Arc::new(Mutex::new(ErrorScanner::new())),
            resume: Arc::new(Mutex::new(auto_resume::ResumeTable::default())),
            resume_texts,
            rebooting: Arc::new(Mutex::new(HashSet::new())),
            plugin_data_dir,
            checklist_body,
            translator,
        }
    }
}

impl Plugin for ClaudePlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // 자식 Claude는 호스트가 만든 일반 터미널에서 실행한다.
        SurfaceResult::default()
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "claude.hook" => hook::handle_claude_hook(
                &mut self.state,
                &self.scanner,
                &self.resume,
                &ctx.host,
                &ctx.params,
                self.plugin_data_dir.as_deref(),
                &self.translator,
            ),
            "claude.checklist_hook" => checklist::handle_checklist_hook(
                &ctx.host,
                self.plugin_data_dir.as_deref(),
                &self.checklist_body,
                &ctx.params,
                &self.translator,
            ),
            "claude.checklist_enable" => checklist::handle_enable(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.checklist_disable" => checklist::handle_disable(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.checklist_status" => checklist::handle_status(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.install" => match install::run_install(&self.translator) {
                Ok(added) => Ok(json!({ "installed": added })),
                Err(e) => Err(IpcMethodError::new(
                    self.translator
                        .t_fmt("claude.install.install_failed", &e.to_string()),
                )),
            },
            "claude.uninstall" => match install::run_uninstall(&self.translator) {
                Ok(removed) => Ok(json!({ "uninstalled": removed })),
                Err(e) => Err(IpcMethodError::new(
                    self.translator
                        .t_fmt("claude.install.uninstall_failed", &e.to_string()),
                )),
            },
            "claude.parent" => handle_parent(&ctx.host, &ctx.params, &self.translator),
            "claude.state" => handle_state(&ctx.host, &ctx.params, &self.translator),
            "claude.children" => handle_children(&ctx.host, &ctx.params, &self.translator),
            "claude.kill" => handle_kill(&self.scanner, &ctx.host, &ctx.params, &self.translator),
            "claude.broadcast" => handle_broadcast(&ctx.host, &ctx.params, &self.translator),
            "claude.tell" => handle_tell(&ctx.host, &ctx.params, &self.translator),
            "claude.notify_done" => handle_notify_done(&ctx.host, &ctx.params, &self.translator),
            "claude.notify_error" => handle_notify_error(&ctx.host, &ctx.params, &self.translator),
            "claude.launch" => handle_launch(
                &self.scanner,
                &ctx.host,
                &ctx.params,
                self.plugin_data_dir.as_deref(),
                &self.translator,
            ),
            "claude.respawn" => handle_respawn(
                &self.scanner,
                &ctx.host,
                &ctx.params,
                self.plugin_data_dir.as_deref(),
                &self.translator,
            ),
            "claude.spawn" => handle_spawn(
                &self.scanner,
                &ctx.host,
                &ctx.params,
                self.plugin_data_dir.as_deref(),
                &self.translator,
            ),
            "claude.reboot" => reboot::handle_reboot(
                &self.rebooting,
                &ctx.host,
                &ctx.params,
                self.plugin_data_dir.as_deref(),
                &self.translator,
            ),
            // reboot과 같은 진행 중 집합을 사용해 같은 자식의 중복 실행을 막는다.
            "claude.child_profile" => handle_child_profile(
                &self.rebooting,
                &ctx.host,
                &ctx.params,
                self.plugin_data_dir.as_deref(),
                &self.translator,
            ),
            "claude.profile_register" => profile::handle_register(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.profile_unregister" => profile::handle_unregister(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.profile_list" => profile::handle_list(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.profile_show" => profile::handle_show(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.profile_current" => profile::handle_current(
                self.plugin_data_dir.as_deref(),
                &ctx.host,
                &ctx.params,
                &self.translator,
            ),
            "claude.gate_register" => gate::handle_register(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.gate_unregister" => gate::handle_unregister(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.gate_list" => gate::handle_list(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            "claude.gate_show" => gate::handle_show(
                self.plugin_data_dir.as_deref(),
                &ctx.params,
                &self.translator,
            ),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_start(&mut self, host: HostHandle, _bus: tasty_plugin_sdk::BusHandle) {
        let input_host = host.clone();
        std::thread::Builder::new()
            .name("claude-input-default".into())
            .spawn(move || {
                if let Err(error) = input_host.call(
                    "settings.initialize_input_rule",
                    serde_json::json!({ "app": "claude", "shift_enter_newline": true }),
                ) {
                    tracing::warn!("could not register Claude input default: {error}");
                }
            })
            .expect("spawn claude-input-default thread");
        // 등록된 최상위·자식 터미널을 별도 스레드에서 주기적으로 확인한다.
        let scanner = self.scanner.clone();
        let resume = self.resume.clone();
        let resume_host = host.clone();
        let resume_texts = self.resume_texts.clone();
        // 스레드 생성 실패 시 이 플러그인 프로세스를 패닉으로 종료한다.
        std::thread::Builder::new()
            .name("claude-error-scan".into())
            .spawn(move || error_scan_loop(scanner, host))
            .expect("spawn claude-error-scan thread");
        // terminal.tell 응답을 기다리는 동안 오류 감시가 멈추지 않도록 스레드를 나눈다.
        std::thread::Builder::new()
            .name("claude-auto-resume".into())
            .spawn(move || auto_resume::run_loop(resume, resume_host, resume_texts))
            .expect("spawn claude-auto-resume thread");
    }
}

fn error_scan_loop(scanner: Arc<Mutex<ErrorScanner>>, host: HostHandle) {
    loop {
        std::thread::sleep(ERROR_SCAN_INTERVAL);
        // 대상 목록을 복사한 뒤 잠금을 풀고, 개별 스캔 때 다시 잠근다.
        // poison은 lock_scanner가 한 번 보고하고 내부 상태를 계속 사용한다.
        let surfaces = crate::error_scan::lock_scanner(&scanner).enabled_snapshot();
        for (sid, target) in surfaces {
            // 등록 경로별 조회 결과로 제거 여부를 판단한다. 조회 실패 처리도 경로마다 다르다.
            if !scan_target_is_alive(&host, sid, target) {
                crate::error_scan::lock_scanner(&scanner).disable(sid);
                continue;
            }
            // 반환된 오류 문구는 시험에서만 사용한다.
            crate::error_scan::lock_scanner(&scanner).scan_one(&host, sid);
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    // SDK 연결에 쓰는 환경에서 여기서는 데이터 경로와 번역 설정을 읽는다.
    // 데이터 경로가 없으면 None을 넘겨 저장 요청을 거부한다.
    let env = PluginEnv::load().ok();
    let plugin_data_dir = env.as_ref().and_then(|e| e.data_dir.clone());
    // 환경을 읽지 못하면 번역 키를 그대로 반환한다.
    let translator = env
        .as_ref()
        .map(Translator::from_plugin_env)
        .unwrap_or_default();
    // 기본 게이트 본문은 활성 언어로 한 번만 읽는다.
    let checklist_body = translator.t("claude.checklist.body").to_string();
    tasty_plugin_sdk::run(ClaudePlugin::new(
        plugin_data_dir,
        checklist_body,
        translator,
    ))
}
