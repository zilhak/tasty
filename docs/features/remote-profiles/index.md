# 원격 접속 프로필 + Passkey (remote_tool)

> Status: Implemented. 도구 메뉴 > **Remote connections**. 화면: [remote-tool](screens/remote-tool.md).
> 설계 근거: 이 문서의 연결 프로필 구조와 [에이전트 메모리 시스템](../../design/systems/memory.md)의 비밀 저장 규칙.

**원격 연결 프로필**과 **자격증명(Passkey)**을 GUI·CLI·IPC에서 같은 저장 로직으로 만들고 관리한다. 프로필은 비밀을 담지 않고 passkey 를 이름으로 참조만 한다.

## 데이터 모델

- **원격 접속 프로필** — `~/.tasty/remote-profiles.toml`. `{ name, label?, kind, passkey_ref?, fields }`.
  - `kind` 는 **열린 string**. known = core 내장(`ssh`/`tasty-attach`/`smb`) ∪ 설치 플러그인 선언 타입. 미등록 타입도 저장되며 노란 "미등록 타입" 배지로만 경고.
  - `fields` 는 타입별 자유 키-값(`FieldValue = Str | List`, TOML 스칼라/배열). typed 접근은 kind 별 view.
  - **2-레이어**(docs/features/remote-profiles/index.md):
    - **`ssh`** = 순수 연결 정보. `host`/`user`/`port`/`extra_options` + `shell`/`detect_failed`(셸 감지 상태). `SshView` 로 접근. **attach 전용 필드(`remote_tasty`/`port_mode`)는 여기 없다.**
    - **`tasty-attach`** = attach 스펙. 연결은 `ssh_ref = <ssh 프로필 name>`(참조 — 연결할 때 다시 읽어 최신 설정 사용) **또는** 인라인(자기 fields 의 ssh 정보). attach 전용: `remote_tasty`(기본 `tasty`)/`port_mode`(기본 `auto`)/`port_file`(원격 port 파일 명시 경로, 관례보다 최우선). `AttachView` 로 접근. attach 동작이 소비한다(→ [remote-attach](../remote-attach/index.md)).
- **Passkey** — `~/.tasty/passkeys.toml`(0600). `{ name, kind, path }`. `kind = path`(사용자 소유 파일 참조) | `inline`(입력값을 `~/.tasty/passkeys/<name>` 0600 파일로 저장). **toml 엔 비밀 값이 없다**(경로뿐). 이름은 `[A-Za-z0-9_-]` 만 허용(파일명·traversal 차단).

## 동작

