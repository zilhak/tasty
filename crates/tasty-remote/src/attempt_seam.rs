//! 시험 전용: 연결 단계 사이에 시험이 끼어드는 자리. 연결 직후, 시도 토큰을 등록한 직후, open·요청 직전에
//! 시험이 건 함수를 한 번 부르고, 어느 자리에 닿았는지 남긴다. 연결 함수는 시험 스레드에서 직접 부르므로
//! thread-local 로 충분하다.

use std::cell::RefCell;
use std::net::TcpStream;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 연결이 돌아온 직후, 상속 끄기(`socket_inherit`)와 등록 전.
    AfterConnect,
    /// `register_socket` 성공 직후, 소켓 기한 설정 전.
    AfterRegister,
    /// 기한 설정 뒤 handshake open 직전.
    BeforeOpen,
    /// JSON-RPC 요청 한 번(`probe_method_bound`)이 요청을 쓰기 직전. 넘기는 소켓은 쓰는 쪽 복제다.
    BeforeRequest,
}

type Hook = Box<dyn FnOnce(&TcpStream)>;

thread_local! {
    static HOOKS: RefCell<Vec<(Stage, Hook)>> = const { RefCell::new(Vec::new()) };
    static REACHED: RefCell<Vec<Stage>> = const { RefCell::new(Vec::new()) };
}

/// 이 스레드의 기록과 건 함수를 지우고 `stage` 에서 부를 함수를 건다.
pub fn arm(stage: Stage, hook: impl FnOnce(&TcpStream) + 'static) {
    REACHED.with(|reached| reached.borrow_mut().clear());
    HOOKS.with(|hooks| *hooks.borrow_mut() = vec![(stage, Box::new(hook) as Hook)]);
}

/// 이미 건 함수를 두고 `stage` 에서 부를 함수를 하나 더 건다. 먼저 [`arm`] 을 부른다.
pub fn arm_also(stage: Stage, hook: impl FnOnce(&TcpStream) + 'static) {
    HOOKS.with(|hooks| hooks.borrow_mut().push((stage, Box::new(hook))));
}

/// 이 스레드에서 연결 함수가 `stage` 에 닿았는지.
pub fn reached(stage: Stage) -> bool {
    REACHED.with(|reached| reached.borrow().contains(&stage))
}

/// 연결 함수가 `stage` 에 닿았다고 남기고, 그 자리에 건 함수가 있으면 한 번 부른다.
pub fn at(stage: Stage, socket: &TcpStream) {
    REACHED.with(|reached| reached.borrow_mut().push(stage));
    let hook = HOOKS.with(|hooks| {
        let mut hooks = hooks.borrow_mut();
        let index = hooks.iter().position(|(armed, _)| *armed == stage)?;
        Some(hooks.remove(index).1)
    });
    if let Some(hook) = hook {
        hook(socket);
    }
}
