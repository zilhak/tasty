//! 내부 훅과 웹훅이 공유하는 핸들러 등록·실행.
//! host·plugin·사용자 설정을 병합하고 트리거 출처와 셸 실행 제한을 확인한다.

pub mod config;
pub mod env;
pub mod exec;
pub mod registry;
pub mod trigger;
pub mod types;

pub use config::UserHookHandlerActionDecl;
pub use env::{HookShellEnv, build_env};
pub use exec::{
    SequenceNotQueued, SequenceOrigin, SubstitutionContext, enqueue_sequence, execute_sequence,
    spawn_shell,
};
pub use registry::{
    HostHookHandlerPort, UserHookHandlerUpsertDecl, global, install_default_sources,
    user_config_path,
};
pub use types::{
    HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource, IpcCall,
    TriggerSource, validate_binding,
};
