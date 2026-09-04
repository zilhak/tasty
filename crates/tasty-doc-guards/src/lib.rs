//! 문서를 읽는 가드들의 집 — **의존이 0 인 것이 이 크레이트의 존재 이유다.**
//!
//! 여기 사는 통합 타깃은 `docs/` · `site/` · `*.md` 를 읽고 소스·워크플로 텍스트와
//! 대조한다. 크레이트 코드는 한 줄도 안 쓴다. 본체 패키지에 얹혀 있을 때 그 가드들의
//! 유일한 자동 채널은 `check-headless` 였는데, 그 잡은
//! `paths-ignore: docs/** · site/** · **/*.md` 뒤에 있다 — **문서만 바뀐 push 에서
//! 정확히 꺼진다.** 즉 그 가드들을 위반할 수 있는 유일한 변경에서만 안 돌았다.
//!
//! 경로 필터를 그냥 떼면 문서 한 줄 고칠 때마다 본체 컴파일(수백 크레이트)이 붙는다.
//! 그래서 필터를 떼는 대신 **잡을 싸게 만들었다** — 의존 0 이면 콜드 빌드가 1 초 미만이라
//! 필터가 필요 없다. 배경·대안·재검토 트리거는 ADR-0138.
//!
//! 여기에 의존을 하나라도 더하면 그 결정의 전제가 사라진다. `Cargo.toml` 의
//! `[dependencies]` 는 비어 있어야 하고, `the_crate_has_no_dependencies` 가 그것을 본다.

use std::path::{Path, PathBuf};

/// 레포 루트. 이 크레이트가 워크스페이스 루트가 아니라 `crates/<이름>` 아래 살기 때문에
/// `CARGO_MANIFEST_DIR` 이 곧 레포 루트가 아니다 — 두 칸 올라간다.
///
/// **틀린 루트로 조용히 진행하지 않는다.** 스캔 가드에서 경로가 틀어지면 예외가 아니라
/// **조용한 0** 이 나오고, 0 인 모수는 언제나 초록이다(ADR-0133). 그래서 올라간 자리가
/// 레포 루트가 맞는지 표지 파일로 확인하고, 아니면 panic 한다. 여기 사는 일곱 타깃이
/// 전부 이 함수를 쓰므로 확인 지점은 하나면 된다.
///
/// 표지는 **이 크레이트 밖에 있고 지워질 리 없는 것**으로 고른다 — `Cargo.toml` 만 보면
/// 이 크레이트 자신의 디렉토리도 통과한다.
pub fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR 에서 두 칸 올라갈 수 없다"))
        .to_path_buf();

    for marker in [
        "Cargo.toml",
        "CHANGELOG.md",
        "docs/adr/index.md",
        ".github/workflows",
    ] {
        assert!(
            root.join(marker).exists(),
            "레포 루트로 잡은 {} 에 표지 {marker} 가 없다 — 경로가 틀어졌다. \
             그냥 진행하면 이 크레이트의 가드 전부가 빈 모수로 조용히 초록이 된다.",
            root.display()
        );
    }
    root
}

