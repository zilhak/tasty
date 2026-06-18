//! SSH 프로필 관리 팝업 (도구 메뉴 > SSH).
//!
//! `~/.tasty/ssh-profiles.toml` (`tasty_ssh_profiles::SshProfiles`) 의 프로필을
//! GUI 에서 CRUD 한다. CLI(`tasty tool ssh ...`)/IPC 와 같은 저장 로직(`SshProfiles::
//! load/save/upsert/remove`)을 그대로 재사용하므로 양쪽 표면이 즉시 일관된다.
//!
//! ## 범위
//! 현재 toml 스키마에 존재하는 필드 중 사용자 편집 대상만 다룬다:
//! name, host, user, port, identity_file, remote_tasty, label, port_mode.
//! (`use_agent` / `extra_options` / `remote_command` 은 편집 UI 를 두지 않되, 편집
//! 시 기존 값을 보존한다. 셸 타입/자동 감지 UI 는 `shell` 필드 신설 후 별도 작업.)
//!
//! ## 상태 / 생명주기
//! port_scanner 와 동일하게 UI 상태를 `egui::Memory` 에 보관하고, 닫힐 때
//! (Escape / × 버튼) 메모리를 비워 재오픈 시 항상 목록 화면 + 디스크 재로드로
//! 시작한다. headless PopupDef 라 자체 헤더(제목 + 닫기)를 그린다.

use std::sync::{Arc, Mutex};

use tasty_ssh_profiles::{SshProfile, SshProfiles};

use crate::adapters::ui::popup::PopupAction;
use crate::core::CoreState;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;

pub const SSH_TOOL_POPUP_ID: &str = "ssh_tool";

/// `port_mode` 가 가질 수 있는 값(toml 스키마와 일치). 기술 식별자라 번역하지 않고
/// 그대로 노출한다 (host / identity_file 경로처럼 비-자연어 값).
const PORT_MODES: &[&str] = &["auto", "subcommand", "file-unix", "file-windows"];

const UI_MEMORY_ID: &str = "ssh_tool.ui";
const PROFILES_MEMORY_ID: &str = "ssh_tool.profiles";

/// 어떤 화면을 보여줄지.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum SshView {
    #[default]
    List,
    /// 추가/편집 폼. 편집이면 [`SshUiState::editing_original`] 이 Some.
    Form,
    /// 삭제 확인 (대상 프로필 name).
    ConfirmDelete(String),
}

/// 추가/편집 폼의 입력 버퍼. `shell` 이 발견 모드를 도출하므로 `port_mode` 는 폼에
/// 직접 노출하지 않고 내부 보존만 한다(명시 셸→매핑, auto→감지가 채운다).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SshForm {
    name: String,
    host: String,
    user: String,
    port: String,
    identity_file: String,
    remote_tasty: String,
    label: String,
    /// 원격 셸: powershell|cmd|bash|zsh|auto.
    shell: String,
    /// 내부 보존용(폼에 노출 안 함) — 셸/감지가 도출.
    port_mode: String,
}

/// 자동감지 워커 진행 상태. 워커 스레드가 `slot` 에 결과를 채우고 repaint 를 요청한다
/// (egui memory 에 보관되므로 `Arc<Mutex>` 로 Clone+Send+Sync 를 만족).
#[derive(Clone, Debug)]
struct DetectJob {
    /// 감지 대상 프로필 name.
    name: String,
    /// `None` = 진행 중, `Some(Ok(mode))` = 성공, `Some(Err(msg))` = 실패.
    slot: Arc<Mutex<Option<Result<String, String>>>>,
}

/// 폼 검증 결과(실패 사유). i18n 키로 변환되어 표시.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SshFormError {
    NameEmpty,
    HostEmpty,
    PortInvalid,
    NameDuplicate,
    SaveFailed,
}

impl SshFormError {
    fn i18n_key(self) -> &'static str {
        match self {
            SshFormError::NameEmpty => "ssh_tool.error_name_empty",
            SshFormError::HostEmpty => "ssh_tool.error_host_empty",
            SshFormError::PortInvalid => "ssh_tool.error_port_invalid",
            SshFormError::NameDuplicate => "ssh_tool.error_name_duplicate",
            SshFormError::SaveFailed => "ssh_tool.error_save_failed",
        }
    }
}

