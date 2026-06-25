//! Layout — 1depth (Plugins 창 idiom).
//!
//! 디자인 `ui_kits/terminal/overlays/plugins_window.jsx` 의 전면 미러:
//! - 820×540 모달(screen-specific 고정값, token-policy §c 화면전용 verbatim).
//! - 48px 헤더: 플러그인 마크 + 제목 + 세그먼트 3탭(Installed | Attention | Add).
//! - 288px 좌측 리스트(`--tasty-plugins-list-width`) + 우측 디테일 + 하단 액션바.
//! - Attention 탭 4케이스(unknown-key / signature-invalid / permissions-changed /
//!   health-error) — `changelog/2026-06-23-plugins.md` spec.
//! - Add 탭: 로컬 폴더 install + 신뢰(trust) 흐름(매니페스트 프리뷰 + 미신뢰 배너).
//!
//! 갤러리는 본체 binary 에 의존할 수 없어 view 로직을 로컬 미러한다(demo=main).
//! 색·폰트·치수는 모두 `Theme` 토큰. 화면전용 고정 px 는 문서화된 const.
//! 아이콘은 공용 글리프 헬퍼(`catalog::components::glyph`) 재사용.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, Input, kbd, switch, tag, TagVariant};

use crate::catalog::components::glyph;

// ── 화면전용 고정값 (token-policy §c: NOT tokenized, reproduce verbatim) ──
/// 모달 폭/높이 — 디자인 `width:820 height:540`.
const MODAL_W: f32 = 820.0;
const MODAL_H: f32 = 540.0;
/// 헤더 높이 — 디자인 `height:48`.
const HEADER_H: f32 = 48.0;
/// 좌측 리스트 폭 — 디자인 `--tasty-plugins-list-width`(288).
const LIST_W: f32 = 288.0;
/// 헤더 검색 Input 폭 — 디자인 `--tasty-field-width-lg`(200).
const SEARCH_W: f32 = 200.0;
/// 세그먼트 버튼 높이 — 디자인 `height:26`.
const SEG_H: f32 = 26.0;
/// 헤더/배너/액션바 좌우 패딩 — 디자인 `--tasty-size-14`(14). Theme 대응 토큰 없음.
const PAD_14: f32 = 14.0;
/// Add 폼 콘텐츠 상하 패딩 — 디자인 `--tasty-size-22`(22). Theme 대응 토큰 없음.
const PAD_22: f32 = 22.0;
/// 리스트 행 아바타 변.
const AVATAR_LIST: f32 = 32.0;
/// 디테일 식별 아바타 변.
const AVATAR_DETAIL: f32 = 46.0;
/// Add 매니페스트 프리뷰 아바타 변 — 디자인 `width:42`.
const AVATAR_MANIFEST: f32 = 42.0;
/// 헤더 제목/세그 구분 vline 높이 — 디자인 `height:20`.
const DIVIDER_H: f32 = 20.0;
/// 상태 점 지름 — 디자인 `--tasty-status-dot-size`(8).
const STATUS_DOT: f32 = 8.0;
/// 선택 행 좌측 강조 inset 폭 — 디자인 `inset var(--tasty-size-2) 0 0`(2).
const SEL_INSET: f32 = 2.0;
/// 디테일 본문 measure(`--tasty-measure-lg` 460) — 설명/노트 최대폭.
const MEASURE_LG: f32 = 460.0;
/// Add 폼 measure(`--tasty-measure-xl` 560) — path picker/매니페스트 최대폭.
const MEASURE_XL: f32 = 560.0;
/// 디테일 Command 행 최소 높이 — 디자인 `--tasty-settings-row-min-height`(32).
const ROW_MIN_H: f32 = 32.0;

// ── 상태 ────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Installed,
    Attention,
    Add,
}

struct State {
    tab: Tab,
    installed_sel: usize,
    attention_sel: usize,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            tab: Tab::Installed,
            installed_sel: 0,
            attention_sel: 0,
        })
    };
}

// ── 데이터 모델 (디자인 PLUGIN_LIST / ATTENTION_LIST 미러) ────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cat {
    SourceControl,
    Ai,
    DevOps,
    Cloud,
}

