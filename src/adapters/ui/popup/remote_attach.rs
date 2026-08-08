//! 원격 워크스페이스 추가 팝업 (사이드바 우클릭 > 원격 워크스페이스 추가).
//!
//! 좌 240px attach 프로필 리스트(single select) → 우 flex pane. 프로필을 고르면
//! 워커 스레드가 RA01 공유 코어(`tasty_cli::remote_browse`)로 원격 tasty 에 붙어
//! `workspace.list`+`attach.list` 를 병합 조회하고, 우측을 4상태(initial / connecting /
//! error / loaded[+empty])로 표시한다. 원격 ws 를 골라 **Connect** 하면 조회에 쓴 터널을
//! 재사용해 로컬 mirror workspace 로 attach 한다.
//!
//! 구조 전사: 디자인 `RemoteAttach`(680×460 headless 2-pane) 1:1. remote_tool 과 같은
//! shell 언어(headless 헤더 · bg-panel 프레임 · ghost/primary footer). 갤러리 specimen
//! (`tasty-gallery` `remote_attach`)의 정적 미러를 실 IPC 위에 얹은 것.
//!
//! 원칙 1: 이 팝업의 조회/attach 는 **사용자 입력 경로**다. Connect 확정 시 focus 가 새
//! mirror ws 로 이동하는데(사용자 동작), 그 focus 이동은 `pending_gui_attach_user` 큐를
//! 통해 이 경로에서만 일어난다(release IPC/에이전트 경로엔 없음). self(loopback) attach 는
//! release 에서 `dispatch_pending_gui_attach` 게이트가 차단한다(원칙 1②).

use std::sync::{Arc, Mutex};

use tasty_cli::remote_browse::{self, RemoteWorkspace};
use tasty_cli::ssh::SshTunnel;
use tasty_remote_profiles::RemoteProfiles;
use tasty_ui_widgets::{Button, ButtonVariant, StatusKind, status_dot};

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::core::CoreState;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;

pub const REMOTE_ATTACH_POPUP_ID: &str = "remote_attach";

const UI_MEMORY_ID: &str = "remote_attach.ui";

/// attach kind(같은 레지스트리의 예약 kind, ADR-0032). remote_tool Attach 탭이 편집하고
/// 이 팝업은 **소비만** 한다.
const ATTACH_KIND: &str = "tasty-attach";

// ── 레이아웃 고정 치수 (디자인 raw px — 화면 전용) ──
const LEFT_W: f32 = 240.0;
const HEADER_H: f32 = 47.0;
const FOOTER_H: f32 = 49.0;
const CAPS_H: f32 = 30.0;
const PROFILE_ROW_H: f32 = 50.0;
const WS_ROW_H: f32 = 34.0;
const BADGE_H: f32 = 16.0;
const HEADER_PAD_L: f32 = 14.0;
const SELECT_BAR_W: f32 = 2.0;

/// 워커가 채우는 browse 결과. 성공 시 터널을 살려 Connect 로 넘긴다.
struct BrowseOk {
    port: u16,
    tunnel: Option<SshTunnel>,
    workspaces: Vec<RemoteWorkspace>,
}

/// Connect 시 재사용할 접속 엔드포인트(성공 후 보관). 터널이 살아 있어야 mirror 세션이
/// 유지되므로 Arc<Mutex> 로 담아 UiState(Clone)에 싣는다.
struct ReadyConn {
    port: u16,
    tunnel: Option<SshTunnel>,
}

/// 진행 중 조회 워커. `slot` 은 완료 시 결과가 들어오는 폴링 슬롯.
#[derive(Clone)]
struct BrowseJob {
    slot: Arc<Mutex<Option<Result<BrowseOk, String>>>>,
}

/// 우측 pane 상태 머신.
#[derive(Clone, Default)]
enum Conn {
    #[default]
    Initial,
    Connecting,
    Error(String),
    Loaded(Vec<RemoteWorkspace>),
}