/// 프레임 간 보존되는 팝업 UI 상태 (프로필 목록 캐시는 별도 슬롯).
#[derive(Clone, Debug, Default)]
struct SshUiState {
    view: SshView,
    form: SshForm,
    /// 편집 중이면 원래 name (rename 감지 / 중복 검사 제외용). None = 신규 추가.
    editing_original: Option<String>,
    error: Option<SshFormError>,
    /// 진행 중인 자동감지(재감지/auto 등록). 한 번에 하나만.
    detecting: Option<DetectJob>,
}

/// 뷰가 호출자(wrapper)에게 올리는 사용자 의도.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SshAction {
    None,
    Close,
    StartAdd,
    StartEdit(usize),
    AskDelete(usize),
    /// 프로필 재감지(새로고침 버튼) — 워커 스레드로 프로브 체인 실행.
    RefreshProfile(usize),
    SaveForm,
    CancelForm,
    ConfirmDelete(String),
    CancelDelete,
}

fn read_ui_state(ctx: &egui::Context) -> SshUiState {
    ctx.memory(|mem| {
        mem.data
            .get_temp::<SshUiState>(egui::Id::new(UI_MEMORY_ID))
            .unwrap_or_default()
    })
}

fn write_ui_state(ctx: &egui::Context, ui: SshUiState) {
    ctx.memory_mut(|mem| mem.data.insert_temp(egui::Id::new(UI_MEMORY_ID), ui));
}

fn read_profiles_cache(ctx: &egui::Context) -> Option<SshProfiles> {
    ctx.memory(|mem| {
        mem.data
            .get_temp::<SshProfiles>(egui::Id::new(PROFILES_MEMORY_ID))
    })
}

fn write_profiles_cache(ctx: &egui::Context, profiles: SshProfiles) {
    ctx.memory_mut(|mem| {
        mem.data
            .insert_temp(egui::Id::new(PROFILES_MEMORY_ID), profiles)
    });
}

fn clear_memory(ctx: &egui::Context) {
    ctx.memory_mut(|mem| {
        mem.data.remove::<SshUiState>(egui::Id::new(UI_MEMORY_ID));
        mem.data
            .remove::<SshProfiles>(egui::Id::new(PROFILES_MEMORY_ID));
    });
}

/// PopupDef.draw_fn. 메모리 상태를 읽어 뷰를 그리고, 액션을 상태 변경 +
/// 디스크 저장으로 번역한다.
pub fn draw_ssh_tool_popup(
    ui: &mut egui::Ui,
    _state: &mut AppState,
    _engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    let mut ui_state = read_ui_state(&ctx);
    // 첫 오픈 / 재오픈 시 디스크에서 로드. 이후 mutation 마다 재로드.
    let mut profiles = read_profiles_cache(&ctx).unwrap_or_else(SshProfiles::load);

    // 자동감지 워커 완료 polling — 완료 시 워커가 toml 을 갱신했으므로 재로드.
    if poll_detect(&mut ui_state) {
        profiles = SshProfiles::load();
    }

    let action = draw_body(ui, &th, &mut ui_state, &profiles);

    let result = apply_action(&ctx, &th, action, &mut ui_state, &mut profiles);

    match result {
        PopupAction::Close => {
            clear_memory(&ctx);
        }
        PopupAction::None => {
            write_ui_state(&ctx, ui_state);
            write_profiles_cache(&ctx, profiles);
        }
    }
    result
}

