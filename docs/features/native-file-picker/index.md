# 네이티브 파일 피커 (로컬+원격 겸용)

- **Status**: Implemented
- **주체**: 로컬 사용자 (Tools 메뉴 트리거) + plugin(`file_picker.trigger` IPC, TODO 21)
- **ADR**: [ADR-0053](../../adr/0053-native-file-picker-remote-attach-channel.md) (attach 커스텀 이벤트 채널 + 하이브리드 신뢰 모델), [ADR-0058](../../adr/0058-plugin-triggered-host-popup-async-ack-push.md) (plugin 트리거 — 즉시 ack + 이벤트 push). 관련: [ADR-0042](../../adr/0042-fs-pick-file-native-dialog-host-delegation.md)(로컬 전용 `fs.pick_file`, 별개 메커니즘으로 계속 유효)
- **코드**: `src/adapters/ui/popup/file_picker.rs`(popup wrapper/view/action), `src/core/fs_list.rs`(공유 디렉토리 나열), `src/adapters/ui/tools_menu.rs`(Tools 메뉴 트리거), `src/adapters/ipc/handler/file_picker.rs`(`file_picker.trigger` — plugin 트리거), `src/app/dispatch/file_picker.rs`(result drain + plugin 에게 `"file_picker.result"` push), `src/core/attach_runtime.rs`(서버측 `handle_list_dir_request`), `src/app/attach_client.rs`(client 원격 파싱 + `MirrorEvent::ListDirResult`), `src/adapters/production/stream_hub.rs`(`ListDirRequestMsg` 분류), `crates/tasty-plugin-markdown/src/main.rs`(Browse 버튼 caller)
- **화면**: 없음 (popup 은 갤러리 specimen `crates/tasty-gallery/src/catalog/components/file_picker.rs` 로 시각 확인)

## 목적

Tasty 자체 in-app "파일 열기" 다이얼로그. 로컬 파일시스템뿐 아니라 **attach mirror 워크스페이스가
브라우징하고 있는 원격 파일시스템**도 같은 UI 로 탐색할 수 있게 한다 — native OS 다이얼로그
(ADR-0042 `fs.pick_file`)는 host 프로세스 로컬 파일시스템만 알 뿐 원격이라는 개념이 없다.

## 내부 동작 (headless-valid)

### 로컬/원격 판별

Tools 메뉴에서 파일 피커 항목을 클릭하면(`src/adapters/ui/tools_menu.rs::open` 경유,
`popup::file_picker::open`), 현재 활성 workspace(`state.active_workspace`)의 `Workspace.mirror`
플래그를 1회 확인해 로컬/원격을 고정한다. `FilePickerData.mirror_ws_id: Option<u32>` 가 `Some`
이면 원격, `None` 이면 로컬이다.

### 로컬 브라우징

`crate::core::fs_list::read_dir_entries` 를 popup wrapper 가 직접 동기 호출한다(host 프로세스
내부 함수 — 별도 IPC 왕복 없음). 결과를 `sort_entries` 로 정렬해 즉시 `Loaded`/`Empty` 로 전이한다.

### 원격 브라우징 (attach 커스텀 이벤트)

