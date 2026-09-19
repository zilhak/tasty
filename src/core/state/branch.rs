//! StatusBar 의 git 브랜치 캐시 — 1Hz `Tick::Busy` 가 갱신하고 StatusBar 가 읽는다.
//!
//! ## 왜 캐시인가
//! 브랜치명은 예전에 `draw_status_bar` 안에서 리페인트마다 `.git/HEAD` 를 새로 열어
//! 읽었다. 데이터 추출이 렌더 함수에 묶여 있으면 추출 비용의 호출 빈도가 자동으로
//! 리페인트 빈도에 묶인다(`docs/dev-guide/model-view-split.md`). 바로 옆 필드인
//! 포그라운드 프로세스명(`foreground_names`)이 이미 같은 이유로 1Hz 캐시로 이관돼
//! 있어, 브랜치도 같은 티커에 편승시킨다. 대가는 최신성 — `git checkout` / `cd` 후
//! 표시가 최대 1초 늦는다(`foreground_name` 과 동일한 트레이드오프).
//!
//! ## 왜 focus surface 한 칸인가
//! StatusBar 는 **focus surface 하나**의 브랜치만 표시한다. `refresh_busy_surfaces`
//! 처럼 전 surface 를 순회하면 surface N 개 환경에서 초당 N 회 IO 가 되어 총 IO 가
//! 오히려 늘어난다. 그런데 focus 는 `AppState` 소유라(`CoreState` 는 focus 를 모른다)
//! 순회 안에서 좁힐 수 없어, 갱신 대상 surface 를 **호출자(`App::poll_busy_states`)가
//! 넘겨준다**. 캐시가 단일 슬롯이라 닫힌 surface 의 stale 엔트리 문제도 구조적으로
//! 생기지 않는다(매 tick 슬롯 통째 교체).
//!
//! 순회 대상이 terminal 로 한정되지 않는 것도 이 설계의 결과다 — cwd 는
//! [`CoreState::surface_cwd`] 가 terminal 이면 OSC 7 캐시(`get_cwd`), 그 외에는
//! `source_cwd()` 로 결정하므로 explorer 등 비-terminal surface 도 그대로 잡힌다.
//! 조회는 **로컬 출처만**(`local_surface_cwd`) 쓴다 — 브랜치 탐색은 로컬 디스크를 cwd
//! 부터 상위로 뒤지므로, mirror surface 의 원격 경로가 들어오면 엉뚱한 로컬 디렉토리를
//! 읽는다. 그래서 mirror surface 는 브랜치가 표시되지 않는다.
//!
//! ## headless 는 대상이 아니다
//! StatusBar 를 그리는 유일한 지점이 `gfx/gpu/egui_bridge.rs` 라
//! `--no-default-features` 빌드는 이 바를 렌더하지 않는다. 그래서 이 모듈 전체와
//! `CoreState::branch_cache` 필드는 `gui` feature 게이트이고, `boot.rs` 의
//! `Tick::Busy` 처리(headless)에는 갱신을 배선하지 않는다 — 읽는 쪽이 없는
//! 파일 IO 를 초당 한 번 도는 것이 되기 때문이다(의도적 비대칭).

use std::path::{Path, PathBuf};

use super::CoreState;

/// `HEAD` 가 가리키는 것 — **파싱 결과의 구조**다. 그리는 쪽의 표지는 여기 없다.
///
/// 두 갈래를 **값으로** 가르는 것이 이 타입의 전부다. 예전에는 둘 다 `String` 이었고
/// detached 를 가르는 유일한 표지가 값 안의 `@ ` 두 글자였다 — 그것은 파싱 결과가
/// 아니라 상태바가 붙이는 어휘다. 그 형태에서는 받는 쪽이 **문자열을 뜯어야** 두
/// 경우를 가를 수 있는데, `@` 로 시작하는 브랜치는 실제로 만들 수 있으므로
/// **문자열만으로는 애초에 못 가른다.**
///
/// "repo 가 아니다" 는 이 타입의 갈래가 아니라 바깥 `Option` 의 `None` 이다. 그 구분을
/// 접으면 안 된다 — 상태바의 브랜치 자리는 값이 없으면 **항목이 통째로 사라지는**
/// 자리라, detached 를 `None` 으로 주면 "repo 밖" 과 구별이 안 된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    /// `ref: refs/heads/<name>` — 이름 그대로.
    Branch(String),
    /// detached — short sha 만 담는다(표지 없음).
    Detached(String),
}

