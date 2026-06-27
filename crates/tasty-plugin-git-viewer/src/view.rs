//! UiNode tree 빌더 — popup 본체 트리 생성.

use tasty_plugin_sdk::Translator;
use tasty_plugin_sdk::ui::{
    hbox, label_color, label_mono, label_mono_color, scroll_v, selectable_row, spacer, splitter_id,
    vbox,
};
use tasty_plugin_sdk::{LabelStyle, SplitDir, UiNode};

use crate::git::{DiffData, DiffLineKind, FileStatus, LogEntry, StatusEntry, WorktreeEntry};

pub struct ViewModel<'a> {
    pub repo_path: Option<String>,
    pub error: Option<&'a str>,
    pub worktrees: &'a [WorktreeEntry],
    pub active_worktree: usize,
    pub status_entries: &'a [StatusEntry],
    pub log_entries: &'a [LogEntry],
    pub selected_file: Option<usize>,
    pub diff_content: Option<&'a DiffData>,
}

/// 단일 인스턴스 가드 placeholder. 이미 popup이 열려 있을 때 보여줄 메시지.
pub fn already_open_tree(tr: &Translator) -> UiNode {
    vbox([label_color(tr.t("git_viewer.already_open"), "subtext0")])
}

pub fn main_tree(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    let header = build_header(vm, tr);

    let mut top: Vec<UiNode> = vec![header];
    if let Some(err) = vm.error {
        top.push(label_color(
            tr.t("git_viewer.error").replace("{0}", err),
            "red",
        ));
    }

    if vm.repo_path.is_none() {
        top.push(label_color(tr.t("git_viewer.no_repo"), "subtext0"));
        return vbox(top);
    }

    let status_pane = build_status_pane(vm, tr);
    let bottom_pane = if vm.selected_file.is_some() && vm.diff_content.is_some() {
        build_diff_pane(vm, tr)
    } else {
        build_log_pane(vm, tr)
    };

    // 우측 컬럼: 기존 status(상)·log/diff(하) 세로 분할.
    let right_column = splitter_id(
        "split.main",
        SplitDir::Vertical,
        0.5,
        status_pane,
        bottom_pane,
    );
    // 좌측 worktree rail | 우측 컬럼. 960px popup 에서 ratio 0.25 ≈ 240px.
    let worktree_pane = build_worktree_pane(vm, tr);
    top.push(splitter_id(
        "split.rail",
        SplitDir::Horizontal,
        0.25,
        worktree_pane,
        right_column,
    ));
    vbox(top)
}

fn build_worktree_pane(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    let heading = UiNode::Label {
        text: format!(
            "{} ({})",
            tr.t("git_viewer.worktrees_heading"),
            vm.worktrees.len()
        ),
        style: LabelStyle::Heading,
        color: None,
    };

    let mut children: Vec<UiNode> = vec![heading];
    if vm.worktrees.is_empty() {
        children.push(label_color(tr.t("git_viewer.no_worktrees"), "subtext0"));
        return scroll_v(vbox(children));
    }
    for (idx, wt) in vm.worktrees.iter().enumerate() {
        children.push(build_worktree_row(idx, wt, idx == vm.active_worktree, tr));
    }
    scroll_v(vbox(children))
}

fn build_worktree_row(idx: usize, wt: &WorktreeEntry, active: bool, tr: &Translator) -> UiNode {
    // 이름 — 활성/비활성에 따른 색. invalid 면 흐리게.
    let name_color = if !wt.is_valid {
        "overlay0"
    } else if active {
        "text"
    } else {
        "subtext0"
    };
    let mut row: Vec<UiNode> = vec![label_mono_color(wt.name.clone(), name_color)];

    if let Some(head) = &wt.head {
        row.push(label_mono_color(format!("{head} "), "blue"));
    }

    // 타입 배지: main / linked.
    let type_key = if wt.is_main {
        "git_viewer.wt_main"
    } else {
        "git_viewer.wt_linked"
    };
    let type_color = if wt.is_main { "blue" } else { "subtext0" };
    row.push(label_mono_color(tr.t(type_key).to_string(), type_color));

    // 상태 배지.
    if wt.is_current {
        row.push(spacer(4));
        row.push(label_mono_color(tr.t("git_viewer.wt_current").to_string(), "green"));
    }
    if wt.locked {
        row.push(spacer(4));
        let label = match &wt.lock_reason {
            Some(r) if !r.is_empty() => {
                format!("{} ({r})", tr.t("git_viewer.wt_locked"))
            }
            _ => tr.t("git_viewer.wt_locked").to_string(),
        };
        row.push(label_mono_color(label, "yellow"));
    }
    if !wt.is_valid {
        row.push(spacer(4));
        row.push(label_mono_color(tr.t("git_viewer.wt_invalid").to_string(), "red"));
    }

    selectable_row(format!("wt.{idx}"), active, row)
}

