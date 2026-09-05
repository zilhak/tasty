//! `file_picker` specimen — Native file picker (local & remote), gallery-first 1단계.
//!
//! 권위 원본: `gallery/overlays-shared.jsx` `FilePickerFrame`/`FpRow`/`FpCrumbs`/
//! `FpHostBadge`, `gallery/overlays-windows.jsx` `#filepicker` Section(스펙 3개).
//! design-request: `design-request/07151555-design-request-remote-file-picker.md`. 매핑:
//! `design-gallery-mapping.md` "file_picker".
//!
//! 640×480 단일 컴포넌트가 로컬/원격 두 모드를 겸한다 — 차이는 헤더 host indicator와
//! 브레드크럼 root뿐, 레이아웃은 불변. 원격 표시는 §6.1 열린 결정(A 배지 / B 글리프 /
//! C 프레임보더) 중 **A 배지가 사용자 확정**되어 이 specimen 은 A만 반영한다 —
//! B/C 는 미채택 대안이라 코드화하지 않는다.
//!
//! **본체 구현**: `src/adapters/ui/popup/file_picker.rs`(`PopupDef` id
//! `"file_picker"`, Tools 메뉴 트리거). 이 specimen 은 mock 데이터로 독립 렌더 —
//! 본체와 코드 공유는 하지 않는다(`file_handler_picker` 갤러리 specimen과 동일
//! 관례). 원격 채널 설계는 `docs/adr/0053-native-file-picker-remote-attach-channel.md`.
//!
//! `draw` = 개요(로컬/원격 loaded 나란히). `draw_states` = loading·empty·
//! permission-denied·connection-lost·multi-select 5상태. 키보드 focus-ring 은
//! loaded 프레임의 `pipeline.yaml` 행에 상시 표시(selection 과 시각 구분).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::{
    CENTER_BLOCK_H_SPECIMEN as EMPTY_BLOCK_H, CENTER_GLYPH_SIZE as EMPTY_GLYPH, STRUCT_GAP_2,
};
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, IconButtonVariant, Spinner, checkbox};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// ── 프레임 고정 치수 (디자인 raw px 근사 — 화면 전용 고정값, token-policy §c) ──
const FRAME_W: LogicalPx = LogicalPx(640.0);
const FRAME_H: f32 = 480.0;
const HEADER_H: f32 = 44.0; // padding ~8/8(디자인 10/10 근사) + content(host badge 22 최대)
const HEADER_PAD_L: f32 = 14.0; // 디자인 L14
const PATH_H: f32 = 36.0; // padding ~6/6 + refresh IconButton(sm)
const LIST_HEAD_H: f32 = 26.0; // caption row — loaded/multi 상태만
const FOOTER_H: f32 = 84.0; // name row(28) + gap(8) + action row(28) + padding 10/10 근사
const BODY_H: f32 = FRAME_H - HEADER_H - PATH_H - FOOTER_H;
const ROW_H: LogicalPx = LogicalPx(28.0); // FpRow padding 6/space-md + content 16
const SIZE_COL_W: f32 = 68.0;
const MOD_COL_W: f32 = 108.0;
const FOOTER_LABEL_W: LogicalPx = LogicalPx(64.0); // 디자인 "File name" 라벨 고정폭
const FOOTER_CHIP_W: f32 = 92.0; // "All files ▾" 타입필터 칩

/// 원격 host 배지 칩의 높이(디자인 size-22). 4px 그리드 밖이고 대응 Theme 토큰이
/// 없다 — 칩 하나의 구조 높이라 spacing 리듬 값이 아니다.
const HOST_BADGE_H: LogicalPx = LogicalPx(22.0);

/// 브레드크럼 구분자·타입필터 칩 화살표의 글리프 한 변. **아이콘 스케일 밖이다** —
/// Theme 은 12(xs) · 14(sm) · 15(row-action) · 16(md) 만 갖는데 디자인은 여기 13 을
///쓴다. 조용히 12/14 로 반올림하지 않고 값을 보존한 채 이름만 붙였다.
const CRUMB_GLYPH: LogicalPx = LogicalPx(13.0);