#[derive(Clone, Default)]
struct UiState {
    /// 선택된 attach 프로필명.
    attach_sel: Option<String>,
    conn: Conn,
    /// 선택된 원격 ws id.
    ws_sel: Option<u32>,
    /// 진행 중 조회 워커.
    job: Option<BrowseJob>,
    /// browse 성공 후 보관한 엔드포인트(Connect 재사용). Cancel/재선택 시 drop → ssh kill.
    ready: Option<Arc<Mutex<Option<ReadyConn>>>>,
}

fn read_ui(ctx: &egui::Context) -> UiState {
    ctx.memory(|m| {
        m.data
            .get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID))
            .unwrap_or_default()
    })
}
fn write_ui(ctx: &egui::Context, ui: UiState) {
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new(UI_MEMORY_ID), ui));
}
fn clear_ui(ctx: &egui::Context) {
    ctx.memory_mut(|m| m.data.remove::<UiState>(egui::Id::new(UI_MEMORY_ID)));
}

/// attach 프로필 1개의 표시용 요약(디자인 좌측 row) — remote_tool `draw_attach_row` 의
/// target/inactive 도출 로직 재사용.
struct ProfileSummary {
    name: String,
    label: String,
    target: String,
    inactive: bool,
}

fn attach_summaries(profiles: &RemoteProfiles) -> Vec<ProfileSummary> {
    profiles
        .profiles
        .iter()
        .filter(|p| p.kind == ATTACH_KIND)
        .filter_map(|p| {
            let v = p.as_attach()?;
            let (missing, inactive) = match v.ssh_ref() {
                Some(r) => {
                    let referenced = profiles.get(r).filter(|rp| rp.kind == "ssh");
                    let disabled = referenced
                        .and_then(|rp| rp.as_ssh())
                        .map(|s| s.is_disabled())
                        .unwrap_or(false);
                    (referenced.is_none(), disabled)
                }
                None => (false, v.detect_failed()),
            };
            let target = match v.ssh_ref() {
                Some(r) => format!("→ {}", if r.is_empty() { "?" } else { r }),
                None => {
                    let mut s = v.ssh_destination();
                    if s.is_empty() {
                        s = "?".into();
                    }
                    if let Some(port) = v.port()
                        && port != 22
                    {
                        s = format!("{s}:{port}");
                    }
                    s
                }
            };
            Some(ProfileSummary {
                name: p.name.clone(),
                label: p.label.clone().unwrap_or_default(),
                target,
                // dangling ref(missing) 도 연결하면 실패하므로 inactive 로 표시.
                inactive: inactive || missing,
            })
        })
        .collect()
}