- **타입별 폼** — ssh 는 연결 그리드(host/user/port/label/shell + Passkey 드롭다운, **순수 연결정보만** — remote_tasty 없음), tasty-attach 는 Connection 세그먼트 토글(ref=SSH 프로필 드롭다운 ↔ inline=host/user/port/shell/passkey) + 공통 Remote tasty 그룹(remote_tasty/port_mode/port_file), 그 외(smb/http/미등록)는 generic key-value 에디터. ssh 셸 `auto` 는 저장 시 1회 SSH 프로브로 셸 도달성을 감지(detect-split: port_mode 는 attach resolve 가 도출).
- **GUI 3탭** — Remote profiles · **Attach** · Passkeys. Attach 탭이 tasty-attach kind 를 전담하고, Profiles 탭 목록·프로토콜 필터에서 tasty-attach 는 제외된다. Attach 행은 mode 태그(profile/inline)와 상태 배지를 보여준다 — 참조 ssh 프로필이 감지실패면 **비활성**, `ssh_ref` 가 dangling 이면 **프로필 없음**(둘 다 경고 배지, hard-error 아님 — 저장은 정상).
- **dangling 참조** — 없는 passkey 를 가리켜도 정상 저장, "passkey 없음" 노란 배지 + 소비 시점 에러.
- **값 마스킹** — passkey 값은 기본 마스킹(`••••••••`). GUI 의 **Reveal**(로컬 전용)만 실제 값을 본다(path=경로, inline=관리 파일 내용). AI Agent/원격은 IPC 로 값을 **읽을 수 없다**(쓰기만).
- **프로토콜 필터** (GUI 전용·세션 한정) — 원격 접속 프로필 탭에서 현재 프로필의 `kind` 가 2종 이상일 때만 필터 버튼이 뜬다. 체크박스 드롭다운에서 프로토콜을 고르고 적용(apply-on-confirm)하면 선택한 `kind` 의 프로필만 목록에 남는다. 결과 0건이면 빈 상태 안내를 표시한다. 필터 상태는 **비영속** — popup 재오픈에는 유지되지만 tasty 재시작 시 전체 선택으로 리셋되며, 저장 파일/CLI/IPC 표면에는 영향이 없다(순수 표시 필터).
- **로컬 ssh config 가져오기** — 사용자의 `~/.ssh/config`(+ `Include`)에 이미 있는 Host alias 를 열거하고(`list-local` / `remote.profile.list_local`), 그중 하나를 ssh 프로필로 등록한다(`import` / `remote.profile.import`). **저장하는 값은 alias 문자열 하나**(`fields.host`)와 `shell="auto"` 뿐이다 — `HostName`/`User`/`Port`/`ProxyJump`/`IdentityFile` 을 펼쳐 복사하지 않는다. ssh 가 `host` 자리의 alias 를 그대로 해석하므로 복사 없이 접속되고, 복사하면 ssh config 수정 시 값이 어긋나며(drift) `ProxyJump` 류는 애초에 복사 대상에서 빠진다. 목록에 함께 보이는 `HostName`/`User`/`Port` 는 **표시 전용 hint** 라 프로필에 들어가지 않는다(GUI 는 셋을 `[User@]HostName[:Port]` 한 줄로 합쳐 보이고, CLI `list-local` 은 열로 나눠 보인다).
  - 열거는 **순수 파일 파싱**이다 — `ssh -G` 를 쓰지 않는다(`Match exec "…"` 를 실제로 실행해 버리고, 애초에 alias *목록* 을 주지도 않는다). 와일드카드·부정 패턴(`Host *` / `jump-*` / `!bad`)은 접속 가능한 이름이 아니라 제외한다.
  - **가져오기(CLI/IPC)는 셸을 감지하지 않는다.** 감지는 실제 SSH 접속이라 목록에서 여러 건을 가져오면 접속이 연쇄로 일어난다 — 가져온 뒤 `tasty tool remote-profile detect --name <n>` 로 사용자가 돌린다(`add-ssh` 와 다른 점). **GUI 는 다르다** — 가져오기가 alias 프리필 폼을 여는 방식이라, 그 폼을 사용자가 저장하는 시점에 `shell="auto"` 인 프로필은 다른 저장과 똑같이 1회 감지 프로브를 탄다(연쇄가 아니라 사용자가 한 건씩 확인하며 저장하는 경로라 문제되지 않는다).
  - 이름이 겹치면 **거부**한다(덮어쓰기 없음). 없는 alias 를 가리켜도 거부한다.
  - **GUI** 는 같은 목록을 Remote profiles 탭의 프로필 목록 **아래 같은 스크롤의 별도 섹션**으로 보여준다(읽기 전용 — 행 액션은 ghost `프로필 추가` 하나, 이미 등록된 호스트는 `등록됨` Tag). 섹션 헤더에 새로고침 아이콘은 없다 — 다시 읽으려면 popup 을 다시 연다. 프로토콜 필터는 이 섹션에 적용되지 않고(ssh config 항목엔 `kind` 가 없다), 프로필이 0건이어도 섹션은 그대로 뜬다. 가져오기는 프로필 폼을 alias 프리필로 여는 방식이라 이름 변경·중복 검증이 기존 폼 그대로다. 상세: [remote-tool 화면](screens/remote-tool.md).