impl Cat {
    fn label(self) -> &'static str {
        match self {
            Cat::SourceControl => "Source control",
            Cat::Ai => "AI",
            Cat::DevOps => "DevOps",
            Cat::Cloud => "Cloud",
        }
    }

    /// 디자인 CAT_COLOR 매핑(mock 데이터가 쓰는 카테고리만).
    fn color(self, theme: &Theme) -> egui::Color32 {
        match self {
            Cat::SourceControl => egui::Color32::from(theme.accent_primary()),
            Cat::Ai | Cat::Cloud => egui::Color32::from(theme.accent_agent()),
            Cat::DevOps => egui::Color32::from(theme.accent_success()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Agent,
    Idle,
    Error,
}

struct Plugin {
    name: &'static str,
    author: &'static str,
    version: &'static str,
    cat: Cat,
    status: Status,
    agent: bool,
    perms: &'static [&'static str],
    cmd: &'static str,
    key: &'static str,
    desc: &'static str,
}

impl Plugin {
    /// enabled = installed(전부 true) && status != idle (디자인 enabled 초기값).
    fn enabled(&self) -> bool {
        self.status != Status::Idle
    }
}

/// 디자인 PLUGIN_LIST 중 installed=true 4건(리스트는 INSTALLED-ONLY).
const INSTALLED: &[Plugin] = &[
    Plugin {
        name: "git-helper",
        author: "tasty-labs",
        version: "1.4.2",
        cat: Cat::SourceControl,
        status: Status::Running,
        agent: false,
        perms: &["fs:read", "clipboard", "ipc:git-helper.*"],
        cmd: "git-helper: open panel",
        key: "Ctrl+Alt+G",
        desc: "Inline git status, blame, and one-key staging inside any terminal surface. \
               Adds a gutter ribbon and a compact branch switcher to the tab strip.",
    },
    Plugin {
        name: "ai-review",
        author: "tasty-labs",
        version: "0.9.0",
        cat: Cat::Ai,
        status: Status::Agent,
        agent: true,
        perms: &["fs:read", "fs:write", "net", "ipc:ai-review.*"],
        cmd: "ai-review: review staged",
        key: "Ctrl+Alt+R",
        desc: "Agent-driven review of staged diffs. Streams suggested patches into a side \
               surface and lets you apply them hunk by hunk.",
    },
    Plugin {
        name: "docker",
        author: "community",
        version: "2.1.0",
        cat: Cat::DevOps,
        status: Status::Idle,
        agent: false,
        perms: &["ipc:docker.*", "net"],
        cmd: "docker: containers",
        key: "Ctrl+Alt+D",
        desc: "A surface for container logs, exec sessions, and compose control. Attaches to \
               the local Docker socket and mirrors `docker ps` live.",
    },
    Plugin {
        name: "k8s-lens",
        author: "community",
        version: "0.6.3",
        cat: Cat::DevOps,
        status: Status::Error,
        agent: false,
        perms: &["net", "fs:read", "ipc:k8s-lens.*"],
        cmd: "k8s-lens: switch context",
        key: "Ctrl+Alt+K",
        desc: "Kubernetes context switcher and pod log streamer. Currently failing to reach \
               the configured cluster — check kubeconfig.",
    },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sev {
    Danger,
    Warning,
}

impl Sev {
    fn color(self, theme: &Theme) -> egui::Color32 {
        match self {
            Sev::Danger => egui::Color32::from(theme.accent_danger()),
            Sev::Warning => egui::Color32::from(theme.accent_warning()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reason {
    UnknownKey,
    SignatureInvalid,
    PermissionsChanged,
    HealthError,
}

impl Reason {
    fn sev(self) -> Sev {
        match self {
            Reason::UnknownKey | Reason::SignatureInvalid => Sev::Danger,
            Reason::PermissionsChanged | Reason::HealthError => Sev::Warning,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Reason::UnknownKey => "Signature not trusted",
            Reason::SignatureInvalid => "Signature invalid",
            Reason::PermissionsChanged => "Permissions changed",
            Reason::HealthError => "Runtime error",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Reason::UnknownKey => {
                "Signed by a key that isn't in your trust store — registration rejected."
            }
            Reason::SignatureInvalid => {
                "Signature missing or failed verification — registration rejected."
            }
            Reason::PermissionsChanged => {
                "Manifest permissions changed since you trusted it — re-approval required."
            }
            Reason::HealthError => "Enabled, but failing at runtime.",
        }
    }

    /// 하단 상태 pill 텍스트.
    fn pill_text(self) -> &'static str {
        match self.sev() {
            Sev::Danger => "Not registered",
            Sev::Warning => "Needs review",
        }
    }
}

enum AttnDetail {
    /// signature 케이스 — fingerprint(옵션) + note(옵션).
    Signature {
        fingerprint: Option<&'static str>,
        note: Option<&'static str>,
    },
    /// permissions-changed — 추가/제거 권한 diff.
    Perms {
        added: &'static [&'static str],
        removed: &'static [&'static str],
    },
    /// health-error — 런타임 에러 텍스트.
    Error(&'static str),
}

struct Attn {
    name: &'static str,
    author: &'static str,
    version: &'static str,
    cat: Cat,
    reason: Reason,
    builtin: bool,
    desc: &'static str,
    detail: AttnDetail,
}

/// Attention = 거부/미등록(서명/신뢰) + enabled-but-failing(health). 디자인의
/// ATTENTION_LIST(3) + 설치목록서 파생되는 health-error(k8s-lens) = 4케이스.
fn attention_items() -> Vec<Attn> {
    vec![
        Attn {
            name: "fleet-sync",
            author: "tasty-labs",
            version: "1.2.0",
            cat: Cat::DevOps,
            reason: Reason::UnknownKey,
            builtin: true,
            desc: "Bundled cluster fleet sync. Its publisher key was rotated and the \
                   signature no longer matches a trusted key.",
            detail: AttnDetail::Signature {
                fingerprint: Some("a13c 4e7f 2b08 9d51  ·  ed25519"),
                note: Some(
                    "The built-in publisher key rotated this release; the bundled signature \
                     was made with a key not yet in your trust store. Update Tasty or import \
                     the new key to restore it.",
                ),
            },
        },
        Attn {
            name: "secrets-vault",
            author: "community",
            version: "0.8.4",
            cat: Cat::Cloud,
            reason: Reason::PermissionsChanged,
            builtin: false,
            desc: "Reads and injects secrets into surfaces. You trusted v0.7 — v0.8.4 \
                   requests a different permission set.",
            detail: AttnDetail::Perms {
                added: &["fs:write", "net"],
                removed: &["clipboard"],
            },
        },
        Attn {
            name: "remote-shell",
            author: "community",
            version: "1.0.0",
            cat: Cat::DevOps,
            reason: Reason::SignatureInvalid,
            builtin: false,
            desc: "Opens remote SSH surfaces. The bundle's signature file is missing or corrupt.",
            detail: AttnDetail::Signature {
                fingerprint: None,
                note: Some(
                    "tasty-plugin.sig is absent or does not match the manifest hash. \
                     Re-download the plugin from its source.",
                ),
            },
        },
        Attn {
            name: "k8s-lens",
            author: "community",
            version: "0.6.3",
            cat: Cat::DevOps,
            reason: Reason::HealthError,
            builtin: false,
            desc: "Kubernetes context switcher and pod log streamer. Currently failing to \
                   reach the configured cluster — check kubeconfig.",
            detail: AttnDetail::Error(
                "Failed to reach the configured cluster at https://10.0.4.11:6443\n\
                 kubeconfig context \"prod\" — connection refused",
            ),
        },
    ]
}

// ── 색 보간 헬퍼 (CSS color-mix 미러) ────────────────────────────────────
/// `color-mix(in srgb, a {t*100}%, b)` — srgb 바이트 선형 보간(둘 다 불투명).
// color-mix 재현 — theme 색 채널 계산 결과로 Color32 재구성, 리터럴 아님.
#[allow(clippy::disallowed_methods)]
fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let lerp = |x: u8, y: u8| (x as f32 * t + y as f32 * (1.0 - t)).round() as u8;
    egui::Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

/// `color-mix(in srgb, c {a*100}%, transparent)` — c 를 alpha a 로(unmultiplied).
// color-mix 재현 — theme 색 채널 계산 결과로 Color32 재구성, 리터럴 아님.
#[allow(clippy::disallowed_methods)]
fn alpha(c: egui::Color32, a: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0).round() as u8)
}

/// `kbd` 위젯 렌더 폭(우측 정렬용). chip.rs 내부 상수를 미러: 키캡 min 16 /
/// pad-x 4 / 키캡·"+" 간 gap 3, 폰트 micro 모노.
fn kbd_width(ui: &egui::Ui, theme: &Theme, keys: &str) -> f32 {
    const PAD_X: f32 = 4.0;
    const GAP: f32 = 3.0;
    const MIN_W: f32 = 16.0;
    let micro = theme.font_size_micro.value();
    let measure = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), egui::FontId::monospace(micro), egui::Color32::PLACEHOLDER)
            .rect
            .width()
    };
    let plus_w = measure("+");
    let mut w = 0.0;
    for (i, key) in keys.split('+').enumerate() {
        if i > 0 {
            w += GAP + plus_w + GAP;
        }
        w += (measure(key) + 2.0 * PAD_X).max(MIN_W);
    }
    w
}