/// 그 블록 본문 텍스트의 최대 폭 — 한 줄이 너무 길어지지 않게 잡는 값.
const EMPTY_BODY_MAX_W: LogicalPx = LogicalPx(340.0);
const HOST: &str = "deploy@10.0.4.12";

#[derive(Clone, Copy, PartialEq, Eq)]
enum FpState {
    Loaded,
    Loading,
    Empty,
    ErrorPerm,
    ErrorConn,
}

struct Row {
    folder: bool,
    name: &'static str,
    size: &'static str,
    modified: &'static str,
}

// 디자인 FilePickerFrame `files` seed 1:1 (overlays-shared.jsx).
const FILES: &[Row] = &[
    Row {
        folder: true,
        name: "configs",
        size: "—",
        modified: "Jul 12 09:14",
    },
    Row {
        folder: true,
        name: "logs",
        size: "—",
        modified: "Jul 14 22:03",
    },
    Row {
        folder: true,
        name: "node_modules",
        size: "—",
        modified: "Jul 02 11:40",
    },
    Row {
        folder: false,
        name: "README.md",
        size: "4.2 KB",
        modified: "Jul 15 08:21",
    },
    Row {
        folder: false,
        name: "package.json",
        size: "1.1 KB",
        modified: "Jul 15 08:21",
    },
    Row {
        folder: false,
        name: "pipeline.yaml",
        size: "3.8 KB",
        modified: "Jul 14 17:55",
    },
    Row {
        folder: false,
        name: "deploy.sh",
        size: "902 B",
        modified: "Jul 11 14:02",
    },
    Row {
        folder: false,
        name: ".env",
        size: "218 B",
        modified: "Jul 09 10:30",
    },
];

// 디자인 multi 상태 checked seed (README.md / package.json / pipeline.yaml).
const MULTI_PICKED: &[&str] = &["README.md", "package.json", "pipeline.yaml"];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "local", |ui| {
            card(ui, theme, FpState::Loaded, false, false);
        });
        spec::cluster(ui, theme, "remote (host badge)", |ui| {
            card(ui, theme, FpState::Loaded, true, false);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "640×480 · PopupDef · bg-panel"),
            ("header", "glyph · title · host badge(remote) · ✕"),
            ("path bar", "breadcrumb + refresh · bg-sidebar"),
            ("row", "checkbox? · icon · name · size · modified"),
            ("footer", "name field · type filter · Cancel / Open"),
            ("open", "enabled only with a selection"),
            ("dismiss", "×/Cancel/Esc · Open = Enter"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "bg-sidebar",
                "path / list-header bar",
                theme.bg_sidebar().to_egui(),
            ),
            TokenChip::new(
                "surface-active",
                "selected row",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "selected bar · folder glyph · Open · crumb link",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "accent-info",
                "remote host badge",
                theme.accent_info().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Tasty's own \"Open file\" dialog — replaces the OS-native picker so it can browse a \
         remote attach host over the same SSH mechanism as attach (the OS picker only ever \
         sees the local disk). Local and remote are the same component; they differ only in \
         the header host indicator and the breadcrumb root, never the layout. Not the Explorer \
         surface (free-roam tab) — this is a select-and-confirm dialog. Not yet wired to the \
         host app: the remote directory-listing channel architecture is still undecided.",
    );
}