/// 프로필명 → 원격 ws 목록/에러(+ 재사용 터널)를 워커 스레드로 조회한다. UI 스레드에서
/// SSH/IPC 를 직접 블록하지 않는다(원칙 2 headless — 코어는 CLI/IPC 와 공유).
fn spawn_browse(ctx: &egui::Context, profile: String) -> BrowseJob {
    let slot: Arc<Mutex<Option<Result<BrowseOk, String>>>> = Arc::new(Mutex::new(None));
    let slot_w = Arc::clone(&slot);
    let ctx_w = ctx.clone();
    let name = profile;
    std::thread::spawn(move || {
        let res = (|| -> Result<BrowseOk, String> {
            let (target, remote_tasty, port_mode, port_file) =
                remote_browse::resolve_connection_spec(Some(&name), None, "", "")
                    .map_err(|e| e.to_string())?;
            let (tunnel, port) = remote_browse::resolve_endpoint(
                &target,
                &remote_tasty,
                &port_mode,
                port_file.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            let workspaces = remote_browse::browse_via_port(port).map_err(|e| e.to_string())?;
            Ok(BrowseOk {
                port,
                tunnel,
                workspaces,
            })
        })();
        if let Ok(mut g) = slot_w.lock() {
            *g = Some(res);
        }
        ctx_w.request_repaint();
    });
    BrowseJob { slot }
}

/// 워커 완료 폴링 — 완료 시 job → conn(+ready) 전이. 재렌더당 1회.
fn poll_browse(st: &mut UiState) {
    let done = match &st.job {
        Some(job) => job.slot.lock().map(|g| g.is_some()).unwrap_or(true),
        None => false,
    };
    if !done {
        return;
    }
    let Some(job) = st.job.take() else { return };
    let outcome = job.slot.lock().ok().and_then(|mut g| g.take());
    match outcome {
        Some(Ok(ok)) => {
            st.conn = Conn::Loaded(ok.workspaces.clone());
            st.ws_sel = None;
            st.ready = Some(Arc::new(Mutex::new(Some(ReadyConn {
                port: ok.port,
                tunnel: ok.tunnel,
            }))));
        }
        Some(Err(e)) => {
            st.conn = Conn::Error(e);
            st.ready = None;
        }
        None => {
            // 슬롯이 비었는데 done — 이론상 도달 안 함. 안전하게 에러로.
            st.conn = Conn::Error(t("remote_attach.error_generic").to_string());
        }
    }
}

/// 프로필 선택 → 조회 시작(상태 리셋 + 워커 spawn).
fn connect(ctx: &egui::Context, st: &mut UiState, name: String) {
    st.attach_sel = Some(name.clone());
    st.ws_sel = None;
    st.ready = None; // 이전 터널 정리(재선택 = 새 연결).
    st.conn = Conn::Connecting;
    st.job = Some(spawn_browse(ctx, name));
}

/// PopupDef::on_close 진입점 — 어떤 경로로 닫히든 `UiState`(진행 중 조회 워커 +
/// 재사용 터널)를 drop 한다. draw_fn 자신의 Escape/Cancel/Connect 경로는 이미
/// `clear_ui`를 부르지만, 지금까지는 headless(X 버튼 없음) + close_on_outside_click
/// =false 라 그 경로들 밖에서 닫히는 일이 우연히 없었을 뿐이다 —
/// `UiIntent::ClosePopup`(디버그 IPC 포함)으로는 지금도 그 경로를 우회할 수 있어
/// ssh 연결이 살아남을 수 있었다(불변식이 구조가 아니라 우연에 의존).
pub fn on_close_remote_attach_popup(
    ctx: &egui::Context,
    _state: &mut AppState,
    _engine: &mut CoreState,
) {
    clear_ui(ctx);
}

/// PopupDef.draw_fn 진입점.
pub fn draw_remote_attach_popup(
    ui: &mut egui::Ui,
    _state: &mut AppState,
    engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();
    let mut st = read_ui(&ctx);

    // 리스트 항목은 텍스트가 아니라 목록으로 다뤄야 한다 — 라벨을 비선택으로 만들어
    // 글자 위 I-beam/드래그 선택을 막는다(egui 기본 selectable_labels=true). 이 팝업의
    // 모든 child ui 는 이 root ui 에서 파생되어 스타일을 상속한다.
    ui.style_mut().interaction.selectable_labels = false;

    poll_browse(&mut st);

    // Escape: 닫기(진행 중 조회/터널은 clear_ui 로 UiState drop 되며 정리).
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        clear_ui(&ctx);
        return PopupAction::Close;
    }

    let profiles = RemoteProfiles::load();
    let summaries = attach_summaries(&profiles);

    let full = ui.max_rect();
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let mut close = false;
    let mut do_connect = false;

    // ── 헤더 ──
    let header_rect = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), HEADER_H));
    if draw_header(ui, &th, header_rect) {
        close = true;
    }

    // ── footer ──
    let footer_rect = egui::Rect::from_min_size(
        egui::pos2(full.left(), full.bottom() - FOOTER_H),
        egui::vec2(full.width(), FOOTER_H),
    );

    // ── body(2-pane) ──
    let body_rect = egui::Rect::from_min_max(
        egui::pos2(full.left(), header_rect.bottom()),
        egui::pos2(full.right(), footer_rect.top()),
    );
    let left_rect =
        egui::Rect::from_min_size(body_rect.min, egui::vec2(LEFT_W, body_rect.height()));
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(body_rect.left() + LEFT_W, body_rect.top()),
        body_rect.max,
    );
    // 좌 pane 배경(bg-sidebar) + borderRight separator.
    ui.painter().rect_filled(left_rect, 0.0, th.bg_sidebar());
    ui.painter().vline(
        left_rect.right(),
        left_rect.y_range(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );

    // 좌: attach 프로필 리스트.
    if let Some(name) = draw_left_pane(ui, &th, left_rect, &summaries, st.attach_sel.as_deref()) {
        connect(&ctx, &mut st, name);
    }
    // 우: 4상태. error 상태의 Retry 클릭 시 선택 프로필로 재조회.
    let retry = draw_right_pane(ui, &th, right_rect, &mut st);
    if retry && let Some(name) = st.attach_sel.clone() {
        connect(&ctx, &mut st, name);
    }

    // footer(Connect 활성 조건 = loaded && 선택 ws 존재).
    let can_connect = matches!(&st.conn, Conn::Loaded(ws) if !ws.is_empty()) && st.ws_sel.is_some();
    match draw_footer(ui, &th, footer_rect, can_connect) {
        FooterAction::Cancel => close = true,
        FooterAction::Connect => do_connect = true,
        FooterAction::None => {}
    }

    // Connect 실행 — 재사용 터널 + 선택 ws 를 사용자-경로 큐에 넣는다(메인 루프 drain).
    if do_connect
        && can_connect
        && let Some(ws) = st.ws_sel
        && let Some(ready_arc) = st.ready.take()
    {
        let ready = ready_arc.lock().ok().and_then(|mut g| g.take());
        if let Some(ReadyConn { port, tunnel }) = ready {
            // focus 이동은 큐 drain(dispatch_pending_gui_attach) 이 담당(원칙 1 —
            // 사용자 입력 경로에서만 새 mirror ws 로 focus 이동).
            engine
                .pending_gui_attach_user
                .push(crate::core::GuiAttachUserReq {
                    port,
                    workspace: ws,
                    tunnel,
                });
        }
        close = true;
    }

    if close {
        clear_ui(&ctx);
        PopupAction::Close
    } else {
        write_ui(&ctx, st);
        PopupAction::None
    }
}

