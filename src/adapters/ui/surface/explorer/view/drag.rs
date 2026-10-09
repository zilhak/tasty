//! explorer 드래그 앤 드롭. 항목을 폴더 행·셀, 트리 노드, 즐겨찾기, 목록 본문(보고 있는 폴더)에
//! 놓으면 붙여넣기와 같은 작업을 요청한다. 다른 explorer 칸에 놓을 수도 있다.
//!
//! 칸들은 egui `DragAndDrop` payload 를 함께 읽는다. 각 칸은 그리는 동안 놓을 수 있는 자리를
//! [`note`] 류로 남기고, 상태줄을 그린 뒤 [`frame`] 이 그 자리로 드래그 시작·대상·놓기를 처리한다.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tasty_settings::keybindings::parse::Combo;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{DragChipProps, DragOp, drag_chip, paint_drop_target};

use super::ExplorerView;
use super::ops::{OpsAction, glyph};
use crate::adapters::ui::icons;
use crate::app::explorer_files::job::OpKind;
use crate::core::fs_list::DirEntryInfo;
use crate::i18n::{t, t_fmt};

/// 닫힌 트리 폴더 위에 이만큼 머물면 펼친다.
const EXPAND_AFTER: Duration = Duration::from_millis(800);

/// 끌고 있는 항목. 시작한 칸만 놓을 곳이 없을 때의 칩을 그린다.
pub(crate) struct Payload {
    paths: Vec<PathBuf>,
    single_dir: bool,
    source: egui::Id,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// 목록·격자·자세히 보기의 항목. `wide` 면 자세히 보기 행이라 본문 폭 전체가 행이다.
    Entry {
        wide: bool,
    },
    Tree {
        open: bool,
    },
    Favorite,
    /// 보고 있는 폴더(목록 본문 전체).
    Body,
}

#[derive(Clone, Debug)]
struct Spot {
    path: PathBuf,
    is_dir: bool,
    rect: egui::Rect,
    kind: Kind,
}

#[derive(Clone, Default)]
struct Spots(Vec<Spot>);

/// 놓을 자리를 차지한 마지막 pass. 아무 칸도 차지하지 않으면 시작한 칸이 거절 칩을 그린다.
#[derive(Clone, Copy, Default)]
struct Claim(u64);

/// 칸 하나의 드래그 상태. 대상 폴더가 바뀔 때만 디스크를 다시 읽는다.
#[derive(Default)]
pub(crate) struct DragState {
    /// 드래그 동작을 뒤집는 modifier. 호스트가 매 프레임 설정에서 채운다.
    pub(crate) flip: Option<Combo>,
    /// popup·modal 이 떠 있다. 그동안은 드래그를 시작하지도 받지도 않는다.
    pub(crate) blocked: bool,
    /// 이번 누름에서 이미 드래그를 시작했다. Esc 로 취소된 뒤 같은 누름으로 다시 시작하지 않는다.
    started: bool,
    hover: Option<(PathBuf, Instant)>,
    probe: Option<Probe>,
}

struct Probe {
    dest: PathBuf,
    source: PathBuf,
    same_volume: bool,
    writable: bool,
}

/// 놓기 판정.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Verdict {
    Go(OpKind),
    /// 거절 이유의 번역 키.
    No(&'static str),
}

fn spots_id() -> egui::Id {
    egui::Id::new("explorer_drag_spots")
}

fn claim_id() -> egui::Id {
    egui::Id::new("explorer_drag_claim")
}

fn push(ui: &egui::Ui, spot: Spot) {
    let rect = spot.rect.intersect(ui.clip_rect());
    if !rect.is_positive() {
        return;
    }
    ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_default::<Spots>(spots_id())
            .0
            .push(Spot { rect, ..spot })
    });
}

/// 목록·격자 항목 자리.
pub(crate) fn note(ui: &egui::Ui, entry: &DirEntryInfo, rect: egui::Rect) {
    if entry.name == ".." {
        return;
    }
    push(
        ui,
        Spot {
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            rect,
            kind: Kind::Entry { wide: false },
        },
    );
}

