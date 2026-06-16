# SSH 프로필 (SSH profiles)

- **Status**: Implemented
- **주체**: 로컬 사용자(GUI CRUD) · AI Agent(`tasty tool ssh` CLI). 원격 접속 사용자는 mirror 로 봄.
- **ADR**: 없음
- **코드**: `src/adapters/ui/popup/ssh_tool.rs`, `crates/tasty-ssh-profiles`
- **화면**: [screens/ssh-tool.md](screens/ssh-tool.md)

## 목적

원격 attach(`tasty remote attach --profile`)에 쓰는 **SSH 프로필을 관리(CRUD)** 하는 기능. [도구 메뉴](../tools-menu/index.md) `SSH profiles` 항목(GUI)과 `tasty tool ssh` CLI 두 표면. 저장은 `~/.tasty/ssh-profiles.toml`.

## 내부 동작

### 프로필 필드

`name` · `host` · `user` · `port` · `identity_file` · `remote_tasty` · `label` · `port_mode`. (`use_agent` / `extra_options` / `remote_command` 은 파일엔 보존하지만 편집 UI 를 두지 않는다.)

### CRUD / 검증

추가 · 편집(폼) · 삭제. 검증: 이름 빈 값 / 이름 중복 / host 빈 값 / port 형식 / 저장 실패.

### 저장

GUI · CLI · IPC 가 **동일 저장 로직**(`SshProfiles`)을 공유 — 어느 표면에서 바꿔도 같은 `~/.tasty/ssh-profiles.toml` 에 반영.

## 인터페이스

- **사용자(GUI)**: 도구 메뉴 `SSH profiles` → 창, 프로필 CRUD.
- **AI Agent(CLI)**: `tasty tool ssh …`.
- **연결**: 프로필은 *원격 attach* 가 소비 → `features/remote-attach/`(점유/원격) *(재작성 예정)*, 메커니즘은 `dev-guide/attach-behavior` *(재작성 예정)*.

## 비-목표

- 실제 SSH 연결 / attach *실행* — 여기선 프로필 *관리* 만. 연결은 원격 attach 기능.
- `use_agent` / `extra_options` / `remote_command` 편집 UI (파일 직접 편집은 가능).

## Acceptance Criteria

- [ ] 도구 메뉴 `SSH profiles` 로 창이 열리고 프로필 목록이 보인다.
- [ ] 프로필 추가/편집/삭제가 되고, 잘못된 입력(이름 중복·빈 host·port 형식)은 검증 에러를 표시한다.
- [ ] `tasty tool ssh` CLI 가 같은 파일에 동일하게 반영한다.
- [ ] 저장된 프로필을 `tasty remote attach --profile <name>` 이 사용한다.

> GUI 는 스크린샷, CRUD/저장은 `tasty tool ssh` CLI + `ssh-profiles.toml` 직접 확인으로 검증.

## 구현

- popup: `src/adapters/ui/popup/ssh_tool.rs` (`SSH_TOOL_POPUP_ID = "ssh_tool"`, 폼/목록/검증, `egui::Memory` UI 상태).
- 저장/모델: `crates/tasty-ssh-profiles` (`SshProfiles`, `SshProfile`).
- CLI: `tasty tool ssh …`.

## 화면

- [screens/ssh-tool.md](screens/ssh-tool.md) — 프로필 목록 + 추가/편집 폼.
