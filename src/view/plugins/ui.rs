//! `PluginsView` modal의 egui UI.
//!
//! 상단 — 탭 바 (`Installed` / `Add plugin`).
//! `Installed` 탭: 좌측 plugin 목록, 우측 상세(매니페스트, enable/disable,
//! 권한 grant/revoke, 설치 경로, uninstall).
//! `Add plugin` 탭: 경로 입력 → 검증 → 추가/취소.
//!
//! 모달은 `PluginsSnapshot`(읽기 전용 데이터)을 들고 있고, 사용자 조작은
//! `PluginsAction` 큐에 쌓여 메인 루프에서 `PluginManager`에 적용된다.

use crate::adapters::ui::icons;
use crate::i18n::t;
use crate::theme;

/// 상세 패널에 표시할 plugin command 한 줄.
#[derive(Debug, Clone)]
pub struct PluginCommandEntry {
    /// 표시 라벨 i18n 키 (plugin lang_dir 의 `title_i18n_key`). `t()` 로 해석.
    pub title_key: String,
    /// 효과 단축키 (override 우선, 없으면 매니페스트 default). `None` 이면 미할당.
    pub keybinding: Option<String>,
}

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
    /// spawn 반복 실패로 자동 비활성화된 error 상태인지. 디자인의 error dot
    /// (목록) + 경고 박스(상세) 표시 기준.
    pub health_error: bool,
    pub builtin: bool,
    pub surface_kinds: Vec<String>,
    pub manifest_permissions: Vec<String>,
    pub granted_permissions: Vec<String>,
    /// plugin 이 contribute 한 command 목록 (`[[contributes.commands]]`).
    pub commands: Vec<PluginCommandEntry>,
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
    /// 상세의 `Configure` 버튼 — 이 모달을 닫고 Settings›Plugins 탭을 연다.
    /// (lifecycle 창 → per-plugin config 의 연결 고리.)
    OpenSettings,
    /// 헤더 X 버튼 — 모달을 닫는다.
    Close,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginsTab {
    #[default]
    List,
    Add,
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
    /// `Installed` 탭 목록 검색/필터 입력 버퍼 (name/authors/description 부분일치).
    pub filter: String,
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
        .exact_height(48.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(10.0);
                // 디자인 헤더: plug 아이콘 + 타이틀.
                ui.add(icons::PLUG.image(17.0, egui::Color32::from(th.peach)));
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t("plugins.title"))
                        .size(14.0)
                        .strong()
                        .color(egui::Color32::from(th.text_primary())),
                );
                ui.add_space(8.0);
                draw_header_divider(ui, &th);
                ui.add_space(8.0);

                // 세그먼트 탭 (Installed N / Add plugin). Installed 만 카운트 노출.
                let installed_count = snapshot.plugins.len();
                if segment_tab(
                    ui,
                    &th,
                    t("plugins.tab_list"),
                    Some(installed_count),
                    ui_state.active_tab == PluginsTab::List,
                ) {
                    ui_state.active_tab = PluginsTab::List;
                }
                ui.add_space(2.0);
                if segment_tab(
                    ui,
                    &th,
                    t("plugins.tab_add"),
                    None,
                    ui_state.active_tab == PluginsTab::Add,
                ) {
                    ui_state.active_tab = PluginsTab::Add;
                }

                // 우측 클러스터 (오른쪽→왼쪽): X 닫기 → 검색 입력.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    if close_icon_button(ui, &th) {
                        actions.push(PluginsAction::Close);
                    }
                    // 검색/필터는 목록이 있는 Installed 탭에서만 동작 (Add 탭은
                    // 로컬 경로 설치 폼이라 필터 대상 목록이 없다).
                    if ui_state.active_tab == PluginsTab::List {
                        ui.add_space(8.0);
                        let edit = egui::TextEdit::singleline(&mut ui_state.filter)
                            .hint_text(tasty_egui_theme::hint_text(
                                &th,
                                t("plugins.filter_placeholder"),
                            ))
                            .desired_width(200.0);
                        ui.add(edit);
                    }
                });
            });
        });

    match ui_state.active_tab {
        PluginsTab::List => draw_list_tab(ctx, snapshot, ui_state, actions),
        PluginsTab::Add => draw_add_tab(ctx, snapshot, ui_state, actions),
    }
}