/// `CACHEDIR.TAG` 의 서명 줄. 규격이 정한 값이라 도구가 달라도 같다 — cargo·ccache·
/// bazel 등이 모두 이 문자열로 캐시 디렉토리를 표시한다.
const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// 이 디렉토리가 **빌드 캐시인가** — 이름이 아니라 표식으로 판정한다.
///
/// 스캔 가드는 빌드 산출물을 건너뛰어야 한다. 종전 수단은 디렉토리 **이름**(`"target"`)
/// 이었는데 이름은 성질이 아니다: `CARGO_TARGET_DIR` 로 다른 이름을 주면 그 디렉토리가
/// 통째로 모수에 들어온다. 실측(2026-09-05)으로 그 상태를 만들었을 때 이 크레이트의
/// `no_todo_file_citation` 은 1.30s → 86.30s (66 배), 루트의
/// `no_early_exit_consumer_in_shell_pipes` 는 0.05s → 89.17s (1783 배) 가 됐다.
///
/// **왜 `CACHEDIR.TAG` 인가 — 후보 셋을 관측으로 갈랐다.**
///
/// - `.gitignore` 준수: 탈락. `.gitignore` 자신이 이름 기반이다(`/target`·`/site/target`
///   앵커 규칙). 다른 이름의 빌드 디렉토리는 `git check-ignore` 가 무시하지 않는다 —
///   고치려는 결함을 그대로 물려받는다.
/// - `cargo metadata` 의 `target_directory`: 탈락. **지금** 설정된 한 곳만 답한다.
///   이전 빌드가 남긴 디렉토리는 못 본다.
/// - `CACHEDIR.TAG`: 채택. cargo 가 **빌드할 때 실제로 만든다**(실측: `cargo metadata`
///   만으로는 안 만든다). 디렉토리 자신이 갖는 성질이라 개명해도 따라온다.
///
/// 존재만 보지 않고 **서명 줄까지 확인한다** — 규격이 파일을 그 첫 줄로 정의하므로,
/// 같은 이름의 무관한 파일이 소스 디렉토리를 통째로 가지치기하게 두지 않는다.
///
/// 이것은 이름 가지치기를 **대체하지 않고 보탠다.** `.git`·`node_modules`·vendored
/// `assets` 는 표식을 달지 않으므로 이름이 여전히 유일한 근거다 — 그쪽은 성질이 다른
/// 문제다(산출물이 아니라 "추적되지만 우리가 안 쓴 콘텐츠").
pub fn is_build_cache_dir(dir: &Path) -> bool {
    let Ok(head) = std::fs::read(dir.join("CACHEDIR.TAG")) else {
        return false;
    };
    head.starts_with(CACHEDIR_SIGNATURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 표식이 있는 임시 디렉토리를 만든다. `tempfile` 을 쓰지 않는 이유는 이 크레이트의
    /// **의존이 0 이어야 하기 때문**이다(ADR-0138) — dev-dependency 도 이 크레이트의
    /// 잡을 비싸게 만든다.
    fn temp_dir_named(suffix: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tasty-cachedir-{}-{suffix}", std::process::id()));
        // 이전 완주가 남긴 것을 치운다. **없는 것이 정상이라 실패가 아니다** — 뒤의
        // create_dir_all 이 진짜 판정이고, 여기서 죽으면 그 판정을 못 본다.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("임시 디렉토리 생성");
        dir
    }

    #[test]
    fn a_directory_with_the_signature_is_a_build_cache() {
        let dir = temp_dir_named("hit");
        std::fs::write(
            dir.join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n# cargo\n",
        )
        .expect("표식 쓰기");
        assert!(is_build_cache_dir(&dir));
        // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
        // 죽으면 진짜 실패가 정리 오류에 가린다.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 반대 극성 — **이름만 같은 파일로는 가지치기되지 않는다.** 이 절이 없으면
    /// "존재하면 참" 으로 퇴화해도 위 테스트가 초록이라, 소스 디렉토리에 우연히
    /// 같은 이름의 파일이 생겼을 때 그 디렉토리가 통째로 모수에서 사라진다.
    #[test]
    fn a_file_with_the_same_name_but_wrong_content_is_not_enough() {
        let dir = temp_dir_named("miss");
        std::fs::write(dir.join("CACHEDIR.TAG"), "메모\n").expect("가짜 표식 쓰기");
        assert!(!is_build_cache_dir(&dir));

        std::fs::remove_file(dir.join("CACHEDIR.TAG")).expect("표식 제거");
        assert!(!is_build_cache_dir(&dir), "표식이 없으면 캐시가 아니다");
        // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
        // 죽으면 진짜 실패가 정리 오류에 가린다.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 레포에서의 실제 관측 — 소스 루트는 캐시가 아니고, 빌드 디렉토리는 캐시다.
    /// `target/` 은 이 테스트를 돌리는 `cargo test` 자신이 만들지만, 다른
    /// `CARGO_TARGET_DIR` 로 돌 수도 있으므로 **있을 때만** 단정한다.
    #[test]
    fn the_repo_root_is_not_a_cache_but_a_build_dir_is() {
        let root = repo_root();
        assert!(!is_build_cache_dir(&root));
        assert!(!is_build_cache_dir(&root.join("crates")));

        let target = root.join("target");
        if target.join("CACHEDIR.TAG").exists() {
            assert!(is_build_cache_dir(&target));
        }
    }

    #[test]
    fn the_root_is_the_repo_not_this_crate() {
        let root = repo_root();
        assert!(root.join("crates/tasty-doc-guards/Cargo.toml").exists());
        assert!(!root.ends_with("tasty-doc-guards"));
    }

    /// 표지 검사가 실제로 무엇을 거르는지 — 이 크레이트 디렉토리는 `Cargo.toml` 이
    /// 있어도 표지 전체는 못 채운다. 표지를 `Cargo.toml` 하나로 줄이면 이 판정이 죽는다.
    #[test]
    fn this_crate_dir_would_not_pass_as_the_root() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(here.join("Cargo.toml").exists(), "대조: Cargo.toml 은 있다");
        assert!(!here.join("CHANGELOG.md").exists());
        assert!(!here.join("docs/adr/index.md").exists());
    }
}
