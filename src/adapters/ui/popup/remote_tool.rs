//! 원격 접속 도구 팝업 (도구 메뉴 > Remote connections). 2탭: 원격 접속 프로필 / Passkey.
//!
//! `~/.tasty/remote-profiles.toml`(`RemoteProfiles`) + `~/.tasty/passkeys.toml`(`Passkeys`)
//! 를 GUI 에서 CRUD 한다. CLI/IPC 와 같은 저장 로직을 재사용하므로 표면이 즉시 일관된다.
//! 프로필은 비밀을 담지 않고 passkey 를 이름으로 참조만 한다. 한 탭 안에서 List/Form/
//! ConfirmDelete 를 라우팅한다(ssh_tool 과 동일 패턴, 두 번 인스턴스화). headless PopupDef.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use tasty_remote_profiles::{
    KNOWN_PASSKEY_KINDS, Passkey, Passkeys, RemoteProfile, RemoteProfiles, SHELLS,
    is_builtin_kind, is_valid_passkey_name, is_valid_shell,
};

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::core::CoreState;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;

pub const REMOTE_TOOL_POPUP_ID: &str = "remote_tool";

/// 콤보박스 제안용 알려진 프로필 타입(열린 string — 자유 입력 허용).
const KNOWN_TYPES: &[&str] = &["ssh", "smb", "http"];

const UI_MEMORY_ID: &str = "remote_tool.ui";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Profiles,
    Passkeys,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum Sub {
    #[default]
    List,
    Form,
    ConfirmDelete(String),
}

/// 프로필 폼 버퍼. ssh 는 전용 필드, 그 외는 generic key-value(`fields`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ProfileForm {
    kind: String,
    name: String,
    label: String,
    // ssh 전용
    host: String,
    user: String,
    port: String,
    remote_tasty: String,
    shell: String,
    // 공통
    passkey_ref: String,
    fields: Vec<(String, String)>, // generic(비-ssh)
    editing_original: Option<String>,
}

/// Passkey 폼 버퍼.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PasskeyForm {
    name: String,
    kind: String,
    value: String,
    editing_original: Option<String>,
}

#[derive(Clone, Debug)]
struct DetectJob {
    name: String,
    slot: Arc<Mutex<Option<Result<String, String>>>>,
}

#[derive(Clone, Debug, Default)]
struct UiState {
    tab: Tab,
    profile_view: Sub,
    passkey_view: Sub,
    pform: ProfileForm,
    kform: PasskeyForm,
    perr: Option<String>,
    kerr: Option<String>,
    revealed: HashSet<String>,
    detecting: Option<DetectJob>,
}

fn read_ui(ctx: &egui::Context) -> UiState {
    ctx.memory(|m| m.data.get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID)).unwrap_or_default())
}
fn write_ui(ctx: &egui::Context, ui: UiState) {
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new(UI_MEMORY_ID), ui));
}
fn clear_ui(ctx: &egui::Context) {
    ctx.memory_mut(|m| m.data.remove::<UiState>(egui::Id::new(UI_MEMORY_ID)));
}

/// PopupDef.draw_fn 진입점.
pub fn draw_remote_tool_popup(
    ui: &mut egui::Ui,
    _state: &mut AppState,
    _engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();
    let mut st = read_ui(&ctx);

    // detect 워커 완료 polling.
    poll_detect(&mut st);

    let mut profiles = RemoteProfiles::load();
    let passkeys = Passkeys::load();

    // Escape: Form/Confirm 은 뒤로, List 면 닫기.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        let sub = match st.tab {
            Tab::Profiles => &mut st.profile_view,
            Tab::Passkeys => &mut st.passkey_view,
        };
        if *sub == Sub::List {
            clear_ui(&ctx);
            return PopupAction::Close;
        }
        *sub = Sub::List;
        write_ui(&ctx, st);
        return PopupAction::None;
    }

    let mut close = false;
    // 디자인(remote_tool.jsx) 컨테이너는 패딩 0 이고 각 구역이 자체 패딩을 가진다.
    // popup content_margin 은 remote_tool 한정 0 (popup.rs) 이라 full 은 popup 가장자리.
    // egui 자동 간격을 죽이고(아래) 구역 divider 는 각 구역 Frame 의 실제 bottom 좌표에
    // 그린다 (design-parity: 어림 add_space 금지).
    let full = ui.max_rect();
    // 구역(헤더/탭바/콘텐츠)을 디자인처럼 딱 붙이려면 세로 자동간격만 죽인다. x 간격은
    // 건드리지 않는다(콘텐츠 행 내부 gap 이 망가지지 않게). 콘텐츠 영역은 아래에서
    // 원래 spacing 을 복원해 행 레이아웃을 보존한다.
    let saved_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    let sep = egui::Stroke::new(th.border_width.value(), th.surface1);

    // 헤더 — 디자인 padding T11 R12 B11 L14 + borderBottom separator.
    let header_ir = egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 14,
            right: 12,
            top: 11,
            bottom: 11,
        })
        .show(ui, |ui| draw_header(ui, &th));
    if header_ir.inner {
        close = true;
    }
    ui.painter()
        .hline(full.x_range(), header_ir.response.rect.bottom(), sep);

    // 탭바 — 디자인 bg-sidebar(mantle) 전체폭, TabBtn height 35, padding L8.
    // 자체 하단 borderBottom 까지 내부에서 그린다.
    draw_tab_bar(ui, &th, &mut st, full.x_range());

    // 콘텐츠 — 디자인 리스트 좌우 14, add bar top 10, 하단 8.
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 10,
            bottom: 8,
        })
        .show(ui, |ui| {
            // 콘텐츠 행 레이아웃은 기존 spacing 으로 복원(프레임 정합과 분리).
            ui.spacing_mut().item_spacing = saved_spacing;
            match st.tab {
                Tab::Profiles => {
                    draw_profiles_tab(ui, &th, &ctx, &mut st, &mut profiles, &passkeys)
                }
                Tab::Passkeys => draw_passkeys_tab(ui, &th, &mut st, &passkeys),
            }
        });

    if close {
        clear_ui(&ctx);
        PopupAction::Close
    } else {
        write_ui(&ctx, st);
        PopupAction::None
    }
}