/// 헤더의 타이틀 ↔ 세그먼트 사이 세로 구분선 (1px × 20px).
fn draw_header_divider(ui: &mut egui::Ui, th: &theme::Theme) {
    let w = th.border_width.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 20.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from(th.separator));
}

/// 헤더 우측 X 닫기 버튼 (IconButton). 클릭 시 true.
fn close_icon_button(ui: &mut egui::Ui, th: &theme::Theme) -> bool {
    let size = 28.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            rect,
            th.corner_radius.value(),
            th.overlay_active().to_egui_premultiplied(),
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            th.corner_radius.value(),
            th.overlay_hover().to_egui_premultiplied(),
        );
    }
    let color = if resp.hovered() {
        egui::Color32::from(th.text_primary())
    } else {
        egui::Color32::from(th.text_muted())
    };
    let glyph = 16.0;
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    icons::CLOSE.image(glyph, color).paint_at(ui, icon_rect);
    resp.clicked()
}

/// 디자인 pill 세그먼트 탭 한 개. active 면 surface-raised 배경 + inset border,
/// hover 면 overlay. `count` 가 있으면 라벨 우측에 mono 카운트를 덧붙인다.
fn segment_tab(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    label: &str,
    count: Option<usize>,
    selected: bool,
) -> bool {
    let label_color = if selected {
        egui::Color32::from(th.text_primary())
    } else {
        egui::Color32::from(th.text_muted())
    };
    let count_color = if selected {
        egui::Color32::from(th.text_secondary())
    } else {
        egui::Color32::from(th.text_muted())
    };
    let label_galley = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::FontId::proportional(12.5),
        label_color,
    );
    let count_galley = count.map(|c| {
        ui.painter()
            .layout_no_wrap(c.to_string(), egui::FontId::monospace(10.5), count_color)
    });

    let pad_x = 12.0;
    let gap = 7.0;
    let height = 26.0;
    let mut width = label_galley.size().x + pad_x * 2.0;
    if let Some(g) = &count_galley {
        width += gap + g.size().x;
    }

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let radius = th.corner_radius.value();
    if selected {
        ui.painter().rect(
            rect,
            radius,
            egui::Color32::from(th.surface_raised()),
            egui::Stroke::new(
                th.border_width.value(),
                egui::Color32::from(th.border_default()),
            ),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius, th.overlay_hover().to_egui_premultiplied());
    }

    let mut x = rect.left() + pad_x;
    let label_y = rect.center().y - label_galley.size().y / 2.0;
    let label_w = label_galley.size().x;
    ui.painter()
        .galley(egui::pos2(x, label_y), label_galley, label_color);
    if let Some(g) = count_galley {
        x += label_w + gap;
        let cy = rect.center().y - g.size().y / 2.0;
        ui.painter().galley(egui::pos2(x, cy), g, count_color);
    }
    resp.clicked()
}

/// 디자인 `Tag` 컴포넌트 — surface-raised 배경 + 1px border 의 작은 pill.
/// 버전 표기 등 inline 메타데이터에 사용. 라벨 색은 `text_secondary`.
pub(super) fn tag(ui: &mut egui::Ui, th: &theme::Theme, text: &str) {
    let color = egui::Color32::from(th.text_secondary());
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), egui::FontId::proportional(11.0), color);
    let pad = egui::vec2(7.0, 3.0);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect(
        rect,
        th.corner_radius.value(),
        egui::Color32::from(th.surface_raised()),
        egui::Stroke::new(
            th.border_width.value(),
            egui::Color32::from(th.border_default()),
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(rect.min + pad, galley, color);
}

mod add;
mod list;

use add::draw_add_tab;
use list::draw_list_tab;
