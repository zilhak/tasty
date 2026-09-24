#![forbid(unsafe_code)]

//! 원격 연결 프로필과 자격증명 파일의 저장·조회. 실제 SSH 연결은 호스트·CLI가 맡는다.
//! 프로필은 Passkey를 이름으로 참조하고 Passkey는 파일 경로를 저장한다. inline 입력은
//! 별도 파일에 쓰며 암호화하지 않는다. Unix에서는 파일 권한을 제한한다.
//! ssh config의 alias는 프로세스를 실행하지 않고 읽기 전용으로 열거한다.

// 이유: 테스트의 let _ =를 제품 코드의 오류 처리 명부에서 제외한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod passkey;
mod profile;
mod ssh_config;

pub use passkey::{
    KNOWN_PASSKEY_KINDS, Passkey, Passkeys, is_valid_passkey_name, sanitize_passkey_name,
};
pub use ssh_config::{
    ConfigAvailability, ImportError, SshConfigHost, config_availability, enumerate_hosts,
    enumerate_hosts_at, imported_as, prepare_import, user_config_path,
};

pub use profile::{
    AttachView, BUILTIN_KINDS, FieldValue, PORT_MODES, RemoteProfile, RemoteProfiles, SHELLS,
    SshView, is_builtin_kind, is_valid_port_mode, is_valid_shell, shell_to_port_mode,
};