- **소비자 분리** — attach 는 **tasty-attach kind** 프로필을 읽는 소비자(docs/features/remote-profiles/index.md). tasty-attach 는 ssh 프로필을 `ssh_ref` 로 참조하거나 인라인 연결을 갖는다. "주소 저장(ssh)"과 "attach 스펙(tasty-attach)"이 분리됐다(→ [remote-attach](../remote-attach/index.md)).
- **입력 격리** — 팝업이 터미널 위에 떠 있어도 팝업 위 클릭/스크롤은 팝업이 소비하며, 뒤 터미널의 포커스·선택·스크롤을 건드리지 않는다. remote_tool 고유 규칙이 아니라 모든 팝업에 적용되는 입력 레이어 계약(Layer 3 = Popup, "팝업 위면 터미널 무시")을 따른 것이다(→ [input-layer](../../architecture/input-layer.md)).

## 인터페이스

| 표면 | 진입 |
|---|---|
| GUI | 도구 메뉴 > Remote connections (popup `remote_tool`). Remote profiles 탭에 **로컬 SSH config** 섹션(열거 + 행의 ghost `프로필 추가`) |
| CLI (프로필 CRUD) | `tasty tool remote-profile list\|show\|add-ssh\|add-attach\|edit\|remove\|detect\|list-local\|import` — ssh + tasty-attach 통합. `list-local [--json]`(로컬 ssh config alias 열거, IMPORTED 열에 이미 가져온 프로필 이름) / `import --from <alias> --name <profile> [--label …]`(alias 만 저장, 셸 감지 없음). `add-ssh`(순수 연결) / `add-attach`(`--ssh-ref` XOR 인라인 + `--remote-tasty`/`--port-mode`/`--port-file`). `--identity` → path passkey 자동 생성. `list --kind ssh\|tasty-attach` 필터 |
| CLI (접속/attach) | `tasty tool ssh <profile> [--command …]` — ssh 프로필로 대화형 접속 실행 · `tasty tool attach <name> <surface\|--workspace> \| --list` — tasty-attach 프로필로 attach(→ [remote-attach](../remote-attach/index.md)) |
| CLI (passkey) | `tasty tool passkey add\|list\|show\|remove` (`--path`/`--inline`; list/show 는 값 비노출) |
| IPC | `remote.profile.{list,get,add,detect,remove,list_local,import}`(kind-generic — `kind="tasty-attach"` + `fields` 로 tasty-attach CRUD 도 양면 노출) · `remote.passkey.{list,get,add,remove}` (값 마스킹). `list_local` 은 `{aliases:[{name,source,hostname,user,port,imported_as}], config_path, config_exists, config_readable}`(`aliases` 가 비었을 때 이유를 가른다 — 파일 부재 / 못 읽음 / Host 항목 없음. `config_readable` 은 **UTF-8 텍스트로 읽히는 정규 파일**인지를 뜻해서 권한 실패뿐 아니라 경로가 디렉토리이거나 내용이 UTF-8 이 아닌 경우에도 false 다. 최상위 config 파일만 판정하고 `Include` 대상은 보지 않는다), `import` 은 `{from,name,label?}` → `{saved,name,from,detecting:false}` (자동 감지 없음). 구 `tool.ssh.*`/`ssh.profile.*` 는 alias 한시 호환 |

> **CLI `list-local --json` 과 IPC `list_local` 은 비대칭이다.** IPC 응답만 `config_exists`·`config_readable` 을 싣고, CLI `--json` 은 alias 맨 배열이라 결과가 비었을 때 이유(부재 / 못 읽음 / Host 항목 없음)를 구분할 수 없다. 배열을 객체로 감싸면 기존 스크립트가 깨지므로(breaking change) 일부러 맞추지 않았다 — 그 구분이 필요하면 IPC 를 쓴다. CLI 의 사람이 읽는 출력(`--json` 없이)은 세 갈래를 문장으로 가른다. 부재·못 읽음 문장은 GUI 섹션과 같은 원인 중립 문장이고 경로 자리에 실제 config 경로(`~` 축약)가 들어간다 — 예: `No ~/.ssh/config found.` / `Can't read ~/.ssh/config. Check the file and its permissions.`

## 에러 코드

`remote.profile.*` / `remote.passkey.*` 의 tasty 전용 코드 블록은 `-3204x` 다.