/// 자세히 보기의 이름 셀. 행 전체를 대상으로 삼는다.
pub(crate) fn note_row(ui: &egui::Ui, entry: &DirEntryInfo) {
    if entry.name == ".." {
        return;
    }
    push(
        ui,
        Spot {
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            rect: ui.max_rect(),
            kind: Kind::Entry { wide: true },
        },
    );
}

pub(crate) fn note_tree(ui: &egui::Ui, dir: &Path, rect: egui::Rect, open: bool) {
    push(
        ui,
        Spot {
            path: dir.to_path_buf(),
            is_dir: true,
            rect,
            kind: Kind::Tree { open },
        },
    );
}

pub(crate) fn note_favorite(ui: &egui::Ui, path: &Path, rect: egui::Rect) {
    push(
        ui,
        Spot {
            path: path.to_path_buf(),
            is_dir: true,
            rect,
            kind: Kind::Favorite,
        },
    );
}

/// 자리의 실제 영역. 자세히 보기 행은 본문 폭으로 넓힌다.
fn area(spot: &Spot, body: egui::Rect) -> egui::Rect {
    match spot.kind {
        Kind::Entry { wide: true } => {
            egui::Rect::from_x_y_ranges(body.x_range(), spot.rect.y_range())
        }
        _ => spot.rect,
    }
}

/// 포인터 아래의 대상: 폴더면 그 폴더, 파일·빈 곳이면 보고 있는 폴더(본문 전체).
fn target(
    spots: &[Spot],
    pos: egui::Pos2,
    body: egui::Rect,
    shown: Option<&Path>,
) -> Option<(PathBuf, egui::Rect, Kind)> {
    let hit = spots.iter().rev().find(|s| area(s, body).contains(pos));
    match hit {
        Some(s) if s.is_dir => Some((s.path.clone(), area(s, body), s.kind)),
        Some(s) if !matches!(s.kind, Kind::Entry { .. }) => None,
        _ if body.contains(pos) => shown.map(|d| (d.to_path_buf(), body, Kind::Body)),
        _ => None,
    }
}

/// 같은 디스크면 이동, 다른 디스크면 복사. `flip` 이면 뒤집는다.
pub(crate) fn verdict(
    paths: &[PathBuf],
    dest: &Path,
    same_volume: bool,
    writable: bool,
    flip: bool,
    remote: bool,
) -> Verdict {
    if remote {
        return Verdict::No("explorer.drag.remote");
    }
    if paths.iter().any(|p| dest.starts_with(p)) {
        return Verdict::No("explorer.drag.into_itself");
    }
    let op = match (same_volume, flip) {
        (true, false) | (false, true) => OpKind::Move,
        _ => OpKind::Copy,
    };
    if op == OpKind::Move && paths.iter().all(|p| p.parent() == Some(dest)) {
        return Verdict::No("explorer.drag.same_folder");
    }
    if !writable {
        return Verdict::No("explorer.drag.no_write");
    }
    Verdict::Go(op)
}

#[cfg(unix)]
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        // 읽지 못하면 작업이 이유와 함께 실패한다. 기본 동작은 원본을 남기는 복사로 둔다.
        _ => false,
    }
}

#[cfg(windows)]
fn same_volume(a: &Path, b: &Path) -> bool {
    let root = |p: &Path| {
        p.components()
            .next()
            .map(|c| c.as_os_str().to_ascii_lowercase())
    };
    root(a) == root(b)
}

#[cfg(not(any(unix, windows)))]
fn same_volume(_a: &Path, _b: &Path) -> bool {
    false
}