// ════════════════════════════════════════════════════════════════════════
// 헤더
// ════════════════════════════════════════════════════════════════════════
fn draw_header(ui: &mut egui::Ui, th: &Theme, rect: egui::Rect) -> bool {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + HEADER_PAD_L, rect.top()),
        egui::pos2(rect.right() - th.spacing_sm.value(), rect.bottom()),
    );
    let mut close = false;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    child.add(icons::TERMINAL_PROMPT.image(16.0, th.text_muted().into()));
    child.label(
        egui::RichText::new(t("remote_attach.heading"))
            .color(th.text_primary())
            .size(th.font_size_heading.value())
            .strong(),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add(
                egui::ImageButton::new(icons::CLOSE.image(16.0, th.text_muted().into()))
                    .frame(false),
            )
            .on_hover_text(t("remote_attach.close"))
            .clicked()
        {
            close = true;
        }
    });
    close
}

// ════════════════════════════════════════════════════════════════════════
// 좌: attach 프로필 리스트
// ════════════════════════════════════════════════════════════════════════
fn draw_left_pane(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    profiles: &[ProfileSummary],
    selected: Option<&str>,
) -> Option<String> {
    let mut clicked: Option<String> = None;
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.set_clip_rect(rect);
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    // caps 헤더.
    caps_header(&mut col, th, t("remote_attach.attach_profiles"), None);
    let list_rect =
        egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + CAPS_H), rect.max);
    let mut list = col.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list.set_clip_rect(list_rect);
    egui::ScrollArea::vertical()
        .id_salt("remote_attach.profiles")
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            if profiles.is_empty() {
                ui.add_space(th.spacing_md.value());
                let mut inner = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(
                            egui::pos2(
                                ui.max_rect().left() + th.spacing_md.value(),
                                ui.cursor().top(),
                            ),
                            egui::vec2(LEFT_W - th.spacing_md.value() * 2.0, 40.0),
                        ))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                inner.label(
                    egui::RichText::new(t("remote_attach.no_profiles"))
                        .color(th.text_muted())
                        .italics()
                        .size(th.font_size_caption.value()),
                );
                return;
            }
            for p in profiles {
                if profile_row(ui, th, p, Some(p.name.as_str()) == selected) {
                    clicked = Some(p.name.clone());
                }
            }
        });
    clicked
}