/// 액션을 상태 변경/저장으로 번역. 닫기면 `PopupAction::Close`.
fn apply_action(
    ctx: &egui::Context,
    _th: &Theme,
    action: SshAction,
    ui_state: &mut SshUiState,
    profiles: &mut SshProfiles,
) -> PopupAction {
    match action {
        SshAction::None => PopupAction::None,
        SshAction::Close => PopupAction::Close,
        SshAction::StartAdd => {
            ui_state.view = SshView::Form;
            ui_state.editing_original = None;
            ui_state.error = None;
            ui_state.form = SshForm {
                remote_tasty: "tasty".to_string(),
                shell: "auto".to_string(),
                port_mode: "auto".to_string(),
                ..SshForm::default()
            };
            PopupAction::None
        }
        SshAction::RefreshProfile(idx) => {
            // 재감지: 진행 중이 아니면 워커 스레드로 프로브 체인 실행.
            if ui_state.detecting.is_none()
                && let Some(p) = profiles.profiles.get(idx)
            {
                ui_state.detecting = Some(spawn_detect(ctx, p.name.clone()));
            }
            PopupAction::None
        }
        SshAction::StartEdit(idx) => {
            if let Some(p) = profiles.profiles.get(idx) {
                ui_state.form = form_from_profile(p);
                ui_state.editing_original = Some(p.name.clone());
                ui_state.error = None;
                ui_state.view = SshView::Form;
            }
            PopupAction::None
        }
        SshAction::AskDelete(idx) => {
            if let Some(p) = profiles.profiles.get(idx) {
                ui_state.view = SshView::ConfirmDelete(p.name.clone());
            }
            PopupAction::None
        }
        SshAction::CancelForm => {
            ui_state.view = SshView::List;
            ui_state.error = None;
            ui_state.editing_original = None;
            PopupAction::None
        }
        SshAction::SaveForm => {
            match validate_form(
                &ui_state.form,
                profiles,
                ui_state.editing_original.as_deref(),
            ) {
                Some(err) => ui_state.error = Some(err),
                None => {
                    let base = ui_state
                        .editing_original
                        .as_deref()
                        .and_then(|n| profiles.get(n))
                        .cloned();
                    let new_profile = build_profile(&ui_state.form, base.as_ref());
                    // shell=auto → 저장 후 등록 시 1회 감지(워커 스레드). 명시 셸은
                    // build_profile 이 이미 port_mode 를 도출했으므로 감지 불요.
                    let needs_detect =
                        tasty_ssh_profiles::shell_to_port_mode(&new_profile.shell).is_none();
                    let saved_name = new_profile.name.clone();
                    // rename: 원래 name 과 다르면 옛 항목 제거.
                    if let Some(orig) = &ui_state.editing_original
                        && orig != &new_profile.name
                    {
                        profiles.remove(orig);
                    }
                    profiles.upsert(new_profile);
                    if let Err(e) = profiles.save() {
                        tracing::error!("ssh_tool: failed to save profiles: {e}");
                        ui_state.error = Some(SshFormError::SaveFailed);
                    } else {
                        *profiles = SshProfiles::load();
                        ui_state.view = SshView::List;
                        ui_state.error = None;
                        ui_state.editing_original = None;
                        ui_state.form = SshForm::default();
                        if needs_detect && ui_state.detecting.is_none() {
                            ui_state.detecting = Some(spawn_detect(ctx, saved_name));
                        }
                    }
                }
            }
            PopupAction::None
        }
        SshAction::ConfirmDelete(name) => {
            profiles.remove(&name);
            if let Err(e) = profiles.save() {
                tracing::error!("ssh_tool: failed to save profiles after delete: {e}");
            } else {
                *profiles = SshProfiles::load();
            }
            ui_state.view = SshView::List;
            PopupAction::None
        }
        SshAction::CancelDelete => {
            ui_state.view = SshView::List;
            PopupAction::None
        }
    }
}