/// 디자인 secondary 버튼 (Button.jsx `--secondary`): surface-raised(surface0) 채움.
/// base(bg-panel) 패널 배경 위에서 한 단계 밝게 떠 보인다. (egui inactive 기본 버튼은
/// fill=base 라 base 패널 위에서 묻히므로 fill 을 명시한다.)
fn secondary_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(th.text).size(th.font_size_body.value()))
            .fill(th.surface0)
            .stroke(egui::Stroke::new(th.border_width.value(), th.surface1)),
    )
}

/// 디자인 separator 선. egui `ui.separator()` 는 theme stroke 색이 배경과 가까워
/// 사실상 비가시 → surface1 색 명시적 hline 으로 그린다.
fn hsep(ui: &mut egui::Ui, th: &Theme) {
    ui.add_space(2.0);
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        ui.cursor().top(),
        egui::Stroke::new(th.border_width.value(), th.surface1),
    );
    ui.add_space(2.0);
}

fn draw_header(ui: &mut egui::Ui, th: &Theme) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        // 디자인 헤더 콘텐츠 높이 ~24 (title fontSize14 line-height). egui label/icon 은
        // 텍스트 박스가 더 낮아(~18) 헤더가 얕아진다 → min_height 로 디자인 높이 강제.
        // popup border 가 stroke Outside 라 콘텐츠가 1px 위에서 시작 → +2 보정해 26.
        ui.set_min_height(26.0);
        // 디자인 헤더 gap 9 (토큰 아닌 raw — 가장 가까운 토큰 spacing_sm=8 과 1px 차).
        ui.spacing_mut().item_spacing.x = 9.0;
        // 헤더 앞 터미널 프롬프트 아이콘(`>_`) — 디자인 remote_tool.jsx 헤더.
        ui.add(icons::TERMINAL_PROMPT.image(16.0, th.subtext0.into()));
        ui.label(
            egui::RichText::new(t("remote_tool.heading"))
                .color(th.text)
                .size(th.font_size_heading.value())
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::ImageButton::new(icons::CLOSE.image(16.0, th.subtext0.into())).frame(false))
                .on_hover_text(t("remote_tool.close"))
                .clicked()
            {
                close = true;
            }
        });
    });
    close
}