// ── 진입점 ───────────────────────────────────────────────────────────────
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    crate::catalog::specimen::caption(
        ui,
        theme,
        "ui_kits/terminal/overlays/plugins_window.jsx — 820×540 모달, 48px 헤더, \
         세그먼트 3탭(Installed | Attention | Add), 288px 리스트 + 디테일 + 액션바.",
    );
    ui.add_space(theme.spacing_md.value());

    let (modal_rect, _) =
        ui.allocate_exact_size(egui::vec2(MODAL_W, MODAL_H), egui::Sense::hover());
    draw_modal(ui, theme, modal_rect);
}

fn draw_modal(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    // 모달 chrome — bg-panel + border-strong.
    ui.painter()
        .rect_filled(rect, radius, egui::Color32::from(theme.bg_panel()));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, egui::Color32::from(theme.border_strong())),
        egui::StrokeKind::Inside,
    );

    let header_rect =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), HEADER_H));
    let body_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.min.y + HEADER_H),
        rect.max,
    );

    draw_header(ui, theme, header_rect);

    let tab = STATE.with(|s| s.borrow().tab);
    match tab {
        Tab::Installed => draw_installed(ui, theme, body_rect),
        Tab::Attention => draw_attention(ui, theme, body_rect),
        Tab::Add => draw_add(ui, theme, body_rect),
    }
}

// ── 헤더 ──────────────────────────────────────────────────────────────────
fn draw_header(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let bw = theme.border_width.value();
    // 헤더 bg-sidebar (모달 좌상단만 둥근 모서리를 따르도록 전체 rect 채움은 모달이
    // 이미 처리; 여기선 헤더 영역만 sidebar 색으로 덮는다).
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius {
            nw: theme.corner_radius.value() as u8,
            ne: theme.corner_radius.value() as u8,
            sw: 0,
            se: 0,
        },
        egui::Color32::from(theme.bg_sidebar()),
    );
    // border-bottom separator.
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - bw,
        egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
    );

    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + PAD_14, rect.min.y),
        egui::pos2(rect.max.x - PAD_14, rect.max.y),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut child;
    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();

    // 플러그인 마크.
    ui.add(glyph::PLUG.image(theme.icon_glyph_size_md.value(), egui::Color32::from(theme.brand_melon_flesh())));
    // 제목.
    ui.label(
        egui::RichText::new("Plugins")
            .size(theme.font_size_max.value())
            .strong()
            .color(egui::Color32::from(theme.text_primary())),
    );
    // 구분 vline.
    let (dv_rect, _) =
        ui.allocate_exact_size(egui::vec2(1.0, DIVIDER_H), egui::Sense::hover());
    ui.painter().vline(
        dv_rect.center().x,
        egui::Rangef::new(dv_rect.center().y - DIVIDER_H * 0.5, dv_rect.center().y + DIVIDER_H * 0.5),
        egui::Stroke::new(1.0, egui::Color32::from(theme.separator)),
    );
    // 세그먼트 3탭.
    draw_segments(ui, theme);

    // 우측 정렬 — close + (installed 탭일 때) 검색.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let _close = IconButton::new().show(ui, theme, &|ui, rect, c| {
            glyph::CLOSE.image(rect.height(), c).paint_at(ui, rect)
        });
        if STATE.with(|s| s.borrow().tab) == Tab::Installed {
            let mut buf = String::new();
            Input::new()
                .placeholder("Filter installed…")
                .width(SEARCH_W)
                .icon(&|ui, rect, c| glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect))
                .show(ui, theme, &mut buf);
        }
    });
}