fn profile_row(ui: &mut egui::Ui, th: &Theme, p: &ProfileSummary, selected: bool) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, PROFILE_ROW_H), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 0.0, th.surface_active());
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(SELECT_BAR_W, rect.height()));
        ui.painter().rect_filled(bar, 0.0, th.accent_primary());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + th.spacing_md.value(),
            rect.top() + th.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - th.spacing_md.value(),
            rect.bottom() - th.spacing_sm.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // 클립은 가로만(truncate 오버플로 가드) — 세로는 행 전체 높이로 둔다. inner 는
    // 상하 spacing_sm 를 깎아 두 줄(name+target)을 담기엔 낮아, inner 로 세로까지
    // 클립하면 둘째 줄(target) 하단 descender 가 잘린다.
    child.set_clip_rect(egui::Rect::from_min_max(
        egui::pos2(inner.left(), rect.top()),
        egui::pos2(inner.right(), rect.bottom()),
    ));
    child.spacing_mut().item_spacing.y = 2.0;
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let name_c = if selected {
            th.text_primary()
        } else {
            th.text_secondary()
        };
        ui.add(
            egui::Label::new(
                egui::RichText::new(&p.name)
                    .size(th.font_size_body.value())
                    .strong()
                    .color(name_c),
            )
            .truncate(),
        );
        if !p.label.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("({})", p.label))
                        .size(th.font_size_body.value())
                        .color(th.text_muted()),
                )
                .truncate(),
            );
        }
        if p.inactive {
            badge(
                ui,
                th,
                t("remote_attach.inactive"),
                th.accent_warning().into(),
                0.12,
                0.40,
                true,
            );
        }
    });
    child.label(
        egui::RichText::new(&p.target)
            .monospace()
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    resp.clicked()
}

// ════════════════════════════════════════════════════════════════════════
// 우: 4상태
// ════════════════════════════════════════════════════════════════════════
/// 우측 pane 렌더. 반환값 = error 상태의 **Retry 버튼 클릭** 여부(caller 가 재조회).
fn draw_right_pane(ui: &mut egui::Ui, th: &Theme, rect: egui::Rect, st: &mut UiState) -> bool {
    let sel_name = st.attach_sel.clone().unwrap_or_default();
    match &st.conn {
        Conn::Initial => {
            center_state(
                ui,
                th,
                rect,
                CenterKind::Glyph(icons::TERMINAL_PROMPT, th.text_placeholder().into()),
                t("remote_attach.select_profile"),
                th.text_muted(),
                t("remote_attach.select_profile_hint"),
                false,
            );
            false
        }
        Conn::Connecting => {
            center_state(
                ui,
                th,
                rect,
                CenterKind::Spinner,
                t("remote_attach.connecting"),
                th.text_secondary(),
                &t("remote_attach.connecting_hint").replace("{name}", &sel_name),
                false,
            );
            false
        }
        Conn::Error(msg) => {
            let msg = msg.clone();
            center_state(
                ui,
                th,
                rect,
                CenterKind::Glyph(icons::ALERT_TRIANGLE, th.accent_danger().into()),
                t("remote_attach.cant_connect"),
                th.text_primary(),
                &msg,
                true,
            )
        }
        Conn::Loaded(ws) => {
            if ws.is_empty() {
                center_state(
                    ui,
                    th,
                    rect,
                    CenterKind::Glyph(icons::FOLDER, th.text_placeholder().into()),
                    t("remote_attach.no_workspaces"),
                    th.text_muted(),
                    &t("remote_attach.no_workspaces_hint").replace("{name}", &sel_name),
                    false,
                );
            } else {
                let ws = ws.clone();
                if let Some(sel) = draw_ws_list(ui, th, rect, &ws, &sel_name, st.ws_sel) {
                    st.ws_sel = Some(sel);
                }
            }
            false
        }
    }
}

