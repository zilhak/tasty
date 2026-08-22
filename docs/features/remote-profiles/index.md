# 원격 접속 프로필 + Passkey (remote_tool)

> Status: Implemented. 도구 메뉴 > **Remote connections**. 화면: [remote-tool](screens/remote-tool.md).
> 설계 근거: [ADR-0032](../../adr/0032-remote-attach-two-layer-split.md)(ssh/tasty-attach 2-레이어) · [ADR-0015](../../adr/0015-remote-profiles-typed-registry.md)(범용 레지스트리 봉투, 부분 superseded) · [ADR-0016](../../adr/0016-passkey-store-path-convergence.md).

타입 무관 **원격 연결 디스크립터**와 **자격증명(Passkey) 저장소** 두 개를 GUI/CLI/IPC 3표면에서 같은 저장 로직으로 CRUD 한다. 프로필은 비밀을 담지 않고 passkey 를 이름으로 참조만 한다.

## 데이터 모델

- **원격 접속 프로필** — `~/.tasty/remote-profiles.toml`. `{ name, label?, kind, passkey_ref?, fields }`.
  - `kind` 는 **열린 string**. known = core 내장(`ssh`/`tasty-attach`/`smb`) ∪ 설치 플러그인 선언 타입. 미등록 타입도 저장되며 노란 "미등록 타입" 배지로만 경고.
  - `fields` 는 타입별 자유 키-값(`FieldValue = Str | List`, TOML 스칼라/배열). typed 접근은 kind 별 view.
  - **2-레이어**(ADR-0032):
    - **`ssh`** = 순수 연결 정보. `host`/`user`/`port`/`extra_options` + `shell`/`detect_failed`(셸 감지 상태). `SshView` 로 접근. **attach 전용 필드(`remote_tasty`/`port_mode`)는 여기 없다.**
    - **`tasty-attach`** = attach 스펙. 연결은 `ssh_ref = <ssh 프로필 name>`(참조 — resolve 시점 재로드, 라이브 팔로우) **또는** 인라인(자기 fields 의 ssh 정보). attach 전용: `remote_tasty`(기본 `tasty`)/`port_mode`(기본 `auto`)/`port_file`(원격 port 파일 명시 경로, 관례보다 최우선). `AttachView` 로 접근. attach 동작이 소비한다(→ [remote-attach](../remote-attach/index.md)).
- **Passkey** — `~/.tasty/passkeys.toml`(0600). `{ name, kind, path }`. `kind = path`(사용자 소유 파일 참조) | `inline`(입력값을 `~/.tasty/passkeys/<name>` 0600 파일로 materialize). **toml 엔 비밀 값이 없다**(경로뿐). 이름은 `[A-Za-z0-9_-]` 만 허용(파일명·traversal 차단).

## 동작

- **타입별 폼** — ssh 는 연결 그리드(host/user/port/label/shell + Passkey 드롭다운, **순수 연결정보만** — remote_tasty 없음), tasty-attach 는 Connection 세그먼트 토글(ref=SSH 프로필 드롭다운 ↔ inline=host/user/port/shell/passkey) + 공통 Remote tasty 그룹(remote_tasty/port_mode/port_file), 그 외(smb/http/미등록)는 generic key-value 에디터. ssh 셸 `auto` 는 저장 시 1회 SSH 프로브로 셸 도달성을 감지(detect-split: port_mode 는 attach resolve 가 도출).
- **GUI 3탭** — Remote profiles · **Attach** · Passkeys. Attach 탭이 tasty-attach kind 를 전담하고, Profiles 탭 목록·프로토콜 필터에서 tasty-attach 는 제외된다. Attach 행은 mode 태그(profile/inline)와 상태 배지를 보여준다 — 참조 ssh 프로필이 감지실패면 **비활성**, `ssh_ref` 가 dangling 이면 **프로필 없음**(둘 다 경고 배지, hard-error 아님 — 저장은 정상).
- **dangling 참조** — 없는 passkey 를 가리켜도 정상 저장, "passkey 없음" 노란 배지 + 소비 시점 에러.
- **값 마스킹** — passkey 값은 기본 마스킹(`••••••••`). GUI 의 **Reveal**(로컬 전용)만 실제 값을 본다(path=경로, inline=관리 파일 내용). AI Agent/원격은 IPC 로 값을 **영구 읽을 수 없다**(쓰기만).
- **프로토콜 필터** (GUI 전용·세션 한정) — 원격 접속 프로필 탭에서 현재 프로필의 `kind` 가 2종 이상일 때만 필터 버튼이 뜬다. 체크박스 드롭다운에서 프로토콜을 고르고 적용(apply-on-confirm)하면 선택한 `kind` 의 프로필만 목록에 남는다. 결과 0건이면 빈 상태 안내를 표시한다. 필터 상태는 **비영속** — popup 재오픈에는 유지되지만 tasty 재시작 시 전체 선택으로 리셋되며, 저장 파일/CLI/IPC 표면에는 영향이 없다(순수 표시 필터).
- **로컬 ssh config 가져오기** — 사용자의 `~/.ssh/config`(+ `Include`)에 이미 있는 Host alias 를 열거하고(`list-local` / `remote.profile.list_local`), 그중 하나를 ssh 프로필로 등록한다(`import` / `remote.profile.import`). **저장하는 값은 alias 문자열 하나**(`fields.host`)와 `shell="auto"` 뿐이다 — `HostName`/`User`/`Port`/`ProxyJump`/`IdentityFile` 을 펼쳐 복사하지 않는다. ssh 가 `host` 자리의 alias 를 그대로 해석하므로 복사 없이 접속되고, 복사하면 ssh config 수정 시 값이 어긋나며(drift) `ProxyJump` 류는 애초에 복사 대상에서 빠진다. 목록에 함께 보이는 `HostName`/`User`/`Port` 는 **표시 전용 hint** 라 프로필에 들어가지 않는다.
  - 열거는 **순수 파일 파싱**이다 — `ssh -G` 를 쓰지 않는다(`Match exec "…"` 를 실제로 실행해 버리고, 애초에 alias *목록* 을 주지도 않는다). 와일드카드·부정 패턴(`Host *` / `jump-*` / `!bad`)은 접속 가능한 이름이 아니라 제외한다.
  - **가져오기는 셸을 감지하지 않는다.** 감지는 실제 SSH 접속이라 목록에서 여러 건을 가져오면 접속이 연쇄로 일어난다 — 가져온 뒤 `tasty tool remote-profile detect --name <n>` 로 사용자가 돌린다(`add-ssh` 와 다른 점).
  - 이름이 겹치면 **거부**한다(덮어쓰기 없음). 없는 alias 를 가리켜도 거부한다.