/// 본문 라우팅: 헤더(제목 + 닫기) + 현재 뷰.
fn draw_body(
    ui: &mut egui::Ui,
    th: &Theme,
    ui_state: &mut SshUiState,
    profiles: &SshProfiles,
) -> SshAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        // 폼/확인 화면에서 Escape 는 한 단계 뒤로, 목록에서는 닫기.
        return match &ui_state.view {
            SshView::List => SshAction::Close,
            SshView::Form => SshAction::CancelForm,
            SshView::ConfirmDelete(_) => SshAction::CancelDelete,
        };
    }

    let detecting_name = ui_state.detecting.as_ref().map(|j| j.name.clone());
    let mut action = SshAction::None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
        if let Some(a) = draw_header(ui, th) {
            action = a;
        }
        ui.separator();
        let view = ui_state.view.clone();
        match view {
            SshView::List => {
                if let Some(a) = draw_list(ui, th, profiles, detecting_name.as_deref()) {
                    action = a;
                }
            }
            SshView::Form => {
                if let Some(a) = draw_form(ui, th, ui_state) {
                    action = a;
                }
            }
            SshView::ConfirmDelete(name) => {
                if let Some(a) = draw_confirm_delete(ui, th, &name) {
                    action = a;
                }
            }
        }
    });
    action
}

/// 제목 + 우측 닫기(×) 버튼.
fn draw_header(ui: &mut egui::Ui, th: &Theme) -> Option<SshAction> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t("ssh_tool.heading"))
                .color(th.text)
                .size(th.font_size_heading.value())
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(
                    egui::RichText::new("×")
                        .size(th.font_size_heading.value())
                        .color(th.text),
                )
                .on_hover_text(t("ssh_tool.close"))
                .clicked()
            {
                out = Some(SshAction::Close);
            }
        });
    });
    out
}

/// 목록 뷰: 프로필 행 + 추가 버튼. 비어있으면 안내 문구.
/// `detecting_name` 은 현재 재감지 중인 프로필 name(있으면 그 행은 "감지 중" 표시).
fn draw_list(
    ui: &mut egui::Ui,
    th: &Theme,
    profiles: &SshProfiles,
    detecting_name: Option<&str>,
) -> Option<SshAction> {
    let mut out = None;

    ui.horizontal(|ui| {
        if ui.button(t("ssh_tool.add")).clicked() {
            out = Some(SshAction::StartAdd);
        }
    });

    ui.add_space(th.spacing_xs.value());

    if profiles.profiles.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(th.spacing_lg.value());
            ui.label(
                egui::RichText::new(t("ssh_tool.empty"))
                    .color(th.subtext0)
                    .italics()
                    .size(th.font_size_body.value()),
            );
        });
        return out;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, p) in profiles.profiles.iter().enumerate() {
            let detecting = detecting_name == Some(p.name.as_str());
            if let Some(a) = draw_profile_row(ui, th, i, p, detecting) {
                out = Some(a);
            }
        }
    });
    out
}

/// 한 프로필 행: name(+label) / destination / 셸·상태 + 새로고침/편집/삭제.
fn draw_profile_row(
    ui: &mut egui::Ui,
    th: &Theme,
    idx: usize,
    p: &SshProfile,
    detecting: bool,
) -> Option<SshAction> {
    let mut out = None;
    ui.horizontal(|ui| {
        // 좌측: 이름/대상/셸 정보.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let title = match &p.label {
                Some(l) if !l.is_empty() => format!("{}  ({})", p.name, l),
                _ => p.name.clone(),
            };
            // 비활성(감지 실패) 프로필은 이름을 흐리게.
            let name_color = if p.is_disabled() {
                th.overlay0
            } else {
                th.text
            };
            ui.label(
                egui::RichText::new(title)
                    .color(name_color)
                    .size(th.font_size_body.value())
                    .strong(),
            );
            let mut sub = p.ssh_destination();
            if let Some(port) = p.port {
                sub = format!("{sub}:{port}");
            }
            ui.label(
                egui::RichText::new(sub)
                    .color(th.subtext0)
                    .size(th.font_size_caption.value())
                    .monospace(),
            );
            // 셸 + 상태(감지 중 / 감지 실패).
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} {}", t("ssh_tool.col_shell"), p.shell))
                        .color(th.subtext0)
                        .size(th.font_size_caption.value()),
                );
                if detecting {
                    ui.add(egui::Spinner::new().size(th.font_size_caption.value()));
                    ui.label(
                        egui::RichText::new(t("ssh_tool.detecting"))
                            .color(th.subtext0)
                            .size(th.font_size_caption.value()),
                    );
                } else if p.is_disabled() {
                    ui.label(
                        egui::RichText::new(t("ssh_tool.detect_failed"))
                            .color(th.accent_danger())
                            .size(th.font_size_caption.value()),
                    );
                }
            });
        });
        // 우측: 액션 버튼 (재감지 중에는 비활성화).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("ssh_tool.delete")).clicked() {
                out = Some(SshAction::AskDelete(idx));
            }
            if ui.button(t("ssh_tool.edit")).clicked() {
                out = Some(SshAction::StartEdit(idx));
            }
            if ui
                .add_enabled(!detecting, egui::Button::new(t("ssh_tool.refresh")))
                .on_hover_text(t("ssh_tool.refresh_tooltip"))
                .clicked()
            {
                out = Some(SshAction::RefreshProfile(idx));
            }
        });
    });
    ui.separator();
    out
}