fn draw_ws_list(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    ws: &[RemoteWorkspace],
    profile_name: &str,
    ws_sel: Option<u32>,
) -> Option<u32> {
    let mut clicked = None;
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.set_clip_rect(rect);
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    caps_header(
        &mut col,
        th,
        t("remote_attach.remote_workspaces"),
        Some(profile_name),
    );
    let list_rect =
        egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + CAPS_H), rect.max);
    let mut list = col.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list.set_clip_rect(list_rect);
    egui::ScrollArea::vertical()
        .id_salt("remote_attach.workspaces")
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            for w in ws {
                if ws_row(ui, th, w, ws_sel == Some(w.id)) {
                    clicked = Some(w.id);
                }
            }
        });
    clicked
}

fn ws_row(ui: &mut egui::Ui, th: &Theme, w: &RemoteWorkspace, selected: bool) -> bool {
    let width = ui.available_width();
    let disabled = w.attached;
    let sense = if disabled {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, WS_ROW_H), sense);
    if selected {
        ui.painter().rect_filled(rect, 0.0, th.surface_active());
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(SELECT_BAR_W, rect.height()));
        ui.painter().rect_filled(bar, 0.0, th.accent_primary());
    } else if !disabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + th.spacing_md.value(), rect.top()),
        egui::pos2(rect.right() - th.spacing_md.value(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(inner);
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    let kind = if w.busy_count > 0 {
        StatusKind::Running
    } else {
        StatusKind::Idle
    };
    status_dot(&mut child, th, kind, "", w.busy_count > 0, false);
    let name_c = if disabled {
        th.text_disabled()
    } else if selected {
        th.text_primary()
    } else {
        th.text_secondary()
    };
    child.add(
        egui::Label::new(
            egui::RichText::new(&w.name)
                .size(th.font_size_body.value())
                .color(name_c),
        )
        .truncate(),
    );
    child.add(icons::SPLIT.image(th.font_size_caption.value(), th.text_muted().into()));
    child.label(
        egui::RichText::new(w.pane_count.to_string())
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if disabled {
            badge(
                ui,
                th,
                t("remote_attach.in_use"),
                th.border_attached().into(),
                0.14,
                0.45,
                false,
            );
        } else if w.busy_count > 0 {
            ui.label(
                egui::RichText::new(t("remote_attach.busy"))
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
        }
    });
    resp.clicked()
}

// ════════════════════════════════════════════════════════════════════════
// center-state (initial / connecting / error / empty 공용)
// ════════════════════════════════════════════════════════════════════════
enum CenterKind {
    Glyph(icons::Icon, egui::Color32),
    Spinner,
}

#[allow(clippy::too_many_arguments)]
fn center_state(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    kind: CenterKind,
    heading: &str,
    heading_color: tasty_type_appearance::color::HexColor,
    caption: &str,
    retry: bool,
) -> bool {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(th.spacing_lg.value(), th.spacing_xl.value())))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    col.set_clip_rect(rect);
    col.add_space((rect.height() - 130.0).max(0.0) * 0.5);
    col.spacing_mut().item_spacing.y = th.spacing_sm.value();
    match kind {
        CenterKind::Glyph(g, c) => {
            col.add(g.image(22.0, c));
        }
        CenterKind::Spinner => {
            tasty_ui_widgets::Spinner::new()
                .size(22.0)
                .show(&mut col, th);
        }
    }
    col.label(
        egui::RichText::new(heading)
            .size(th.font_size_body.value())
            .strong()
            .color(heading_color),
    );
    col.label(
        egui::RichText::new(caption)
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    if retry {
        col.add_space(th.spacing_xs.value());
        // Retry — 선택된 프로필로 재조회한다(caller 가 bool 을 받아 connect 재실행).
        return Button::new(t("remote_attach.retry"))
            .variant(ButtonVariant::Secondary)
            .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
            .show(&mut col, th)
            .clicked();
    }
    false
}

// ════════════════════════════════════════════════════════════════════════
// footer
// ════════════════════════════════════════════════════════════════════════
enum FooterAction {
    None,
    Cancel,
    Connect,
}

