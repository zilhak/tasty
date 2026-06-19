# 원격 접속 프로필 + Passkey (remote_tool)

> Status: Implemented. 도구 메뉴 > **Remote connections**. 화면: [remote-tool](screens/remote-tool.md).
> 설계 근거: [ADR-0015](../../adr/0015-remote-profiles-typed-registry.md) · [ADR-0016](../../adr/0016-passkey-store-path-convergence.md).

타입 무관 **원격 연결 디스크립터**와 **자격증명(Passkey) 저장소** 두 개를 GUI/CLI/IPC 3표면에서 같은 저장 로직으로 CRUD 한다. 프로필은 비밀을 담지 않고 passkey 를 이름으로 참조만 한다.

## 데이터 모델

- **원격 접속 프로필** — `~/.tasty/remote-profiles.toml`. `{ name, label?, kind, passkey_ref?, fields }`.
  - `kind` 는 **열린 string**. known = core 내장(`ssh`/`smb`) ∪ 설치 플러그인 선언 타입. 미등록 타입도 저장되며 노란 "미등록 타입" 배지로만 경고.
  - `fields` 는 타입별 자유 키-값(`FieldValue = Str | List`, TOML 스칼라/배열). ssh 는 `host`/`user`/`port`/`remote_tasty`/`port_mode`/`shell` 등을 쓰며 `SshView` 로 typed 접근.
- **Passkey** — `~/.tasty/passkeys.toml`(0600). `{ name, kind, path }`. `kind = path`(사용자 소유 파일 참조) | `inline`(입력값을 `~/.tasty/passkeys/<name>` 0600 파일로 materialize). **toml 엔 비밀 값이 없다**(경로뿐). 이름은 `[A-Za-z0-9_-]` 만 허용(파일명·traversal 차단).

## 동작

- **타입별 폼** — ssh 는 전용 그리드(host/user/port/label/remote tasty/shell + Passkey 드롭다운), 그 외(smb/http/미등록)는 generic key-value 에디터. 셸 `auto` 는 저장 시 1회 SSH 프로브로 포트 발견 모드를 감지.
- **dangling 참조** — 없는 passkey 를 가리켜도 정상 저장, "passkey 없음" 노란 배지 + 소비 시점 에러.
- **값 마스킹** — passkey 값은 기본 마스킹(`••••••••`). GUI 의 **Reveal**(로컬 전용)만 실제 값을 본다(path=경로, inline=관리 파일 내용). AI Agent/원격은 IPC 로 값을 **영구 읽을 수 없다**(쓰기만).
- **소비자 분리** — attach 는 ssh kind 프로필을 읽는 소비자 중 하나. "주소 저장"과 "attach"가 분리됐다(→ [remote-attach](../remote-attach/index.md)).

## 인터페이스

| 표면 | 진입 |
|---|---|
| GUI | 도구 메뉴 > Remote connections (popup `remote_tool`, 2탭) |
| CLI | `tasty tool ssh add\|list\|show\|edit\|remove\|detect` (ssh kind. `--identity` → path passkey 자동 생성) |
| IPC | `remote.profile.{list,get,add,detect,remove}` · `remote.passkey.{list,get,add,remove}` (값 마스킹). 구 `tool.ssh.*`/`ssh.profile.*` 는 alias 한시 호환 |

## 마이그레이션

부팅 시 멱등 변환: 구 `ssh-profiles.toml` → `remote-profiles.toml` + `passkeys.toml`, `identity_file` → path passkey 자동 분리, 구파일은 `.bak` 으로 보존.