- **`-32040`** (not found) — 없는 프로필/passkey 이름, 또는 `import` 의 `from` 이 ssh config 에 없는 alias.
- **`-32041`** (name conflict) — `import` 의 `name` 이 이미 있는 프로필과 충돌. 표준 `-32602` 와 구분한다: 요청 자체는 멀쩡하고 **저장소 상태**가 부딪힌 것이라 호출자가 할 일이 다르다(다른 이름을 쓰거나, 덮어쓰려면 upsert 인 `remote.profile.add` 로 간다). 메시지 파싱 없이 코드만으로 분기할 수 있다.
- **`-32602`** — 필수 파라미터 누락 등 잘못된 요청.
- **`-32603`** — 프로필 저장 실패 등 내부 오류.

## 마이그레이션

없음. 구 `ssh-profiles.toml → remote-profiles.toml` 자동 마이그레이션은 이 문서(하위호환 제거)로 삭제됐다. 기존 프로필에 남은 `remote_tasty`/`port_mode` 필드는 열린 스키마라 무시되며 크래시하지 않는다 — attach 하려면 `tool remote-profile add-attach` 로 tasty-attach 프로필을 새로 만든다.

## 파일 참여자 API와 standalone CLI 저장

`RemoteFileStore`는 파일 worker와 standalone CLI가 공유하는 동기 명령 경계다. 실행 중 IPC·GUI 저장과 탐지 상태 patch는 같은 file writer에 소유 명령을 제출한다. standalone CLI는 호출 스레드에서 같은 참여자 API를 사용한다. 호환 `save` API가 남아 있어도 host의 동기 저장 fallback으로 사용하지 않는다.

- `insert_passkey`는 새 이름을 같은 목록 잠금 안에서 검사한 뒤 생성한다. 중복 이름은 inline 파일을 쓰기 전에 `AlreadyExists`로 거절한다. 기존 `passkey` upsert와 CLI의 덮어쓰기 동작은 유지한다.
- 호환 읽기 API는 파일 없음·읽기 실패·파싱 실패 시 빈 목록으로 폴백한다. 오류 로그에는 고정 파일명과 I/O 종류 또는 invalid TOML 원인만 남기며 입력 본문을 싣지 않는다. 읽기 실패나 손상이 확인된 원본은 읽기 과정에서 바꾸지 않는다.
- `ProfileCommand::Rename`은 같은 profile 목록 교체 안에서 이름·kind·필드 patch를 적용한다. `rename_passkey`는 새 inline 파일 확정 → 목록 이름 교체 → 옛 관리 파일 정리 순서다. 이름 충돌·원본 없음은 목록 잠금 안에서 구분한다. 기존 profile의 passkey 참조 이름은 유지하므로 rename 뒤 dangling이 될 수 있다.
- profile의 `Put`은 명령에 포함된 필드를 병합하고, `Patch`는 지정 필드만 변경한다. `Insert`는 잠금 안에서 이름 충돌을 검사한다. standalone CLI의 add/edit/remove/import와 감지 결과 저장은 이 API를 사용한다.
- 고정 sidecar 잠금 안에서 최신 TOML을 읽고 변경한다. 다른 profile, 미등록 kind, 건드리지 않은 invalid 필드, foreign 섹션과 주석은 유지한다. 손상·읽기 실패는 빈 목록으로 덮지 않는다. 마지막 fingerprint/링크 대상 확인 후 교체하며, 참여하지 않는 편집기의 최종 확인과 rename 사이 경합은 막지 못한다.
- 복합 연산의 잠금 순서는 profile → passkey → inline 파일이다. 반환 결과는 owned 값이며 IPC·SSH 감지·외부 실행 전에 잠금이 풀린다. CLI의 SSH 감지는 첫 저장 성공 뒤 실행하고, 감지 중 변경·삭제된 profile에는 결과를 덮지 않는다. 비교 조건은 typed projection이며 표현할 수 없는 foreign 값의 변화까지 비교하지 않는다.
- inline 파일 → passkeys 목록 → profile 참조 순서로 확정한다. 파일 사이 transaction이나 rollback은 없다. `MutationFailure`는 실패 단계와 `secret_written`·`passkeys_saved`를 구분한다. 기존 참조는 실패 전 상태로 남을 수 있고 새 비밀 파일은 orphan으로 남을 수 있다.
- inline 임시 파일은 내용 쓰기 전에 Unix 0600 또는 Windows 현재 사용자만 허용하는 보호 ACL을 적용·검증한다. 넓은 기존 mode/ACL을 물려받지 않는다. 관리 inline symlink는 원본을 보존하며 거절한다. Windows ACL 설정 도구를 실행하거나 검증할 수 없으면 payload를 쓰지 않는다.
- 삭제는 목록 확정 뒤 관리 파일 정리를 시도한다. 목록 저장 실패면 기존 관리 파일을 지우지 않는다. path-kind 파일은 수정·삭제하지 않는다. 관리 경로 소유 확인/삭제 실패는 `ManagedCleanup::Failed`로 목록 반영과 구분한다. 이 경우 CLI는 목록이 반영됐지만 정리가 실패했다고 오류를 반환한다.