/// 추가/편집 폼.
fn draw_form(ui: &mut egui::Ui, th: &Theme, ui_state: &mut SshUiState) -> Option<SshAction> {
    let mut out = None;
    let editing = ui_state.editing_original.is_some();
    ui.label(
        egui::RichText::new(if editing {
            t("ssh_tool.form_edit_title")
        } else {
            t("ssh_tool.form_add_title")
        })
        .color(th.text)
        .size(th.font_size_body.value())
        .strong(),
    );
    ui.add_space(th.spacing_xs.value());

    let form = &mut ui_state.form;
    egui::Grid::new("ssh_tool.form_grid")
        .num_columns(2)
        .spacing([th.spacing_sm.value(), th.spacing_xs.value()])
        .show(ui, |ui| {
            text_field_row(ui, th, t("ssh_tool.field_name"), &mut form.name);
            text_field_row(ui, th, t("ssh_tool.field_host"), &mut form.host);
            text_field_row(ui, th, t("ssh_tool.field_user"), &mut form.user);
            text_field_row(ui, th, t("ssh_tool.field_port"), &mut form.port);
            text_field_row(
                ui,
                th,
                t("ssh_tool.field_identity_file"),
                &mut form.identity_file,
            );
            text_field_row(ui, th, t("ssh_tool.field_label"), &mut form.label);
            text_field_row(
                ui,
                th,
                t("ssh_tool.field_remote_tasty"),
                &mut form.remote_tasty,
            );
            // shell 드롭다운(auto 포함). auto → 저장 시 자동감지, 명시 셸 → 모드 즉시 도출.
            field_label(ui, th, t("ssh_tool.field_shell"));
            egui::ComboBox::from_id_salt("ssh_tool.shell")
                .selected_text(form.shell.clone())
                .show_ui(ui, |ui| {
                    for shell in tasty_ssh_profiles::SHELLS {
                        ui.selectable_value(&mut form.shell, (*shell).to_string(), *shell);
                    }
                });
            ui.end_row();
        });

    // auto 선택 시 저장 후 감지가 실행됨을 알린다(필드 밑 힌트).
    if form.shell == "auto" {
        ui.label(
            egui::RichText::new(t("ssh_tool.shell_auto_hint"))
                .color(th.subtext0)
                .size(th.font_size_caption.value()),
        );
    }

    if let Some(err) = ui_state.error {
        ui.add_space(th.spacing_xs.value());
        ui.label(
            egui::RichText::new(t(err.i18n_key()))
                .color(th.accent_danger())
                .size(th.font_size_caption.value()),
        );
    }

    ui.add_space(th.spacing_sm.value());
    ui.horizontal(|ui| {
        if ui.button(t("ssh_tool.save")).clicked() {
            out = Some(SshAction::SaveForm);
        }
        if ui.button(t("ssh_tool.cancel")).clicked() {
            out = Some(SshAction::CancelForm);
        }
    });
    out
}

/// Grid 한 줄: 라벨 + 단일행 TextEdit.
fn text_field_row(ui: &mut egui::Ui, th: &Theme, label: &str, value: &mut String) {
    field_label(ui, th, label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    ui.end_row();
}

fn field_label(ui: &mut egui::Ui, th: &Theme, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .color(th.subtext0)
            .size(th.font_size_body.value()),
    );
}