fn draw_segments(ui: &mut egui::Ui, theme: &Theme) {
    let cur = STATE.with(|s| s.borrow().tab);
    let installed_count = INSTALLED.len();
    let attention_count = attention_items().len();

    // (탭, 라벨, count, danger-badge 여부).
    let segs: [(Tab, &str, Option<usize>, bool); 3] = [
        (Tab::Installed, "Installed", Some(installed_count), false),
        (Tab::Attention, "Attention", Some(attention_count), true),
        (Tab::Add, "Add plugin", None, false),
    ];

    let pad_md = theme.spacing_md.value();
    let gap_sm = theme.spacing_sm.value();
    let micro = theme.font_size_micro.value();
    let term_sm = theme.font_size_term_sm.value();
    let inner_pad = 2.0; // 세그 컨테이너 padding:2
    let seg_gap = 2.0; // 세그 간 gap:2

    // 각 세그 폭 선계산.
    let mut widths = [0.0f32; 3];
    for (i, (_, label, count, danger)) in segs.iter().enumerate() {
        let lg = ui.painter().layout_no_wrap(
            (*label).to_owned(),
            egui::FontId::proportional(term_sm),
            egui::Color32::PLACEHOLDER,
        );
        let mut w = 2.0 * pad_md + lg.rect.width();
        match count {
            Some(c) if *danger => {
                if *c > 0 {
                    w += gap_sm + badge_width(ui, micro, *c);
                }
            }
            Some(c) => {
                let cg = ui.painter().layout_no_wrap(
                    c.to_string(),
                    egui::FontId::monospace(micro),
                    egui::Color32::PLACEHOLDER,
                );
                w += gap_sm + cg.rect.width();
            }
            None => {}
        }
        widths[i] = w;
    }

    let container_w = widths.iter().sum::<f32>() + 2.0 * seg_gap + 2.0 * inner_pad;
    let container_h = SEG_H + 2.0 * inner_pad;
    let (cont_rect, _) =
        ui.allocate_exact_size(egui::vec2(container_w, container_h), egui::Sense::hover());
    ui.painter().rect_filled(
        cont_rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.surface_active()),
    );

    let mut x = cont_rect.left() + inner_pad;
    let top = cont_rect.top() + inner_pad;
    for (i, (tab, label, count, danger)) in segs.iter().enumerate() {
        let seg_rect =
            egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(widths[i], SEG_H));
        let resp = ui.interact(
            seg_rect,
            ui.id().with(("plugins_seg", i)),
            egui::Sense::click(),
        );
        let on = *tab == cur;
        if on {
            ui.painter().rect(
                seg_rect,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_raised()),
                egui::Stroke::new(
                    theme.border_width.value(),
                    egui::Color32::from(theme.border_default()),
                ),
                egui::StrokeKind::Inside,
            );
        }
        // 콘텐츠: 라벨 + count.
        let fg = if on {
            egui::Color32::from(theme.text_primary())
        } else {
            egui::Color32::from(theme.text_muted())
        };
        let lg = ui.painter().layout_no_wrap(
            (*label).to_owned(),
            egui::FontId::proportional(term_sm),
            fg,
        );
        let mut cx = seg_rect.left() + pad_md;
        let label_pos = egui::pos2(cx, seg_rect.center().y - lg.rect.height() * 0.5);
        cx += lg.rect.width();
        ui.painter().galley(label_pos, lg, fg);
        match count {
            Some(c) if *danger => {
                if *c > 0 {
                    cx += gap_sm;
                    let bw = badge_width(ui, micro, *c);
                    let badge_rect = egui::Rect::from_min_size(
                        egui::pos2(cx, seg_rect.center().y - 8.0),
                        egui::vec2(bw, 16.0),
                    );
                    draw_count_badge(ui, theme, badge_rect, *c, micro);
                }
            }
            Some(c) => {
                cx += gap_sm;
                let col = if on {
                    egui::Color32::from(theme.text_secondary())
                } else {
                    egui::Color32::from(theme.text_muted())
                };
                let cg = ui.painter().layout_no_wrap(
                    c.to_string(),
                    egui::FontId::monospace(micro),
                    col,
                );
                let cpos = egui::pos2(cx, seg_rect.center().y - cg.rect.height() * 0.5);
                ui.painter().galley(cpos, cg, col);
            }
            None => {}
        }

        if resp.clicked() {
            let t = *tab;
            STATE.with(|s| s.borrow_mut().tab = t);
        }
        x += widths[i] + seg_gap;
    }
}

/// danger Badge 폭 = max(16, 텍스트 + 2*pad-xs).
fn badge_width(ui: &egui::Ui, micro: f32, count: usize) -> f32 {
    let g = ui.painter().layout_no_wrap(
        count.to_string(),
        egui::FontId::monospace(micro),
        egui::Color32::PLACEHOLDER,
    );
    (g.rect.width() + 2.0 * 4.0).max(16.0)
}

fn draw_count_badge(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, count: usize, micro: f32) {
    ui.painter().rect_filled(
        rect,
        rect.height() * 0.5,
        egui::Color32::from(theme.accent_danger()),
    );
    let fg = egui::Color32::from(theme.text_on_accent());
    let g = ui.painter()
        .layout_no_wrap(count.to_string(), egui::FontId::monospace(micro), fg);
    let pos = rect.center() - g.rect.size() * 0.5;
    ui.painter().galley(pos, g, fg);
}

// ── 아바타 ─────────────────────────────────────────────────────────────────
#[allow(clippy::too_many_arguments)]
fn draw_avatar(
    ui: &egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    letter: char,
    accent: egui::Color32,
    base: egui::Color32,
    fill_t: f32,
    border_a: f32,
    font: f32,
) {
    ui.painter()
        .rect_filled(rect, theme.corner_radius.value(), mix(accent, base, fill_t));
    ui.painter().rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), alpha(accent, border_a)),
        egui::StrokeKind::Inside,
    );
    let g = ui.painter().layout_no_wrap(
        letter.to_uppercase().to_string(),
        egui::FontId::monospace(font),
        accent,
    );
    let pos = rect.center() - g.rect.size() * 0.5;
    ui.painter().galley(pos, g, accent);
}

/// 카테고리 아바타(리스트/디테일) — bg 18% / border 38%, font≈변*0.42.
fn cat_avatar(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, name: &str, cat: Cat) {
    draw_avatar(
        ui,
        theme,
        rect,
        name.chars().next().unwrap_or('?'),
        cat.color(theme),
        egui::Color32::from(theme.surface_raised()),
        0.18,
        0.38,
        (rect.width() * 0.42).round(),
    );
}

// ── 좌측 리스트 공통 chrome ───────────────────────────────────────────────
/// 리스트 패널 bg-sidebar + border-right 를 칠하고, padding 8 적용한 child Ui 반환용
/// 콜백을 실행한다.
fn with_list_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    salt: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let bw = theme.border_width.value();
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
    ui.painter().vline(
        rect.right() - bw,
        rect.y_range(),
        egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
    );
    let pad = theme.spacing_sm.value();
    let inner = rect.shrink(pad);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.set_width(inner.width());
            body(ui);
        });
}

