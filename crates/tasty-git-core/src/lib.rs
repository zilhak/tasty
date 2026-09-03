//! git2 래핑 — repo 탐색, status / log / diff 수집.
//!
//! 모든 함수는 **read-only**. mutate 작업(commit/stage/checkout 등) 없음.
//!
//! `tasty-plugin-git-viewer`(로컬 프로세스 내 직접 호출, 원격 attach 모드에서는 host
//! 가 회신한 wire JSON 을 이 crate 의 타입으로 역직렬화)와 host core
//! `src/core/attach_runtime.rs::handle_git_query_request`(원격 attach 세션의 git
//! 조회 — host 는 `serde_json::json!` 로 직접 조립하고 `Serialize` 는 쓰지 않는다)
//! 양쪽이 공유하는 순수 로직 crate. 의존은 `git2` + `anyhow` + `tasty-utils` +
//! `serde`(plugin 원격 모드의 역직렬화 전용, plugin SDK/protocol 타입 비의존).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{Repository, StatusOptions, WorktreeLockStatus};
use serde::Deserialize;
use tasty_utils::path::strip_verbatim_prefix;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusEntry {
    pub status: FileStatus,
    pub path: String,
}

/// 커밋 한 줄. `summary` / `author` 가 **빈 문자열이면 git 에 값이 없다는 뜻**이다
/// (메시지 없는 커밋, 이름 없는 작성자). 이 crate 는 `tasty-i18n` 을 의존하지 않는
/// 데이터 crate 라 그 자리에 보여줄 자연어를 만들지 않는다 — 표시 문구는 소비자가
/// 고른다(git-viewer plugin 은 자기 `Translator`, host 는 `t()`). 원격 attach 조회도
/// 같은 wire(`attach_runtime::log_entry_wire`)를 타므로 빈 값은 빈 값 그대로 건너간다.
/// 근거: `docs/adr/0106-non-widget-user-strings-go-through-i18n.md` 결정 4.
#[derive(Debug, Clone, Deserialize)]
pub struct LogEntry {
    pub oid_short: String,
    /// 첫 문단 요약. 없으면 빈 문자열.
    pub summary: String,
    /// 작성자 이름. 없으면 빈 문자열.
    pub author: String,
    pub time: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiffData {
    pub file_path: String,
    pub hunks: Vec<DiffHunk>,
}

/// 하나의 worktree(읽기 전용 종합 목록의 한 항목).
///
/// libgit2 의 `worktrees()` 는 **linked worktree 만** 주므로 main working tree 는
/// 별도로 합성해 목록 선두에 넣는다 (`git worktree list` 와 동등한 종합 목록).
#[derive(Debug, Clone, Deserialize)]
pub struct WorktreeEntry {
    /// 표시 이름 — 디렉토리 basename (linked 는 worktree 이름과 동일).
    pub name: String,
    /// working tree 최상위 경로 (verbatim prefix 제거 후 저장; 재열기·표시 공용).
    pub path: PathBuf,
    /// 브랜치 shorthand — detached(HEAD) / unborn 이면 None. context strip 은 None 을
    /// "detached" 로 표시한다.
    pub branch: Option<String>,
    /// HEAD short oid (7자). HEAD 를 못 읽으면 None. (rail 2번째 줄 · context strip 공용)
    pub oid: Option<String>,
    /// main working tree 인가.
    pub is_main: bool,
    /// popup 이 받은 cwd 가 속한 worktree 인가.
    pub is_current: bool,
    /// 잠금 상태(`git worktree lock`).
    pub locked: bool,
    /// 잠금 사유(있으면).
    pub lock_reason: Option<String>,
    /// fs 상 유효한가 (경로 존재·메타데이터 정합). false 면 전환 불가.
    pub is_valid: bool,
}

/// 경로 동등 비교용 정규화. canonicalize 실패(경로 소실 등) 시 원본 반환.
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// 표시·재열기용 경로 — Windows verbatim prefix(`\\?\`) 제거.
fn display_path(p: &Path) -> PathBuf {
    PathBuf::from(strip_verbatim_prefix(&p.to_string_lossy()))
}

/// 디렉토리 basename 을 표시 이름으로. 추출 실패 시 `fallback`.
fn dir_name(p: &Path, fallback: &str) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// HEAD 정보 — (브랜치 shorthand, short oid). detached/unborn 이면 branch=None.
/// HEAD 를 못 읽으면 (None, None).
fn head_info(repo: &Repository) -> (Option<String>, Option<String>) {
    let Ok(head) = repo.head() else {
        return (None, None);
    };
    let branch = head
        .shorthand()
        .filter(|sh| *sh != "HEAD")
        .map(|sh| sh.to_string());
    let oid = head.target().map(|oid| format!("{oid:.7}"));
    (branch, oid)
}

/// linked worktree 의 공유 `.git`(common dir) 으로부터 main working tree 도출.
///
/// git2 0.19 에 `commondir()` accessor 가 없어 직접 끌어낸다.
/// - (B) `repo.path()/commondir` 파일(공유 `.git` 으로의 상대경로) 우선 — git 표준.
/// - (A) 경로 추론 폴백 — `<main>/.git/worktrees/<name>` → 조부모(`.git`)의 부모.
///
/// 둘 다 실패하면 None → 호출부가 main 항목을 생략(graceful degrade).
fn derive_main_workdir(repo: &Repository) -> Option<PathBuf> {
    if !repo.is_worktree() {
        // 현재가 main working tree → workdir 이 곧 main.
        return repo.workdir().map(|p| p.to_path_buf());
    }
    let git_dir = repo.path(); // <main>/.git/worktrees/<name>/
    main_workdir_via_commondir(git_dir).or_else(|| main_workdir_via_path(git_dir))
}

fn main_workdir_via_commondir(git_dir: &Path) -> Option<PathBuf> {
    let rel = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
    let rel = rel.trim();
    if rel.is_empty() {
        return None;
    }
    // 공유 `.git` 디렉토리. canonicalize 로 `..` 정리.
    let shared_git = canon(&git_dir.join(rel));
    shared_git.parent().map(|p| p.to_path_buf())
}

fn main_workdir_via_path(git_dir: &Path) -> Option<PathBuf> {
    // git_dir = <main>/.git/worktrees/<name>
    let worktrees = git_dir.parent()?; // <main>/.git/worktrees
    let dot_git = worktrees.parent()?; // <main>/.git
    dot_git.parent().map(|p| p.to_path_buf()) // <main>
}

/// main working tree + 모든 linked worktree 의 종합 목록을 수집한다 (읽기 전용).
///
/// `current_workdir` = popup 이 받은 cwd 에서 discover 한 repo 의 workdir.
/// 각 항목의 `is_current` 는 이 경로와의 정규화 비교로 판정한다.
///
/// 부분 실패(개별 worktree head 못 읽음 등)는 그 항목만 degrade 하고 전체는 Ok. 항목별
/// 조립은 `main_worktree_entry`/`linked_worktree_entry` 로 분리돼 있다(clippy 복잡도 게이트 —
/// `docs/adr/0037-complexity-gate.md`).
pub fn collect_worktrees(repo: &Repository, current_workdir: &Path) -> Result<Vec<WorktreeEntry>> {
    let current_canon = canon(current_workdir);
    let mut out: Vec<WorktreeEntry> = Vec::new();
    // 이미 담은 항목의 **정규화 경로**. 중복 검사(아래 비표준 레이아웃 방어)가 목록을
    // 훑을 때마다 `canonicalize` 를 다시 부르면 항목 수의 제곱에 비례하는 syscall 이
    // 난다 — 항목마다 어차피 한 번 재는 값을 여기 모아 그대로 비교한다.
    let mut seen_canon: Vec<PathBuf> = Vec::new();

    // 1) main working tree 합성 (libgit2 worktrees() 가 안 줌).
    if let Some((entry, entry_canon)) = main_worktree_entry(repo, &current_canon) {
        out.push(entry);
        seen_canon.push(entry_canon);
    }

    // 2) linked worktrees.
    if let Ok(names) = repo.worktrees() {
        for name in names.iter().flatten() {
            if let Some((entry, entry_canon)) =
                linked_worktree_entry(repo, name, &current_canon, &seen_canon)
            {
                out.push(entry);
                seen_canon.push(entry_canon);
            }
        }
    }

    Ok(out)
}

/// main working tree 항목 합성. `derive_main_workdir` 이 못 찾으면(비표준 레이아웃 등) None.
/// 반환값의 두 번째 원소는 이 항목의 정규화 경로 — 호출자가 중복 검사에 재사용한다.
fn main_worktree_entry(
    repo: &Repository,
    current_canon: &Path,
) -> Option<(WorktreeEntry, PathBuf)> {
    let main_wd = derive_main_workdir(repo)?;
    let is_valid = main_wd.is_dir();
    let main_canon = canon(&main_wd);
    let (branch, oid) = Repository::open(&main_wd)
        .ok()
        .map(|r| head_info(&r))
        .unwrap_or((None, None));
    let entry = WorktreeEntry {
        name: dir_name(&main_wd, "main"),
        branch,
        oid,
        is_main: true,
        is_current: main_canon == current_canon,
        locked: false,
        lock_reason: None,
        is_valid,
        path: display_path(&main_wd),
    };
    Some((entry, main_canon))
}

/// 단일 linked worktree 항목 조회. 중복(main 합성분과 경로 중복) · lookup 실패 시 `None`.
/// `seen_canon` 은 이미 목록에 담긴 항목들의 정규화 경로이며, 반환값의 두 번째 원소는
/// 이 항목의 정규화 경로다(호출자가 `seen_canon` 에 누적한다).
fn linked_worktree_entry(
    repo: &Repository,
    name: &str,
    current_canon: &Path,
    seen_canon: &[PathBuf],
) -> Option<(WorktreeEntry, PathBuf)> {
    let wt = match repo.find_worktree(name) {
        Ok(w) => w,
        Err(e) => {
            tracing::debug!("find_worktree {name} failed: {e}");
            return None;
        }
    };
    let wt_path = wt.path().to_path_buf();
    let wt_canon = canon(&wt_path);
    // main 합성분과 경로 중복 방지 (비표준 레이아웃 방어). 비교 대상은 이미 재 둔
    // 정규화 경로라 여기서 canonicalize 를 다시 부르지 않는다.
    if seen_canon.contains(&wt_canon) {
        return None;
    }
    let is_valid = wt.validate().is_ok();
    let (locked, lock_reason) = match wt.is_locked() {
        Ok(WorktreeLockStatus::Locked(reason)) => (true, reason),
        Ok(WorktreeLockStatus::Unlocked) => (false, None),
        Err(e) => {
            tracing::debug!("is_locked {name} failed: {e}");
            (false, None)
        }
    };
    // head 는 유효한 worktree 만 읽고 핸들은 바로 drop (목록 단계는 status/log 안 읽음).
    let (branch, oid) = if is_valid {
        Repository::open_from_worktree(&wt)
            .ok()
            .map(|r| head_info(&r))
            .unwrap_or((None, None))
    } else {
        (None, None)
    };
    let entry = WorktreeEntry {
        name: name.to_string(),
        branch,
        oid,
        is_main: false,
        is_current: wt_canon == current_canon,
        locked,
        lock_reason,
        is_valid,
        path: display_path(&wt_path),
    };
    Some((entry, wt_canon))
}

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
    let mut walker = repo.revwalk().context("revwalk failed")?;
    // 정렬 설정 실패는 치명적이지 않다 — libgit2 기본 순서로 그대로 진행한다.
    walker
        .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .ok();
    if walker.push_head().is_err() {
        // unborn HEAD — 걸을 커밋이 없으니 아래 ref 스캔도 부질없다.
        return Ok(Vec::new());
    }

