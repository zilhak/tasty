//! 재연결 사이에 공유하는 stdin 리더와 세션별 라우팅 슬롯.

use super::*;

/// 활성 세션의 sender. install_sender만 쓴다. 리더가 송신 실패 후 슬롯을 비우면
/// 그 사이 설치한 새 sender까지 지울 수 있으므로 리더는 읽기만 한다.
pub(super) type StdinSlot = Arc<Mutex<Option<mpsc::Sender<RawEvent>>>>;

/// stdin 슬롯 poison은 최초 한 번만 보고한다.
pub(super) static STDIN_SLOT_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// stdin 슬롯은 poison을 보고한 뒤 복구한다. 여기서 패닉하면 stdin 전달이 끝난다.
/// 아래의 소켓 writer 락은 부분 프레임 위험 때문에 복구하지 않으므로 처리 방식을 구분한다.
pub(super) fn lock_stdin_slot(
    slot: &StdinSlot,
) -> std::sync::MutexGuard<'_, Option<mpsc::Sender<RawEvent>>> {
    slot.lock().unwrap_or_else(|p| {
        if !STDIN_SLOT_POISON_REPORTED.swap(true, Ordering::Relaxed) {
            tracing::error!(
                "attach: stdin slot lock poisoned — a thread panicked while holding it; \
                 recovering (the slot holds a plain `Option<Sender>`), later occurrences \
                 are not logged"
            );
        }
        p.into_inner()
    })
}

/// 세션 전환 중의 stdin EOF/오류를 기억한다. 다음 install_sender가 즉시 전달해
/// 이미 닫힌 stdin을 새 세션이 계속 기다리지 않도록 한다.
pub(super) type StdinEofLatch = Arc<AtomicBool>;

/// 프로세스에서 공유할 stdin 리더를 시작한다. 각 세션은 install_sender로 수신 대상만 바꾼다.
/// stdin을 여러 스레드가 읽으면 재연결 뒤 입력을 서로 가져갈 수 있어 하나만 유지한다.
/// 별도 종료 신호 없이 프로세스 종료 때 회수한다.
pub(super) fn spawn_stdin_reader() -> (StdinSlot, StdinEofLatch) {
    let slot: StdinSlot = Arc::new(Mutex::new(None));
    let eof_latch: StdinEofLatch = Arc::new(AtomicBool::new(false));
    {
        let slot = slot.clone();
        let eof_latch = eof_latch.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut stdin = std::io::stdin();
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => {
                        route_stdin_eof(&slot, &eof_latch);
                        break; // 진짜 EOF — 다시 읽어도 항상 0 이므로 더 읽지 않는다.
                    }
                    Ok(n) => route_stdin_chunk(&slot, &buf[..n]),
                    Err(_) => {
                        route_stdin_eof(&slot, &eof_latch);
                        break;
                    }
                }
            }
        });
    }
    (slot, eof_latch)
}

/// 프로세스 전역 stdin 라우팅 슬롯 — 최초 호출 시 1 회 [`spawn_stdin_reader`] 로
/// 초기화된다. 이후 모든 `run_raw_bridge` 호출(=재연결마다)은 이미 떠 있는 같은
/// 리더 스레드의 슬롯에 자신의 sender 를 설치할 뿐, 새 스레드를 스폰하지 않는다.
pub(super) fn stdin_router() -> &'static (StdinSlot, StdinEofLatch) {
    static ROUTER: OnceLock<(StdinSlot, StdinEofLatch)> = OnceLock::new();
    ROUTER.get_or_init(spawn_stdin_reader)
}

/// 현재 sender에 청크를 보낸다. 세션 전환 중 sender가 없거나 전송에 실패하면 버린다.
/// 리더 스레드는 계속 읽으며 새 sender를 지우지 않도록 슬롯을 수정하지 않는다.
pub(super) fn route_stdin_chunk(slot: &StdinSlot, data: &[u8]) {
    // poison 이어도 계속 진행 — 감싼 `Option<Sender>` 은 항상 유효한 값이라 poison
    // 후에도 안전하게 읽을 수 있다(tearing 불가). 이 상시 리더 스레드는 `OnceLock`
    // 초기화로 프로세스 생애주기에 1번만 도므로, 여기서 패닉하면 재시작 없이 영구
    // 사망해 이후 모든 재연결 세션이 stdin 을 못 받는다.
    let sender = lock_stdin_slot(slot).clone();
    if let Some(tx) = sender {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "The receiving session or event loop may have already ended."
        )]
        let _ = tx.send(RawEvent::Stdin(data.to_vec())); // 세션 전환 중 송신 실패는 버림 — 위 불변식 참고
    }
}

/// EOF를 현재 sender에 전달한다. 실패하면 latch에 남겨 다음 세션 설치 때 전달한다.
/// 청크 전달과 마찬가지로 슬롯은 수정하지 않는다.
pub(super) fn route_stdin_eof(slot: &StdinSlot, eof_latch: &StdinEofLatch) {
    // route_stdin_chunk 와 동일한 이유로 poison 을 무시하고 계속 진행한다 — 이
    // 함수도 같은 상시 리더 스레드에서 돌므로 여기서 패닉하면 마찬가지로 영구 사망.
    let sender = lock_stdin_slot(slot).clone();
    let delivered = match sender {
        Some(tx) => tx.send(RawEvent::StdinEof).is_ok(),
        None => false,
    };
    if !delivered {
        eof_latch.store(true, Ordering::Release);
    }
}

/// 슬롯을 쓰는 유일한 함수. 새 sender 설치 시 남아 있는 EOF를 전달하고 latch를 내린다.
pub(super) fn install_sender(
    slot: &StdinSlot,
    eof_latch: &StdinEofLatch,
    tx: mpsc::Sender<RawEvent>,
) {
    // poison 이어도 대입은 안전(값 자체가 tearing 불가) — 이 함수는 메인 스레드
    // (재연결 루프)에서 매 세션마다 호출되므로, 여기서 패닉하면 백오프 재연결
    // 루프조차 못 돌고 `tasty attach --raw --ssh` 프로세스 자체가 종료된다.
    *lock_stdin_slot(slot) = Some(tx.clone());
    if eof_latch.swap(false, Ordering::AcqRel) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "The receiving session or event loop may have already ended."
        )]
        let _ = tx.send(RawEvent::StdinEof); // best-effort — 세션이 이미 끝났으면 무시.
    }
}