/// 리스트 행 — 아바타 + 이름 + 보조줄 + (선택 시 좌측 inset 강조). 클릭 응답 반환.
// specimen 빌더라 행 구성 인자(이름/카테고리/보조줄/색/트레일링/선택/액센트)가 많음 — 정당.
#[allow(clippy::too_many_arguments)]
fn list_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    cat: Cat,
    sub: &str,
    sub_color: egui::Color32,
    trailing: Option<&str>,
    selected: bool,
    accent: egui::Color32,
) -> egui::Response {
    let pad = theme.spacing_sm.value();
    let row_h = AVATAR_LIST + 2.0 * pad;
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());

    if selected {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius.value(),
            egui::Color32::from(theme.surface_active()),
        );
        // 좌측 inset 강조 바.
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(SEL_INSET, rect.height()));
        ui.painter()
            .rect_filled(bar, 0.0, accent);
    }

    let av_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad, rect.center().y - AVATAR_LIST * 0.5),
        egui::vec2(AVATAR_LIST, AVATAR_LIST),
    );
    cat_avatar(ui, theme, av_rect, name, cat);

    let text_x = av_rect.right() + pad;
    let name_col = if selected {
        egui::Color32::from(theme.text_primary())
    } else {
        egui::Color32::from(theme.text_secondary())
    };
    let name_g = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        name_col,
    );
    let sub_g = ui.painter().layout_no_wrap(
        sub.to_owned(),
        egui::FontId::monospace(theme.font_size_micro.value()),
        sub_color,
    );
    let name_h = name_g.rect.height();
    let block_h = name_h + 2.0 + sub_g.rect.height();
    let ty = rect.center().y - block_h * 0.5;
    ui.painter()
        .galley(egui::pos2(text_x, ty), name_g, name_col);
    ui.painter()
        .galley(egui::pos2(text_x, ty + name_h + 2.0), sub_g, sub_color);

    if let Some(t) = trailing {
        let tg = ui.painter().layout_no_wrap(
            t.to_owned(),
            egui::FontId::monospace(theme.font_size_micro.value()),
            egui::Color32::from(theme.text_muted()),
        );
        let pos = egui::pos2(
            rect.right() - pad - tg.rect.width(),
            rect.center().y - tg.rect.height() * 0.5,
        );
        ui.painter()
            .galley(pos, tg, egui::Color32::from(theme.text_muted()));
    }

    resp
}

// ── 디테일 패널 공통(콘텐츠 스크롤 + 하단 액션바) ─────────────────────────
/// 디테일 영역을 콘텐츠(스크롤) + 하단 액션바로 분할. `content` 는 padding-lg child,
/// `action_bar` 는 좌우 PAD_14 / 상하 md, 상단 border child.
fn with_detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    salt: &str,
    content: impl FnOnce(&mut egui::Ui),
    action_bar: impl FnOnce(&mut egui::Ui),
) {
    let md = theme.spacing_md.value();
    let bw = theme.border_width.value();
    let bar_h = theme.item_height_interactive.value() + 2.0 * md;

    let content_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.max.x, rect.max.y - bar_h),
    );
    let bar_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.max.y - bar_h),
        rect.max,
    );

    // 콘텐츠 (padding lg).
    let lg = theme.spacing_lg.value();
    let inner = content_rect.shrink(lg);
    let mut cchild = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .show(&mut cchild, |ui| {
            ui.set_width(inner.width());
            ui.spacing_mut().item_spacing.y = lg;
            content(ui);
        });

    // 액션바 border-top.
    ui.painter().hline(
        bar_rect.x_range(),
        bar_rect.top(),
        egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
    );
    let bar_inner = egui::Rect::from_min_max(
        egui::pos2(bar_rect.min.x + PAD_14, bar_rect.min.y + md),
        egui::pos2(bar_rect.max.x - PAD_14, bar_rect.max.y - md),
    );
    let mut bchild = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bchild.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    action_bar(&mut bchild);
}

/// 디테일 식별 헤더(아바타 46 + 이름 max + 태그들 + author·cat 보조줄).
fn detail_identity(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    cat: Cat,
    version: &str,
    extra_tag: Option<(&str, TagVariant)>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        let (av_rect, _) =
            ui.allocate_exact_size(egui::vec2(AVATAR_DETAIL, AVATAR_DETAIL), egui::Sense::hover());
        cat_avatar(ui, theme, av_rect, name, cat);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                ui.label(
                    egui::RichText::new(name)
                        .size(theme.font_size_max.value())
                        .strong()
                        .color(egui::Color32::from(theme.text_primary())),
                );
                tag(ui, theme, &format!("v{version}"), TagVariant::Default, false);
                if let Some((label, variant)) = extra_tag {
                    tag(ui, theme, label, variant, false);
                }
            });
            ui.add_space(theme.spacing_xs.value());
            ui.label(
                egui::RichText::new(format!("{} · {}", author_of(name), cat.label()))
                    .size(theme.font_size_caption.value())
                    .monospace()
                    .color(egui::Color32::from(theme.text_muted())),
            );
        });
    });
}

/// 이름 → author 역참조(설치/Attention 공통 식별 보조줄 단순화용). 미상은 "—".
fn author_of(name: &str) -> &'static str {
    if let Some(p) = INSTALLED.iter().find(|p| p.name == name) {
        return p.author;
    }
    for a in attention_items() {
        if a.name == name {
            return a.author;
        }
    }
    "—"
}

/// Mono 라벨(대문자 micro 모노 muted) — 디자인 `Mono` 컴포넌트.
fn mono_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(theme.font_size_micro.value())
            .monospace()
            .color(egui::Color32::from(theme.text_muted())),
    );
}

/// 본문 단락(body, secondary, measure-lg 폭).
fn body_paragraph(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(MEASURE_LG);
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_body.value())
                .color(egui::Color32::from(theme.text_secondary())),
        );
    });
}