    // oid → ref 이름 맵. **호출마다 새로 만든다** — ref 는 커밋/브랜치 조작 한 번으로
    // 바뀌고, 이 함수가 불리는 시점이 곧 "최신 상태를 보여달라"(popup open / Refresh /
    // worktree 전환)는 순간이라 캐시는 낡은 pill 을 띄울 위험만 만든다. 비용도
    // ref 개수에 선형이라 아래 revwalk 대비 미미하다.
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
        // 없음은 빈 문자열로 — 자연어 폴백은 소비자(plugin Translator / host t()) 몫이다.
        // `LogEntry` doc 참조.
        let summary = commit.summary().unwrap_or_default().to_string();
        let author = commit.author();
        let author_name = author.name().unwrap_or_default().to_string();
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

/// chrono 미사용 — UTC 절대시각 포맷터. Howard Hinnant 의 civil_from_days 알고리즘.
fn format_time(t: git2::Time) -> String {
    let secs = t.seconds().max(0) as u64;
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let tod = secs % 86_400;
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 메시지 없는 커밋·이름 없는 작성자는 **빈 문자열**로 전달된다 — 이 crate 는 자연어
    /// 폴백을 만들지 않는다(표시 문구는 소비자 몫, ADR-0106 결정 4).
    #[test]
    fn collect_log_leaves_missing_summary_and_author_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        let tree_oid = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();

        // 1) 빈 메시지 커밋 — libgit2 는 만들어 주고 `summary()` 는 `Some("")` 을 돌려준다
        //    (`git_commit_summary` 가 빈 요약을 "" 로 strdup). `None` 은 메시지가 UTF-8 이
        //    아닐 때뿐이다 — 어느 쪽이든 `LogEntry.summary` 는 빈 문자열로 전달돼야 한다.
        let c1 = repo
            .commit(Some("HEAD"), &sig, &sig, "", &tree, &[])
            .unwrap();

        // 2) 이름 없는 작성자 — `Signature::new` 는 빈 이름을 거부하므로 raw commit
        //    object 를 직접 써서 만든다(파서는 관대해 읽기는 된다).
        let raw = format!(
            "tree {tree_oid}\nparent {c1}\nauthor  <nobody@example.com> 0 +0000\n\
             committer test <test@example.com> 0 +0000\n\nhas message\n"
        );
        let c2 = repo
            .odb()
            .unwrap()
            .write(git2::ObjectType::Commit, raw.as_bytes())
            .unwrap();
        repo.set_head_detached(c2).unwrap();

        let log = collect_log(&repo, 10).unwrap();
        assert_eq!(log.len(), 2, "{log:?}");
        assert_eq!(log[0].summary, "has message");
        assert_eq!(
            log[0].author, "",
            "이름 없는 작성자는 빈 문자열: {:?}",
            log[0]
        );
        assert_eq!(log[1].summary, "", "빈 메시지는 빈 문자열: {:?}", log[1]);
        assert_eq!(log[1].author, "test");
    }

