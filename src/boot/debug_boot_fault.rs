//! debug 빌드에서만 부팅 실패를 주입한다. 데이터 홈 부재나 스레드 생성 실패는 격리 홈으로 재현할 수
//! 없어서, 그 실패가 사용자에게 보이는 경로와 종료 코드·instance 파일 정리를 실제 실행으로 확인하는 데 쓴다.
//! `TASTY_DEBUG_BOOT_FAULT`에 `app_new` 또는 `assemble`을 주면 해당 단계가 실패한 것처럼 동작한다.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    AppNew,
    Assemble,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::AppNew => "app_new",
            Stage::Assemble => "assemble",
        }
    }
}

/// 이 단계에 주입이 요청됐으면 그 실패를 돌려준다.
pub(crate) fn take(stage: Stage) -> Option<anyhow::Error> {
    let wanted = std::env::var("TASTY_DEBUG_BOOT_FAULT").ok()?;
    (wanted == stage.name())
        .then(|| anyhow::anyhow!("injected boot fault at {} (debug build)", stage.name()))
}