#[cfg(unix)]
fn writable(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: c 는 NUL 로 끝나는 유효한 경로 문자열이고 호출 동안 살아 있다.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn writable(dir: &Path) -> bool {
    // Windows 폴더의 읽기 전용 속성은 쓰기를 막지 않는다. 실제 거부는 작업이 이유와 함께 보고한다.
    std::fs::metadata(dir).is_ok()
}

fn held(mods: egui::Modifiers, flip: Option<Combo>) -> bool {
    let Some(flip) = flip else {
        return false;
    };
    #[cfg(target_os = "macos")]
    let (alt, option) = (mods.mac_cmd, mods.alt);
    #[cfg(not(target_os = "macos"))]
    let (alt, option) = (mods.alt, false);
    let now = Combo {
        ctrl: mods.ctrl,
        alt,
        option,
        shift: mods.shift,
    };
    now.contains_all(flip)
}

fn start(
    ctx: &egui::Context,
    me: egui::Id,
    view: &mut ExplorerView,
    spots: &[Spot],
    body: egui::Rect,
) {
    let (dragging, origin) = ctx.input(|i| {
        (
            i.pointer.primary_down() && i.pointer.is_decidedly_dragging(),
            i.pointer.press_origin(),
        )
    });
    if !ctx.input(|i| i.pointer.primary_down()) {
        view.ops.drag.started = false;
    }
    if !dragging || view.ops.drag.started || ctx.dragged_id().is_some() || view.is_remote() {
        return;
    }
    let Some(origin) = origin else {
        return;
    };
    let Some(spot) = spots
        .iter()
        .rev()
        .find(|s| matches!(s.kind, Kind::Entry { .. }) && area(s, body).contains(origin))
    else {
        return;
    };
    let paths = if view.selected.contains(&spot.path) {
        let mut paths: Vec<PathBuf> = view.selected.iter().cloned().collect();
        paths.sort();
        paths
    } else {
        view.select_only(&spot.path);
        vec![spot.path.clone()]
    };
    let single_dir = paths.len() == 1 && spot.is_dir;
    view.ops.drag.started = true;
    egui::DragAndDrop::set_payload(
        ctx,
        Payload {
            paths,
            single_dir,
            source: me,
        },
    );
}

/// 대상 폴더의 디스크·쓰기 가능 여부. 대상이나 원본이 바뀔 때만 다시 읽는다.
fn probe<'a>(state: &'a mut DragState, dest: &Path, source: &Path) -> &'a Probe {
    let stale = state
        .probe
        .as_ref()
        .is_none_or(|p| p.dest != dest || p.source != source);
    if stale {
        state.probe = Some(Probe {
            dest: dest.to_path_buf(),
            source: source.to_path_buf(),
            same_volume: same_volume(source, dest),
            writable: writable(dest),
        });
    }
    state.probe.as_ref().expect("probe was just filled")
}

/// 닫힌 트리 폴더 위에 머문 시간을 재고, 다 되면 펼친다.
fn expand_after_hover(ctx: &egui::Context, view: &mut ExplorerView, over: Option<(&Path, Kind)>) {
    let Some((dir, Kind::Tree { open: false })) = over else {
        view.ops.drag.hover = None;
        return;
    };
    let now = Instant::now();
    match &view.ops.drag.hover {
        Some((p, since)) if p == dir => {
            let waited = now.saturating_duration_since(*since);
            if waited >= EXPAND_AFTER {
                view.expanded.insert(dir.to_path_buf());
                view.ops.drag.hover = None;
            } else {
                ctx.request_repaint_after(EXPAND_AFTER - waited);
            }
        }
        _ => {
            view.ops.drag.hover = Some((dir.to_path_buf(), now));
            ctx.request_repaint_after(EXPAND_AFTER);
        }
    }
}