/// 삭제 확인 뷰.
fn draw_confirm_delete(ui: &mut egui::Ui, th: &Theme, name: &str) -> Option<SshAction> {
    let mut out = None;
    ui.add_space(th.spacing_sm.value());
    ui.label(
        egui::RichText::new(t("ssh_tool.confirm_delete").replace("{name}", name))
            .color(th.text)
            .size(th.font_size_body.value()),
    );
    ui.add_space(th.spacing_sm.value());
    ui.horizontal(|ui| {
        if ui
            .button(egui::RichText::new(t("ssh_tool.delete")).color(th.accent_danger()))
            .clicked()
        {
            out = Some(SshAction::ConfirmDelete(name.to_string()));
        }
        if ui.button(t("ssh_tool.cancel")).clicked() {
            out = Some(SshAction::CancelDelete);
        }
    });
    out
}

// ── 순수 헬퍼 (테스트 대상) ──────────────────────────────────────────────

fn nonempty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// `port` 버퍼 파싱: 비어있으면 Ok(None), 유효한 u16 이면 Ok(Some), 그 외 Err.
fn parse_port(buf: &str) -> Result<Option<u16>, ()> {
    let t = buf.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<u16>().map(Some).map_err(|_| ())
}

/// 프로필 → 폼 버퍼.
fn form_from_profile(p: &SshProfile) -> SshForm {
    SshForm {
        name: p.name.clone(),
        host: p.host.clone(),
        user: p.user.clone().unwrap_or_default(),
        port: p.port.map(|n| n.to_string()).unwrap_or_default(),
        identity_file: p.identity_file.clone().unwrap_or_default(),
        remote_tasty: p.remote_tasty.clone(),
        label: p.label.clone().unwrap_or_default(),
        shell: p.shell.clone(),
        port_mode: p.port_mode.clone(),
    }
}

/// 폼 버퍼 + (편집 시) base 프로필 → 저장할 `SshProfile`. base 가 있으면 편집 범위
/// 밖 필드(use_agent/extra_options/remote_command)를 보존한다.
fn build_profile(form: &SshForm, base: Option<&SshProfile>) -> SshProfile {
    let name = form.name.trim();
    let host = form.host.trim();
    let mut p = base.cloned().unwrap_or_else(|| SshProfile::new(name, host));
    p.name = name.to_string();
    p.host = host.to_string();
    p.user = nonempty(&form.user);
    p.port = parse_port(&form.port).ok().flatten();
    p.identity_file = nonempty(&form.identity_file);
    let rt = form.remote_tasty.trim();
    p.remote_tasty = if rt.is_empty() {
        "tasty".to_string()
    } else {
        rt.to_string()
    };
    p.label = nonempty(&form.label);
    // 셸: 빈 값/알 수 없는 값은 auto 로 정규화.
    let shell = if tasty_ssh_profiles::is_valid_shell(&form.shell) {
        form.shell.clone()
    } else {
        "auto".to_string()
    };
    p.shell = shell;
    // 명시 셸 → 발견 모드 즉시 도출 + 활성화(감지 없이). auto → 기존/기본 port_mode 유지
    // (저장 후 워커 감지가 채운다). 어느 쪽이든 저장 시점엔 비활성 해제.
    p.detect_failed = false;
    if let Some(mode) = tasty_ssh_profiles::shell_to_port_mode(&p.shell) {
        p.port_mode = mode.to_string();
    } else if !PORT_MODES.contains(&form.port_mode.as_str()) {
        p.port_mode = "auto".to_string();
    } else {
        p.port_mode = form.port_mode.clone();
    }
    p
}

