#![forbid(unsafe_code)]

//! 원격 접속 프로필 + Passkey 저장소.
//!
//! 두 개의 deps-free 저장소를 제공한다
//! (설계: `docs/adr/0032-remote-attach-two-layer-split.md`):
//!
//! - [`RemoteProfiles`] (`~/.tasty/remote-profiles.toml`) — 타입 태그(열린 string)가
//!   붙은 **범용 연결 디스크립터**. 비밀을 담지 않고 [`Passkey`] 를 이름으로 참조만 한다.
//!   attach(ssh kind)·explorer·플러그인이 각자 자기 타입을 소비한다.
//! - [`Passkeys`] (`~/.tasty/passkeys.toml`, 0600) — name 으로 등록하는 자격증명.
//!   모든 자격증명은 at-rest 에서 **파일 경로 하나로 수렴**한다(inline 입력은
//!   `~/.tasty/passkeys/<name>` 0600 파일로 materialize). toml 엔 비밀 값이 없다.
//!
//! 여기에 더해 [`enumerate_hosts`] 가 사용자의 로컬 `~/.ssh/config` 에 이미 등록된
//! Host alias 를 **읽기 전용**으로 열거한다 — tasty 프로필로 가져올 후보 목록용이며,
//! 프로세스를 띄우지 않는 순수 파싱이다.
//!
//! 보호는 **암호화가 아니라 OS 파일권한 위임**이다(ADR-0004/0005 와 일관) — 같은 OS
//! 유저 FS read 는 신뢰모델상 범위 밖. 이 크레이트는 디스크 저장/해석만 하고 SSH 실행·
//! IPC 표면은 상위(host/CLI)가 맡는다.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
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