fn draw_tab_bar(ui: &mut egui::Ui, th: &Theme, st: &mut UiState, x_range: egui::Rangef) {
    // 언더라인 탭 (디자인 remote_tool.jsx TabBtn): 전체폭 bg-sidebar(mantle), height 35,
    // padding L8 / TabBtn padding 0 13 / gap 2. 활성 = text-primary + 하단 2px accent,
    // 비활성 = text-muted. 좌표를 직접 계산해 그린다(egui 자동 배치 우회).
    let tab_h = 36.0; // 디자인 TabBtn 35 + borderBottom 1 = 탭바 컨테이너 36
    let pad_l = 8.0; // 디자인 탭바 padding-left
    let pad_x = 13.0; // 디자인 TabBtn padding 0 13
    let gap = 2.0; // 디자인 탭바 gap
    let font = egui::FontId::proportional(th.font_size_body.value());

    let top = ui.cursor().top();
    let bar = egui::Rect::from_min_size(
        egui::pos2(x_range.min, top),
        egui::vec2(x_range.span(), tab_h),
    );
    // bg-sidebar 전체폭 + 하단 borderBottom separator (mantle 위 → surface1 근사).
    ui.painter().rect_filled(bar, 0.0, th.mantle);
    ui.painter().hline(
        x_range,
        bar.max.y,
        egui::Stroke::new(th.border_width.value(), th.surface1),
    );

    let mut x = x_range.min + pad_l;
    for (tab, key) in [
        (Tab::Profiles, "remote_tool.tab_profiles"),
        (Tab::Passkeys, "remote_tool.tab_passkeys"),
    ] {
        let on = st.tab == tab;
        let label = t(key);
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(label.to_string(), font.clone(), th.text.into())
                .size()
                .x
        });
        let w = text_w + pad_x * 2.0;
        let rect = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, tab_h));
        let resp = ui.interact(rect, ui.id().with((key, "rt_tab")), egui::Sense::click());
        if resp.hovered() && !on {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font.clone(),
            if on { th.text.into() } else { th.subtext0.into() },
        );
        // 활성 탭 하단 2px accent — separator 위에 그려 덮는다.
        if on {
            ui.painter().hline(
                rect.x_range(),
                bar.max.y - 1.0,
                egui::Stroke::new(2.0, th.accent_primary()),
            );
        }
        if resp.clicked() && st.tab != tab {
            st.tab = tab;
            st.profile_view = Sub::List;
            st.passkey_view = Sub::List;
            st.perr = None;
            st.kerr = None;
        }
        x += w + gap;
    }
    // 탭바 영역만큼 커서 전진 → 다음 구역(콘텐츠)이 그 아래로.
    ui.allocate_rect(bar, egui::Sense::hover());
}

// ── 경고 배지 ────────────────────────────────────────────────────────────
fn warn_badge(ui: &mut egui::Ui, th: &Theme, text: &str, tooltip: &str) {
    ui.label(
        egui::RichText::new(format!("⚠ {text}"))
            .color(th.yellow)
            .size(th.font_size_caption.value()),
    )
    .on_hover_text(tooltip);
}

// ════════════════════════════════════════════════════════════════════════
// TAB A — 원격 접속 프로필
// ════════════════════════════════════════════════════════════════════════
fn draw_profiles_tab(
    ui: &mut egui::Ui,
    th: &Theme,
    ctx: &egui::Context,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    passkeys: &Passkeys,
) {
    match st.profile_view.clone() {
        Sub::List => draw_profile_list(ui, th, st, profiles, passkeys),
        Sub::Form => draw_profile_form(ui, th, ctx, st, profiles, passkeys),
        Sub::ConfirmDelete(name) => {
            if let Some(act) = draw_confirm_delete(ui, th, t("remote_tool.noun_profile"), &name, None) {
                if act {
                    profiles.remove(&name);
                    let _ = profiles.save();
                }
                st.profile_view = Sub::List;
            }
        }
    }
}

fn draw_profile_list(
    ui: &mut egui::Ui,
    th: &Theme,
    st: &mut UiState,
    profiles: &RemoteProfiles,
    passkeys: &Passkeys,
) {
    if secondary_button(ui, th, t("remote_tool.profile_add")).clicked() {
        st.pform = ProfileForm {
            kind: "ssh".into(),
            shell: "auto".into(),
            remote_tasty: "tasty".into(),
            ..Default::default()
        };
        st.perr = None;
        st.profile_view = Sub::Form;
        return;
    }
    ui.add_space(th.spacing_xs.value());
    if profiles.profiles.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(th.spacing_lg.value());
            ui.label(
                egui::RichText::new(t("remote_tool.profile_empty"))
                    .color(th.subtext0)
                    .italics()
                    .size(th.font_size_body.value()),
            );
        });
        return;
    }
    let detecting = st.detecting.as_ref().map(|j| j.name.clone());
    let known: Vec<String> = passkeys.passkeys.iter().map(|k| k.name.clone()).collect();
    let mut action: Option<(usize, ProfileRowAction)> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, p) in profiles.profiles.iter().enumerate() {
            if let Some(a) = draw_profile_row(ui, th, p, detecting.as_deref(), &known) {
                action = Some((i, a));
            }
        }
    });
    if let Some((i, a)) = action {
        let p = &profiles.profiles[i];
        match a {
            ProfileRowAction::Edit => {
                st.pform = form_from_profile(p, passkeys);
                st.perr = None;
                st.profile_view = Sub::Form;
            }
            ProfileRowAction::Delete => {
                st.profile_view = Sub::ConfirmDelete(p.name.clone());
            }
            ProfileRowAction::Redetect => {
                if st.detecting.is_none() {
                    st.detecting = Some(spawn_detect(ui.ctx(), p.name.clone()));
                }
            }
        }
    }
}

enum ProfileRowAction {
    Edit,
    Delete,
    Redetect,
}