pub fn draw_states(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "loading (remote)", |ui| {
            card(ui, theme, FpState::Loading, true, false);
        });
        spec::cluster(ui, theme, "empty folder", |ui| {
            card(ui, theme, FpState::Empty, false, false);
        });
        spec::cluster(ui, theme, "permission denied (local)", |ui| {
            card(ui, theme, FpState::ErrorPerm, false, false);
        });
        spec::cluster(ui, theme, "connection lost (remote)", |ui| {
            card(ui, theme, FpState::ErrorConn, true, false);
        });
        spec::cluster(ui, theme, "multi-select (remote)", |ui| {
            card(ui, theme, FpState::Loaded, true, true);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("loading", "Spinner · reads dir (remote: over SSH)"),
            ("empty", "folderOpen glyph + muted line"),
            ("error", "danger glyph · title · reason · action"),
            ("perm vs. conn", "Retry vs. Reconnect (resumes)"),
            ("multi", "checkbox col · joined names · N selected"),
            ("Open", "disabled while loading / error / empty"),
        ],
        &[
            TokenChip::new(
                "accent-danger",
                "error glyph",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "text-placeholder",
                "empty glyph",
                theme.text_placeholder().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Row focus ring (keyboard nav) is shown on pipeline.yaml in the loaded frames above — \
         a 1px accent-primary outline, distinct from the filled selection background so focus \
         and selection never merge visually. Multi-select is spec'd for the future \
         (checkbox column, comma-joined name field, \"N selected\" count); single-select ships \
         by default. Favorites/recent locations are deferred, not designed this pass.",
    );
}

// ════════════════════════════════════════════════════════════════════════
/// 640×480 카드 한 장.
fn card(ui: &mut egui::Ui, theme: &Theme, state: FpState, remote: bool, multi: bool) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_width(FRAME_W.value());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.vertical(|ui| {
                ui.set_width(FRAME_W.value());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                header(ui, theme, remote);
                path_bar(ui, theme, remote);
                body(ui, theme, state, multi);
                footer(ui, theme, state, multi);
            });
        });
}

fn header(ui: &mut egui::Ui, theme: &Theme, remote: bool) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(FRAME_W.value(), HEADER_H), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + HEADER_PAD_L,
            rect.top() + theme.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_sm.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    // A 배지 채택: 헤더 글리프는 원격/로컬 공통 FILE (glyph 후보 B 의 remote 스왑은 미반영).
    kit::icon(
        &mut child,
        icons::FILE,
        theme.icon_glyph_size_md,
        theme.text_muted().to_egui(),
    );
    kit::title(&mut child, theme, "Open file");
    if remote {
        host_badge(&mut child, theme, HOST);
    }
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .show(ui, theme, &|ui, rect, c| {
                icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
            });
    });
}

/// 원격 host 배지(§6.1 A안, 채택) — mono `user@host` 칩, `accent-info` 축.
fn host_badge(ui: &mut egui::Ui, theme: &Theme, host: &str) {
    let info = theme.accent_info().to_egui();
    let font = egui::FontId::monospace(theme.font_size_caption.value());
    let galley = ui
        .painter()
        .layout_no_wrap(host.to_owned(), font, egui::Color32::PLACEHOLDER);
    let glyph = theme.icon_glyph_size_xs.value();
    let gap = theme.spacing_xs.value();
    let pad_x = theme.spacing_sm.value();
    let h = HOST_BADGE_H.value();
    let w = pad_x * 2.0 + glyph + gap + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let radius = theme.corner_radius.value();
    ui.painter()
        .rect_filled(rect, radius, info.gamma_multiply(0.14));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), info.gamma_multiply(0.45)),
        egui::StrokeKind::Inside,
    );
    let gy = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, rect.center().y - glyph * 0.5),
        egui::vec2(glyph, glyph),
    );
    icons::REMOTE.image(glyph, info).paint_at(ui, gy);
    let pos = egui::pos2(
        rect.left() + pad_x + glyph + gap,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter().galley(pos, galley, info);
}

fn path_bar(ui: &mut egui::Ui, theme: &Theme, remote: bool) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(FRAME_W.value(), PATH_H), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + HEADER_PAD_L, rect.top()),
        egui::pos2(rect.right() - theme.spacing_sm.value(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    crumbs(&mut child, theme, remote);
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .show(ui, theme, &|ui, rect, c| {
                icons::REFRESH.image(rect.height(), c).paint_at(ui, rect)
            });
    });
}

