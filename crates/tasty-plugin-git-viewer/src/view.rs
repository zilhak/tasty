//! UiNode tree 빌더 — popup 본체 트리 생성.

use tasty_plugin_sdk::Translator;
use tasty_plugin_sdk::ui::{
    hbox, label_color, label_mono, label_mono_color, scroll_v, selectable_row, spacer, splitter,
    vbox,
};
use tasty_plugin_sdk::{LabelStyle, SplitDir, UiNode};

use crate::git::{DiffData, DiffLineKind, FileStatus, LogEntry, StatusEntry};

pub struct ViewModel<'a> {
    pub repo_path: Option<String>,
    pub error: Option<&'a str>,
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

    top.push(splitter(SplitDir::Vertical, 0.5, status_pane, bottom_pane));
    vbox(top)
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
