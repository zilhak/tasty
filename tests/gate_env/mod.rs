//! 특정 도구가 없는 환경을 재현하도록 필요한 실행 파일의 심볼릭 링크만 가진 PATH를 만든다.

//! 이 모듈을 포함하는 타깃은 Unix 전용이라 Unix API를 직접 사용한다.

//! GateRun은 종료 코드 단정에 실패했을 때 해당 실행의 stdout과 stderr도 보여 준다.

use std::os::unix::fs::symlink;
use std::path::Path;

use tempfile::TempDir;

/// 찾지 못한 이름은 생략한다. 필요한 도구 이름의 오타도 같은 결과이므로 호출자는 오류 메시지까지 확인한다.
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

/// 종료 코드와 stdout·stderr를 보관한다. i32와 비교할 수 있으며 Debug 출력에는 실행 로그도 포함된다.
pub struct GateRun {
    pub code: i32,
    pub output: String,
}

impl GateRun {
    pub fn from_output(out: &std::process::Output) -> Self {
        Self {
            // 시그널 종료는 0·1·2와 겹치지 않는 값으로 구별한다.
            code: out.status.code().unwrap_or(-1),
            output: format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        }
    }
}

impl PartialEq<i32> for GateRun {
    fn eq(&self, other: &i32) -> bool {
        self.code == *other
    }
}

impl std::fmt::Debug for GateRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rc {}\n--- 게이트 출력 (stdout+stderr) ---\n{}--- 끝 ---",
            self.code, self.output
        )
    }
}
