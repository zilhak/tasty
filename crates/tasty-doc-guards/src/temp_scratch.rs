//! 범위를 벗어나면 임시 디렉터리를 정리하는 테스트용 RAII 도구.
//! 성공 경로 끝의 수동 삭제와 달리 패닉 되감기에서도 Drop이 정리를 시도한다.
//! 정리 오류는 무시하며, 강제 종료처럼 Drop이 실행되지 않는 경우까지 보장하지 않는다.
//! 이 크레이트에 의존하지 않는 제품 코드의 임시 경로 관리에는 적용하지 않는다.

use std::path::{Path, PathBuf};

/// pid와 함께 호출을 구분할 단조 카운터. 시계 해상도와 같은 스레드의 재호출에 의존하지 않는다.
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 생성 시 디렉터리를 만들고 Drop에서 삭제를 시도한다.
///
/// ```ignore
/// let dir = Scratch::new("bundle-empty");
/// std::fs::write(dir.path().join("x"), b"y").unwrap();
/// // 패닉 되감기에서도 Drop이 정리를 시도한다.
/// ```
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// what을 경로명에 넣어 남은 디렉터리의 용도를 확인할 수 있게 한다.
    ///
    /// # Panics
    ///
    /// 디렉터리를 만들지 못하면 panic한다.
    pub fn new(what: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tasty-{what}-scratch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap_or_else(|e| {
            panic!(
                "임시 디렉토리를 만들 수 있어야 한다 ({}): {e}",
                path.display()
            )
        });
        Self { path }
    }

    /// 만들어진 디렉토리의 경로.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // 정리 오류가 원래 시험 실패를 가리거나 패닉 되감기 중 abort를 일으키지 않게 한다.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_exists_while_it_lives_and_is_gone_after() {
        let kept;
        {
            let s = Scratch::new("selftest-lifetime");
            kept = s.path().to_path_buf();
            assert!(kept.is_dir(), "지어지면 있어야 한다: {}", kept.display());
        }
        assert!(
            !kept.exists(),
            "Drop 뒤 임시 디렉터리가 남았다: {}",
            kept.display()
        );
    }

    /// 패닉 때 수동 삭제는 실행되지 않지만 RAII Drop은 실행되는지 비교한다.
    #[test]
    fn a_panic_still_removes_the_directory() {
        let leaked = std::env::temp_dir().join(format!(
            "tasty-selftest-handrolled-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let raii = std::sync::Mutex::new(None::<PathBuf>);

        let hand = std::panic::catch_unwind({
            let leaked = leaked.clone();
            move || {
                std::fs::create_dir_all(&leaked).expect("탐침을 만들 수 있어야 한다");
                panic!("시험이 여기서 죽는다");
                // 손 정리라면 이 아래에 `remove_dir_all` 이 있고, 그것은 안 돈다.
            }
        });
        let raii_run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let s = Scratch::new("selftest-raii");
            *raii.lock().expect("잠금") = Some(s.path().to_path_buf());
            panic!("시험이 여기서 죽는다");
        }));

        assert!(
            hand.is_err() && raii_run.is_err(),
            "둘 다 패닉해야 대조가 성립한다"
        );
        assert!(
            leaked.is_dir(),
            "손 정리 갈래는 남는다 — 이것이 오늘의 결함이다"
        );
        let raii = raii
            .lock()
            .expect("잠금")
            .clone()
            .expect("경로가 잡혔어야 한다");
        assert!(
            !raii.exists(),
            "RAII 갈래는 패닉해도 지워진다: {}",
            raii.display()
        );

        // 대조를 위해 남긴 임시 디렉터리를 정리한다.
        let _ = std::fs::remove_dir_all(&leaked);
    }
}
