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

#[cfg(test)]
mod tests {
    use super::*;

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
