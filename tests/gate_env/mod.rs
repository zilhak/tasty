//! 셸 게이트를 **도구가 없는 환경**에서 돌리기 위한 PATH 조립기.
//!
//! 이 계열 게이트는 `command -v <도구>` 로 전제 도구의 유무를 보고, 없으면 **판정 불가
//! (rc=2)** 로 나간다. 그 갈래를 시험하려면 그 도구만 안 보이는 PATH 가 필요하다.
//! 환경변수로는 못 만든다 — 도구의 부재는 값이 아니라 **자리**의 문제다.
//!
//! **디렉토리를 훑지 않는다.** 필요한 이름을 이미 알고 있으므로 PATH 의 각 항목에 그
//! 이름을 붙여 보는 것으로 충분하다. 훑으면 이 코드가 `read_dir` 를 들이게 되고, 그
//! 형태는 `scripts/check-shared-walk-ratchet.sh` 의 상한 래칫이 세는 것이다(여유 0).
//! 여기는 타깃 한 겹 밖이라 지금은 안 세어지지만, 나중에 이 코드가 타깃으로 올라가면
//! 그때 세어진다 — 처음부터 안 훑는 쪽이 옮겨도 안전하다.

//!
//! 이 모듈은 `#![cfg(unix)]` 인 타깃에서만 컴파일된다 — Windows 에서는 그 타깃 자체가
//! 비므로 여기도 안 들어간다(그 cfg 가 그 타깃에서 거짓인 것은 잰 값이다:
//! `rustc --print cfg --target x86_64-pc-windows-gnu` 에 `unix` 선언이 없다). 그래서
//! `std::os::unix` 를 조건 없이 쓴다.

use std::os::unix::fs::symlink;
use std::path::Path;

use tempfile::TempDir;

/// **이름을 준 실행 파일만** 보이는 PATH 디렉토리를 만든다.
///
/// 못 찾은 이름은 조용히 빠진다 — 그것이 이 함수의 용도다(없는 것을 재현한다). 다만
/// 호출자가 **필요한** 도구를 오타로 적으면 그것도 조용히 빠지므로, 각 시험은 자기가
/// 재려는 rc 와 함께 **메시지**도 단정해 "다른 이유로 죽은 것" 과 갈라야 한다.
pub fn only(names: &[&str]) -> TempDir {
    let d = tempfile::tempdir().expect("임시 디렉토리");
    let path = std::env::var("PATH").unwrap_or_default();
    for name in names {
        for dir in path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let cand = Path::new(dir).join(name);
            if cand.is_file() {
                symlink(&cand, d.path().join(name))
                    .unwrap_or_else(|e| panic!("{name} 심볼릭 링크 실패: {e}"));
                break;
            }
        }
    }
    d
}