/// 자동감지 워커를 띄운다(SSH 프로브 + toml 갱신). 결과는 `slot` 에 채워지고 완료
/// 시 repaint 를 요청한다. 호출자는 반환된 [`DetectJob`] 을 `ui_state.detecting` 에 보관.
fn spawn_detect(ctx: &egui::Context, name: String) -> DetectJob {
    let slot: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let slot_worker = Arc::clone(&slot);
    let ctx_worker = ctx.clone();
    let name_worker = name.clone();
    std::thread::spawn(move || {
        let res = tasty_cli::ssh::detect_and_persist(&name_worker)
            .map(|m| m.as_str().to_string())
            .map_err(|e| e.to_string());
        if let Ok(mut guard) = slot_worker.lock() {
            *guard = Some(res);
        }
        ctx_worker.request_repaint();
    });
    DetectJob { name, slot }
}

/// 진행 중인 감지 워커를 점검한다. 완료됐으면 `detecting` 을 비우고 `true` 를 반환한다
/// (호출자가 프로필 재로드). 아직 진행 중이면 `false`.
fn poll_detect(ui_state: &mut SshUiState) -> bool {
    let done = match &ui_state.detecting {
        Some(job) => job.slot.lock().map(|g| g.is_some()).unwrap_or(true), // lock poisoned(워커 panic) → 완료로 간주해 해제.
        None => false,
    };
    if done {
        ui_state.detecting = None;
    }
    done
}