fn draw_footer(ui: &mut egui::Ui, th: &Theme, rect: egui::Rect, can_connect: bool) -> FooterAction {
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + th.spacing_lg.value(), rect.top()),
        egui::pos2(rect.right() - th.spacing_lg.value(), rect.bottom()),
    );
    let mut action = FooterAction::None;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    if Button::new(t("remote_attach.connect"))
        .variant(ButtonVariant::Primary)
        .enabled(can_connect)
        .show(&mut child, th)
        .clicked()
    {
        action = FooterAction::Connect;
    }
    if Button::new(t("remote_attach.cancel"))
        .variant(ButtonVariant::Ghost)
        .show(&mut child, th)
        .clicked()
    {
        action = FooterAction::Cancel;
    }
    action
}

// ════════════════════════════════════════════════════════════════════════
// 공용 헬퍼
// ════════════════════════════════════════════════════════════════════════
/// caps 헤더 — mono micro uppercase muted. `suffix` 있으면 "· {suffix}" 를 붙인다.
fn caps_header(ui: &mut egui::Ui, th: &Theme, label: &str, suffix: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CAPS_H),
        egui::Sense::hover(),
    );
    let base_x = rect.left() + th.spacing_md.value();
    let y = rect.top() + th.spacing_md.value();
    let galley = ui.painter().layout_no_wrap(
        label.to_uppercase(),
        egui::FontId::monospace(th.font_size_micro.value()),
        th.text_muted().into(),
    );
    let w = galley.rect.width();
    ui.painter()
        .galley(egui::pos2(base_x, y), galley, th.text_muted().into());
    if let Some(s) = suffix {
        ui.painter().text(
            egui::pos2(base_x + w + th.spacing_sm.value(), y),
            egui::Align2::LEFT_TOP,
            format!("· {s}"),
            egui::FontId::proportional(th.font_size_caption.value()),
            th.text_muted().into(),
        );
    }
}

/// 원격/inactive pill — fill/border alpha 는 디자인 color-mix(% transparent) 근사.
fn badge(
    ui: &mut egui::Ui,
    th: &Theme,
    text: &str,
    color: egui::Color32,
    fill_a: f32,
    border_a: f32,
    warn_icon: bool,
) {
    let font = egui::FontId::monospace(th.font_size_micro.value());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER);
    let pad_x = th.spacing_sm.value();
    let icon_w = if warn_icon { 12.0 + 4.0 } else { 0.0 };
    let w = pad_x * 2.0 + icon_w + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, BADGE_H), egui::Sense::hover());
    let radius = th.corner_radius_sm.value();
    ui.painter()
        .rect_filled(rect, radius, color.gamma_multiply(fill_a));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(th.border_width.value(), color.gamma_multiply(border_a)),
        egui::StrokeKind::Inside,
    );
    let mut tx = rect.left() + pad_x;
    if warn_icon {
        let ir = egui::Rect::from_min_size(
            egui::pos2(tx, rect.center().y - 6.0),
            egui::vec2(12.0, 12.0),
        );
        icons::ALERT_TRIANGLE.image(12.0, color).paint_at(ui, ir);
        tx += 12.0 + 4.0;
    }
    ui.painter().galley(
        egui::pos2(tx, rect.center().y - galley.rect.height() * 0.5),
        galley,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `UiState`(진행 중 조회 워커 + 재사용 터널을 쥐고 있는 egui temp memory)가
    /// 훅 호출 한 번으로 drop 되는지 확인 — draw_fn 을 거치지 않는 닫힘 경로(바깥
    /// 클릭/`UiIntent::ClosePopup`/디버그 IPC)에서도 이 훅이 정리를 담당한다.
    #[test]
    fn on_close_clears_ui_state() {
        let ctx = egui::Context::default();
        write_ui(
            &ctx,
            UiState {
                attach_sel: Some("prod".to_string()),
                ..Default::default()
            },
        );
        assert!(
            ctx.memory(|m| m.data.get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID)))
                .is_some()
        );

        let (mut state, mut engine) = crate::state::tests::test_state();
        on_close_remote_attach_popup(&ctx, &mut state, &mut engine);

        assert!(
            ctx.memory(|m| m.data.get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID)))
                .is_none()
        );
    }
}
