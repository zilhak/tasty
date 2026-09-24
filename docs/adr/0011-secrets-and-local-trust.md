# ADR-0011: 비밀 데이터의 보호 범위를 IPC와 파일 권한으로 구분한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: security, secrets, passkey
- **Group**: foundation

## Context

secret이라는 이름만으로 파일 암호화나 같은 사용자 프로세스 간 격리가 보장되는 것은 아니다.
플러그인은 호스트와 같은 OS 사용자로 실행되며 현재 OS sandbox로 격리하지 않는다.
지원하는 보호와 지원하지 않는 보호를 구분해야 사용자가 저장 위치를 올바르게 선택할 수 있다.

## Decision

memory secret은 평문 BLOB으로 저장한다. 보장하는 것은 plugin별 IPC owner 격리다.
다른 plugin의 secret을 조회하거나 그 존재를 열거할 경로를 제공하지 않는다.
DB 직접 읽기, 백업·동기화, 장치 도난 시의 비밀 보호는 보장하지 않는다.

Passkey는 별도 이름으로 등록하고 원격 프로필은 그 이름을 참조한다.
기존 파일을 참조하는 path와 inline 입력을 구분하지만 저장된 TOML에는 비밀 대신 파일 경로만 둔다.
inline 값은 Tasty가 권한을 제한한 파일로 만들고 Passkey 삭제 때 함께 삭제한다.
사용자가 소유한 path 파일의 수명에는 관여하지 않는다.

대화형 이름 입력은 경로 이동이 불가능한 허용 문자로 제한한다.
자동 migration 이름은 거절 대신 치환하며 이 두 용도를 섞지 않는다.
IPC·CLI는 파일 내용을 반환하지 않고 마스킹한다.
로컬 GUI와 승인된 plugin의 선언 타입 열람은 설치 신뢰에 따른 편의 기능이다.

정말 민감한 자격증명은 plugin이 OS keyring이나 외부 보관소 정책으로 관리한다.
Tasty 자체의 master passphrase 암호화와 자동 headless 해제는 현재 도입하지 않는다.

## Consequences

프로필과 설정에 비밀 문자열이 섞이는 것을 줄이고 소유 파일의 수명을 명확히 한다.
IPC에서 plugin 간 secret을 분리하면서 실제보다 강한 저장 보호를 주장하지 않는다.

같은 OS 사용자의 프로세스가 파일을 읽는 것은 막지 못한다.
loopback TCP도 다른 OS 사용자를 인증하지 않으므로 공유 머신의 격리 수단이 아니다.
keyring을 쓴다는 사실만으로 모든 플랫폼에서 같은 사용자 공격자까지 막는다고 약속하지 않는다.

## Alternatives Considered

- host가 AES-GCM과 keyring만 추가해도 sandbox 없는 plugin의 접근 모델 전체를 해결하지 못한다.
- 환경에 따라 암호화·평문 fallback을 섞으면 행별 형식과 복구가 복잡해진다.
- inline 값을 TOML에 그대로 넣으면 표시·로그·백업에 노출되는 범위가 넓어진다.
- master passphrase는 실제 저장 암호화를 제공할 수 있지만 무인 headless 접속과 해제 UX를 별도로 설계해야 한다.

## Reconsideration Triggers

OS sandbox, 공유 머신·다중 사용자 운영, 실제 저장 시 암호화 요구가 생기면 보호 모델을 다시 정한다.
sandbox 도입만으로 안전하다고 선언하지 않고 DB·Passkey·keyring 접근이 실제로 차단되는지 확인한다.

보호 설명은 기능이 제공하는 IPC 제약과 OS가 제공하는 파일·프로세스 제약을 구분해 유지한다.

## References

- [메모리 보안과 Passkey 운영](../design/systems/memory.md#보안신뢰-모델)
- [IPC 연결의 신뢰 범위](../architecture/ipc-server.md#연결과-신뢰-범위)
- [플러그인 민감 데이터](../dev-guide/plugin-development.md#민감-데이터--regular--secret--keyring-선택)
- 구현: `crates/tasty-memory/`, `crates/tasty-remote-profiles/src/passkey.rs`.