/// 폼 검증. 통과면 None.
fn validate_form(
    form: &SshForm,
    profiles: &SshProfiles,
    editing_original: Option<&str>,
) -> Option<SshFormError> {
    if form.name.trim().is_empty() {
        return Some(SshFormError::NameEmpty);
    }
    if form.host.trim().is_empty() {
        return Some(SshFormError::HostEmpty);
    }
    if parse_port(&form.port).is_err() {
        return Some(SshFormError::PortInvalid);
    }
    let name = form.name.trim();
    let duplicate = profiles
        .profiles
        .iter()
        .any(|p| p.name == name && Some(p.name.as_str()) != editing_original);
    if duplicate {
        return Some(SshFormError::NameDuplicate);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn form(name: &str, host: &str) -> SshForm {
        SshForm {
            name: name.to_string(),
            host: host.to_string(),
            remote_tasty: "tasty".to_string(),
            shell: "auto".to_string(),
            port_mode: "auto".to_string(),
            ..SshForm::default()
        }
    }

    #[test]
    fn build_profile_explicit_shell_maps_port_mode_and_activates() {
        // 명시 셸 → 매핑으로 port_mode 도출 + detect_failed=false (감지 없이 활성).
        let mut base = SshProfile::new("gx", "h");
        base.detect_failed = true; // 이전 비활성 상태였더라도
        let mut f = form("gx", "h");
        f.shell = "cmd".into();
        let p = build_profile(&f, Some(&base));
        assert_eq!(p.shell, "cmd");
        assert_eq!(p.port_mode, "file-windows");
        assert!(!p.detect_failed); // 활성으로 복귀.
    }

    #[test]
    fn build_profile_auto_keeps_mode_for_worker_detection() {
        // auto → 매핑 없음. port_mode 는 폼 값(검증된)으로 유지(이후 워커 감지가 채움).
        let mut f = form("gx", "h");
        f.shell = "auto".into();
        f.port_mode = "subcommand".into();
        let p = build_profile(&f, None);
        assert_eq!(p.shell, "auto");
        assert_eq!(p.port_mode, "subcommand");
        assert!(!p.detect_failed);
    }

    #[test]
    fn parse_port_handles_empty_valid_invalid() {
        assert_eq!(parse_port(""), Ok(None));
        assert_eq!(parse_port("  "), Ok(None));
        assert_eq!(parse_port("2222"), Ok(Some(2222)));
        assert_eq!(parse_port("70000"), Err(()));
        assert_eq!(parse_port("abc"), Err(()));
    }

    #[test]
    fn validate_rejects_empty_name_and_host() {
        let ps = SshProfiles::default();
        assert_eq!(
            validate_form(&form("", "h"), &ps, None),
            Some(SshFormError::NameEmpty)
        );
        assert_eq!(
            validate_form(&form("n", ""), &ps, None),
            Some(SshFormError::HostEmpty)
        );
    }

    #[test]
    fn validate_rejects_invalid_port() {
        let ps = SshProfiles::default();
        let mut f = form("n", "h");
        f.port = "notaport".to_string();
        assert_eq!(
            validate_form(&f, &ps, None),
            Some(SshFormError::PortInvalid)
        );
    }

    #[test]
    fn validate_rejects_duplicate_name_when_adding() {
        let mut ps = SshProfiles::default();
        ps.upsert(SshProfile::new("dup", "h"));
        assert_eq!(
            validate_form(&form("dup", "h2"), &ps, None),
            Some(SshFormError::NameDuplicate)
        );
    }

    #[test]
    fn validate_allows_same_name_when_editing_self() {
        let mut ps = SshProfiles::default();
        ps.upsert(SshProfile::new("self", "h"));
        // 자기 자신을 편집 — 같은 name 허용.
        assert_eq!(validate_form(&form("self", "h"), &ps, Some("self")), None);
    }

    #[test]
    fn build_profile_new_sets_scope_fields() {
        let mut f = form("gx", "box");
        f.user = "zilhak".to_string();
        f.port = "2222".to_string();
        f.identity_file = "~/.ssh/id".to_string();
        f.label = "GX".to_string();
        f.remote_tasty = "/usr/bin/tasty".to_string();
        f.port_mode = "file-unix".to_string();
        let p = build_profile(&f, None);
        assert_eq!(p.name, "gx");
        assert_eq!(p.host, "box");
        assert_eq!(p.user.as_deref(), Some("zilhak"));
        assert_eq!(p.port, Some(2222));
        assert_eq!(p.identity_file.as_deref(), Some("~/.ssh/id"));
        assert_eq!(p.label.as_deref(), Some("GX"));
        assert_eq!(p.remote_tasty, "/usr/bin/tasty");
        assert_eq!(p.port_mode, "file-unix");
        assert!(p.use_agent); // 기본값 보존.
    }

    #[test]
    fn build_profile_edit_preserves_out_of_scope_fields() {
        let mut base = SshProfile::new("gx", "box");
        base.use_agent = false;
        base.extra_options = vec!["ServerAliveInterval=30".to_string()];
        base.remote_command = Some("tmux a".to_string());
        // 폼에서 host 만 변경.
        let mut f = form("gx", "newbox");
        f.remote_tasty = base.remote_tasty.clone();
        let p = build_profile(&f, Some(&base));
        assert_eq!(p.host, "newbox");
        // 범위 밖 필드 보존.
        assert!(!p.use_agent);
        assert_eq!(p.extra_options, vec!["ServerAliveInterval=30".to_string()]);
        assert_eq!(p.remote_command.as_deref(), Some("tmux a"));
    }

    #[test]
    fn empty_port_buffer_yields_none() {
        let p = build_profile(&form("n", "h"), None);
        assert!(p.port.is_none());
    }

    fn run_body(view: SshView, profiles: &SshProfiles, raw: egui::RawInput) -> SshAction {
        let ctx = egui::Context::default();
        let th = theme();
        let mut out = SshAction::None;
        let mut ui_state = SshUiState {
            view,
            ..SshUiState::default()
        };
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                out = draw_body(ui, &th, &mut ui_state, profiles);
            });
        }));
        out
    }

    #[test]
    fn escape_on_list_closes() {
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        assert_eq!(
            run_body(SshView::List, &SshProfiles::default(), raw),
            SshAction::Close
        );
    }

    #[test]
    fn escape_on_form_cancels() {
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        assert_eq!(
            run_body(SshView::Form, &SshProfiles::default(), raw),
            SshAction::CancelForm
        );
    }

    #[test]
    fn list_renders_with_profiles_without_panic() {
        let mut ps = SshProfiles::default();
        ps.upsert(SshProfile::new("a", "ha"));
        ps.upsert(SshProfile::new("b", "hb"));
        assert_eq!(
            run_body(SshView::List, &ps, egui::RawInput::default()),
            SshAction::None
        );
    }

    #[test]
    fn empty_list_renders_without_panic() {
        assert_eq!(
            run_body(
                SshView::List,
                &SshProfiles::default(),
                egui::RawInput::default()
            ),
            SshAction::None
        );
    }
}