fn draw_profile_row(
    ui: &mut egui::Ui,
    th: &Theme,
    p: &RemoteProfile,
    detecting: Option<&str>,
    passkey_names: &[String],
) -> Option<ProfileRowAction> {
    let mut out = None;
    let ssh = p.as_ssh();
    let is_ssh = ssh.is_some();
    let disabled = ssh.as_ref().map(|v| v.is_disabled()).unwrap_or(false);
    let detecting_now = detecting == Some(p.name.as_str());
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            // row1: name + type badge
            ui.horizontal(|ui| {
                let title = match &p.label {
                    Some(l) if !l.is_empty() => format!("{}  ({})", p.name, l),
                    _ => p.name.clone(),
                };
                ui.label(
                    egui::RichText::new(title)
                        .color(if disabled { th.overlay0 } else { th.text })
                        .size(th.font_size_body.value())
                        .strong(),
                );
                if is_builtin_kind(&p.kind) || KNOWN_TYPES.contains(&p.kind.as_str()) {
                    ui.label(
                        egui::RichText::new(&p.kind).color(th.subtext0).size(th.font_size_caption.value()),
                    );
                } else {
                    warn_badge(ui, th, &p.kind, t("remote_tool.type_unknown_hint"));
                }
            });
            // row2: target summary
            ui.label(
                egui::RichText::new(profile_summary(p))
                    .color(th.subtext0)
                    .size(th.font_size_caption.value())
                    .monospace(),
            );
            // row3: passkey + (ssh) shell/state
            ui.horizontal(|ui| {
                match &p.passkey_ref {
                    Some(pr) if !pr.is_empty() => {
                        ui.label(
                            egui::RichText::new(format!("passkey: {pr}"))
                                .color(th.subtext0)
                                .size(th.font_size_caption.value()),
                        );
                        if !passkey_names.contains(pr) {
                            warn_badge(ui, th, t("remote_tool.passkey_missing"), t("remote_tool.passkey_missing_hint"));
                        }
                    }
                    _ => {
                        ui.label(
                            egui::RichText::new("passkey: —")
                                .color(th.subtext0)
                                .size(th.font_size_caption.value()),
                        );
                    }
                }
                if let Some(v) = &ssh {
                    ui.label(
                        egui::RichText::new(format!("shell: {}", v.shell()))
                            .color(th.subtext0)
                            .size(th.font_size_caption.value()),
                    );
                    if detecting_now {
                        ui.add(egui::Spinner::new().size(th.font_size_caption.value()));
                        ui.label(
                            egui::RichText::new(t("remote_tool.detecting"))
                                .color(th.subtext0)
                                .size(th.font_size_caption.value()),
                        );
                    } else if disabled {
                        ui.label(
                            egui::RichText::new(t("remote_tool.detect_failed"))
                                .color(th.accent_danger())
                                .size(th.font_size_caption.value()),
                        );
                    }
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            // 아이콘 버튼 (디자인 IconButton): delete / edit / re-detect.
            // right_to_left 이라 추가 순서 = 우→좌. 디자인 우측 끝이 trash.
            if ui
                .add(egui::ImageButton::new(icons::TRASH.image(15.0, th.subtext0.into())).frame(false))
                .on_hover_text(t("remote_tool.delete"))
                .clicked()
            {
                out = Some(ProfileRowAction::Delete);
            }
            if ui
                .add(egui::ImageButton::new(icons::EDIT.image(15.0, th.subtext0.into())).frame(false))
                .on_hover_text(t("remote_tool.edit"))
                .clicked()
            {
                out = Some(ProfileRowAction::Edit);
            }
            if is_ssh
                && ui
                    .add_enabled(
                        !detecting_now,
                        egui::ImageButton::new(icons::REFRESH.image(15.0, th.subtext0.into()))
                            .frame(false),
                    )
                    .on_hover_text(t("remote_tool.refresh_tooltip"))
                    .clicked()
            {
                out = Some(ProfileRowAction::Redetect);
            }
        });
    });
    hsep(ui, th);
    out
}

fn profile_summary(p: &RemoteProfile) -> String {
    if let Some(v) = p.as_ssh() {
        let mut s = v.ssh_destination();
        if let Some(port) = v.port()
            && port != 22
        {
            s = format!("{s}:{port}");
        }
        s
    } else {
        let parts: Vec<String> = p
            .fields
            .iter()
            .filter_map(|(k, val)| val.as_str().map(|v| format!("{k}={v}")))
            .take(2)
            .collect();
        if parts.is_empty() { "—".into() } else { parts.join("  ") }
    }
}

fn form_from_profile(p: &RemoteProfile, _passkeys: &Passkeys) -> ProfileForm {
    let mut f = ProfileForm {
        kind: p.kind.clone(),
        name: p.name.clone(),
        label: p.label.clone().unwrap_or_default(),
        passkey_ref: p.passkey_ref.clone().unwrap_or_default(),
        editing_original: Some(p.name.clone()),
        shell: "auto".into(),
        remote_tasty: "tasty".into(),
        ..Default::default()
    };
    if let Some(v) = p.as_ssh() {
        f.host = v.host().unwrap_or("").to_string();
        f.user = v.user().unwrap_or("").to_string();
        f.port = v.port().map(|n| n.to_string()).unwrap_or_default();
        f.remote_tasty = v.remote_tasty().to_string();
        f.shell = v.shell().to_string();
    } else {
        f.fields = p
            .fields
            .iter()
            .filter_map(|(k, val)| val.as_str().map(|v| (k.clone(), v.to_string())))
            .collect();
    }
    f
}