원격은 attach 세션이 이미 열어둔 `StreamTag::Control` 채널을 그대로 재사용한다(스크린샷 캡처
기능의 `capture_chunk`/`capture_commit`/`capture_result` 와 동일한 "`StreamControl` enum 이
인식 못 하는 `event` 태그" 패턴).

1. wrapper 가 `CoreState::pending_list_dir_forward` 에 `{ local_ws_id, request_id, dir }` 를
   push 하고 popup 상태를 `FpLoadState::Loading { request_id, sent_at }` 로 전이.
2. App 이 `about_to_wait` 에서 큐를 drain 해 attach 세션 writer 로 `list_dir_request` 프레임을
   전송(`src/app/attach_client.rs::send_list_dir_request`).
3. 원격 인스턴스 서버측(`src/core/attach_runtime.rs::handle_list_dir_request`)이
   **attach 점유 = 신뢰**(`engine.attach.client_holds_workspace(client_id)`)만으로 인가 판정 —
   별도 permission 게이트 없음. 인가되면 같은 `read_dir_entries` 로 대상 디렉토리를 읽어
   `list_dir_result` 로 회신.
4. client 의 attach reader thread 가 `list_dir_result` 를 파싱해 `MirrorEvent::ListDirResult` 로
   변환하고, `apply_attach_client_output` 이 그 popup 상태(`request_id` 일치 확인 — stale reply
   무시)에 직접 반영한다.

### 타임아웃(soft timeout + mirror 소멸 관측)

- `Loading` 상태에서 8초(`LIST_DIR_SOFT_TIMEOUT`) 안에 응답이 없으면 wrapper 가 매 프레임
  `sent_at.elapsed()` 를 확인해 `ErrorConn` 으로 전이한다.
- 별도로, mirror workspace 자체가 사라지면(원격 disconnect 로 `cleanup_mirror_workspace` 가
  실행돼) `engine.find_workspace_index_for_id` 조회가 실패하는 것을 관측해 즉시 `ErrorConn` 으로
  전이한다 — attach 세션의 raw `disconnected` 플래그는 App 소유라 popup wrapper 에서 직접 읽을
  수 없어, 그 대신 이 "mirror workspace 소멸"이라는 상위 결과로 판별한다.

### wire 포맷 (epoch ↔ SystemTime)

`DirEntryInfo`(`src/core/fs_list.rs`)는 로컬/원격 어디서 만들어졌든 항상 `Option<SystemTime>` 을
든다. wire 조립/파싱 경계(`list_dir_entry_wire`/`parse_list_dir_result`)에서만
`modified_unix: u64`(unix epoch 초)로 변환한다. 사람이 읽는 포맷(`"YYYY-MM-DD"`,
`fs_list::format_modified`)은 view 렌더 직전에만 계산 — 로컬/원격 어느 쪽도 이 함수 하나를
공유한다.

### 대형 디렉토리 truncation (프레임 크기 안전장치)

attach 채널의 프레임 하드 상한(`crate::ipc::stream::MAX_FRAME_LEN`, 1MiB)을 넘는 `list_dir_result`
는 `write_frame` 이 에러를 반환하고, 그 결과 그 attach 세션의 write thread 전체가 종료된다(다른
forward/tap 도 동반 — file picker 뿐 아니라 mirror 연결 자체가 끊긴다). `handle_list_dir_request`
는 이를 막기 위해 entries 를 직렬화하며 누적 바이트를 추적하다 예산
(`LIST_DIR_ENTRIES_BYTE_BUDGET`, 700KiB)을 넘기기 직전에 멈추고 `truncated: true` 를 wire 에
싣는다. client 는 `truncated` 를 받으면 toast(`filepicker.remote_listing_truncated`)로 알린다 —
현재 UI 는 "다음 페이지" 개념이 없어 사실상 상위 700KiB 분량만 보여주는 제약이다(비-목표 참고).

### 확정(Confirm)

- **로컬**: 선택 경로로 `DomainIntent::DispatchFile { depth: Deep, .. }` 를 발화해 기존 파일 핸들러
  디스패치 경로(explorer/markdown 오픈과 동일)로 넘긴다.
- **원격**: 이번 구현은 **디렉토리 나열까지만** 스코프이며 원격 파일 **내용**을 이 세션으로 가져오는
  fetch 는 하지 않는다. 선택 경로를 클립보드에 복사하고 결과 toast 를 띄운다
  (`src/app/dispatch/file_picker.rs::apply_remote_confirm`).
- **디렉토리는 확정할 수 없다**: [열기] 버튼은 선택된 엔트리가 전부 파일일 때만 활성화되고
  (`draw_file_picker_view` 의 `can_open`), wrapper 의 `apply_action` 도 동일 조건을 다시
  확인한다(방어적 중복 검증). 디렉토리는 더블클릭으로만 진입한다.

### 원격 경로 구분자(POSIX/Windows)

원격 host 의 OS 는 client 가 사전에 알 수 없다 — `path_ancestors`/`join_dir`/`crumb_label` 은
서버가 돌려준 경로 문자열 자체에 `\` 가 있는지로 Windows 스타일(`C:\Users\alice`, 드라이브 루트
보존)과 POSIX(`/`)를 구분한다(`is_windows_style_remote_path`).

### plugin 트리거(TODO 21, ADR-0058) — 즉시 ack + 이벤트 push

markdown plugin 의 "파일 열기" 팝업 Browse 버튼처럼, host 소유 popup 을 열고 사용자가 몇 프레임
뒤에나 확정/취소할지 모르는 인터랙션을 **plugin 이** 트리거해야 하는 경우의 IPC 경로다.
`fs.pick_file`(동기 inline dispatch — OS native 모달이 자기 run loop 를 돌리므로 host 메인
스레드를 블로킹해도 안전)과 달리, host 자체 egui popup 은 OS 가 대신 블로킹해주지 않는다 —
지연 회신 방식(`host.call` 자체를 확정 시점까지 붙잡아 둠)은 plugin 의 렌더/입력 루프를
멈추고 60 초 `HostCallTimeout` 위험을 진다(ADR-0058 Alternatives Considered).

1. plugin 이 `file_picker.trigger { filters?: string[] }` 를 호출한다(`FsRead` 권한,
   `gui` feature 전용). host 는 popup 확정을 **기다리지 않고** `{ request_id }` 만 즉시
   회신한다(`src/adapters/ipc/handler/file_picker.rs::handle_trigger`).
2. host 는 `(plugin_id, request_id)` 를 `FilePickerData.requester`(`FilePickerRequester`)
   에 기록하고 popup 을 연다 — 이후 로컬/원격 판별·엔트리 로드는 위 기존 경로(Tools 메뉴
   트리거와 동일)를 그대로 탄다.
3. 사용자가 확정/취소하면(`dispatch_pending_file_picker_results`/`apply_remote_confirm`),
   기존 동작(로컬 `DomainIntent::DispatchFile`, 원격 클립보드 복사+toast)에 **더해**
   `PluginManager::emit_host_event_to_plugin` 으로 `"file_picker.result"` 이벤트를 그
   plugin 에만 unicast 한다: `{ request_id, paths, cancelled }`(확정도 취소도 항상 이 세
   필드를 전부 채운다 — 취소는 `paths: []`/`cancelled: true`).
4. plugin 은 `on_event` 에서 이 key 를 받아 자기 `request_id` pending-map 으로 상관관계를
   맞춘다(신규 SDK 콜백 불필요 — 기존 `EventDispatchCtx` 재사용).

**`filters`**: 확장자 목록(점 없이, 예: `["md", "markdown"]`) — `draw_file_picker` 가 렌더
직전 `matches_filters` 로 파일 엔트리만 걸러낸다(디렉토리는 필터와 무관하게 항상 표시 —
내비게이션 대상이라 숨기면 하위로 못 들어간다).

**동시성 정책(ADR-0058 이 이 구현에 위임한 결정)**: `file_picker` popup 은 단일 인스턴스만
존재한다. 이미 열려 있는 상태에서 두 번째 `file_picker.trigger` 가 오면 **거부**한다(즉시
`-32000` JSON-RPC 에러) — "이전 요청을 대체" 는 채택하지 않았다. 트리거 핸들러는 `CoreState`
에만 접근하고 `PluginManager`(이벤트 emit 에 필요)에 접근권이 없어(이 코드베이스의 확립된
관례 — IPC 핸들러는 pending 을 큐잉하고 App 레벨 dispatch 가 실제 emit), "대체" 를 택하면
밀려난 요청의 plugin 에게 즉시 취소를 통지할 방법이 없어 그 plugin 의 pending-map 항목이
응답을 영영 못 받는다(ADR-0058 이 세운 "모든 트리거는 정확히 하나의 결과를 받는다" 계약
위반). 거부는 두 번째 plugin 의 `host.call` 이 그 자리에서 에러로 끝나 재시도 여부를
판단하게 하므로 이 계약을 지킨다. 근거 전문: `src/adapters/ipc/handler/file_picker.rs` 모듈
doc.

**Origin 태깅**: `file_picker.trigger` 로 연 popup 의 `OpenPopup` intent 는
`Intent::from_agent_plugin(plugin_id)` 로 발화한다(Tools 메뉴는 `from_user_menu` 그대로) —
`from_agent_plugin` 은 이 작업 전까지 실사용처가 없던 builder 였다(TODO13 wiring 전 상태,
`src/intent.rs`).

## 인터페이스

- **사용자 트리거**: Tools 메뉴 "파일 열기…"(`filepicker.tools_menu_item`). 목록 행 더블클릭
  (디렉토리는 진입, 파일은 즉시 확정) / 브레드크럼 클릭 / 상위 폴더 버튼 / 새로고침 버튼 /
  ESC(취소) / X 버튼(취소).
- **AI Agent (IPC/CLI)**: 없음 — popup 조작(선택/확정/취소) 자체는 순수 로컬 사용자 입력
  UI 다(release 의 사용자 입력 재현 금지 원칙). 단, **popup 을 여는 트리거**는
  `file_picker.trigger` IPC 로 plugin 에 열려 있다(TODO 21, ADR-0058) — markdown Browse
  버튼이 실사용처. 이건 "에이전트가 사용자 대신 파일을 고른다"가 아니라 "plugin 이 host 소유
  UI 를 사용자에게 대신 띄워준다" 는 위임이라 원칙과 상충하지 않는다(뒤이은 선택/확정은
  여전히 사용자 몫).
- **원격 / 점유**: 원격 디렉토리 요청은 그 attach 세션이 대상 workspace 를 이미 점유하고 있어야
  서버가 응답한다(mirror 워크스페이스 존재의 전제조건과 동일).

## 비-목표 (Out of scope)

- **원격 파일 내용 fetch** — 디렉토리 나열만. 확정 시 클립보드 복사 + toast 로 그친다. 이건
  `file_picker.trigger` 로 열린 경우도 동일 — 확정 결과는 plugin 에 `paths` 로만 전달되고,
  그 경로의 내용을 이 세션으로 가져오는 fetch 는 없다.
- **멀티 셀렉트 / 파일명 직접 입력** — 현재는 단일 선택만 지원(`FilePickerData::selected` 는 매번
  교체). 파일명 텍스트 편집 필드도 없다 — footer 는 선택된 이름을 읽기전용으로 보여줄 뿐이다.
- **`StreamControl` enum 확장** — capture 패턴과 동일하게 그 enum 을 건드리지 않고 별도 `event`
  태그를 같은 채널에 얹었다.
- **대형 원격 디렉토리의 페이지네이션** — 700KiB 예산을 넘는 나머지는 truncation 으로만
  처리한다(toast 통지). "다음 페이지 로드" 는 이번 스코프 밖.
- **다중 plugin 트리거 큐잉/대체** — 동시성 정책은 "거부" 뿐이다. 대체/큐잉을 원하는
  케이스가 실사용에서 반복되면 ADR-0058 의 Reconsideration Triggers 대상이다.

## Acceptance Criteria

- [x] Given 로컬(비-mirror) workspace 가 활성 When Tools 메뉴에서 파일 피커를 열면 Then 로컬 홈
  디렉토리 엔트리가 즉시(동기) 로드되어 표시된다.
- [x] Given mirror workspace 가 활성 When Tools 메뉴에서 파일 피커를 열면 Then `Loading` 상태를
  거쳐 attach 채널로 받은 원격 홈 디렉토리 엔트리가 표시되고 헤더에 host 배지가 뜬다.
- [x] Given 원격 요청 전송 후 8 초 안에 응답이 없음 Then `ErrorConn` 상태로 전이한다.
- [x] Given mirror workspace 가 도중에 사라짐(disconnect) Then popup 이 즉시 `ErrorConn` 으로
  전이한다(soft timeout 만료를 기다리지 않음).
- [x] Given 원격 디렉토리 읽기가 권한 거부로 실패 Then `ErrorPerm` 상태로 전이한다.
- [x] Given attach 점유가 없는 client 가 `list_dir_request` 를 보냄 Then 서버가 거부 회신한다
  (`ok: false`).
- [x] Given 로컬 파일을 확정 Then `DomainIntent::DispatchFile` 이 발화되어 기존 오픈 경로로
  이어진다.
- [x] Given 원격 파일을 확정 Then 선택 경로가 로컬 클립보드에 복사되고 toast 가 뜬다(원격 콘텐츠
  fetch 는 일어나지 않는다).
- [x] Given X 버튼/ESC/외부 클릭으로 popup 이 닫힘 Then 결과가 `Cancelled` 로 명시되어 dialog
  상태가 정리된다(다음 오픈에 이전 상태가 새지 않음).
- [x] Given 디렉토리 엔트리가 선택됨(더블클릭 아님) Then [열기] 버튼이 비활성화되고, 우회
  호출로 Confirm 이 와도 wrapper 가 다시 거부한다(디렉토리는 파일로 확정되지 않는다).
- [x] Given entries 직렬화가 바이트 예산(700KiB)을 넘는 대형 원격 디렉토리 Then 서버가
  entries 를 잘라 `truncated: true` 로 회신하고, client 는 경고 toast 를 띄운다(attach 세션
  자체는 끊기지 않는다).
- [x] Given 원격 host 의 경로가 Windows 스타일(`C:\Users\alice`) Then 브레드크럼/내비게이션이
  `\` 구분자와 드라이브 루트를 올바르게 다룬다(POSIX 원격도 계속 정상 동작).
- [x] Given plugin 이 `file_picker.trigger` 를 호출 When popup 이 아직 열려있지 않음 Then
  즉시 `{ request_id }` 로 회신하고 popup 이 열린다(확정을 기다리지 않음).
- [x] Given `file_picker` popup 이 이미 열려 있음 When 두 번째 `file_picker.trigger` 가 옴
  Then 거부(`-32000` 에러) — 첫 요청의 `requester` 는 대체되지 않고 그대로 유지된다.
- [x] Given `filters: ["md"]` 로 트리거됨 Then 렌더된 엔트리 목록에서 `.md` 가 아닌 파일은
  제외되고(디렉토리는 필터와 무관하게 항상 표시), 확정/취소 시 `"file_picker.result"` 가
  그 요청을 낸 plugin 에만(unicast) push 된다.
- [x] Given Tools 메뉴로 연 기존 흐름(`requester: None`) Then `file_picker.trigger` 도입 후에도
  동일하게 동작하고 결과 이벤트가 발화되지 않는다(회귀 없음).

> **검증 한계(문서화)**: 원격 attach loopback e2e(`--ssh 127.0.0.1:<port>`)로 실제 GUI 두
> 인스턴스를 띄워 popup 을 열고 눈으로 확인하는 것은 이 headless 작업 환경(GPU 디스플레이
> 없음)에서 실행할 수 없었다. 대신 `tests/attach_list_dir_loopback.rs` 가
> `tests/attach_silent_disconnect.rs` 와 동일한 방식으로 **실제로 기동한 `tasty` 서버
> 인스턴스**에 raw `TcpStream` 으로 `stream.open{target_workspace}` 핸드셰이크를 걸어 진짜
> attach 점유를 획득한 뒤, `list_dir_request` 를 보내 서버가 **실제 디스크의 임시 디렉토리**를
> 읽어 `list_dir_result` 로 정확히 회신하는 전체 왕복을 검증한다(성공 케이스, 존재하지 않는
> 디렉토리의 에러 케이스, attach 점유 없는 client 의 거부 케이스 3가지). GUI 렌더링(popup 이
> 그 결과를 실제로 화면에 그리는 것)만 코드 리뷰로 대체했다 — `draw_file_picker`/
> `draw_file_picker_view` 는 `AppState`/`CoreState` 를 받는 순수 함수라 GUI 없이도 로직은
> 동일 경로를 타지만, 실제 픽셀 렌더는 이 환경에서 확인하지 못했다. 이는
> `docs/features/remote-attach/index.md` / `remote-screenshot-clipboard/index.md` 가 이미
> 기록한 것과 동일한 종류의 한계이되, 이번 작업은 실제 서버 프로세스를 상대로 한 프로토콜
> 왕복까지는 실행 검증했다는 점에서 그 두 문서보다 한 단계 더 나아간 커버리지다.
>
> **`file_picker.trigger`(TODO 21) 검증**: 격리된 `TASTY_HOME` 으로 기동한 실제 debug
> `tasty` 인스턴스에 raw `TcpStream` 으로 JSON-RPC(`file_picker.trigger`)를 직접 보내
> `route_engine_handler` 라우팅 전체(dispatch table → `handle_trigger` → `popup::file_picker::
> open`)를 실행 검증했다 — 1 차 호출은 `{ request_id: 1 }` 로 성공, popup 이 열린 상태에서의
> 2 차 호출은 정확히 그 자리에서 설계한 busy 에러(`-32000`, "file_picker popup is already
> open — retry after it closes")로 거부됨을 확인했다. plugin 프로세스(markdown)가 실제로
> `trigger_file_picker`/`on_event` 를 왕복하는 것과 popup 의 픽셀 렌더는 이 환경에서
> 실행하지 못해 코드 리뷰로 대체했다 — 다만 그 왕복이 재사용하는 `emit_host_event_to_plugin`
> 자체는 `git_viewer.query_result` 로 이미 프로덕션에서 검증된 동일 경로다.

## 구현

- Popup: `src/adapters/ui/popup/file_picker.rs`(`FilePickerProps`/`FilePickerAction`/
  `draw_file_picker_view`/`draw_file_picker`), `src/adapters/ui/popup/defs.rs`(`PopupDef` 등록),
  `src/adapters/ui/popup.rs`(모듈 선언), `src/adapters/ui/notification.rs`(X/외부 닫기 →
  `Cancelled` 명시).
- 공유 나열: `src/core/fs_list.rs`(`DirEntryInfo`/`read_dir_entries`/`sort_entries`/`human_size`/
  `format_modified`) — `src/adapters/ui/surface/explorer/view.rs`(Explorer surface)와 공유.
- 트리거: `src/adapters/ui/tools_menu.rs`(`BuiltinAction::OpenFilePicker`, `popup::file_picker::open`,
  `requester: None`), `src/adapters/ipc/handler/file_picker.rs`(`file_picker.trigger` — plugin
  트리거, TODO 21/ADR-0058), `crates/tasty-ipc/src/method_meta.rs`(`file_picker.trigger` →
  `FsRead`).
- Result drain: `src/app/dispatch/file_picker.rs`(`dispatch_pending_file_picker_results`,
  `apply_remote_confirm`, `emit_file_picker_result` — `requester` 가 `Some` 이면
  `"file_picker.result"` unicast), `src/app/event_handler.rs`(`about_to_wait` 호출).
- plugin 요청자 상태: `src/state.rs`(`FilePickerRequester`, `FilePickerData.requester`/
  `filters`), `src/core/mod.rs`(`next_file_picker_trigger_request_id` — `FpLoadState::Loading`
  의 내부 `request_id` 와 별개 네임스페이스), `src/intent.rs`(`Intent`/`UiIntent::
  from_agent_plugin` — 이 트리거가 첫 실사용처).
- plugin caller: `crates/tasty-plugin-markdown/src/main.rs`(`trigger_file_picker`,
  `pending_file_picker`, `on_event` 의 `"file_picker.result"` 수신, `FILE_PICKER_RESULT_EVENT`).
- 원격 요청 큐: `src/core/mod.rs`(`PendingListDirForward`, `next_list_dir_request_id`),
  `src/core/state.rs`(`CoreState.pending_list_dir_forward`).
- 원격 전송(client): `src/app/attach_client.rs`(`send_list_dir_request`, `parse_list_dir_result`,
  `MirrorEvent::ListDirResult`, `apply_attach_client_output` 반영, `dispatch_pending_list_dir_forwards`).
- 원격 수신(server): `src/adapters/production/stream_hub.rs`(`ListDirRequestMsg`, `pump_inbound`
  분류), `src/core/attach_runtime.rs`(`handle_list_dir_request`, `list_dir_for_request`,
  `list_dir_entry_wire`, `list_dir_entries_wire_capped`/`LIST_DIR_ENTRIES_BYTE_BUDGET`). GUI
  (`src/app/event_handler.rs::apply_list_dir_request_msg`)와 headless(`src/boot.rs`) 양쪽
  진입점에서 동일 서버 로직을 호출.
- Popup 상태: `src/state.rs`(`FilePickerData`, `FpLoadState`, `FilePickerResult`).
- i18n: `lang/{en,ko,ja}.toml` `[filepicker]`/`[filepicker.error_perm]`/`[filepicker.error_conn]`.
- 갤러리 specimen: `crates/tasty-gallery/src/catalog/components/file_picker.rs`.
- 테스트: `src/adapters/production/stream_hub.rs`(`pump_inbound_classifies_list_dir_request`),
  `src/core/fs_list.rs`(`human_size_units`/`sort_dirs_first`/`read_dir_entries_lists_files_and_dirs`),
  `src/core/attach_runtime.rs`(`list_dir_entries_wire_capped_tests` — byte-budget truncation),
  `src/adapters/ui/popup/file_picker.rs`(`path_helper_tests` — POSIX/Windows 원격 경로 처리 +
  `matches_filters_*` 확장자 필터), `src/adapters/ipc/handler/file_picker.rs`(`tests` —
  trigger 성공/requester 기록/busy 거부/filters 전달, 실제 `AppState`/`CoreState` fixture),
  `crates/tasty-ipc/src/method_meta_tests.rs`(`file_picker_trigger_requires_fs_read`),
  `tests/attach_list_dir_loopback.rs`(실제 서버 인스턴스 상대 loopback 왕복 3종 — 성공/디렉토리
  없음 에러/attach 점유 없는 client 거부).