struct Crumb {
    label: &'static str,
    root: bool,
    current: bool,
}

/// 브레드크럼 — root(mono) → 중간(accent 링크) → current(bold, 비클릭).
fn crumbs(ui: &mut egui::Ui, theme: &Theme, remote: bool) {
    const REMOTE_CRUMBS: &[Crumb] = &[
        Crumb {
            label: HOST,
            root: true,
            current: false,
        },
        Crumb {
            label: "home",
            root: false,
            current: false,
        },
        Crumb {
            label: "deploy",
            root: false,
            current: false,
        },
        Crumb {
            label: "agents-prod",
            root: false,
            current: true,
        },
    ];
    const LOCAL_CRUMBS: &[Crumb] = &[
        Crumb {
            label: "/",
            root: true,
            current: false,
        },
        Crumb {
            label: "Users",
            root: false,
            current: false,
        },
        Crumb {
            label: "maya",
            root: false,
            current: false,
        },
        Crumb {
            label: "projects",
            root: false,
            current: true,
        },
    ];
    let items = if remote { REMOTE_CRUMBS } else { LOCAL_CRUMBS };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = STRUCT_GAP_2.value();
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                kit::icon(
                    ui,
                    icons::CHEVRON_RIGHT,
                    CRUMB_GLYPH,
                    theme.text_disabled().to_egui(),
                );
            }
            let color = if it.current {
                theme.text_primary()
            } else {
                theme.accent_primary()
            };
            let mut rt = egui::RichText::new(it.label)
                .size(theme.font_size_caption.value())
                .color(color.to_egui());
            if it.root {
                rt = rt.monospace();
            }
            if it.current {
                rt = rt.strong();
            }
            ui.label(rt);
        }
    });
}

/// 행 컬럼 x좌표 — list header 와 `row` 가 동일 레이아웃을 공유.
struct Cols {
    checkbox_x: Option<f32>,
    icon_x: f32,
    name_left: f32,
    /// name 컬럼 우측 한계(= size 컬럼 좌측 - gap) — 디자인 `flex:1; max-width:0;
    /// overflow:hidden; ellipsis` 흉내(긴 이름 말줄임)에 쓰인다.
    name_right: f32,
    size_right: f32,
    mod_right: f32,
}

fn cols(rect: egui::Rect, theme: &Theme, multi: bool) -> Cols {
    let pad = theme.spacing_md.value();
    let gap = theme.spacing_sm.value();
    let glyph = theme.icon_glyph_size_md.value();
    let mut x = rect.left() + pad;
    let checkbox_x = if multi {
        let cx = x;
        x += glyph + gap;
        Some(cx)
    } else {
        None
    };
    let icon_x = x;
    x += glyph + gap;
    let name_left = x;
    let mod_right = rect.right() - pad;
    let size_right = mod_right - MOD_COL_W - gap;
    let name_right = size_right - SIZE_COL_W - gap;
    Cols {
        checkbox_x,
        icon_x,
        name_left,
        name_right,
        size_right,
        mod_right,
    }
}

/// 폭이 `max_w` 를 넘으면 문자 단위로 잘라 `…` 을 붙인다 (디자인 `text-overflow:
/// ellipsis` 흉내 — FpRow name 컬럼).
fn elide(ui: &egui::Ui, text: &str, font: egui::FontId, max_w: f32) -> String {
    let measure = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
            .rect
            .width()
    };
    if max_w <= 0.0 || measure(text) <= max_w {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if measure(&candidate) <= max_w {
            return candidate;
        }
    }
    "…".to_owned()
}