/// focus surface 한 칸짜리 브랜치 캐시. 매 1Hz tick 통째로 교체된다.
#[derive(Debug, Default)]
pub(crate) struct BranchCache {
    /// 이 값이 캐시된 대상 surface. 다른 surface 를 조회하면 `None` 을 돌려준다.
    surface_id: Option<u32>,
    /// HEAD 가 가리키는 것. **표시 표지는 안 들어 있다** — 붙이는 쪽은 상태바
    /// wrapper 다([`HeadState`] 문서).
    /// `None` 은 "아직 못 구함"이 아니라 **"repo 가 아니다"** 라는 확정 결과이며,
    /// 이 실패도 그대로 캐시된다 — 실패는 파일시스템 루트까지 올라가는 최악
    /// 케이스라 캐시하지 않으면 개선 효과가 사라진다.
    branch: Option<HeadState>,
}

impl CoreState {
    /// `surface_id` 의 브랜치를 재조회해 캐시를 교체한다. 반환값은 "표시가 바뀌었는가"
    /// = 호출자가 redraw 를 걸어야 하는가 — 이걸 `mark_dirty` 조건에 합류시키지 않으면
    /// `git checkout` 후 다음 리페인트 유발 이벤트가 올 때까지 옛 값이 남는다.
    ///
    /// 매 tick 재조회한다(cwd 가 그대로여도) — 같은 디렉토리 안에서의 `git checkout`
    /// 을 1초 안에 반영하려면 cwd 변화 감지만으로는 부족하기 때문이다. 비용은
    /// 초당 `.git/HEAD` 1~2 회 open 이다.
    pub(crate) fn refresh_status_bar_branch(&mut self, surface_id: Option<u32>) -> bool {
        let branch = surface_id
            .and_then(|sid| self.local_surface_cwd(sid))
            .and_then(|cwd| git_branch(&cwd));
        let changed =
            self.branch_cache.surface_id != surface_id || self.branch_cache.branch != branch;
        self.branch_cache = BranchCache { surface_id, branch };
        changed
    }

    /// 캐시된 HEAD 상태(마지막 `Tick::Busy` 가 조회한 값). StatusBar 가 매 프레임
    /// `.git/HEAD` 를 다시 여는 대신 이걸 읽는다. 캐시 대상이 아닌 surface, repo 밖,
    /// 그리고 첫 tick 전(≤1초)에는 `None`.
    ///
    /// 돌려주는 것은 **구조지 표시 문자열이 아니다** — detached 표지를 붙이는 것은
    /// 그리는 쪽의 일이다([`HeadState`]).
    pub fn status_bar_branch(&self, surface_id: u32) -> Option<&HeadState> {
        if self.branch_cache.surface_id != Some(surface_id) {
            return None;
        }
        self.branch_cache.branch.as_ref()
    }
}

/// cwd 기준 git HEAD 상태. `.git` 을 cwd 부터 상위로 올라가며 찾아 파싱한다
/// (git 바이너리/libgit2 비의존, `std::fs` 만 — 크로스플랫폼). repo 가 아니면 `None`.
///
/// `.git` 은 두 형태를 모두 지원한다:
/// - **디렉토리**(일반 clone) → `<dir>/.git/HEAD`
/// - **파일**(worktree / submodule) → 내용의 `gitdir: <경로>` 를 따라가 `<gitdir>/HEAD`.
///   이 프로젝트는 병렬 작업에 worktree 를 상시 쓰므로 드문 예외가 아니라 일상 경로다.
fn git_branch(cwd: &Path) -> Option<HeadState> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let dot_git = d.join(".git");
        // 일반 repo: `.git` 이 디렉토리.
        if let Ok(content) = std::fs::read_to_string(dot_git.join("HEAD")) {
            return parse_head(&content);
        }
        // worktree / submodule: `.git` 이 gitdir 경로를 담은 파일.
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

/// `.git` **파일**(worktree/submodule)의 내용에서 gitdir 경로를 뽑는다. 상대 경로는
/// `.git` 이 있는 디렉토리 기준으로 해석한다(git 의 규칙).
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

