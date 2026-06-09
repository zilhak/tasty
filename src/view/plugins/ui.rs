//! `PluginsView` modal의 egui UI.
//!
//! 상단 — 탭 바 (`Installed` / `Add plugin`).
//! `Installed` 탭: 좌측 plugin 목록, 우측 상세(매니페스트, enable/disable,
//! 권한 grant/revoke, 설치 경로, uninstall).
//! `Add plugin` 탭: 경로 입력 → 검증 → 추가/취소.
//!
//! 모달은 `PluginsSnapshot`(읽기 전용 데이터)을 들고 있고, 사용자 조작은
//! `PluginsAction` 큐에 쌓여 메인 루프에서 `PluginManager`에 적용된다.

use crate::i18n::t;
use crate::theme;

/// 한 plugin의 화면 표시용 스냅샷.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub homepage: String,
    pub enabled: bool,
    pub running: bool,
    pub builtin: bool,
    pub surface_kinds: Vec<String>,
    pub manifest_permissions: Vec<String>,
    pub granted_permissions: Vec<String>,
    pub log_path: String,
    /// 설치 디렉터리 (`~/.tasty/plugins/<id>/`).
    pub install_dir: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginsSnapshot {
    pub plugins: Vec<PluginEntry>,
}

/// `PluginsView`가 메인 루프에 발행하는 동작.
#[derive(Debug, Clone)]
pub enum PluginsAction {
    SetEnabled {
        id: String,
        enabled: bool,
    },
    Grant {
        id: String,
        permission: String,
    },
    Revoke {
        id: String,
        permission: String,
    },
    Uninstall {
        id: String,
    },
    /// 설치 디렉터리를 OS 파일 매니저로 연다.
    OpenInstallDir {
        path: String,
    },
    /// 외부 디렉터리(`src_path`)를 `~/.tasty/plugins/<id>/`로 복사 설치.
    Install {
        src_path: String,
    },
    /// 임베드 키 / known-plugins.toml 모두 통과하지 못한 외부 plugin 에 대해
    /// 사용자가 *명시적으로* trust 한 후 install.
    ///
    /// 호스트 측 핸들러는 [`crate::plugin_bridge::known_plugins::KnownPlugins`]
    /// 에 `(plugin_id, KnownPluginEntry { pubkey, permissions, ... })` 항목을
    /// 추가/덮어쓴 다음, 일반 `Install` 과 동일한 디스크 복사 + discover 재호출
    /// 흐름을 진행한다.
    TrustAndInstall {
        src_path: String,
        plugin_id: String,
        /// base64 (32-byte ed25519) — `.pub` sidecar 에서 추출.
        pubkey_b64: String,
        /// 사용자가 trust 한 시점의 매니페스트 권한 목록 (변경 감지용 스냅샷).
        permissions: Vec<String>,
        /// 표시용 fingerprint — `KnownPluginEntry::publisher_fingerprint` 에
        /// 그대로 기록.
        publisher_fingerprint: String,
    },
}

/// 현재 활성 탭.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginsTab {
    List,
    Add,
}

impl Default for PluginsTab {
    fn default() -> Self {
        Self::List
    }
}

/// `Add` 탭에서 사용자가 경로를 검증한 결과 — 추가/취소 확인 단계로 진입.
#[derive(Debug, Clone)]
pub struct AddPreview {
    pub src_path: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub homepage: String,
    pub surface_kinds: Vec<String>,
    pub permissions: Vec<String>,
    /// 이미 같은 id의 플러그인이 설치되어 있으면 메시지 — 추가 버튼 비활성화.
    pub already_installed: Option<String>,
    /// 매니페스트 sig 검증으로 결정된 trust 상태. UI 분기 (빨간 경고 표시 여부).
    pub trust_state: AddTrustState,
}

/// 매니페스트 trust 결정 — UI 가 분기.
#[derive(Debug, Clone)]
pub enum AddTrustState {
    /// 임베드 키 또는 known-plugins.toml 통과. 바로 install 가능.
    Trusted,
    /// 출처 미상. 사용자 명시 trust + install 가능 (`.pub` sidecar 존재).
    UntrustedWithPubkey {
        /// `KnownPluginEntry::publisher_fingerprint` 로 표시 / 저장.
        fingerprint: String,
        /// base64 (32-byte) — TrustAndInstall action 으로 전달.
        pubkey_b64: String,
        /// 권한 변경 vs 첫 설치 — 메시지 분기.
        reason: AddTrustReason,
    },
    /// 출처 미상 + `.pub` sidecar 가 없거나 손상 — trust 등록 불가, install 차단.
    UntrustedNoPubkey {
        fingerprint: String,
        reason: AddTrustReason,
    },
    /// 매니페스트 sig 검증 자체 에러 (sidecar 누락 / 길이 / placeholder 키 등).
    SigError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddTrustReason {
    UnknownKey,
    PermissionsChanged,
}

/// 모달 자체 상태 (탭, 선택, 검색 입력 등).
#[derive(Debug, Default)]
pub struct PluginsUiState {
    pub active_tab: PluginsTab,
    pub selected_id: Option<String>,
    pub confirm_uninstall_id: Option<String>,
    /// `Add` 탭의 경로 입력 버퍼.
    pub add_path_input: String,
    /// 검증 후 preview 정보. 있으면 추가/취소 화면을 보여준다.
    pub add_preview: Option<AddPreview>,
    /// 검증 실패 시 에러 메시지 (UI 하단에 빨간 글씨로 표시).
    pub add_error: Option<String>,
}

/// modal 메인 그리기. snapshot은 읽기 전용, action은 큐에 추가.
pub fn draw_plugins_panel(
    ctx: &egui::Context,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let th = theme::theme();

    egui::TopBottomPanel::top("plugins_header")
        .exact_height(72.0)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(t("plugins.title"))
                        .size(14.0)
                        .color(egui::Color32::from(th.text)),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let tabs = [
                    (PluginsTab::List, t("plugins.tab_list")),
                    (PluginsTab::Add, t("plugins.tab_add")),
                ];
                for (tab, label) in &tabs {
                    let selected = ui_state.active_tab == *tab;
                    if ui.selectable_label(selected, *label).clicked() {
                        ui_state.active_tab = *tab;
                    }
                }
            });
            ui.add_space(2.0);
        });

    match ui_state.active_tab {
        PluginsTab::List => draw_list_tab(ctx, snapshot, ui_state, actions),
        PluginsTab::Add => draw_add_tab(ctx, snapshot, ui_state, actions),
    }
}

mod add;
mod list;

use add::draw_add_tab;
use list::draw_list_tab;