fn list_header(ui: &mut egui::Ui, theme: &Theme, multi: bool) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), LIST_HEAD_H),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let c = cols(rect, theme, multi);
    let font = egui::FontId::monospace(theme.font_size_micro.value());
    let muted = theme.text_muted().to_egui();
    ui.painter().text(
        egui::pos2(c.name_left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "NAME",
        font.clone(),
        muted,
    );
    ui.painter().text(
        egui::pos2(c.size_right, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "SIZE",
        font.clone(),
        muted,
    );
    ui.painter().text(
        egui::pos2(c.mod_right, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "MODIFIED",
        font,
        muted,
    );
}

fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    r: &Row,
    multi: bool,
    checked: bool,
    selected: bool,
    focus: bool,
) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, ROW_H.value()), egui::Sense::hover());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(theme.tab_indicator_width.value(), rect.height()),
        );
        ui.painter()
            .rect_filled(bar, 0.0, theme.accent_primary().to_egui());
    }
    if focus {
        // outlineOffset -1 흉내 — rect 안쪽 1px.
        ui.painter().rect_stroke(
            rect.shrink(theme.border_width.value() * 0.5),
            0.0,
            egui::Stroke::new(theme.border_width.value(), theme.accent_primary().to_egui()),
            egui::StrokeKind::Inside,
        );
    }
    let c = cols(rect, theme, multi);
    let glyph_size = theme.icon_glyph_size_md.value();
    if let Some(cx) = c.checkbox_x {
        let mut chk = checked;
        let cb_rect = egui::Rect::from_min_size(
            egui::pos2(cx, rect.center().y - glyph_size * 0.5),
            egui::vec2(glyph_size, glyph_size),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cb_rect));
        checkbox(&mut child, theme, &mut chk, "", true);
    }
    let (icon_glyph, icon_color): (MockGlyph, egui::Color32) = if r.folder {
        (icons::FOLDER, theme.accent_primary().to_egui())
    } else {
        (icons::FILE, theme.text_muted().to_egui())
    };
    let ir = egui::Rect::from_min_size(
        egui::pos2(c.icon_x, rect.center().y - glyph_size * 0.5),
        egui::vec2(glyph_size, glyph_size),
    );
    icon_glyph.image(glyph_size, icon_color).paint_at(ui, ir);
    let name_color = if selected {
        theme.text_primary()
    } else {
        theme.text_secondary()
    };
    let name_font = egui::FontId::proportional(theme.font_size_body.value());
    let name_text = elide(ui, r.name, name_font.clone(), c.name_right - c.name_left);
    ui.painter().text(
        egui::pos2(c.name_left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name_text,
        name_font,
        name_color.to_egui(),
    );
    let mono_caption = egui::FontId::monospace(theme.font_size_caption.value());
    let muted = theme.text_muted().to_egui();
    ui.painter().text(
        egui::pos2(c.size_right, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        r.size,
        mono_caption.clone(),
        muted,
    );
    ui.painter().text(
        egui::pos2(c.mod_right, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        r.modified,
        mono_caption,
        muted,
    );
}

fn body(ui: &mut egui::Ui, theme: &Theme, state: FpState, multi: bool) {
    match state {
        FpState::Loaded => {
            list_header(ui, theme, multi);
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(FRAME_W.value(), BODY_H - LIST_HEAD_H),
                egui::Sense::hover(),
            );
            let mut col = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            for f in FILES {
                let selected = !multi && f.name == "README.md";
                let focus = !multi && f.name == "pipeline.yaml";
                let checked = multi && MULTI_PICKED.contains(&f.name);
                row(&mut col, theme, f, multi, checked, selected, focus);
            }
        }
        FpState::Loading => center(
            ui,
            theme,
            icons::FOLDER,
            theme.text_placeholder().to_egui(),
            true,
            "Loading folder…",
            theme.text_secondary().to_egui(),
            Some("Reading the directory over SSH."),
            None,
        ),
        FpState::Empty => center(
            ui,
            theme,
            icons::FOLDER_OPEN,
            theme.text_placeholder().to_egui(),
            false,
            "This folder is empty",
            theme.text_muted().to_egui(),
            None,
            None,
        ),
        FpState::ErrorPerm => center(
            ui,
            theme,
            icons::ALERT_TRIANGLE,
            theme.accent_danger().to_egui(),
            false,
            "Permission denied",
            theme.text_primary().to_egui(),
            Some(
                "You don't have permission to read this folder. Try a different folder or check access.",
            ),
            Some("Retry"),
        ),
        FpState::ErrorConn => center(
            ui,
            theme,
            icons::ALERT_TRIANGLE,
            theme.accent_danger().to_egui(),
            false,
            "Remote connection lost",
            theme.text_primary().to_egui(),
            Some("The SSH tunnel dropped. Reconnect to resume browsing from the last folder."),
            Some("Reconnect"),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn center(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: MockGlyph,
    glyph_color: egui::Color32,
    spinner: bool,
    heading: &str,
    heading_color: egui::Color32,
    body_text: Option<&str>,
    action: Option<&str>,
) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(FRAME_W.value(), BODY_H), egui::Sense::hover());
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    col.add_space((BODY_H - EMPTY_BLOCK_H).max(0.0) * 0.5);
    col.spacing_mut().item_spacing.y = theme.spacing_sm.value();
    if spinner {
        Spinner::new().size(EMPTY_GLYPH).show(&mut col, theme);
    } else {
        kit::icon(&mut col, glyph, LogicalPx(EMPTY_GLYPH), glyph_color);
    }
    col.label(
        egui::RichText::new(heading)
            .size(theme.font_size_body.value())
            .strong()
            .color(heading_color),
    );
    if let Some(b) = body_text {
        col.set_max_width(EMPTY_BODY_MAX_W.value());
        col.label(
            egui::RichText::new(b)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    }
    if let Some(label) = action {
        col.add_space(theme.spacing_xs.value());
        Button::new(label)
            .variant(ButtonVariant::Secondary)
            .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
            .show(&mut col, theme);
    }
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: FpState, multi: bool) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(FRAME_W.value(), FOOTER_H), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + theme.spacing_lg.value(),
            rect.top() + theme.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_lg.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing.y = theme.spacing_sm.value();

    let (name_text, placeholder): (String, bool) = match state {
        FpState::Loaded if multi => (MULTI_PICKED.join(", "), false),
        FpState::Loaded => ("README.md".to_owned(), false),
        _ => (String::new(), true),
    };
    col.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        ui.allocate_ui_with_layout(
            egui::vec2(FOOTER_LABEL_W.value(), 0.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("File name")
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            },
        );
        let remaining = ui.available_width();
        let input_w = (remaining - FOOTER_CHIP_W - theme.spacing_sm.value()).max(0.0);
        kit::field(
            ui,
            theme,
            Some(input_w),
            if placeholder {
                "No file selected"
            } else {
                name_text.as_str()
            },
            placeholder,
            false,
        );
        type_filter_chip(ui, theme);
    });

    col.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        if multi {
            ui.label(
                egui::RichText::new(format!("{} selected", MULTI_PICKED.len()))
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_open = state == FpState::Loaded;
            Button::new("Open")
                .variant(ButtonVariant::Primary)
                .enabled(can_open)
                .show(ui, theme);
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .show(ui, theme);
        });
    });
}

/// "All files ▾" 타입 필터 칩 — 정적(팝오버 미열림) specimen.
fn type_filter_chip(ui: &mut egui::Ui, theme: &Theme) {
    let h = theme.item_height_interactive.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(FOOTER_CHIP_W, h), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.bg_panel().to_egui(),
    );
    ui.painter().rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
    let pad = theme.spacing_sm.value();
    ui.painter().text(
        egui::pos2(rect.left() + pad, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "All files",
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_secondary().to_egui(),
    );
    let ir = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - pad - CRUMB_GLYPH.value(),
            rect.center().y - CRUMB_GLYPH.value() * 0.5,
        ),
        egui::vec2(CRUMB_GLYPH.value(), CRUMB_GLYPH.value()),
    );
    icons::CHEVRON_DOWN
        .image(CRUMB_GLYPH.value(), theme.text_muted().to_egui())
        .paint_at(ui, ir);
}