// ── Installed 탭 ──────────────────────────────────────────────────────────
fn draw_installed(ui: &mut egui::Ui, theme: &Theme, body: egui::Rect) {
    let list_rect = egui::Rect::from_min_size(body.min, egui::vec2(LIST_W, body.height()));
    let detail_rect = egui::Rect::from_min_max(
        egui::pos2(body.min.x + LIST_W, body.min.y),
        body.max,
    );

    let sel = STATE.with(|s| s.borrow().installed_sel);
    let accent = egui::Color32::from(theme.accent_primary());

    with_list_panel(ui, theme, list_rect, "plugins_installed_list", |ui| {
        for (idx, p) in INSTALLED.iter().enumerate() {
            let sub = format!("{} · v{}", p.author, p.version);
            // installed 행 trailing: enabled=false 면 "off".
            let trailing = if p.enabled() { None } else { Some("off") };
            if list_row(
                ui,
                theme,
                p.name,
                p.cat,
                &sub,
                egui::Color32::from(theme.text_muted()),
                trailing,
                idx == sel,
                accent,
            )
            .clicked()
            {
                STATE.with(|s| s.borrow_mut().installed_sel = idx);
            }
        }
    });

    let p = &INSTALLED[sel.min(INSTALLED.len() - 1)];
    with_detail(
        ui,
        theme,
        detail_rect,
        "plugins_installed_detail",
        |ui| {
            let extra = if p.agent {
                Some(("agent", TagVariant::Agent))
            } else {
                None
            };
            detail_identity(ui, theme, p.name, p.cat, p.version, extra);
            body_paragraph(ui, theme, p.desc);

            // error 배너(설치+활성+error).
            if p.status == Status::Error {
                error_banner(
                    ui,
                    theme,
                    "Failed to connect. Check the plugin's configuration in Settings.",
                );
            }

            // Permissions.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                mono_label(ui, theme, "Permissions");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(
                        theme.spacing_sm.value(),
                        theme.spacing_sm.value(),
                    );
                    for perm in p.perms {
                        tag(ui, theme, perm, TagVariant::Default, false);
                    }
                });
            });

            // Command 행.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                mono_label(ui, theme, "Command");
                let (row_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), ROW_MIN_H),
                    egui::Sense::hover(),
                );
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(row_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                row.label(
                    egui::RichText::new(p.cmd)
                        .size(theme.font_size_term_sm.value())
                        .monospace()
                        .color(egui::Color32::from(theme.text_secondary())),
                );
                // Kbd 우측 정렬 — RTL 레이아웃 안에서는 `kbd` 내부 `ui.horizontal` 이
                // 부모 방향을 상속해 키캡 순서가 뒤집힌다. 폭을 직접 재서 spacer 로
                // 밀고 LTR 그대로 그린다.
                if !p.key.is_empty() {
                    let kw = kbd_width(&row, theme, p.key);
                    let pad = (row.available_width() - kw).max(0.0);
                    row.add_space(pad);
                    kbd(&mut row, theme, p.key);
                }
                // borderBottom separator.
                ui.painter().hline(
                    row_rect.x_range(),
                    row_rect.bottom(),
                    egui::Stroke::new(
                        theme.border_width.value(),
                        egui::Color32::from(theme.separator),
                    ),
                );
            });
        },
        |ui| {
            // Switch enabled/disabled.
            let mut on = p.enabled();
            let on_label = if on { "Enabled" } else { "Disabled" };
            switch(ui, theme, &mut on, Some(on_label), true);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let _uninstall = Button::new("Uninstall")
                    .variant(ButtonVariant::Secondary)
                    .show(ui, theme);
                let _configure = Button::new("Configure")
                    .variant(ButtonVariant::Ghost)
                    .leading_icon(&|ui, rect, c| {
                        glyph::SETTINGS.image(rect.height(), c).paint_at(ui, rect)
                    })
                    .show(ui, theme);
            });
        },
    );
}

/// danger 톤 인라인 배너(아이콘 circle + 메시지).
fn error_banner(ui: &mut egui::Ui, theme: &Theme, msg: &str) {
    let danger = egui::Color32::from(theme.accent_danger());
    egui::Frame::default()
        .fill(alpha(danger, 0.12))
        .stroke(egui::Stroke::new(theme.border_width.value(), alpha(danger, 0.35)))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                ui.add(glyph::ALERT_CIRCLE.image(theme.icon_glyph_size_md.value(), danger));
                ui.label(
                    egui::RichText::new(msg)
                        .size(theme.font_size_term_sm.value())
                        .color(danger),
                );
            });
        });
}

// ── Attention 탭 ──────────────────────────────────────────────────────────
fn draw_attention(ui: &mut egui::Ui, theme: &Theme, body: egui::Rect) {
    let items = attention_items();
    let list_rect = egui::Rect::from_min_size(body.min, egui::vec2(LIST_W, body.height()));
    let detail_rect = egui::Rect::from_min_max(
        egui::pos2(body.min.x + LIST_W, body.min.y),
        body.max,
    );

    let sel = STATE.with(|s| s.borrow().attention_sel).min(items.len() - 1);

    with_list_panel(ui, theme, list_rect, "plugins_attention_list", |ui| {
        for (idx, a) in items.iter().enumerate() {
            let sev_col = a.reason.sev().color(theme);
            if list_row(
                ui,
                theme,
                a.name,
                a.cat,
                a.reason.label(),
                sev_col,
                None,
                idx == sel,
                sev_col,
            )
            .clicked()
            {
                STATE.with(|s| s.borrow_mut().attention_sel = idx);
            }
        }
    });

    let a = &items[sel];
    let sev_col = a.reason.sev().color(theme);
    with_detail(
        ui,
        theme,
        detail_rect,
        "plugins_attention_detail",
        |ui| {
            let extra = if a.builtin {
                Some(("built-in", TagVariant::Default))
            } else {
                None
            };
            detail_identity(ui, theme, a.name, a.cat, a.version, extra);
            body_paragraph(ui, theme, a.desc);
            reason_banner(ui, theme, a.reason, sev_col);
            reason_detail(ui, theme, &a.detail);
        },
        |ui| {
            // 상태 pill (dot + 텍스트).
            let (dot_rect, _) = ui.allocate_exact_size(
                egui::vec2(STATUS_DOT, STATUS_DOT),
                egui::Sense::hover(),
            );
            ui.painter()
                .circle_filled(dot_rect.center(), STATUS_DOT * 0.5, sev_col);
            ui.label(
                egui::RichText::new(a.reason.pill_text())
                    .size(theme.font_size_term_sm.value())
                    .color(sev_col),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match a.reason {
                    Reason::PermissionsChanged => {
                        let _b = Button::new("Re-approve")
                            .variant(ButtonVariant::Primary)
                            .show(ui, theme);
                    }
                    Reason::HealthError => {
                        let _b = Button::new("Configure")
                            .variant(ButtonVariant::Ghost)
                            .leading_icon(&|ui, rect, c| {
                                glyph::SETTINGS.image(rect.height(), c).paint_at(ui, rect)
                            })
                            .show(ui, theme);
                    }
                    Reason::UnknownKey | Reason::SignatureInvalid => {
                        let _b = Button::new("Details")
                            .variant(ButtonVariant::Secondary)
                            .show(ui, theme);
                    }
                }
            });
        },
    );
}