/// `HEAD` 파일 한 줄을 [`HeadState`] 로 파싱. **표시 문자열을 만들지 않는다.**
///
/// - `ref: refs/heads/<branch>` → `Some(HeadState::Branch(branch))`
/// - detached(40자 SHA 직접 기록) → `Some(HeadState::Detached(<short sha>))`
/// - 그 밖(`refs/tags/...` 등) → `None`
///
/// detached 를 `None` 으로 접지 않는 이유: 상태바의 브랜치 자리는 **값이 없으면 항목이
/// 통째로 사라지는** 자리라, detached 를 `None` 으로 주면 "repo 밖" 과 구별이 안 된다.
/// 그래서 그 자리에 short sha 를 보인다 — 동작 명세는
/// `docs/features/workspace-status-bar/index.md` 의 "표시 데이터".
///
/// 후행 개행/공백은 값에 섞이지 않는다.
fn parse_head(content: &str) -> Option<HeadState> {
    let line = content.trim();
    if let Some(branch) = line.strip_prefix("ref: refs/heads/") {
        return Some(HeadState::Branch(branch.trim().to_owned()));
    }
    // ref 가 아니면 detached — HEAD 가 커밋 SHA 를 직접 담는다.
    if line.starts_with("ref: ") {
        return None;
    }
    let sha_len = DETACHED_SHORT_SHA_LEN;
    if line.len() >= sha_len && line.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(HeadState::Detached(line[..sha_len].to_owned()));
    }
    None
}

/// detached HEAD 표시에 쓰는 short sha 길이. git 의 기본 축약과 같은 7 이다.
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
        // 후행 개행/공백이 있어도 브랜치명에 섞이지 않는다.
        assert_eq!(
            parse_head("ref: refs/heads/main  \n\n"),
            Some(HeadState::Branch("main".into()))
        );
        // 슬래시 포함 브랜치명이 온전히 나온다.
        assert_eq!(
            parse_head("ref: refs/heads/feature/a/b\n"),
            Some(HeadState::Branch("feature/a/b".into()))
        );
        // detached: 40자 SHA 직접 기록 → short sha(7자) **만**. 표지는 안 붙는다.
        assert_eq!(
            parse_head("4af6ac9d4af6ac9d4af6ac9d4af6ac9d4af6ac9d\n"),
            Some(HeadState::Detached("4af6ac9".into()))
        );
        // refs/tags 등 브랜치가 아닌 ref → None.
        assert_eq!(parse_head("ref: refs/tags/v1.0\n"), None);
    }

    /// 이 모듈이 돌려주는 값에서 **문자열을 뜯지 않고** 두 갈래를 가를 수 있어야 한다.
    ///
    /// 표시 문자열로도 오늘은 안 겹친다 — 다만 그 이유가 표지에 **공백**이 들어 있는
    /// 것뿐이고, git 은 ref 이름에 공백을 금지한다(실측 2026-09-20: `git check-ref-format
    /// --branch` 가 `@ 4af6ac9` 는 거부, `@4af6ac9` 는 허용, 후자는 실제로 생성된다).
    /// 즉 안 겹치는 것이 **표지의 생김새에 걸려 있고**, 그 값은 디자인이 정한다 — 공백이
    /// 빠지면 그날로 겹친다. 그래서 갈래를 문자열이 아니라 타입으로 둔다.
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

    /// 일반 repo(`.git` 디렉토리): cwd 하위 디렉토리에서 시작해도 상위 순회로 찾는다.
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

    /// worktree(`.git` 파일 + `gitdir:`): 수정 전에는 `.git/HEAD` 가 `ENOTDIR` 로 깨져
    /// 브랜치가 아예 표시되지 않았다.
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

    /// repo 밖(git 이 없는 격리 디렉토리) → `None`. 이 실패 결과도 캐시 대상이다.
    #[test]
    fn git_branch_is_none_outside_a_repo() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let deep = tmp.path().join("x").join("y");
        std::fs::create_dir_all(&deep).expect("mkdir");
        // tempdir 상위(/tmp 등)에 .git 이 없다는 전제 — 표준 환경에서 성립한다.
        assert_eq!(git_branch(&deep), None);
    }
}
