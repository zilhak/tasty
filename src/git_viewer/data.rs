//! git2 래핑 — repo 탐색, status / log / diff 수집.
//!
//! 모든 함수는 **read-only**. mutate 작업(commit/stage/checkout 등) 없음.

use std::path::Path;

use anyhow::{Context, Result};
use git2::{Repository, StatusOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub status: FileStatus,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub oid_short: String,
    pub summary: String,
    pub author: String,
    pub time: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone)]
pub struct DiffData {
    pub file_path: String,
    pub hunks: Vec<DiffHunk>,
}

/// 주어진 경로에서 git repo 를 발견한다. 상위 디렉토리까지 자동 탐색.
pub fn discover_repo(start: &Path) -> Option<Repository> {
    Repository::discover(start)
        .map_err(|e| {
            tracing::debug!("git discover failed at {}: {e}", start.display());
            e
        })
        .ok()
}

pub fn collect_status(repo: &Repository) -> Result<Vec<StatusEntry>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);

    let statuses = repo
        .statuses(Some(&mut opts))
        .context("repo.statuses failed")?;

    let mut out = Vec::with_capacity(statuses.len());
    for entry in statuses.iter() {
        let path = match entry.path() {
            Some(p) => p.to_string(),
            None => continue,
        };
        let flags = entry.status();
        let status = if flags.is_conflicted() {
            FileStatus::Conflicted
        } else if flags.is_index_new() {
            FileStatus::Added
        } else if flags.is_wt_new() {
            FileStatus::Untracked
        } else if flags.is_index_deleted() || flags.is_wt_deleted() {
            FileStatus::Deleted
        } else if flags.is_index_renamed() || flags.is_wt_renamed() {
            FileStatus::Renamed
        } else if flags.is_index_modified()
            || flags.is_wt_modified()
            || flags.is_index_typechange()
            || flags.is_wt_typechange()
        {
            FileStatus::Modified
        } else {
            continue;
        };
        out.push(StatusEntry { status, path });
    }
    Ok(out)
}

pub fn collect_log(repo: &Repository, limit: usize) -> Result<Vec<LogEntry>> {
    // ref name 빠른 조회용 oid → refs 맵 미리 구성.
    let mut ref_map: std::collections::HashMap<git2::Oid, Vec<String>> =
        std::collections::HashMap::new();
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            if let Some(target) = r.target() {
                let name = r
                    .shorthand()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| r.name().unwrap_or("").to_string());
                if name.is_empty() {
                    continue;
                }
                ref_map.entry(target).or_default().push(name);
            }
        }
    }

    let mut walker = repo.revwalk().context("revwalk failed")?;
    walker
        .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .ok();
    if walker.push_head().is_err() {
        // 빈 repo (HEAD unborn 등)
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity(limit);
    for (i, oid) in walker.enumerate() {
        if i >= limit {
            break;
        }
        let oid = match oid {
            Ok(o) => o,
            Err(_) => continue,
        };
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let summary = commit.summary().unwrap_or("(no message)").to_string();
        let author = commit.author();
        let author_name = author.name().unwrap_or("(unknown)").to_string();
        let time = format_time(commit.time());
        let refs = ref_map.get(&oid).cloned().unwrap_or_default();
        out.push(LogEntry {
            oid_short: format!("{:.7}", oid),
            summary,
            author: author_name,
            time,
            refs,
        });
    }
    Ok(out)
}

pub fn collect_diff(repo: &Repository, path: &str) -> Result<DiffData> {
    let head_tree = repo.head().and_then(|r| r.peel_to_tree()).ok();

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(path).context_lines(3);

    let diff = match &head_tree {
        Some(tree) => repo
            .diff_tree_to_workdir_with_index(Some(tree), Some(&mut diff_opts))
            .context("diff_tree_to_workdir_with_index failed")?,
        None => repo
            .diff_tree_to_workdir_with_index(None, Some(&mut diff_opts))
            .context("diff_tree_to_workdir_with_index failed (no HEAD)")?,
    };

    let hunks: std::cell::RefCell<Vec<DiffHunk>> = std::cell::RefCell::new(Vec::new());
    diff.foreach(
        &mut |_, _| true,
        None,
        Some(&mut |_, hunk| {
            let header = std::str::from_utf8(hunk.header())
                .unwrap_or("")
                .trim_end()
                .to_string();
            hunks.borrow_mut().push(DiffHunk {
                header,
                lines: Vec::new(),
            });
            true
        }),
        Some(&mut |_, _, line| {
            let kind = match line.origin() {
                '+' => DiffLineKind::Addition,
                '-' => DiffLineKind::Deletion,
                _ => DiffLineKind::Context,
            };
            let content = std::str::from_utf8(line.content())
                .unwrap_or("")
                .trim_end_matches('\n')
                .to_string();
            let mut h = hunks.borrow_mut();
            if let Some(last) = h.last_mut() {
                last.lines.push(DiffLine {
                    kind,
                    content,
                    old_lineno: line.old_lineno(),
                    new_lineno: line.new_lineno(),
                });
            }
            true
        }),
    )
    .context("diff.foreach failed")?;

    Ok(DiffData {
        file_path: path.to_string(),
        hunks: hunks.into_inner(),
    })
}

/// chrono 미사용 — UTC 절대시각 포맷터.
fn format_time(t: git2::Time) -> String {
    let secs = t.seconds().max(0) as u64;
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let tod = secs % 86_400;
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

/// Howard Hinnant 의 civil_from_days 알고리즘. UTC 기준.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
