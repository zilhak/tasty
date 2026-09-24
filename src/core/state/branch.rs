//! 상태바가 표시할 한 surface의 Git HEAD 캐시. GUI의 busy 폴링에서 갱신한다.
//! 매 렌더링마다 디스크를 읽지 않으며 원격 cwd를 로컬 경로로 해석하지 않는다.

use std::path::{Path, PathBuf};

use super::CoreState;

/// 브랜치 이름과 detached SHA를 구분한다. 표시용 접두사는 상태바가 붙인다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    Detached(String),
}

#[derive(Debug, Default)]
pub(crate) struct BranchCache {
    surface_id: Option<u32>,
    /// 읽기·파싱 실패나 저장소를 찾지 못한 결과도 None으로 캐시한다.
    branch: Option<HeadState>,
}

impl CoreState {
    /// cwd가 같아도 checkout 결과가 바뀔 수 있어 다시 읽는다.
    /// 대상 ID 또는 HEAD 값이 바뀌었으면 true이며 호출자가 redraw에 반영한다.
    pub(crate) fn refresh_status_bar_branch(&mut self, surface_id: Option<u32>) -> bool {
        let branch = surface_id
            .and_then(|sid| self.local_surface_cwd(sid))
            .and_then(|cwd| git_branch(&cwd));
        let changed =
            self.branch_cache.surface_id != surface_id || self.branch_cache.branch != branch;
        self.branch_cache = BranchCache { surface_id, branch };
        changed
    }

    /// 마지막으로 조회한 값. 대상이 다르거나 아직 조회하지 못했거나 조회가 실패하면 None이다.
    pub fn status_bar_branch(&self, surface_id: u32) -> Option<&HeadState> {
        if self.branch_cache.surface_id != Some(surface_id) {
            return None;
        }
        self.branch_cache.branch.as_ref()
    }
}

/// cwd부터 상위에서 .git/HEAD 또는 gitdir 파일을 따라 읽는다.
/// 읽기·파싱 실패와 저장소가 없는 경우를 반환값에서 구별하지 않는다.
fn git_branch(cwd: &Path) -> Option<HeadState> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let dot_git = d.join(".git");
        if let Ok(content) = std::fs::read_to_string(dot_git.join("HEAD")) {
            return parse_head(&content);
        }
        if let Ok(content) = std::fs::read_to_string(&dot_git)
            && let Some(gitdir) = resolve_gitdir_file(d, &content)
            && let Ok(head) = std::fs::read_to_string(gitdir.join("HEAD"))
        {
            return parse_head(&head);
        }
        dir = d.parent();
    }
    None
}

/// 상대 gitdir은 .git 파일이 있는 디렉터리를 기준으로 해석한다.
fn resolve_gitdir_file(base: &Path, content: &str) -> Option<PathBuf> {
    let raw = content.trim().strip_prefix("gitdir:")?.trim();
    if raw.is_empty() {
        return None;
    }
    let p = Path::new(raw);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    })
}

/// heads 참조는 브랜치로, 길이 하한 이상의 16진 문자열은 detached SHA로 읽는다.
/// 실제 객체 존재나 완전한 SHA 길이는 검사하지 않는다. 다른 ref와 파싱 실패는 None이다.
fn parse_head(content: &str) -> Option<HeadState> {
    let line = content.trim();
    if let Some(branch) = line.strip_prefix("ref: refs/heads/") {
        return Some(HeadState::Branch(branch.trim().to_owned()));
    }
    if line.starts_with("ref: ") {
        return None;
    }
    let sha_len = DETACHED_SHORT_SHA_LEN;
    if line.len() >= sha_len && line.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(HeadState::Detached(line[..sha_len].to_owned()));
    }
    None
}

const DETACHED_SHORT_SHA_LEN: usize = 7;

#[cfg(test)]
mod tests {
    use super::{HeadState, git_branch, parse_head, resolve_gitdir_file};
    use std::path::Path;

    #[test]
    fn head_parsing_extracts_branch_and_rejects_detached() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n"),
            Some(HeadState::Branch("main".into()))
        );
        assert_eq!(
            parse_head("ref: refs/heads/main  \n\n"),
            Some(HeadState::Branch("main".into()))
        );
        assert_eq!(
            parse_head("ref: refs/heads/feature/a/b\n"),
            Some(HeadState::Branch("feature/a/b".into()))
        );
        assert_eq!(
            parse_head("4af6ac9d4af6ac9d4af6ac9d4af6ac9d4af6ac9d\n"),
            Some(HeadState::Detached("4af6ac9".into()))
        );
        assert_eq!(parse_head("ref: refs/tags/v1.0\n"), None);
    }

    #[test]
    fn a_branch_that_looks_like_the_detached_marker_is_still_a_branch() {
        assert_eq!(
            parse_head("ref: refs/heads/@4af6ac9\n"),
            Some(HeadState::Branch("@4af6ac9".into()))
        );
        assert_eq!(
            parse_head("4af6ac9d4af6ac9d4af6ac9d4af6ac9d4af6ac9d\n"),
            Some(HeadState::Detached("4af6ac9".into()))
        );
    }

    #[test]
    fn gitdir_file_resolves_absolute_and_relative() {
        let base = Path::new("/repo/wt");
        let abs =
            resolve_gitdir_file(base, "gitdir: /main/.git/worktrees/wt\n").expect("절대 경로");
        assert_eq!(abs, Path::new("/main/.git/worktrees/wt"));
        let rel = resolve_gitdir_file(base, "gitdir: ../main/.git/worktrees/wt\n")
            .expect("상대 경로는 .git 이 있는 디렉토리 기준");
        assert_eq!(rel, Path::new("/repo/wt/../main/.git/worktrees/wt"));
        assert_eq!(resolve_gitdir_file(base, "not a gitdir file"), None);
        assert_eq!(resolve_gitdir_file(base, "gitdir:   \n"), None);
    }

    #[test]
    fn git_branch_finds_branch_in_plain_repo_from_subdir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        std::fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n").expect("HEAD");
        let deep = repo.join("a").join("b");
        std::fs::create_dir_all(&deep).expect("mkdir deep");
        assert_eq!(git_branch(&deep), Some(HeadState::Branch("main".into())));
    }

    #[test]
    fn git_branch_follows_worktree_gitdir_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let gitdir = tmp
            .path()
            .join("main")
            .join(".git")
            .join("worktrees")
            .join("wt");
        std::fs::create_dir_all(&gitdir).expect("mkdir gitdir");
        std::fs::write(
            gitdir.join("HEAD"),
            "ref: refs/heads/feature/worktree-support\n",
        )
        .expect("HEAD");

        let wt = tmp.path().join("wt");
        std::fs::create_dir_all(&wt).expect("mkdir wt");
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display()))
            .expect(".git file");

        assert_eq!(
            git_branch(&wt),
            Some(HeadState::Branch("feature/worktree-support".into()))
        );
    }

    #[test]
    fn git_branch_is_none_outside_a_repo() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let deep = tmp.path().join("x").join("y");
        std::fs::create_dir_all(&deep).expect("mkdir");
        // tempdir의 상위에도 .git이 없다는 환경 전제다.
        assert_eq!(git_branch(&deep), None);
    }
}
