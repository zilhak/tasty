//! 시험이 띄우는 자식 프로세스에서 저장소를 지정하는 Git 환경변수를 지운다.
//!
//! `git rebase --exec`나 훅 안에서 시험을 돌리면 부모가 GIT_DIR·GIT_INDEX_FILE 등을 넘긴다.
//! 그대로 상속한 자식이 임시 폴더에서 `git init --bare`나 `git commit`을 실행하면 임시 저장소가
//! 아니라 바깥 저장소의 config·index·이력을 바꾼다. 지울 목록은 Git 자신이 알려 주는
//! `git rev-parse --local-env-vars`를 쓴다. 작성자·커미터 같은 다른 GIT_* 값은 남긴다.
//!
//! 바깥 저장소를 읽으려고 그 작업 트리에서 git을 실행하는 자리는 대상이 아니다.
//! 넘어온 값이 같은 저장소를 가리키므로 그대로 둔다.

use std::ffi::OsStr;
use std::process::Command;
use std::sync::OnceLock;

/// 이 Git이 저장소 지정 변수로 보는 이름. 한 번만 묻는다.
/// Git을 실행할 수 없으면 빈 목록이다. 그 환경에서는 자식도 git을 실행하지 못한다.
pub fn local_env_vars() -> &'static [String] {
    static VARS: OnceLock<Vec<String>> = OnceLock::new();
    VARS.get_or_init(|| {
        let out = match Command::new("git")
            .args(["rev-parse", "--local-env-vars"])
            .output()
        {
            Ok(out) => out,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => panic!("git rev-parse --local-env-vars 를 실행하지 못했다: {e}"),
        };
        assert!(
            out.status.success(),
            "git rev-parse --local-env-vars 가 실패했다: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        let vars: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        assert!(
            vars.iter().any(|v| v == "GIT_DIR"),
            "git rev-parse --local-env-vars 출력에 GIT_DIR 가 없다: {vars:?}"
        );
        vars
    })
}

/// `cmd`가 부모의 저장소 지정 변수를 상속하지 않게 한다.
pub fn isolate(cmd: &mut Command) -> &mut Command {
    for name in local_env_vars() {
        cmd.env_remove(name);
    }
    cmd
}

/// 저장소 지정 변수를 지운 `Command::new(program)`.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    isolate(&mut cmd);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp_scratch::Scratch;
    use std::path::Path;

    fn git(cmd: Command, dir: &Path, args: &[&str]) {
        let mut cmd = cmd;
        let out = cmd
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 바깥 저장소의 GIT_DIR 를 자식에게 직접 넘긴다. linked worktree 에서 `git rebase --exec`가
    /// 넘기는 것과 같은 형태다. 시험 프로세스 자신의 환경은 바꾸지 않는다.
    fn leaking(program: Command, outer: &Path) -> Command {
        let mut cmd = program;
        cmd.env("GIT_DIR", outer.join(".git"));
        cmd
    }

    struct Outer {
        _scratch: Scratch,
        root: std::path::PathBuf,
        child: std::path::PathBuf,
        config: Vec<u8>,
        index: Vec<u8>,
    }

    fn outer_repo(tag: &str) -> Outer {
        let scratch = Scratch::new(tag);
        let root = scratch.path().join("outer");
        let child = scratch.path().join("child");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        git(command("git"), &root, &["init", "-q"]);
        std::fs::write(root.join("f"), "outer\n").unwrap();
        git(command("git"), &root, &["add", "f"]);
        git(
            command("git"),
            &root,
            &["-c", "commit.gpgsign=false", "commit", "-qm", "outer"],
        );
        let config = std::fs::read(root.join(".git/config")).unwrap();
        let index = std::fs::read(root.join(".git/index")).unwrap();
        Outer {
            _scratch: scratch,
            root,
            child,
            config,
            index,
        }
    }

    impl Outer {
        fn unchanged(&self) -> bool {
            std::fs::read(self.root.join(".git/config")).unwrap() == self.config
                && std::fs::read(self.root.join(".git/index")).unwrap() == self.index
        }
    }

    #[test]
    fn the_list_names_the_variables_that_redirect_a_repository() {
        let vars = local_env_vars();
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
        ] {
            assert!(vars.iter().any(|v| v == name), "{name} 가 없다: {vars:?}");
        }
        assert!(!vars.iter().any(|v| v == "GIT_AUTHOR_NAME"));
    }

    #[test]
    fn an_isolated_child_initializes_its_own_repository() {
        let o = outer_repo("git-env-isolated");
        let mut bare = leaking(Command::new("git"), &o.root);
        isolate(&mut bare);
        git(bare, &o.child, &["init", "-q", "--bare"]);
        let mut work = leaking(Command::new("git"), &o.root);
        isolate(&mut work);
        git(work, &o.child, &["config", "core.bare", "false"]);
        assert!(o.unchanged(), "바깥 저장소의 config·index 가 바뀌었다");
        assert!(
            o.child.join("config").is_file(),
            "자식 저장소가 만들어지지 않았다"
        );
    }

    /// 헬퍼 없이 같은 명령을 실행하면 바깥 저장소가 바뀌는지 확인한다. 위 시험이 의미 있는 이유다.
    #[test]
    fn without_isolation_the_same_child_rewrites_the_outer_repository() {
        let o = outer_repo("git-env-leaking");
        git(
            leaking(Command::new("git"), &o.root),
            &o.child,
            &["init", "-q", "--bare"],
        );
        assert!(!o.unchanged(), "넘어온 GIT_DIR 로도 바깥 저장소가 그대로다");
        assert!(!o.child.join("config").exists());
    }
}