fn build_header(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    let mut children: Vec<UiNode> = vec![UiNode::Button {
        id: "refresh".into(),
        label: tr.t("git_viewer.refresh").to_string(),
        enabled: true,
        style: Default::default(),
        tooltip_i18n_key: None,
    }];
    if let Some(p) = vm.repo_path.as_deref() {
        children.push(label_color(format!("({p})"), "subtext0"));
    }
    hbox(children)
}

fn build_status_pane(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    let heading = UiNode::Label {
        text: format!(
            "{} ({})",
            tr.t("git_viewer.status_heading"),
            vm.status_entries.len()
        ),
        style: LabelStyle::Heading,
        color: None,
    };

    let mut children: Vec<UiNode> = vec![heading];
    if vm.status_entries.is_empty() {
        children.push(label_color(tr.t("git_viewer.no_changes"), "subtext0"));
        return scroll_v(vbox(children));
    }
    for (idx, entry) in vm.status_entries.iter().enumerate() {
        let (prefix, color) = status_label(entry.status);
        let selected = vm.selected_file == Some(idx);
        children.push(selectable_row(
            format!("file.{idx}"),
            selected,
            [
                label_mono_color(prefix, color),
                label_mono(entry.path.clone()),
            ],
        ));
    }
    scroll_v(vbox(children))
}

fn build_log_pane(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    let heading = UiNode::Label {
        text: tr.t("git_viewer.log_heading").to_string(),
        style: LabelStyle::Heading,
        color: None,
    };
    let mut children: Vec<UiNode> = vec![heading];
    if vm.log_entries.is_empty() {
        children.push(label_color(tr.t("git_viewer.no_commits"), "subtext0"));
        return scroll_v(vbox(children));
    }
    for entry in vm.log_entries {
        children.push(build_log_row(entry));
    }
    scroll_v(vbox(children))
}

fn build_log_row(entry: &LogEntry) -> UiNode {
    let oid = label_mono_color(format!("{} ", entry.oid_short), "yellow");
    let mut row: Vec<UiNode> = vec![oid];
    if !entry.refs.is_empty() {
        row.push(label_mono_color(
            format!("({}) ", entry.refs.join(", ")),
            "blue",
        ));
    }
    row.push(label_mono(entry.summary.clone()));
    row.push(spacer(8));
    row.push(label_mono_color(entry.author.clone(), "subtext0"));
    row.push(spacer(4));
    row.push(label_mono_color(entry.time.clone(), "subtext0"));
    hbox(row)
}

fn build_diff_pane(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    let diff = match vm.diff_content {
        Some(d) => d,
        None => return scroll_v(label_color(tr.t("git_viewer.loading"), "subtext0")),
    };

    let toolbar = hbox([
        UiNode::Button {
            id: "back".into(),
            label: tr.t("git_viewer.back_to_log").to_string(),
            enabled: true,
            style: Default::default(),
            tooltip_i18n_key: None,
        },
        label_color(diff.file_path.clone(), "subtext0"),
    ]);

    if diff.hunks.is_empty() {
        return vbox([
            toolbar,
            label_color(tr.t("git_viewer.no_changes"), "subtext0"),
        ]);
    }

    let mut lines: Vec<UiNode> = Vec::new();
    for hunk in &diff.hunks {
        lines.push(label_mono_color(hunk.header.clone(), "blue"));
        for line in &hunk.lines {
            let (prefix, color) = match line.kind {
                DiffLineKind::Addition => ("+", "green"),
                DiffLineKind::Deletion => ("-", "red"),
                DiffLineKind::Context => (" ", "text"),
            };
            let old_no = line
                .old_lineno
                .map(|n| format!("{n:>4}"))
                .unwrap_or_else(|| "    ".to_string());
            let new_no = line
                .new_lineno
                .map(|n| format!("{n:>4}"))
                .unwrap_or_else(|| "    ".to_string());
            let text = format!("{old_no} {new_no} {prefix} {}", line.content);
            lines.push(label_mono_color(text, color));
        }
    }
    vbox([toolbar, scroll_v(vbox(lines))])
}

fn status_label(s: FileStatus) -> (&'static str, &'static str) {
    match s {
        FileStatus::Modified => (" M ", "yellow"),
        FileStatus::Added => (" A ", "green"),
        FileStatus::Deleted => (" D ", "red"),
        FileStatus::Renamed => (" R ", "blue"),
        FileStatus::Untracked => (" ? ", "overlay0"),
        FileStatus::Conflicted => (" U ", "red"),
    }
}