- **소비자 분리** — attach 는 **tasty-attach kind** 프로필을 읽는 소비자(ADR-0032). tasty-attach 는 ssh 프로필을 `ssh_ref` 로 참조하거나 인라인 연결을 갖는다. "주소 저장(ssh)"과 "attach 스펙(tasty-attach)"이 분리됐다(→ [remote-attach](../remote-attach/index.md)).
- **입력 격리** — 팝업이 터미널 위에 떠 있어도 팝업 위 클릭/스크롤은 팝업이 소비하며, 뒤 터미널의 포커스·선택·스크롤을 건드리지 않는다. remote_tool 고유 규칙이 아니라 모든 팝업에 적용되는 입력 레이어 계약(Layer 3 = Popup, "팝업 위면 터미널 무시")을 따른 것이다(→ [input-layer](../../architecture/input-layer.md)).

## 인터페이스

| 표면 | 진입 |
|---|---|
| GUI | 도구 메뉴 > Remote connections (popup `remote_tool`) |
| CLI (프로필 CRUD) | `tasty tool remote-profile list\|show\|add-ssh\|add-attach\|edit\|remove\|detect\|list-local\|import` — ssh + tasty-attach 통합. `list-local [--json]`(로컬 ssh config alias 열거, IMPORTED 열에 이미 가져온 프로필 이름) / `import --from <alias> --name <profile> [--label …]`(alias 만 저장, 셸 감지 없음). `add-ssh`(순수 연결) / `add-attach`(`--ssh-ref` XOR 인라인 + `--remote-tasty`/`--port-mode`/`--port-file`). `--identity` → path passkey 자동 생성. `list --kind ssh\|tasty-attach` 필터 |
| CLI (접속/attach) | `tasty tool ssh <profile> [--command …]` — ssh 프로필로 대화형 접속 실행 · `tasty tool attach <name> <surface\|--workspace> \| --list` — tasty-attach 프로필로 attach(→ [remote-attach](../remote-attach/index.md)) |
| CLI (passkey) | `tasty tool passkey add\|list\|show\|remove` (`--path`/`--inline`; list/show 는 값 비노출) |
| IPC | `remote.profile.{list,get,add,detect,remove,list_local,import}`(kind-generic — `kind="tasty-attach"` + `fields` 로 tasty-attach CRUD 도 양면 노출) · `remote.passkey.{list,get,add,remove}` (값 마스킹). `list_local` 은 `{aliases:[{name,source,hostname,user,port,imported_as}], config_path, config_exists}`, `import` 은 `{from,name,label?}` → `{saved,name,from,detecting:false}` (자동 감지 없음). 구 `tool.ssh.*`/`ssh.profile.*` 는 alias 한시 호환 |

## 마이그레이션

없음. 구 `ssh-profiles.toml → remote-profiles.toml` 자동 마이그레이션은 [ADR-0032](../../adr/0032-remote-attach-two-layer-split.md)(하위호환 제거)로 삭제됐다. 기존 프로필에 남은 `remote_tasty`/`port_mode` 필드는 열린 스키마라 무시되며 크래시하지 않는다 — attach 하려면 `tool remote-profile add-attach` 로 tasty-attach 프로필을 새로 만든다.