fn draw_profile_form(
    ui: &mut egui::Ui,
    th: &Theme,
    ctx: &egui::Context,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    passkeys: &Passkeys,
) {
    let editing = st.pform.editing_original.is_some();
    ui.label(
        egui::RichText::new(if editing {
            t("remote_tool.profile_form_edit")
        } else {
            t("remote_tool.profile_form_add")
        })
        .color(th.text)
        .size(th.font_size_body.value())
        .strong(),
    );
    ui.add_space(th.spacing_xs.value());

    let f = &mut st.pform;
    let is_ssh = f.kind.trim() == "ssh";
    let unknown = !f.kind.trim().is_empty()
        && !is_builtin_kind(f.kind.trim())
        && !KNOWN_TYPES.contains(&f.kind.trim());

    egui::ScrollArea::vertical().max_height(th.spacing_lg.value() * 14.0).show(ui, |ui| {
        // Type (열린 콤보 — 텍스트 + 제안 드롭다운)
        ui.horizontal(|ui| {
            field_label(ui, th, t("remote_tool.field_type"));
            ui.add(egui::TextEdit::singleline(&mut f.kind).desired_width(160.0));
            egui::ComboBox::from_id_salt("remote_tool.type_suggest")
                .selected_text("▾")
                .width(24.0)
                .show_ui(ui, |ui| {
                    for kt in KNOWN_TYPES {
                        ui.selectable_value(&mut f.kind, (*kt).to_string(), *kt);
                    }
                });
        });
        if unknown {
            ui.label(
                egui::RichText::new(t("remote_tool.type_unknown_hint"))
                    .color(th.yellow)
                    .size(th.font_size_caption.value()),
            );
        }
        ui.add_space(th.spacing_xs.value());

        if is_ssh {
            egui::Grid::new("remote_tool.ssh_grid").num_columns(2).show(ui, |ui| {
                text_row(ui, th, t("remote_tool.field_name"), &mut f.name);
                text_row(ui, th, t("remote_tool.field_host"), &mut f.host);
                text_row(ui, th, t("remote_tool.field_user"), &mut f.user);
                text_row(ui, th, t("remote_tool.field_port"), &mut f.port);
                text_row(ui, th, t("remote_tool.field_label"), &mut f.label);
                text_row(ui, th, t("remote_tool.field_remote_tasty"), &mut f.remote_tasty);
                field_label(ui, th, t("remote_tool.field_shell"));
                egui::ComboBox::from_id_salt("remote_tool.shell")
                    .selected_text(f.shell.clone())
                    .show_ui(ui, |ui| {
                        for sh in SHELLS {
                            ui.selectable_value(&mut f.shell, (*sh).to_string(), *sh);
                        }
                    });
                ui.end_row();
                passkey_dropdown_row(ui, th, &mut f.passkey_ref, passkeys);
            });
            if f.shell == "auto" {
                ui.label(
                    egui::RichText::new(t("remote_tool.shell_auto_hint"))
                        .color(th.subtext0)
                        .size(th.font_size_caption.value()),
                );
            }
        } else {
            egui::Grid::new("remote_tool.gen_grid").num_columns(2).show(ui, |ui| {
                text_row(ui, th, t("remote_tool.field_name"), &mut f.name);
                text_row(ui, th, t("remote_tool.field_label"), &mut f.label);
                passkey_dropdown_row(ui, th, &mut f.passkey_ref, passkeys);
            });
            ui.add_space(th.spacing_xs.value());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t("remote_tool.fields_section"))
                        .color(th.subtext0)
                        .size(th.font_size_caption.value()),
                );
                if ui.button(t("remote_tool.field_add")).clicked() {
                    f.fields.push((String::new(), String::new()));
                }
            });
            let mut remove_idx = None;
            for (i, (k, v)) in f.fields.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(k).desired_width(110.0).hint_text("key"));
                    ui.add(egui::TextEdit::singleline(v).desired_width(180.0).hint_text("value"));
                    if ui.button("×").clicked() {
                        remove_idx = Some(i);
                    }
                });
            }
            if let Some(i) = remove_idx {
                f.fields.remove(i);
            }
        }

        if let Some(err) = &st.perr {
            ui.add_space(th.spacing_xs.value());
            ui.label(
                egui::RichText::new(err).color(th.accent_danger()).size(th.font_size_caption.value()),
            );
        }
    });

    ui.add_space(th.spacing_sm.value());
    let mut do_save = false;
    let mut do_cancel = false;
    ui.horizontal(|ui| {
        if ui.button(t("remote_tool.save")).clicked() {
            do_save = true;
        }
        if ui.button(t("remote_tool.cancel")).clicked() {
            do_cancel = true;
        }
    });
    if do_cancel {
        st.perr = None;
        st.profile_view = Sub::List;
        return;
    }
    if do_save {
        match save_profile(ctx, st, profiles, passkeys) {
            Ok(()) => {
                st.perr = None;
                st.profile_view = Sub::List;
            }
            Err(e) => st.perr = Some(e),
        }
    }
}