## 실행 중 저장과 탐지 완료

- IPC 저장·삭제는 원 caller를 보유하고 typed receipt 뒤 실제 결과를 한 번 돌려준다. GUI는 실패한 폼·삭제 확인과 입력값을 유지하고, 같은 popup의 같은 초안에만 완료를 적용한다. popup·View가 닫혀도 접수된 저장은 끝내며 replacement observer에 reveal·완료 효과를 전달하지 않는다.
- 자격증명 Reveal은 편집 회차와 한 번이라도 변경된 초안을 구별한다. 값을 입력한 뒤 다시 비워도 과거 preload가 덮지 않는다. 취소 후 같은 자격증명을 다시 편집하면 새 회차의 결과만 받는다. 읽기 실패는 원래의 변경 없는 폼 또는 아직 활성인 목록 Reveal에만 표시한다.
- GUI 저장 대기 중에는 저장·삭제 버튼을 비활성화하고 편집·취소는 계속 허용한다. worker input/result와 별도로 보유하는 GUI 요청 복사본은 기존 queue 기본 count/byte 한도 안에서 접수하며, 마지막 소유자가 결과를 처리하면 예산을 돌려준다. 이는 보유값의 추정 바이트 예산이며 RSS 상한은 아니다.
- form 편집은 보이는 변경 필드만 patch하며 표시하지 않는 list 값·foreign 필드와 최신의 미변경 필드를 유지한다. 새 Passkey는 잠금 안에서 중복을 거절한다. backend의 `ManagedCleanup::Failed`는 transport 성공과 달리 실제 부분 실패로 분류한다.
- 명시 shell은 기존 실패 표시를 지운다. GUI는 첫 변경 patch에서 지우며, IPC upsert는 저장 결과에 표시가 남아 있을 때 CLI와 같은 expected 조건부 patch를 같은 worker 명령 안에서 실행한다. 후속 상태 단계 실패는 이미 확정된 profile·연결 자격증명을 부분 성공으로 알리고 외부 변경을 덮지 않는다.
- 저장 성공 뒤 자동 탐지를 접수한다. 탐지 입력은 같은 file queue의 ordered fresh read로 준비하고 이전 같은 이름 probe를 취소한다. 준비 비교는 `detect_failed` 차이만 제외하며, 실제 상태 저장은 새로 읽은 profile 전체 typed projection을 expected 조건으로 검사한다. 바뀌거나 삭제된 profile을 오래된 결과로 복원하지 않는다.
- SSH worker의 종료만으로 완료를 발표하지 않는다. 상태 patch receipt를 보유하고 저장 결과 뒤 탐지 완료를 알린다. 실패·Unknown을 성공으로 바꾸거나 재실행하지 않는다. 종료 시 새 준비·probe·patch 제출을 멈추고 이미 접수한 receipt를 기존 drain 기한 안에서 수거한다. 파일 receipt·GUI 결과 수거가 generic deferred reply보다 먼저다.
- inline/list/reference는 파일 사이 transaction이 아니다. 실패 응답은 `saved`와 `partial`, `secret_written`, `passkeys_saved`, `stage`를 구분한다. Unknown의 `saved`/`partial`은 null이며 성공·rollback을 추정하지 않는다. 알려진 목록 삭제 뒤 정리 실패는 `removed`/`cleanup_failed`와 목록 반영을 별도로 알린다.