/// reason 배너 — 아이콘(danger=circle / warning=triangle) + 라벨 + blurb.
fn reason_banner(ui: &mut egui::Ui, theme: &Theme, reason: Reason, sev: egui::Color32) {
    egui::Frame::default()
        .fill(alpha(sev, 0.11))
        .stroke(egui::Stroke::new(theme.border_width.value(), alpha(sev, 0.36)))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin {
            left: PAD_14 as i8,
            right: PAD_14 as i8,
            top: theme.spacing_md.value() as i8,
            bottom: theme.spacing_md.value() as i8,
        })
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    let g = if reason.sev() == Sev::Danger {
                        glyph::ALERT_CIRCLE
                    } else {
                        glyph::ALERT_TRIANGLE
                    };
                    ui.add(g.image(theme.icon_glyph_size_md.value(), sev));
                    ui.label(
                        egui::RichText::new(reason.label())
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(sev),
                    );
                });
                ui.label(
                    egui::RichText::new(reason.blurb())
                        .size(theme.font_size_term_sm.value())
                        .color(egui::Color32::from(theme.text_secondary())),
                );
            });
        });
}

/// reason-specific 디테일 블록.
fn reason_detail(ui: &mut egui::Ui, theme: &Theme, detail: &AttnDetail) {
    match detail {
        AttnDetail::Signature { fingerprint, note } => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                mono_label(ui, theme, "Signature");
                if let Some(fp) = fingerprint {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        ui.label(
                            egui::RichText::new("fingerprint")
                                .size(theme.font_size_caption.value())
                                .monospace()
                                .color(egui::Color32::from(theme.text_secondary())),
                        );
                        ui.label(
                            egui::RichText::new(*fp)
                                .size(theme.font_size_caption.value())
                                .monospace()
                                .color(egui::Color32::from(theme.text_muted())),
                        );
                    });
                }
                if let Some(n) = note {
                    ui.scope(|ui| {
                        ui.set_max_width(MEASURE_LG);
                        ui.label(
                            egui::RichText::new(*n)
                                .size(theme.font_size_term_sm.value())
                                .color(egui::Color32::from(theme.text_muted())),
                        );
                    });
                }
            });
        }
        AttnDetail::Perms { added, removed } => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                mono_label(ui, theme, "Permission changes");
                for p in *added {
                    perm_diff_row(ui, theme, "+", p, "newly requested", true);
                }
                for p in *removed {
                    perm_diff_row(ui, theme, "−", p, "no longer used", false);
                }
            });
        }
        AttnDetail::Error(err) => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                mono_label(ui, theme, "Error");
                let danger = egui::Color32::from(theme.accent_danger());
                egui::Frame::default()
                    .fill(egui::Color32::from(theme.bg_app()))
                    .stroke(egui::Stroke::new(
                        theme.border_width.value(),
                        egui::Color32::from(theme.separator),
                    ))
                    .corner_radius(theme.corner_radius.value())
                    .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(*err)
                                .size(theme.font_size_term_sm.value())
                                .monospace()
                                .color(danger),
                        );
                    });
            });
        }
    }
}

/// 권한 diff 한 줄 — `+`(success) / `−`(muted, 취소선) + 권한명 + 부연.
fn perm_diff_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    sign: &str,
    perm: &str,
    note: &str,
    added: bool,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        let sign_col = if added {
            egui::Color32::from(theme.accent_success())
        } else {
            egui::Color32::from(theme.text_muted())
        };
        ui.label(
            egui::RichText::new(sign)
                .size(theme.font_size_term_sm.value())
                .strong()
                .monospace()
                .color(sign_col),
        );
        let perm_text = egui::RichText::new(perm)
            .size(theme.font_size_term_sm.value())
            .monospace()
            .color(if added {
                egui::Color32::from(theme.text_secondary())
            } else {
                egui::Color32::from(theme.text_muted())
            });
        let perm_text = if added {
            perm_text
        } else {
            perm_text.strikethrough()
        };
        ui.label(perm_text);
        ui.label(
            egui::RichText::new(note)
                .size(theme.font_size_caption.value())
                .color(egui::Color32::from(theme.text_muted())),
        );
    });
}

// ── Add 탭 (로컬 폴더 install + trust 흐름) ────────────────────────────────
fn draw_add(ui: &mut egui::Ui, theme: &Theme, body: egui::Rect) {
    let md = theme.spacing_md.value();
    let bw = theme.border_width.value();
    let bar_h = theme.item_height_interactive.value() + 2.0 * md;

    let content_rect = egui::Rect::from_min_max(
        body.min,
        egui::pos2(body.max.x, body.max.y - bar_h),
    );
    let bar_rect = egui::Rect::from_min_max(
        egui::pos2(body.min.x, body.max.y - bar_h),
        body.max,
    );

    // 콘텐츠 (padding 22 상하 / xl 좌우).
    let xl = theme.spacing_xl.value();
    let inner = egui::Rect::from_min_max(
        egui::pos2(content_rect.min.x + xl, content_rect.min.y + PAD_22),
        egui::pos2(content_rect.max.x - xl, content_rect.max.y - PAD_22),
    );
    let mut cchild = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("plugins_add_form")
        .auto_shrink([false, false])
        .show(&mut cchild, |ui| {
            ui.set_width(inner.width());
            ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
            add_path_picker(ui, theme);
            add_manifest_preview(ui, theme);
        });

    // 액션바.
    ui.painter().hline(
        bar_rect.x_range(),
        bar_rect.top(),
        egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
    );
    let bar_inner = egui::Rect::from_min_max(
        egui::pos2(bar_rect.min.x + PAD_14, bar_rect.min.y + md),
        egui::pos2(bar_rect.max.x - PAD_14, bar_rect.max.y - md),
    );
    let mut bchild = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bchild.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    bchild.label(
        egui::RichText::new("Grants 3 permissions")
            .size(theme.font_size_term_sm.value())
            .color(egui::Color32::from(theme.text_muted())),
    );
    bchild.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // 미신뢰 매니페스트 → agent 변형 "Trust & add".
        let _add = Button::new("Trust & add")
            .variant(ButtonVariant::Agent)
            .show(ui, theme);
        let _cancel = Button::new("Cancel")
            .variant(ButtonVariant::Ghost)
            .show(ui, theme);
    });
}