fn passkey_dropdown_row(ui: &mut egui::Ui, th: &Theme, value: &mut String, passkeys: &Passkeys) {
    field_label(ui, th, t("remote_tool.field_passkey"));
    let sel = if value.is_empty() { t("remote_tool.passkey_none").to_string() } else { value.clone() };
    egui::ComboBox::from_id_salt("remote_tool.passkey_ref")
        .selected_text(sel)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, String::new(), t("remote_tool.passkey_none"));
            for k in &passkeys.passkeys {
                ui.selectable_value(value, k.name.clone(), &k.name);
            }
        });
    ui.end_row();
}

fn save_profile(
    ctx: &egui::Context,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    _passkeys: &Passkeys,
) -> Result<(), String> {
    let f = st.pform.clone();
    let kind = f.kind.trim();
    if kind.is_empty() {
        return Err(t("remote_tool.err_type_empty").to_string());
    }
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    let is_ssh = kind == "ssh";
    if is_ssh {
        if f.host.trim().is_empty() {
            return Err(t("remote_tool.err_host_empty").to_string());
        }
        if !f.port.trim().is_empty() && f.port.trim().parse::<u16>().is_err() {
            return Err(t("remote_tool.err_port_invalid").to_string());
        }
    }
    // 이름 중복(자기 자신 제외).
    if profiles
        .profiles
        .iter()
        .any(|p| p.name == name && Some(p.name.as_str()) != st.pform.editing_original.as_deref())
    {
        return Err(t("remote_tool.err_name_dup").to_string());
    }

    let mut p = RemoteProfile::new(name, kind);
    if !f.label.trim().is_empty() {
        p.label = Some(f.label.trim().to_string());
    }
    if !f.passkey_ref.is_empty() {
        p.passkey_ref = Some(f.passkey_ref.clone());
    }
    let mut needs_detect = false;
    if is_ssh {
        p.set_field("host", f.host.trim().to_string());
        if !f.user.trim().is_empty() {
            p.set_field("user", f.user.trim().to_string());
        }
        if !f.port.trim().is_empty() {
            p.set_field("port", f.port.trim().to_string());
        }
        let rt: String = if f.remote_tasty.trim().is_empty() {
            "tasty".to_string()
        } else {
            f.remote_tasty.trim().to_string()
        };
        p.set_field("remote_tasty", rt);
        let shell = if is_valid_shell(&f.shell) { f.shell.clone() } else { "auto".into() };
        p.set_field("shell", shell.clone());
        // 명시 셸 → port_mode 즉시 도출, auto → 저장 후 워커 감지.
        needs_detect = tasty_remote_profiles::shell_to_port_mode(&shell).is_none();
        if let Some(mode) = tasty_remote_profiles::shell_to_port_mode(&shell) {
            p.set_field("port_mode", mode);
        }
    } else {
        for (k, v) in &f.fields {
            if !k.trim().is_empty() {
                p.set_field(k.trim().to_string(), v.clone());
            }
        }
    }

    // rename: 원래 name 과 다르면 옛 항목 제거.
    if let Some(orig) = &st.pform.editing_original
        && orig != name
    {
        profiles.remove(orig);
    }
    profiles.upsert(p);
    profiles.save().map_err(|e| format!("save: {e}"))?;
    if needs_detect && st.detecting.is_none() {
        st.detecting = Some(spawn_detect(ctx, name.to_string()));
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// TAB B — Passkey
// ════════════════════════════════════════════════════════════════════════
fn draw_passkeys_tab(ui: &mut egui::Ui, th: &Theme, st: &mut UiState, passkeys: &Passkeys) {
    match st.passkey_view.clone() {
        Sub::List => draw_passkey_list(ui, th, st, passkeys),
        Sub::Form => draw_passkey_form(ui, th, st),
        Sub::ConfirmDelete(name) => {
            if let Some(act) = draw_confirm_delete(
                ui,
                th,
                t("remote_tool.noun_passkey"),
                &name,
                Some(t("remote_tool.passkey_delete_hint")),
            ) {
                if act {
                    let mut pk = Passkeys::load();
                    pk.remove(&name);
                    let _ = pk.save();
                }
                st.passkey_view = Sub::List;
            }
        }
    }
}

fn draw_passkey_list(ui: &mut egui::Ui, th: &Theme, st: &mut UiState, passkeys: &Passkeys) {
    if secondary_button(ui, th, t("remote_tool.passkey_add")).clicked() {
        st.kform = PasskeyForm { kind: "path".into(), ..Default::default() };
        st.kerr = None;
        st.passkey_view = Sub::Form;
        return;
    }
    ui.add_space(th.spacing_xs.value());
    if passkeys.passkeys.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(th.spacing_lg.value());
            ui.label(
                egui::RichText::new(t("remote_tool.passkey_empty"))
                    .color(th.subtext0)
                    .italics()
                    .size(th.font_size_body.value()),
            );
        });
        return;
    }
    let mut action: Option<(String, PasskeyRowAction)> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for k in &passkeys.passkeys {
            let revealed = st.revealed.contains(&k.name);
            if let Some(a) = draw_passkey_row(ui, th, k, revealed) {
                action = Some((k.name.clone(), a));
            }
        }
    });
    if let Some((name, a)) = action {
        match a {
            PasskeyRowAction::Reveal => {
                if st.revealed.contains(&name) {
                    st.revealed.remove(&name);
                } else {
                    st.revealed.insert(name);
                }
            }
            PasskeyRowAction::Edit => {
                if let Some(k) = passkeys.get(&name) {
                    st.kform = PasskeyForm {
                        name: k.name.clone(),
                        kind: k.kind.clone(),
                        value: reveal_value(k),
                        editing_original: Some(k.name.clone()),
                    };
                    st.kerr = None;
                    st.passkey_view = Sub::Form;
                }
            }
            PasskeyRowAction::Delete => {
                st.passkey_view = Sub::ConfirmDelete(name);
            }
        }
    }
}

