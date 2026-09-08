//! **스코프가 끝나면 지워지는 임시 디렉토리.** 손 정리를 대신한다.
//!
//! [`temp_path`](crate::temp_path) 는 공유 temp 아래 경로가 **유니크화됐는가**를 판정한다.
//! 이 모듈은 그 다음 물음에 답한다 — **그 경로가 완주 뒤에 지워지는가.** 둘은 독립이다:
//! 유일한 이름을 잘 지어도 안 지우면 남고, 남은 것은 다음 완주의 판정에 안 걸린다(이름이
//! 다르니까). 그래서 새는 것이 **조용하다.**
//!
//! ## 왜 손 정리가 안 되는가
//!
//! 손 정리는 `let dir = ..; ..; remove_dir_all(&dir)` 꼴이고, 그 마지막 줄은 **성공
//! 경로에만** 있다. 시험이 중간에서 패닉하면(단정 실패가 그것이다) 안 돈다. 즉 손 정리는
//! **초록일 때만 도는 정리**이고, 정작 디렉토리에 볼 것이 남는 빨강일 때 안 돈다.
//!
//! 실측 2026-09-08(이 저장소, `/tmp`): `*-probe-*` 꼴 잔여 **11 개**, 그중 빈 것 **0 개** —
//! 전부 내용이 있었고 가장 오래된 것이 **하루를 넘겼다**. 같은 날 같은 좌변에서 정리 코드가
//! **아예 없는** 자리도 둘 나왔다(`tests/spawn_diag/mod.rs` 의 `stale`·`tick`). 손 정리는
//! 안 적어도 초록이라, 안 적힌 것과 안 도는 것이 화면에서 같다.
//!
//! ## 왜 한 자리인가
//!
//! 실측 2026-09-08: 이 저장소에서 임시 경로에 `impl Drop` 을 **각자 적은 파일이 11 개**다
//! (`floored_walk.rs` · `tests/common/mod.rs` · `tests/gui_common/mod.rs` ·
//! `tests/webhook_common/mod.rs` · `src/test_support.rs` 등). 전부 같은 세 줄이다. 사본이
//! 여럿인 것 자체가 결함은 아니지만, **그 행동을 안 적은 자리가 그 사본들 사이에서 안
//! 보인다** — 세는 좌변이 없기 때문이다. 여기 한 타입을 두면 "이 타입을 안 쓴 임시 경로"
//! 라는 셀 수 있는 좌변이 생긴다.
//!
//! 다만 **모든 사본이 여기로 모이지는 않는다.** 이 크레이트는 루트 패키지의
//! `[dev-dependencies]` 이고 제품 크레이트가 의존하지 않는다. 그래서 제품 소스에 사는
//! 사본(`crates/tasty-terminal/src/disk_scrollback.rs` 등)은 대상이 아니다 — 그 경계를
//! 좌변에 적지 않으면 다음 사람이 그것을 미이행으로 센다.

use std::path::{Path, PathBuf};

/// 이름의 유일화 성분. **시각을 안 쓴다.**
///
/// 시계의 해상도는 플랫폼의 성질이라 같은 코드가 어떤 OS 에서는 유일하고 어떤 OS 에서는
/// 겹친다 — 겹치면 두 시험이 같은 경로를 쓰고 먼저 끝난 쪽의 Drop 이 다른 쪽의 파일을
/// 지운다. 실측 2026-09-08: macOS 러너에서만 그렇게 죽었다(Linux 는 안 죽었다).
///
/// 단조 카운터는 해상도가 없어 플랫폼을 안 읽고, **프로세스 전역**이라 같은 스레드의
/// 재호출도 가른다 — `process::id()` 가 못 가르는 축이 그것이다.
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 지어지면 만들어지고, 떨어지면 지워지는 임시 디렉토리.
///
/// ```ignore
/// let dir = Scratch::new("bundle-empty");
/// std::fs::write(dir.path().join("x"), b"y").unwrap();
/// // 여기서 패닉해도 지워진다.
/// ```
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// `what` 을 이름에 넣어 만든다 — 남았을 때 **어느 시험의 것인지** 이름만 보고 알려고
    /// 그렇게 한다(잔여를 세는 것이 이 축의 유일한 관측 수단이다).
    ///
    /// # Panics
    ///
    /// 디렉토리를 못 만들면 패닉한다. 부르는 자리가 전부 시험이고, 거기서 이 실패를
    /// 되돌릴 방법이 없다 — 자리마다 같은 `expect` 를 적는 대신 여기 한 번 적는다.
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
        // 정리 실패는 무시한다 — 판정은 이미 끝났고, 여기서 실패를 올리면 임시 디렉토리
        // 삭제 실패가 시험의 빨강으로 둔갑한다. 게다가 Drop 은 패닉 되감기 중에도 돌아,
        // 여기서 패닉하면 원래 실패 문구가 abort 로 덮인다.
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
        assert!(!kept.exists(), "떨어지면 없어야 한다: {}", kept.display());
    }

    /// **양성 대조 — 이 타입이 존재하는 이유.** 손 정리는 패닉 경로에서 안 돌고, 이 타입은
    /// 돈다. 되감기로 두 갈래를 같은 시험 안에서 잰다.
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

        // 대조가 끝났으니 일부러 남긴 쪽을 치운다. 실패해도 판정과 무관하다.
        let _ = std::fs::remove_dir_all(&leaked);
    }
}