fn chip(
    ctx: &egui::Context,
    theme: &Theme,
    pos: egui::Pos2,
    payload: &Payload,
    shown: Option<(&Path, Verdict)>,
) {
    let label = match payload.paths.as_slice() {
        [one] => one
            .file_name()
            .map_or_else(|| one.to_string_lossy(), |n| n.to_string_lossy())
            .into_owned(),
        many => t_fmt("explorer.drag.items", &many.len().to_string()),
    };
    let folder = |dest: &Path| {
        dest.file_name()
            .map_or_else(|| dest.to_string_lossy(), |n| n.to_string_lossy())
            .into_owned()
    };
    let (line, reason, op) = match shown {
        Some((dest, Verdict::Go(OpKind::Copy))) => (
            t_fmt("explorer.drag.copy_to", &folder(dest)),
            None,
            DragOp::Copy,
        ),
        Some((dest, Verdict::Go(_))) => (
            t_fmt("explorer.drag.move_to", &folder(dest)),
            None,
            DragOp::Move,
        ),
        Some((_, Verdict::No(key))) => (
            t("explorer.drag.refused").to_owned(),
            Some(t_fmt("explorer.drag.refused_reason", t(key))),
            DragOp::Refused,
        ),
        None => (t("explorer.drag.refused").to_owned(), None, DragOp::Refused),
    };
    let item = if payload.paths.len() > 1 {
        icons::LAYERS
    } else if payload.single_dir {
        icons::FOLDER
    } else {
        icons::FILE
    };
    let op_icon = match op {
        DragOp::Move => icons::MOVE,
        DragOp::Copy => icons::PLUS,
        DragOp::Refused => icons::CLOSE,
    };
    let offset = theme.spacing_md.value();
    egui::Area::new(egui::Id::new("explorer_drag_chip"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(offset, offset))
        .interactable(false)
        .show(ctx, |ui| {
            drag_chip(
                ui,
                theme,
                &DragChipProps {
                    label: &label,
                    line: &line,
                    reason: reason.as_deref(),
                    op,
                },
                &glyph(item),
                &glyph(op_icon),
            );
        });
}

/// 칸의 드래그 처리. 본문과 상태줄을 그린 뒤 한 번 부른다.
pub(crate) fn frame(
    ui: &egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    body: egui::Rect,
    action: &mut Option<super::super::ExplorerAction>,
) {
    let ctx = ui.ctx().clone();
    // 자세히 보기의 넓은 열이 본문 rect 를 칸 밖으로 넓힐 수 있다. 칸에서 보이는 부분만 대상이다.
    let body = body.intersect(ui.clip_rect());
    let spots = ctx
        .data_mut(|d| d.remove_temp::<Spots>(spots_id()))
        .unwrap_or_default()
        .0;
    let me = ui.id();
    if view.ops.drag.blocked {
        view.ops.drag.hover = None;
        return;
    }
    let Some(payload) = egui::DragAndDrop::payload::<Payload>(&ctx) else {
        view.ops.drag.hover = None;
        start(&ctx, me, view, &spots, body);
        return;
    };
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    let shown = view.loaded_dir().map(Path::to_path_buf);
    let Some((dest, ring, kind)) = target(&spots, pos, body, shown.as_deref()) else {
        expand_after_hover(&ctx, view, None);
        let last = ctx
            .data(|d| d.get_temp::<Claim>(claim_id()))
            .unwrap_or_default();
        if payload.source == me && last.0 + 1 < pass {
            chip(&ctx, theme, pos, &payload, None);
        }
        return;
    };
    ctx.data_mut(|d| d.insert_temp(claim_id(), Claim(pass)));
    expand_after_hover(&ctx, view, Some((&dest, kind)));
    let flip = held(ctx.input(|i| i.modifiers), view.ops.drag.flip);
    let remote = view.is_remote();
    let first = payload.paths.first().cloned().unwrap_or_default();
    let probed = probe(&mut view.ops.drag, &dest, &first);
    let decided = verdict(
        &payload.paths,
        &dest,
        probed.same_volume,
        probed.writable,
        flip,
        remote,
    );
    if let Verdict::Go(_) = decided {
        let radius = if kind == Kind::Body {
            0.0
        } else {
            theme.corner_radius.value()
        };
        // 칸 자신의 레이어에 그려 칸 위에 뜬 popup·카드보다 아래에 둔다.
        let painter = ctx.layer_painter(ui.layer_id()).with_clip_rect(ring);
        paint_drop_target(&painter, theme, ring, radius);
    }
    chip(&ctx, theme, pos, &payload, Some((&dest, decided)));
    if let Verdict::Go(kind) = decided
        && ctx.input(|i| i.pointer.primary_released())
        && action.is_none()
    {
        *action = Some(super::super::ExplorerAction::Ops(OpsAction::Drop {
            kind,
            paths: payload.paths.clone(),
            dest,
        }));
        egui::DragAndDrop::clear_payload(&ctx);
    }
}

#[cfg(test)]
#[path = "drag_tests.rs"]
mod tests;