enum PasskeyRowAction {
    Reveal,
    Edit,
    Delete,
}

fn draw_passkey_row(ui: &mut egui::Ui, th: &Theme, k: &Passkey, revealed: bool) -> Option<PasskeyRowAction> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&k.name).color(th.text).size(th.font_size_body.value()).strong(),
                );
                if KNOWN_PASSKEY_KINDS.contains(&k.kind.as_str()) {
                    ui.label(egui::RichText::new(&k.kind).color(th.subtext0).size(th.font_size_caption.value()));
                } else {
                    warn_badge(ui, th, &k.kind, t("remote_tool.kind_unknown_hint"));
                }
            });
            let val = if revealed { reveal_value(k) } else { "••••••••".into() };
            ui.label(
                egui::RichText::new(format!("{} · {}", k.kind, val))
                    .color(th.subtext0)
                    .size(th.font_size_caption.value())
                    .monospace(),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            // 아이콘 버튼 (디자인 IconButton): delete / edit / reveal(eye 토글).
            if ui
                .add(egui::ImageButton::new(icons::TRASH.image(15.0, th.subtext0.into())).frame(false))
                .on_hover_text(t("remote_tool.delete"))
                .clicked()
            {
                out = Some(PasskeyRowAction::Delete);
            }
            if ui
                .add(egui::ImageButton::new(icons::EDIT.image(15.0, th.subtext0.into())).frame(false))
                .on_hover_text(t("remote_tool.edit"))
                .clicked()
            {
                out = Some(PasskeyRowAction::Edit);
            }
            // revealed 면 eye-off + active(밝은) tint, 아니면 eye + muted.
            let (reveal_icon, reveal_tint) = if revealed {
                (icons::EYE_OFF, th.text)
            } else {
                (icons::EYE, th.subtext0)
            };
            if ui
                .add(egui::ImageButton::new(reveal_icon.image(15.0, reveal_tint.into())).frame(false))
                .on_hover_text(t("remote_tool.reveal_tooltip"))
                .clicked()
            {
                out = Some(PasskeyRowAction::Reveal);
            }
        });
    });
    hsep(ui, th);
    out
}

/// 로컬 GUI 전용 값 노출. path kind 는 경로, inline kind 는 관리 파일 내용을 읽는다.
fn reveal_value(k: &Passkey) -> String {
    if k.kind == "inline" {
        std::fs::read_to_string(&k.path).unwrap_or_else(|_| "(unreadable)".into())
    } else {
        k.path.clone()
    }
}