    /// 임시 repo + linked worktree 를 만들어 종합 목록 수집을 검증한다.
    /// (런타임 확인 사항: `worktrees()` 형제 나열 + main 합성 + is_current 판정.)
    #[test]
    fn collect_worktrees_lists_main_and_linked() {
        let tmp = tempfile::tempdir().unwrap();
        let main_dir = tmp.path().join("main");
        std::fs::create_dir_all(&main_dir).unwrap();

        // main repo 초기화 + 최초 커밋(worktree 생성에 HEAD 필요).
        let repo = Repository::init(&main_dir).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "test").unwrap();
            cfg.set_str("user.email", "test@example.com").unwrap();
        }
        std::fs::write(main_dir.join("a.txt"), "hello").unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        let tree_oid = {
            let mut idx = repo.index().unwrap();
            idx.add_path(Path::new("a.txt")).unwrap();
            idx.write().unwrap();
            idx.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // linked worktree 생성.
        let wt_dir = tmp.path().join("linked-wt");
        repo.worktree("linked-wt", &wt_dir, None).unwrap();

        // main repo 관점: main(현재) + linked 가 모두 보여야 한다.
        let main_wd = repo.workdir().unwrap().to_path_buf();
        let list = collect_worktrees(&repo, &main_wd).unwrap();
        assert!(
            list.iter().any(|e| e.is_main && e.is_current),
            "main 항목이 is_main + is_current 여야 함: {list:?}"
        );
        assert!(
            list.iter().any(|e| !e.is_main && e.name == "linked-wt"),
            "linked worktree 가 목록에 있어야 함: {list:?}"
        );

        // linked worktree 관점: 형제 나열 + linked 가 is_current 여야 한다.
        let linked_repo = Repository::open(&wt_dir).unwrap();
        assert!(linked_repo.is_worktree());
        let linked_wd = linked_repo.workdir().unwrap().to_path_buf();
        let list2 = collect_worktrees(&linked_repo, &linked_wd).unwrap();
        assert!(
            list2.iter().any(|e| e.is_main && !e.is_current),
            "linked 관점에서 main 은 is_main 이고 current 아님: {list2:?}"
        );
        assert!(
            list2
                .iter()
                .any(|e| e.name == "linked-wt" && e.is_current && !e.is_main),
            "linked worktree 가 is_current 여야 함: {list2:?}"
        );
    }

    /// 결과 고정 — worktree 여러 개에서 목록의 불변식이 유지되는지 본다.
    ///
    /// 중복 검사는 항목마다 `canonicalize` 를 다시 부르지 않고 미리 잰 정규화 경로를
    /// 비교한다(항목 수에 선형). 그 비교의 결과 — 중복 없음 · main 정확히 1개 ·
    /// current 정확히 1개 · main 이 선두 — 를 고정한다.
    #[test]
    fn collect_worktrees_result_invariants_with_multiple_linked() {
        let tmp = tempfile::tempdir().unwrap();
        let main_dir = tmp.path().join("main");
        std::fs::create_dir_all(&main_dir).unwrap();

        let repo = Repository::init(&main_dir).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "test").unwrap();
            cfg.set_str("user.email", "test@example.com").unwrap();
        }
        std::fs::write(main_dir.join("a.txt"), "hello").unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        let tree_oid = {
            let mut idx = repo.index().unwrap();
            idx.add_path(Path::new("a.txt")).unwrap();
            idx.write().unwrap();
            idx.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let names = ["wt-a", "wt-b", "wt-c"];
        for name in names {
            repo.worktree(name, &tmp.path().join(name), None).unwrap();
        }

        let main_wd = repo.workdir().unwrap().to_path_buf();
        let list = collect_worktrees(&repo, &main_wd).unwrap();

        assert_eq!(list.len(), 1 + names.len(), "main + linked 전부: {list:?}");
        assert!(list[0].is_main, "main 항목이 선두여야 함: {list:?}");
        assert_eq!(
            list.iter().filter(|e| e.is_main).count(),
            1,
            "is_main 은 정확히 1개: {list:?}"
        );
        assert_eq!(
            list.iter().filter(|e| e.is_current).count(),
            1,
            "is_current 는 정확히 1개: {list:?}"
        );

        // 경로 중복이 없어야 한다 (비표준 레이아웃 방어의 본래 목적).
        let mut canons: Vec<PathBuf> = list.iter().map(|e| canon(&e.path)).collect();
        canons.sort();
        let before = canons.len();
        canons.dedup();
        assert_eq!(before, canons.len(), "정규화 경로 중복 없음: {list:?}");

        // linked 이름이 전부 살아 있고 각각 유효해야 한다.
        for name in names {
            let e = list
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} 항목이 있어야 함: {list:?}"));
            assert!(!e.is_main, "{name} 은 linked: {e:?}");
            assert!(e.is_valid, "{name} 은 유효해야 함: {e:?}");
        }
    }
}
