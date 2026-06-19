//! 원격 접속 프로필 + Passkey 저장소.
//!
//! 두 개의 deps-free 저장소를 제공한다 (설계: `.claude-workspace/plans/
//! remote-profiles-redesign.md`):
//!
//! - [`RemoteProfiles`] (`~/.tasty/remote-profiles.toml`) — 타입 태그(열린 string)가
//!   붙은 **범용 연결 디스크립터**. 비밀을 담지 않고 [`Passkey`] 를 이름으로 참조만 한다.
//!   attach(ssh kind)·explorer·플러그인이 각자 자기 타입을 소비한다.
//! - [`Passkeys`] (`~/.tasty/passkeys.toml`, 0600) — name 으로 등록하는 자격증명.
//!   모든 자격증명은 at-rest 에서 **파일 경로 하나로 수렴**한다(inline 입력은
//!   `~/.tasty/passkeys/<name>` 0600 파일로 materialize). toml 엔 비밀 값이 없다.
//!
//! 보호는 **암호화가 아니라 OS 파일권한 위임**이다(ADR-0004/0005 와 일관) — 같은 OS
//! 유저 FS read 는 신뢰모델상 범위 밖. 이 크레이트는 디스크 저장/해석만 하고 SSH 실행·
//! IPC 표면은 상위(host/CLI)가 맡는다.

mod passkey;
mod profile;

pub mod migration;

pub use passkey::{
    KNOWN_PASSKEY_KINDS, Passkey, Passkeys, is_valid_passkey_name, sanitize_passkey_name,
};
pub use profile::{BUILTIN_KINDS, FieldValue, RemoteProfile, RemoteProfiles, SshView, is_builtin_kind};