fn draw_passkey_form(ui: &mut egui::Ui, th: &Theme, st: &mut UiState) {
    let editing = st.kform.editing_original.is_some();
    ui.label(
        egui::RichText::new(if editing {
            t("remote_tool.passkey_form_edit")
        } else {
            t("remote_tool.passkey_form_add")
        })
        .color(th.text)
        .size(th.font_size_body.value())
        .strong(),
    );
    ui.add_space(th.spacing_xs.value());

    let f = &mut st.kform;
    egui::Grid::new("remote_tool.passkey_grid").num_columns(2).show(ui, |ui| {
        text_row(ui, th, t("remote_tool.field_name"), &mut f.name);
        field_label(ui, th, t("remote_tool.field_kind"));
        ui.horizontal(|ui| {
            for opt in KNOWN_PASSKEY_KINDS {
                ui.selectable_value(&mut f.kind, (*opt).to_string(), *opt);
            }
        });
        ui.end_row();
        field_label(ui, th, t("remote_tool.field_value"));
        if f.kind == "inline" {
            ui.add(egui::TextEdit::multiline(&mut f.value).desired_rows(3).hint_text(t("remote_tool.value_inline_hint")));
        } else {
            ui.add(egui::TextEdit::singleline(&mut f.value).desired_width(f32::INFINITY).hint_text("~/.ssh/id_ed25519"));
        }
        ui.end_row();
    });
    ui.label(
        egui::RichText::new(t("remote_tool.passkey_value_note"))
            .color(th.subtext0)
            .size(th.font_size_caption.value()),
    );
    if let Some(err) = &st.kerr {
        ui.add_space(th.spacing_xs.value());
        ui.label(egui::RichText::new(err).color(th.accent_danger()).size(th.font_size_caption.value()));
    }

    ui.add_space(th.spacing_sm.value());
    let mut do_save = false;
    let mut do_cancel = false;
    ui.horizontal(|ui| {
        if ui.button(t("remote_tool.save")).clicked() {
            do_save = true;
        }
        if ui.button(t("remote_tool.cancel")).clicked() {
            do_cancel = true;
        }
    });
    if do_cancel {
        st.kerr = None;
        st.passkey_view = Sub::List;
        return;
    }
    if do_save {
        match save_passkey(st) {
            Ok(()) => {
                st.kerr = None;
                st.passkey_view = Sub::List;
            }
            Err(e) => st.kerr = Some(e),
        }
    }
}

fn save_passkey(st: &mut UiState) -> Result<(), String> {
    let f = st.kform.clone();
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    if !is_valid_passkey_name(name) {
        return Err(t("remote_tool.err_name_format").to_string());
    }
    if f.value.trim().is_empty() {
        return Err(t("remote_tool.err_value_empty").to_string());
    }
    let mut pk = Passkeys::load();
    if pk.passkeys.iter().any(|k| k.name == name && Some(k.name.as_str()) != f.editing_original.as_deref()) {
        return Err(t("remote_tool.err_name_dup").to_string());
    }
    // rename: 옛 이름(+관리 파일) 제거.
    if let Some(orig) = &f.editing_original
        && orig != name
    {
        pk.remove(orig);
    }
    let res = if f.kind == "inline" {
        pk.upsert_inline(name, &f.value)
    } else {
        pk.upsert_path(name, f.value.trim().to_string())
    };
    res.map_err(|e| format!("{e}"))?;
    pk.save().map_err(|e| format!("save: {e}"))?;
    Ok(())
}

// ── 공통 ─────────────────────────────────────────────────────────────────
fn draw_confirm_delete(
    ui: &mut egui::Ui,
    th: &Theme,
    noun: &str,
    name: &str,
    hint: Option<&str>,
) -> Option<bool> {
    let mut out = None;
    ui.add_space(th.spacing_sm.value());
    ui.label(
        egui::RichText::new(format!("{}: \"{name}\"?", noun))
            .color(th.text)
            .size(th.font_size_body.value()),
    );
    if let Some(h) = hint {
        ui.label(egui::RichText::new(h).color(th.subtext0).size(th.font_size_caption.value()));
    }
    ui.add_space(th.spacing_sm.value());
    ui.horizontal(|ui| {
        if ui.button(egui::RichText::new(t("remote_tool.delete")).color(th.accent_danger())).clicked() {
            out = Some(true);
        }
        if ui.button(t("remote_tool.cancel")).clicked() {
            out = Some(false);
        }
    });
    out
}

fn text_row(ui: &mut egui::Ui, th: &Theme, label: &str, value: &mut String) {
    field_label(ui, th, label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    ui.end_row();
}

fn field_label(ui: &mut egui::Ui, th: &Theme, label: &str) {
    ui.label(egui::RichText::new(label).color(th.subtext0).size(th.font_size_body.value()));
}

// ── detect 워커 (ssh_tool 과 동일 패턴) ───────────────────────────────────
fn spawn_detect(ctx: &egui::Context, name: String) -> DetectJob {
    let slot: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let slot_w = Arc::clone(&slot);
    let ctx_w = ctx.clone();
    let name_w = name.clone();
    std::thread::spawn(move || {
        let res = tasty_cli::ssh::detect_and_persist(&name_w)
            .map(|m| m.as_str().to_string())
            .map_err(|e| e.to_string());
        if let Ok(mut g) = slot_w.lock() {
            *g = Some(res);
        }
        ctx_w.request_repaint();
    });
    DetectJob { name, slot }
}

fn poll_detect(st: &mut UiState) -> bool {
    let done = match &st.detecting {
        Some(job) => job.slot.lock().map(|g| g.is_some()).unwrap_or(true),
        None => false,
    };
    if done {
        st.detecting = None;
    }
    done
}
