//! `IpcMethodCtx` 의 공개 형태를 외부 crate 에서 확인한다.
//!
//! 네 필드가 모두 공개라 외부 plugin 은 `..` 없이 전체 분해하고, 받은 ctx 로 새 ctx 를
//! 구조체 리터럴로 만든다. 비공개 필드가 더해지면 이 파일이 컴파일되지 않는다. crate 안의
//! 시험은 `pub(crate)` 필드도 볼 수 있어 이 깨짐을 잡지 못하므로 통합 시험으로 둔다.

use tasty_plugin_sdk::IpcMethodCtx;

/// 전체 분해와 구조체 리터럴 — 외부 plugin 이 쓰는 두 형태.
fn rebuild(ctx: IpcMethodCtx) -> IpcMethodCtx {
    let IpcMethodCtx {
        method,
        params,
        caller_plugin_id,
        host,
    } = ctx;
    IpcMethodCtx {
        method,
        params,
        caller_plugin_id,
        host,
    }
}

#[test]
fn ipc_method_ctx_can_be_destructured_and_built_outside_the_sdk() {
    // HostHandle 은 런타임만 만들 수 있어 값을 넣어 부르지는 않는다. 위 함수가 컴파일되는
    // 것이 이 시험의 판정이다.
    let _: fn(IpcMethodCtx) -> IpcMethodCtx = rebuild;
}