fn add_path_picker(ui: &mut egui::Ui, theme: &Theme) {
    ui.scope(|ui| {
        ui.set_max_width(MEASURE_XL);
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        mono_label(ui, theme, "Plugin folder");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            let mut buf = "~/dev/tasty-logwatch".to_string();
            Input::new()
                .mono(true)
                .placeholder("~/dev/my-plugin")
                .width(260.0)
                .icon(&|ui, rect, c| glyph::FOLDER.image(rect.height(), c).paint_at(ui, rect))
                .show(ui, theme, &mut buf);
            let _find = Button::new("Find folder…")
                .variant(ButtonVariant::Secondary)
                .leading_icon(&|ui, rect, c| {
                    glyph::FOLDER.image(rect.height(), c).paint_at(ui, rect)
                })
                .show(ui, theme);
            let _verify = Button::new("Verify")
                .variant(ButtonVariant::Primary)
                .show(ui, theme);
        });
        ui.label(
            egui::RichText::new(
                "Point Tasty at a local folder containing a tasty-plugin.toml. Verifying reads \
                 its manifest and checks the signature; adding copies it into ~/.tasty/plugins.",
            )
            .size(theme.font_size_term_sm.value())
            .color(egui::Color32::from(theme.text_muted())),
        );
    });
}

fn add_manifest_preview(ui: &mut egui::Ui, theme: &Theme) {
    ui.scope(|ui| {
        ui.set_max_width(MEASURE_XL);
        ui.spacing_mut().item_spacing.y = theme.spacing_md.value();

        // 매니페스트 카드.
        egui::Frame::default()
            .fill(egui::Color32::from(theme.surface_raised()))
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                egui::Color32::from(theme.border_default()),
            ))
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                    let accent = egui::Color32::from(theme.accent_primary());
                    let (av_rect, _) = ui.allocate_exact_size(
                        egui::vec2(AVATAR_MANIFEST, AVATAR_MANIFEST),
                        egui::Sense::hover(),
                    );
                    draw_avatar(
                        ui,
                        theme,
                        av_rect,
                        'L',
                        accent,
                        egui::Color32::from(theme.surface_active()),
                        0.16,
                        0.34,
                        theme.font_size_max.value(),
                    );
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                            ui.label(
                                egui::RichText::new("logwatch")
                                    .size(theme.font_size_max.value())
                                    .strong()
                                    .color(egui::Color32::from(theme.text_primary())),
                            );
                            tag(ui, theme, "v0.3.1", TagVariant::Default, false);
                        });
                        ui.add_space(3.0);
                        ui.label(
                            egui::RichText::new("com.aurelia.logwatch · aurelia")
                                .size(theme.font_size_caption.value())
                                .monospace()
                                .color(egui::Color32::from(theme.text_muted())),
                        );
                    });
                });
                ui.label(
                    egui::RichText::new(
                        "Tails and highlights structured log files as a dedicated surface — \
                         severity filters, a jump-to-error gutter, and live follow on the \
                         active workspace.",
                    )
                    .size(theme.font_size_body.value())
                    .color(egui::Color32::from(theme.text_secondary())),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    mono_label(ui, theme, "Permissions");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(
                            theme.spacing_sm.value(),
                            theme.spacing_sm.value(),
                        );
                        for p in ["fs:read", "fs:watch", "ipc:logwatch.*"] {
                            tag(ui, theme, p, TagVariant::Default, false);
                        }
                    });
                });
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                    mono_label(ui, theme, "Source");
                    ui.label(
                        egui::RichText::new("~/dev/tasty-logwatch")
                            .size(theme.font_size_term_sm.value())
                            .monospace()
                            .color(egui::Color32::from(theme.text_secondary())),
                    );
                });
            });

        // 미신뢰 publisher 배너(trust 경고).
        let warn = egui::Color32::from(theme.accent_warning());
        egui::Frame::default()
            .fill(alpha(warn, 0.11))
            .stroke(egui::Stroke::new(theme.border_width.value(), alpha(warn, 0.36)))
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin {
                left: PAD_14 as i8,
                right: PAD_14 as i8,
                top: theme.spacing_md.value() as i8,
                bottom: theme.spacing_md.value() as i8,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ui.add(glyph::ALERT_TRIANGLE.image(theme.icon_glyph_size_md.value(), warn));
                    ui.label(
                        egui::RichText::new("Unverified publisher")
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(warn),
                    );
                });
                ui.label(
                    egui::RichText::new(
                        "This plugin isn't signed by a key in your trust store. It runs with \
                         the permissions above on every launch — review them, and only add \
                         plugins from sources you trust.",
                    )
                    .size(theme.font_size_term_sm.value())
                    .color(egui::Color32::from(theme.text_secondary())),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ui.label(
                        egui::RichText::new("fingerprint")
                            .size(theme.font_size_caption.value())
                            .monospace()
                            .color(egui::Color32::from(theme.text_secondary())),
                    );
                    ui.label(
                        egui::RichText::new("9f2c 4ad1 b770 e3a6  ·  ed25519")
                            .size(theme.font_size_caption.value())
                            .monospace()
                            .color(egui::Color32::from(theme.text_muted())),
                    );
                });
            });
    });
}
