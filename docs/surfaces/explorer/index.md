# Explorer (내장 파일 관리자 surface)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent ([주체](../../concepts/actors.md))
- **ADR**: [ADR-0022](../../adr/0022-remote-mirror-content-and-queries.md) (원격 attach mirror 브라우징 — list_dir 채널 재사용, 파일 변경 미지원, 더블클릭은 원격 탭 열기)
- **코드**: surface kind 등록 `register_explorer` (`src/runtime/surface_registry/builtins.rs`), 모델 `ExplorerPanel`/`ExplorerTab` (`crates/tasty-model/src/explorer_panel.rs`), 뷰 스토어 `ExplorerView`/`ExplorerViewStore` (`src/adapters/ui/surface/explorer/view.rs`), 렌더 `draw_explorer` (`src/adapters/ui/surface/explorer.rs`), deferred action 적용 `apply_explorer_action` (`src/adapters/ui/egui_panels.rs`)
- **화면**: 호스트 내장 egui surface

## 목적·지원 범위

OS 파일 관리자에 의존하지 않고 tasty surface 안에서 디렉토리를 탐색하고 파일을 열기 위한 내장 파일 관리자다. 다른 surface(terminal/markdown/image)와 동일하게 pane/tab 레이아웃에 들어가고, surface 변환·이동·레이아웃 영속화 대상이 된다. surface kind `"explorer"`는 부팅 시 `register_builtin_kinds`가 등록한다.

지원하지 않는 범위:

- 파일 식별/렌더 정책 — explorer 는 열기를 [file-handler](../../features/file-handler/index.md) 에 위임한다.
- 컨텍스트 메뉴 파일 조작(복사/잘라내기/붙여넣기/이름변경)은 **에이전트(IPC/CLI) 노출 대상이 아니다** — 사용자 우클릭 조작 전용. surface 단위 이동은 [surface-move](../../features/surface-move/index.md) 가 별도 제공한다.

## 상태 소유자

| 상태 | 소유자 | 바꾸는 경로 |
|---|---|---|
| 구조 트리의 leaf(`SurfaceDescriptor`, kind `explorer`) | engine 의 구조 트리 | 공통 생성·닫기·이동·변환([작업 영역](../../features/work-area/index.md)) |
| 내비게이션 모델 `ExplorerPanel`(내부 탭·cwd·current·히스토리·뷰 모드·정렬·미리보기 토글과 폭) | engine 의 surface 실행 객체(`EngineRuntime::surfaces`) | View 가 `EngineAction::Explorer`·`EngineAction::ExplorerCwd`·`EngineAction::ExplorerPreview` 로 요청하고, App 이 요청에 담긴 `SurfaceBinding` 이 아직 그 surface 를 가리킬 때만 적용한 뒤 레이아웃을 dirty 로 표시한다(`src/app/engine_action.rs`) |
| 즐겨찾기 목록 | 프로세스 원본 `RuntimeRegistries::explorer_favorites`(`SharedList<ExplorerFavorites>`). engine 의 `EngineRuntime::explorer_favorites` 는 그리기용 사본(`Replica`) | `EngineAction::AddExplorerFavorite`·`RemoveExplorerFavorite` 가 원본을 바꾸고 바로 파일에 쓴다(아래 "즐겨찾기") |
| 목록 캐시·로딩 상태·선택·트리 펼침·주소창 편집·타입어헤드 | 창의 View 상태 `MainViewState::explorer_views`(surface id 별 `ExplorerView`) | View 가 직접 바꾼다. 저장하지 않는다 |
| 파일 클립보드 | 창의 View 상태 `MainViewState::explorer_clipboard` | 복사·잘라내기가 채우고 붙여넣기가 소비한다. 저장하지 않는다 |
| 로컬 디렉터리 읽기 | App 의 읽기 worker(`src/app/local_reads.rs`, 동시 실행 최대 4개) | View 가 요청을 만들고 받은 receipt 로 결과를 가져간다 |
| 파일 변경 작업 | App 의 `explorer_files` worker(한 번에 한 작업) | View 가 대상 경로와 binding 을 고정한 요청을 넘긴다([파일 작업 계약](file-operations.md)) |
| mirror 목록 조회 | 원격 attach 채널(`list_dir_request`/`list_dir_result`) | View 가 요청을 outbox 에 쌓고 렌더 뒤 App 이 원격으로 보낸다 |

View 는 모델을 직접 바꾸지 않고 파일시스템도 직접 읽지 않는다. 경계 전체는 [Model·View 분리](../../dev-guide/model-view-split.md)에 있다.

### 모델 (`ExplorerPanel`)

- `ExplorerPanel` 은 식별(`id`)과 내비게이션 상태, 미리보기 패널 상태를 보유한다 — 내부 탭 목록(`tabs`)·활성 탭 인덱스(`active`)·미리보기(`preview`: `ExplorerPreview { open, width }`, 내부 탭과 관계없이 surface 에 하나, 폭은 UI 배율 1 기준이며 `None` 이면 토큰 기본 폭). 각 `ExplorerTab` 은 **cwd(고정 루트)** 와 **current(현재 폴더, 필드명 `root`)** 를 분리해 보유하고, 히스토리(back/forward 스택), 뷰 모드(`view_mode`), 정렬 컬럼/방향(`sort_column`/`sort_dir`)을 가진다.
- **cwd ↔ current 분리** (VS Code 식 "고정 프로젝트 + 자유 탐색"): `cwd()` 는 explorer 를 연 프로젝트 루트로 **좌측 사이드바 트리 루트**·**스폰 cwd**(`source_cwd()`)·**surface/탭 표시명**의 기준이며 내비게이션에 불변. `current()`(=`current_root()`) 는 **우측 목록**·**상단 주소창(편집형 PathField)** 이 따라가는 탐색 폴더로, back/forward/go_up 이 이것만 움직인다. current 는 cwd 하위로 제한되지 않고 파일시스템 어디로든 자유 이동한다.
- 내비게이션: `navigate_to(dir)` / `go_back` / `go_forward` / `go_up` — 모두 **current 에만** 작용. `can_go_up` 은 current 의 파일시스템 부모 존재만 본다(cwd 경계로 clamp 안 함). `set_cwd(folder)` 는 cwd·current 를 folder 로 재설정하고 히스토리를 비운다(explorer-03 "이 폴더로 루트 설정"). 히스토리는 탭별로 독립.
- **`..` 상위 이동**: current 에 부모가 있으면(파일시스템 루트 아님) 우측 목록 최상단에 `..` 특수 행을 그려 상위 폴더로 이동한다. `..` 는 **렌더 전용**이라 `view.entries`/선택/상태줄/컨텍스트 메뉴 대상이 아니며 더블클릭 시 `Navigate(parent)` 만 emit 한다.
- 내부 탭: `add_tab`(활성 탭 cwd 복제, current=cwd) / `close_tab(idx)` / `active_tab[_mut]`. surface 하나 안에 여러 디렉토리 탭을 둔다 (상위 pane 탭과 별개). 탭별 cwd 는 독립(per-tab).

## 생성·갱신·종료

- **생성**: 공통 생성 경로(`tasty new tab --type explorer`, split, 변환, 단축키 `open_explorer`, 메뉴 "새 탭으로 열기")가 kind `explorer` 의 `create` 를 부른다. params 는 `path`(선택)와 `view_mode`(선택, 없으면 설정의 마지막 뷰 모드)다. root 결정 규칙은 아래 [IPC·CLI](#ipccli)에 있다. 메뉴 "새 탭으로 열기"는 우클릭한 surface 의 소유 pane 을 찾아 `DomainIntent::CreateTab` 을 보낸다(`RequestContext::add_kind_tab_by_owner`, `src/state/tab.rs`). 요청의 상속 cwd 는 우클릭한 surface 에서 가져오며 포커스와 무관하다([cwd 정책](../../design/policies/cwd.md)). Pane 을 직접 바꾸지 않는다.
- **목록 갱신**: View 를 그릴 때 `sync` 가 활성 탭의 (current, 정렬) 이 마지막으로 읽은 값과 다르거나 새로고침이 요청됐으면 읽기 요청을 새로 만든다. 상태는 `Loading` 이 되고, 이전 요청의 receipt 는 버린다. 버린 receipt 의 결과는 새 목록에 반영되지 않는다. 폴더가 바뀌었을 때만 선택을 비운다. 같은 폴더를 다시 읽으면 결과 목록에 없는 경로만 선택과 범위 선택 기준(anchor)에서 뺀다. 남은 선택은 같은 선택으로 본다(선택 식별자를 바꾸지 않는다). 폴더가 바뀌었거나 새로고침(`F5`·툴바·Retry)이 요청됐으면 사이드바 트리 캐시도 비워, 펼친 하위 폴더를 다시 읽는다. mirror 의 새로고침은 응답을 기다리는 경로를 빼고 경로별 결과를 지워 현재 폴더와 펼친 트리를 다시 요청한다.
- **결과 상태**: 읽은 목록은 `Ok`(항목 0개면 빈 폴더 화면), 권한 거부는 `NoPermission`, 그 밖의 실패(경로 없음·폴더 아님·IO 오류·worker 끊김)는 `Error` 다. 자동 재시도는 없고 Retry(새로고침)가 같은 경로를 다시 읽는다. mirror 는 응답이 8초 안에 오지 않으면 `Error`(시간 초과)로 바꾼다.
- **보이지 않는 동안**: 읽기 요청은 View 를 그릴 때만 만든다. 이미 보낸 요청의 결과는 그 View 가 남아 있는 한 다른 탭에 가려져 있어도 받아 둔다.
- **파일 작업 뒤 갱신**: 로컬 파일 작업이 끝나면 성공·실패와 관계없이 모든 윈도우의 로컬 explorer 중 작업이 바꿀 수 있는 폴더를 보는 View 를 다시 읽고, 그 폴더의 트리 캐시를 지운다. 범위는 [파일 조작](file-operations.md#작업-뒤-목록-갱신) 에 있다. mirror explorer 는 원격 파일을 보므로 제외한다.
- **현재 폴더가 사라짐**: 다시 읽은 결과가 경로 없음이면 읽기 오류 화면(다시 시도 · 상위 폴더로)을 보인다. 다른 경로로 자동으로 옮기지 않는다. 상위 폴더도 함께 사라졌으면 "상위 폴더로" 는 남아 있는 가장 가까운 상위 폴더로 간다. 다른 프로그램이 지웠다면 아래 [외부 변경 확인](#외부-변경-확인)이 찾았을 때 같은 화면이 된다.
- **늦은 결과**: 목록 결과는 receipt 를 가진 View 에만 들어간다. 그 사이 사용자가 바꾼 선택은 지우지 않는다. 파일 작업 완료의 선택 정리와 결과 카드·오류 토스트는 원 View 와 binding 이 유효할 때만 낸다. 실패한 rename·trash 는 선택을 유지하고 목록을 다시 읽는다. 부분 성공한 붙여넣기는 결과 카드에 실패 경로를 보이고 cut 클립보드를 유지한다. 사용자가 그 사이 새로 담은 클립보드는 건드리지 않는다.
- **종료**: surface 를 닫으면 engine 이 모델을 지우고, 창은 `release_surface_views` 로 그 surface 의 `ExplorerView` 를 지운다. 대기 중인 로컬 읽기의 receipt 도 함께 버려지고, 늦게 온 원격 응답은 기다리는 View 가 없어 버려진다. 아직 시작하지 않은 파일 작업 요청은 binding 이 더는 그 surface 를 가리키지 않아 시작하지 않는다. 이미 시작한 작업은 끝까지 실행되고 결과 카드·목록 갱신만 생략된다. 단, 이름 충돌 답을 기다리던 작업은 물을 칸이 없어졌으므로 취소한다(`src/app/explorer_files.rs`·`src/app/explorer_files/ui_sync.rs`). 진행·충돌·결과 표시는 [파일 작업](file-operations.md#작업-실행--진행충돌결과)에 있다.

### 외부 변경 확인

다른 프로그램이 바꾼 폴더는 OS 파일 감시 없이 주기 확인으로 찾는다. 결정과 대안은 [ADR-0076](../../adr/0076-explorer-outside-changes-by-focused-tab-polling.md)에 있다(`src/adapters/ui/surface/explorer/view/poll.rs`).

- **대상 칸**: 입력을 받는 창(OS 창 포커스)에서 포커스를 가진 surface 가 속한 탭의 로컬 explorer 만 확인한다(`checks_outside_changes`). 같은 탭을 나눈 다른 surface 에 포커스가 있어도 그 탭의 explorer 는 확인한다. 다른 탭에 가려졌거나 다른 Pane 의 탭에 있는 explorer, 포커스가 없는 창의 explorer 는 확인하지 않는다. Tasty 가 배경 앱이면 어느 explorer 도 확인하지 않는다. 창 포커스는 egui-winit 이 winit `Focused` 이벤트로 갱신하는 `RawInput.focused` 를 읽는다.
- **주기**: 탭이나 창이 포커스를 얻은 프레임에 바로 한 번 확인하고, 그 뒤로는 2 초(`EXTERNAL_POLL_INTERVAL`)마다 확인한다. 포커스를 잃으면 멈추고, 다시 얻으면 주기를 기다리지 않고 확인한다. 창 포커스가 바뀌면 `Focused` 이벤트가 그 창을 다시 그리게 표시하므로 그 프레임이 확인을 시작하거나 멈춘다. 주기 확인은 창을 그리지 않는다. App 은 가장 이른 다음 확인 시각에 `Tick::ExplorerPoll` 로 깨어나 마지막 프레임에 포커스 창의 포커스 탭으로 그려진 칸의 확인을 시작하고(`ExplorerViewStore::start_due_checks`), 확인 결과에서 바뀐 폴더만 다시 읽는다. 바뀐 폴더가 없으면 프레임을 그리지 않으며, 다시 읽은 결과가 와야 그린다.
- **확인 대상**: 현재 폴더와, 하위 목록을 읽어 둔 펼친 트리 폴더다. 펼치지 않은 트리 노드의 펼침 표시는 확인하지 않는다.
- **비교**: worker 에서 폴더 metadata(수정 시각·크기)만 읽어 마지막으로 읽었을 때의 표지와 비교한다. 항목은 읽지 않으므로 바뀌지 않은 폴더는 `read_dir` 하지 않는다. 목록을 읽을 때는 표지를 항목보다 먼저 읽어, 그 사이의 변경은 다음 확인에서 다시 읽힌다. 다른 읽기를 기다리는 동안에는 그 회차를 건너뛴다.
- **다시 읽기**: 표지가 바뀐 현재 폴더는 보이던 목록을 둔 채 다시 읽고, 결과가 오면 목록만 바꾼다. `Loading` 화면, 타입어헤드 초기화, 하위 폴더 검색 재시작은 하지 않는다. 선택·스크롤·포커스는 그대로이고, 결과 목록에 없는 항목만 선택과 anchor 에서 빠진다. 표지가 바뀐 펼친 트리 폴더는 보이던 하위 목록을 둔 채 그 폴더만 다시 읽고 결과로 바꾼다. 캐시를 먼저 비우지 않으므로 사이드바 트리의 높이와 스크롤도 그대로다. 트리에 캐시된 현재 폴더의 하위 목록은 목록 결과로 함께 바꾸므로 따로 읽지 않는다. 폴더가 사라졌으면 읽기 오류 화면이 된다.
- **mirror**: 원격 IO 비용 때문에 확인하지 않는다. 새로고침(`F5`)으로 맞춘다.
- **한계**: 폴더의 수정 시각은 항목이 생기거나 사라지거나 이름이 바뀔 때 바뀐다. 폴더 안 파일의 내용·크기·수정 시각만 바뀌면 찾지 못하므로 Detail 의 크기·날짜 열은 새로고침해야 맞는다. 수정 시각 해상도가 거친 파일시스템(FAT 2 초, HFS+ 1 초 등)에서는 목록을 읽은 직후 같은 시각 단위 안의 변경을 놓칠 수 있고, 다음 변경이나 새로고침 때 보인다. 확인 사이에 생겼다 사라진 항목은 보이지 않는다.

### 사용자 조작의 적용

렌더 중 발생한 사용자 상호작용은 `ExplorerAction`(OpenFile / Navigate / GoBack / GoForward / GoUp / Refresh / SetViewMode / SetSort / NewTab / CloseTab / SelectTab / ContextMenu / AddressRejected) 으로 모았다가 `apply_explorer_action(state, engine, sid, act)` 에서 적용한다. 파일 열기/새로고침은 뷰 스토어만 다룬다. 내비게이션·뷰모드·탭 조작은 origin surface 의 `SurfaceBinding` 을 담은 `EngineAction::Explorer` 로 요청하고, App 이 binding 이 아직 그 surface 를 가리킬 때만 `ExplorerPanel` 에 적용한다(포커스 독립). 경로가 바뀌면 `ExplorerView` 가 다음 draw 에서 자동 감지해 재로드한다.

- `AddressRejected` 는 주소 입력을 이동하지 않은 이유를 그 surface 범위의 오류 toast 로 알린다. 패널은 바꾸지 않는다.
- 파일 열기는 `DomainIntent::DispatchFile { origin_surface_id: Some(sid) }` 로 [file-handler](../../features/file-handler/index.md) 에 위임한다 — explorer 자신은 파일 식별/디스패치 정책을 모른다.

## 저장·복원

- **저장하는 값**: 내부 탭마다 cwd·current(`root`)·뷰 모드·정렬 열·정렬 방향, 활성 내부 탭 번호, 미리보기 패널(`register_explorer` 의 `snapshot`). 레이아웃 journal 에 들어간다.
- **미리보기 키**: `"preview": {"open": true, "width": 428.0}`. 닫혀 있으면 `open` 을, 기본 폭이면 `width` 를 싣지 않고, 둘 다 기본값이면 `preview` 키 자체를 싣지 않는다. preset capture 는 이 snapshot 을 TOML 로 저장하는데 TOML 에는 null 이 없기 때문이다. 형식이 맞지 않는 값(문자열 `open`, 0 이하·유한하지 않은 `width`)은 기본값으로 읽는다. 폭은 그릴 때 토큰 범위(200…460)와 칸 폭으로 다시 제한한다.
- **저장하지 않는 값**: back/forward 히스토리, 목록 캐시, 선택, 트리 펼침, 주소창 편집, 타입어헤드, Find 바, 파일 클립보드. 복원하면 히스토리는 비어 있고 목록은 다시 읽는다.
- **복원**: `restore` 가 저장한 탭으로 `ExplorerPanel` 을 만든다. 탭이 없으면 홈 하나로, 활성 번호가 범위를 넘으면 마지막 탭으로 맞춘다. 경로가 없거나 상대 경로면 홈으로 교정한다. 복원은 create 를 거치지 않으므로 설정의 뷰 모드가 아니라 저장한 탭별 값을 쓴다. 저장한 폴더가 사라졌으면 복원은 성공하고 목록 자리에 읽기 오류 화면이 나온다. 미리보기 토글·폭도 같이 복원한다.
- **preset**: preset 적용은 `restore` 가 아니라 `create` 로 explorer 를 만든다. capture 한 preset 의 params 는 위 snapshot 그대로다(`tabs`·`active`·`preview`). `create` 는 명시 `path` param 도 cwd 도 없고 `tabs` 가 비어 있지 않으면 `restore` 와 같은 함수(`explorer_panel_from_snapshot`)로 내부 탭·활성 탭·미리보기를 되살린다. 탭 보정 규칙(빈 목록, 범위를 넘은 활성 번호, 없거나 상대 경로인 cwd·current)도 레이아웃 복원과 같고, 사라진 절대 경로는 그대로 두어 읽기 오류 화면이 나온다(홈으로 바꾸지 않고 알림도 띄우지 않는다). preset 계획이 채우는 기본 `view_mode` param 은 보지 않고 탭마다 저장한 뷰 모드를 쓴다. preset 편집기에서 작업 디렉터리(cwd 필드)를 지정하거나 명시 `path` 가 있으면 저장한 탭 대신 그 폴더 하나로 연다. capture 는 cwd 를 채우지 않으므로 편집기에서 지정하지 않는 한 저장한 탭이 열린다. 미리보기는 이때도 params 에서 읽는다.
- **preset 탭 이름**: preset 계획은 surface 를 만들기 전에 첫 leaf 의 cwd·params 로 페인 탭 이름을 정한다(`default_tab_name_for_kind`). explorer 는 `path` param 이 있으면 그 폴더 이름이고, 없으면 `tab_name_from_params`(`explorer_tab_name`)가 create 와 같은 순서로 열릴 폴더의 이름을 고른다. cwd 가 있으면 그 폴더, 없고 capture 한 탭이 있으면 활성 탭의 cwd, 둘 다 없으면 create 가 여는 기본 폴더(홈)의 이름이다. 폴더 이름이 없는 파일시스템 루트일 때만 종류 이름("Explorer")이다. 같은 규칙을 IPC 의 explorer 워크스페이스 생성도 쓴다.
- **즐겨찾기**는 레이아웃이 아니라 별도 파일에 저장한다(아래 "즐겨찾기").
- mirror workspace 는 로컬에 저장하지 않는다([원격 attach](../../features/remote-attach/index.md)).

## IPC·CLI

explorer 는 일반 surface 생성 메커니즘으로 다룬다 (전용 IPC 추가 없이 generic 경로):

- 생성: `tasty new tab --type explorer [--path <dir>] [--cwd <dir>]` / `tasty new workspace --type explorer [--path <dir>] [--cwd <dir>]` / `tasty split ... --type explorer [--cwd <dir>]`. 명시 `--cwd` 는 아래 root 결정 규칙의 carry cwd 로 들어간다(`--path` 가 있으면 `--path` 가 이긴다). 탭 이름도 그 폴더 이름이다. 둘 다 없으면 새 탭은 explorer `default_params` 의 `path = "@home"` 로 home 이 주입되고(fresh-context), 워크스페이스·split 은 cwd 없이 만들어 홈을 연다. CLI 는 빠진 `--path`·`--cwd` 를 null 키로 보내므로 이때는 주입을 건너뛰지만(`apply_kind_default_params` 는 키가 있으면 채우지 않는다) create 의 기본값도 홈이라 결과는 같다. 탭 이름은 어느 경우든 연 폴더(홈)의 이름이다. explorer 는 다른 surface 의 cwd 를 이어받지 않는다([cwd 정책](../../design/policies/cwd.md#2-carry-경로-강제--surfacekinddefcreate-시그니처)). (IPC: `DomainIntent::CreateTab { kind: "explorer", surface_params }`.)
- **root 결정 규칙**: `path` param → carry cwd → (둘 다 없고 params 에 capture 한 `tabs` 가 있으면 저장한 내부 탭, 위 "preset") → `$HOME`/`%USERPROFILE%` → (홈 조회 실패 시) 절대경로로 확정한 프로세스 cwd. 앞 두 단계의 값이 **상대경로면 채택하지 않고** 홈으로 내려간다 — explorer root 는 어떤 생성 경로(`split`/`new tab`/`new workspace`/convert)에서도 **항상 절대경로**다. 상대 root 는 프로세스 cwd 를 root 로 승격시키고 그 문자열이 주소창·경로 복사·attach `list_dir` wire 로 새어나가기 때문이다. `"."` 로 저장된 구 `layout.json` 스냅샷도 복원 시 홈으로 교정된다. 근거·강제 수단: [surface cwd 불변식 §5](../../design/policies/cwd.md#5-explorer-root-fallback-host-builtin).
- 조회/닫기: `tasty list surfaces` 에 `foreground_process`/`pane_id`/`workspace_id` 와 함께 나타나고, `tasty close ...` 로 닫는다 — 전 워크스페이스 순회·ID 직접 지정(포커스 독립).
- 변환: 다른 surface 를 explorer 로 in-place 변환 — `Intent::ConvertSurface { surface_id, target: ConvertTarget::Kind { kind: "explorer", .. } }`. cwd 미지정 시 source surface 에서 carry. [convert-surface](../../features/convert-surface/index.md) 의 generic convert popup 도 registry kind 열거로 explorer 를 노출한다.

## headless·원격 제약

- **headless**: kind 는 GUI 와 관계없이 부팅 때 등록된다. 생성·조회·닫기·변환·저장·복원은 모델만으로 동작한다. 목록 읽기·화면·컨텍스트 메뉴·파일 작업은 View 가 있는 창에서만 일어난다.
- **mirror**: 목록은 원격 조회로 받고 로컬 파일시스템을 읽지 않는다. 파일 변경과 즐겨찾기 추가는 막고, 파일 열기는 원격에 탭을 만든다. 아래 "mirror(attach) explorer 의 파일 변경 차단"과 "mirror explorer 의 파일 열기"에 있다.

### mirror(attach) explorer 의 파일 변경 차단

ADR-0022에 따라 mirror explorer 는 파일 변경(rename/delete/새 폴더 만들기 등)을 아직 지원하지 않으며, 이 제한은 컨텍스트 메뉴·키보드 단축키 레벨까지 강제된다. 파일 더블클릭 열기(`OpenFile`)는 로컬과 같은 `DispatchFile` 로 가고, origin 이 mirror surface 라 원격 열기 규칙을 따른다(아래 "mirror explorer 의 파일 열기"). mirror 워크스페이스(`ws.mirror`)에 속한 explorer surface 에서는:

- **컨텍스트 메뉴에서부터 숨김**: 새 폴더/새 파일/붙여넣기/잘라내기/이름 변경/휴지통으로 이동/시스템에서 열기/새 탭으로 열기 항목이 `build_explorer_context_menu`(즐겨찾기 행은 `handle_explorer_favorite_native_menu`)에서 아예 노출되지 않는다. copy_path/복사/즐겨찾기 추가/이 폴더로 루트 설정은 그대로 노출된다.
- **액션별 개별 가드**: 메뉴가 아닌 다른 경로(키보드 단축키 등)로 같은 핸들러가 호출되는 경우를 방어하기 위해, 각 핸들러(`explorer_menu_paste`/`_trash`/`_rename`/`_open_in_system`/`_add_favorite`/`_open_in_new_tab`, `explorer_menu_set_clipboard`의 `cut=true`)가 진입부에서 `CoreState::is_mirror_surface(surface_id)` 로 재확인하고, mirror 면 로컬 fs 를 건드리지 않고 `explorer.state.remote_write_unsupported` toast 로 안내한 뒤 반환한다.
- **rename 팝업의 대상 게이트**(`rename_target_exists`, `src/adapters/ui/dialog.rs`)는 메뉴 시점 세대가 그대로이고 경로가 남아 있을 때만 팝업을 유지한다. mirror 경로는 위 가드가 먼저 막아 팝업이 열리지 않는다.
- **즐겨찾기**: `~/.tasty/explorer-favorites.toml` 는 surface/host 무관 전역 저장소다. mirror explorer 의 경로(원격 호스트 경로)가 이 전역 목록에 섞이면 로컬/다른 호스트 explorer 의 사이드바를 오염시키므로, 즐겨찾기 추가는 mirror 에서 팝업을 열기 전에 차단된다.
- **새 탭으로 열기**: 메뉴 숨김(탐색기 항목 메뉴와 즐겨찾기 행 메뉴)과 핸들러 가드(`explorer_menu_open_in_new_tab`, `remote_write_unsupported` 토스트)로 막는다. 파일을 바꾸는 작업이 아니므로 ADR-0022 의 파일 변경 미지원과는 별개의 제한이다. 이 메뉴가 쓰는 `DomainIntent::CreateTab` 은 mirror pane 이면 App 이 원격 `StructuralOp::NewTab` 으로 전달하므로(`src/app/services/impl_mirror.rs`) 탭이 로컬에만 생기는 경로는 없다. 이 제한을 유지할지는 정해지지 않았다.

### mirror explorer 의 파일 열기

mirror explorer 에서 파일을 더블클릭하면 원격 호스트에 그 파일의 탭을 만든다.

- 식별은 파일 이름만 본다(`DetectDepth::Name` — 확장자·path glob). client 에 같은 경로의 파일·디렉터리가 있어도 읽지 않는다.
- 매칭 핸들러 중 `open_surface` 이면서 client 가 그 kind 의 콘텐츠를 mirror 하는 것(현재 markdown, 허용된 egui-mesh kind)만 실행한다. 1순위가 그런 핸들러면 바로 열고, 아니면 그런 핸들러만 담은 핸들러 picker 를 띄운다. 선택한 핸들러의 `CreateTab` 은 원격 `StructuralOp::NewTab` 으로 forward 되고 사용자 origin 으로 표시돼 원격과 client 양쪽에서 새 탭이 선택된다.
- 그런 핸들러가 하나도 없으면(`system`·`ipc` 핸들러뿐, html 처럼 placeholder 로 보이는 kind, 매칭 없음) picker 없이 `explorer.state.remote_open_unsupported` toast 로 안내한다.
- 원격 경로는 로컬 최근 목록에 기록하지 않는다. 규칙 전체는 [파일 핸들러](../../features/file-handler/index.md) 의 원격 대상 절을 따른다.

## 화면

화면의 픽셀·색 값은 Theme 토큰의 scale 1 값이다. 정본은 토큰과 [디자인·갤러리 매핑](../../design/systems/design-gallery-mapping.md)이다.

### 뷰 상태 (`ExplorerView`, surface id 로 keying)

디렉토리 엔트리 캐시·선택 집합·트리 펼침 같은 무거운 GUI 상태는 모델이 아니라 per-surface 뷰 스토어에 둔다.

- **엔트리 캐시**: `sync(panel)` 이 활성 탭의 `(root, sort_column, sort_dir)` 키를 보고 디렉토리/정렬이 바뀌었거나 새로고침이 요청됐을 때만 App 읽기 worker 에 다시 읽기를 요청한다(위 "생성·갱신·종료"). 디렉토리가 바뀌면 선택을 초기화한다. 읽기 실패는 `LoadState::NoPermission`(권한 거부) / `LoadState::Error(msg)` 로 분류해 콘텐츠 중앙 상태 화면으로 표현한다.
- **상태 화면**: 내용 영역이 목록 대신 가운데 정렬한 글리프 · 제목 · 선택 보조 줄을 보여 준다(`explorer/state_screen.rs`, 시안 `ExpState`). 배치는 공용 `tasty_ui_widgets::state_screen` 이 그리며 갤러리 탐색기 상태 예제와 이미지 캔버스 상태 화면도 같은 함수를 쓴다.
  - **줄바꿈**: 제목과 보조 줄은 줄 길이를 고르게 바꾼다(`tasty-ui-widgets` 내부 `balanced_wrap::balanced_galley`, 시안 `text-wrap: balance`). 최대 폭에서 나온 줄 수를 유지하는 가장 좁은 폭으로 다시 배치해 가장 긴 줄을 최소로 만들므로, 마지막 줄에 한두 글자만 남지 않는다. 미리보기 패널의 상태 화면도 같다. 이유 줄(mono)은 그대로 둔다.
  - **compact 한 줄**: 내용 영역 높이가 `explorer_state_compact_below()`(120px) 미만이면 모든 상태 화면이 한 줄로 바뀐다. 확대하지 않은 16px 글리프(`icon_glyph_size_md`) · 제목(body, 넘치면 끝 말줄임) · 그 상태의 버튼(읽기 오류의 Retry · Go up, 간격 space-xs)을 space-sm 간격으로 놓고 줄 전체를 내용 영역 가운데에 둔다. 보조 줄과 OS 이유 문구는 제목의 툴팁으로 옮긴다. 창을 줄여 탐색기 칸이 하한 아래가 돼도 블록이 잘리지 않게 하기 위한 형태다. 공용 `tasty_ui_widgets::compact_state_row` 가 그리며 갤러리 Short cell 예제와 같은 함수다.
  - 빈 폴더(`Ok`이고 항목 0개): `folderOpen` 글리프(text-muted) + "This folder is empty"(text-secondary). 시안 탐색기 Spec 은 항목 0개와 빈 폴더를 구분하지 않는다.
  - 읽기 오류(`Error`, 권한 거부 밖의 실패 — 경로 없음, 디렉터리 아님, 로컬 IO 오류, 원격 읽기 실패·응답 시간 초과): `alertTriangle` 글리프와 제목 "Can't read this folder"를 `explorer_error_fg`(→ accent-danger)로 칠한다. 그 아래 받은 오류 문구를 mono caption(text-muted, 최대 폭 200) 한 줄로 번역하지 않고 보인다(원격 시간 초과는 `explorer.state.error_conn_timeout`). 버튼 줄은 space-xs 를 더 띄우고 Retry(Secondary sm, 같은 경로를 다시 읽음 = `ExplorerAction::Refresh`)와 Go up(Ghost sm, 상위 폴더 = `ExplorerAction::GoUp`)을 space-sm 간격으로 가운데에 둔다. 로컬 폴더가 없거나 폴더가 아니어서 실패했으면 읽기 worker 가 남아 있는 가장 가까운 상위 폴더를 오류에 붙여 보내고(`local_reads::MissingFolder`), 그 폴더가 바로 위 폴더가 아니면 Go up 은 그 폴더로 `Navigate` 한다. 툴바의 위로·`Alt+Up` 은 한 단계 위 그대로다. 현재 경로에 상위가 없으면(루트) Go up 을 숨긴다. 툴바·트리·상태줄은 그대로 쓸 수 있다(design `ExpState` read error).
  - 권한 거부(`NoPermission`): `lock` 글리프와 제목 "Permission denied"를 accent-warning으로 칠하고, 보조 줄(caption, text-muted, 최대 폭 200)에 이유를 적는다.
  - 불러오는 중(`Loading`): 글리프 자리에 Spinner + "Loading…".
  - 글리프 배치 크기는 `icon-glyph-size-md`이고 그림만 그 칸 가운데에서 1.6배로 그린다(시안 `transform: scale(1.6)`은 배치에 영향이 없다). 줄 간격은 `space-sm`이다.
- **주소창 편집 상태**: `addr_buffer`(편집 텍스트) / `addr_editing`(포커스=편집모드) / `addr_active`(후보 드롭다운 keyboard-active 행)를 뷰가 소유한다(PathField 계약 — 상태는 호출측 소유). `sync()` 는 **비편집 시** 버퍼를 활성 탭 current(`root`) 로 재동기화하고, 편집 중이면 사용자 입력을 보존한다. 내부 탭은 surface 단위 `ExplorerView` 를 공유하므로, cwd/내부 탭을 바꾸는 액션(`Navigate/GoBack/GoForward/GoUp/NewTab/CloseTab/SelectTab`) 적용 시 `cancel_addr_edit()` 로 편집을 취소해 버퍼가 다른 탭/경로로 새지 않게 하고(다음 `sync()` 가 새 current 로 맞춘다), id_salt 는 surface+내부탭 index 로 고유화한다.
- **타입어헤드 상태**: `type_ahead`(입력 버퍼와 마지막 입력 시각)와 `scroll_to: Option<PathBuf>`(이번 프레임에 화면에 보이게 할 항목). `scroll_to`는 한 프레임만 유지한다. 남겨두면 매 프레임 다시 스크롤해서 사용자가 휠로 다른 곳을 보는 동안 화면이 끌려간다. 순환 시작 위치, 접두사 확장, 입력 되돌리기 같은 규칙은 egui와 파일시스템에 의존하지 않는 `explorer/type_ahead.rs`에 있고, 그리는 쪽은 그 결과를 선택과 스크롤로 옮기는 일만 한다.
- **선택**: `selected: HashSet<PathBuf>` + `anchor`(범위 선택 기준). 클릭은 그 항목만 고르고, Ctrl·Cmd+클릭은 그 항목을 넣거나 빼며, 둘 다 앵커를 그 항목으로 옮긴다. Shift+클릭은 앵커부터 클릭한 항목까지 목록 순서의 범위를 고르고, Ctrl·Cmd+Shift+클릭은 기존 선택에 그 범위를 더한다. Shift 클릭은 앵커를 옮기지 않는다. 앵커가 없거나 다시 읽은 목록에 없으면 Shift 를 뺀 클릭과 같다. grid·list·detail 이 같은 `ExplorerView::click_select` 를 쓴다. `select_all()` 은 보이는 항목 전체(Find 로 거르는 중이면 걸러진 항목)를 선택, `selected_paths_text()` 는 선택 경로를 정렬·개행 결합한 클립보드 페이로드를 만든다.
- **사이드바 트리**: `expanded` 펼침 집합 + `tree_children` lazy 하위 디렉토리 캐시. 폭 196(design `ExpSidebar`). 사이드바는 **2-region 고정 분할**이다 — 상단 **Files**(트리, cwd 루트 고정)는 사이드바 본문 남는 공간 전부를 차지하며 자체 스크롤되고, 하단 **Favorites**는 계산된 고정 높이 영역에서 독립적으로 스크롤된다(Files 를 아무리 스크롤해도 Favorites 위치는 움직이지 않고, 반대도 마찬가지). 두 영역 사이 1px 구분선은 **하단 고정 영역의 상단 경계**에 고정 좌표로 그려진다 — 트리 길이와 무관하며, 트리가 짧아도 그 위 빈 공간은 배경만 남고 구분선이 따라 올라오지 않는다. 트리에서 **현재 폴더(current)** 노드는 surface-active 배경 + text-primary 로 하이라이트되고, 폴더 아이콘은 text-muted. 섹션 캡션은 monospace·micro·uppercase(design `SideHead`).
- **Favorites 고정 높이 계산**(design `favPinHeight`): 사이드바 본문 높이가 600px 이상이면 240px 고정. 600px 미만이면 `round(본문높이 × 0.4 / 4) × 4`(4px 그리드 스냅)와 120px(하한) 중 큰 값. 임계값 전환은 보간 없는 하드 전환이다. 본문 높이가 `explorer_favorites_hide_below`(240px) 미만이면 Favorites 영역과 그 위 구분선을 그리지 않고 Files 가 본문 전체를 쓴다(design Short cell). 본문이 다시 240px 이상이 되면 위 사다리대로 돌아온다. 사이드바 자체는 숨기지 않는다. 본체 구현은 `src/adapters/ui/surface/explorer/favorites_pin.rs`.
- **낮은 칸**: 툴바 아래 행은 칸에 남은 높이의 고정 사각형이고, 사이드바 열과 내용 열은 각자 자기 사각형 안에서만 그린다. 본문이 240px 미만이면 Favorites 를 빼므로 사이드바는 Files 캡션과 트리만 그리고, 트리는 자기 스크롤 영역 안에서 넘친다. 내용 열과 상태줄은 칸 안에 남는다. 내용 목록의 ScrollArea 는 최소 높이를 0 으로 두어 본문이 낮아도 상태줄을 밀어내지 않는다.
- **분할 하한**(design Short cell): pane·surface 분할선을 끌어 탐색기 칸을 줄이면 칸 높이 `explorer_min_height`(180px)에서 멈춘다. 대상은 각 pane 의 활성 탭에 보이는 탐색기 surface 다. 드래그 미리보기 비율이 어떤 탐색기 칸을 하한 아래로 줄이면 이분 탐색으로 하한을 지키는 마지막 비율에 둔다. 창 크기 변경처럼 드래그가 아닌 이유로 이미 하한보다 낮은 칸은 드래그로 더 줄지만 않는다. 창 크기 변경은 하한을 지키지 않는다(낮아진 칸은 위 compact 한 줄로 그린다). 분할(`tasty split`·분할 메뉴)은 분할을 확정할 때 같은 하한을 지키도록 분할선을 옮긴다. 다른 종류의 형제 칸은 남는 높이를 받되 새 분할에서는 56px(`split_sibling_min_height`) 아래가 되지 않는다. 탐색기 칸끼리 하한을 지킬 수 없거나 형제 칸의 56px 을 지킬 수 없으면 거절한다. 메뉴·단축키로 거절되면 info 토스트, 에이전트 split 은 IPC 오류만 받는다([split 명령](../../features/work-area/index.md#분할-비율과-탐색기-칸-하한)). 탐색기 칸을 키우는 쪽과 다른 종류의 칸은 제한하지 않는다. 구현은 `src/state/layout_preview/explorer_floor.rs`.

### 뷰 모드 / 정렬

- 뷰 모드 3 종(grid / list / detail)을 toolbar 우측의 **아이콘 view-mode 토글**(`seg_toggle`, design `SegToggle`)로 전환한다 — grid/list/detail 아이콘 세그먼트, active = segtoggle-on-bg(accent-primary) 채움 + segtoggle-on-fg(text-on-accent) 글리프, inactive = text-muted. detail 뷰는 정렬 컬럼 헤더를 클릭하면 해당 컬럼으로 정렬(같은 컬럼 재클릭 시 방향 토글).
- toolbar 의 **주소표시줄**(`address_bar`, design `ExpToolbar`/`PathField`)은 공용 **편집형 `PathField`** 다 — folderOpen leading 아이콘 + mono 경로(비편집=text-secondary / 편집=text-primary) + 우측 Go(arrow-right) 버튼(input-bg/input-border(-focus) 토큰).
  클릭하면 편집 모드로 들어가 임의 디렉토리 경로를 타이핑하고 `↵` 또는 Go 로 **current 이동**한다. 입력을 폴더로 바꾸는 규칙과 거부는 아래 "주소 입력" 절에 있다.
  `Esc` 또는 확정 없는 포커스 이탈은 현재 current 로 원복.
  상위 폴더는 Back/Forward/Up 버튼, 사이드바 트리, 경로 입력으로 이동한다.
  편집 진입 시 **최근 방문 디렉토리** 자동완성 후보 드롭다운이 뜨고(타이핑에 맞춰 substring 필터), 이 후보는 `RecentFiles` 의 `"directory"` kind(markdown 의 파일 recent 와 대칭·영속)에서 온다 — 사용자가 `ExplorerAction::Navigate` 로 이동 확정한 cwd 를 host 가 kind 로 적재(`egui_panels`), draw 경계로 slice 주입.
  주소표시줄 flex:1 / 토글 flex:none.
- **마지막 view mode 기억**: 사용자가 뷰 모드를 바꾸면 그 값이 `Settings.general.explorer_view_mode`(`~/.tasty/config.toml`)에 영속되고, **새로 생성되는** explorer surface 는 이 값으로 열린다(주입 지점: 생성 요청을 만들 때 `EngineRef::apply_kind_default_params` 가 explorer 의 `default_params` `view_mode = "@settings.explorer_view_mode"` 정책 토큰을 `view_mode` param 미지정 시 해석해 params 에 넣고, 그 params 가 explorer `create` 에 전달된다 — kind별 default_params 는 [plugin-development.md](../../dev-guide/plugin-development.md) 참조). 같은 surface 안의 새 내부 탭(`add_tab`)은 활성 탭의 view mode 를 승계한다. snapshot 복원 경로는 create 를 거치지 않아 per-tab 저장값을 그대로 유지한다.
- list 행과 사이드바 디렉토리 행은 공용 `tree_row`(`tree_row_height` 22)를, detail 데이터 행은 공용 `Table`(selectable)을 재사용한다. 탐색기 전용 행 높이는 없다. detail 헤더와 본문 행은 Table 기본 높이 `table_cell_height`(28)이고 헤더 채움은 `table_header_bg`다. detail 컬럼은 Name(1fr)/Size(80)/Date(132)/Type(92)이며, 이름은 선택 행만 text-primary 이고 나머지 행은 `table_row_fg`(text-secondary)다. Size·Date 는 **monospace·caption(11)·text-muted**, Size 는 우측 정렬 + 8px 우측 패딩으로 Date 와 시각적 간격을 둔다(design `DetailRow`). Size 열 제목도 같은 8px(`spacing_sm`) 오른쪽 여백을 둔다(design `DetailHeader`, 공용 Table `header_pad_right`). 열 제목은 공용 Table 머리글 그대로 대문자에 `table_header_tracking` 자간이다.
- 세 뷰 모두 **화면에 걸친 항목만 그린다**. 행 높이가 모드 안에서 모두 같으므로(list `tree_row_height`, grid 칸 높이 + `spacing_md`, detail `table_cell_height`) 화면 밖 행은 같은 높이의 빈자리로 두고 스크롤 길이는 항목 수 그대로다. 프레임 비용은 항목 수가 아니라 보이는 행 수를 따른다. 그리는 행만 목록에서 복제하고, detail 은 복제 없이 참조로 표에 넘긴다.
  - 새 항목 이름 입력은 스크롤해 화면 밖으로 나가도 그린다. 그래야 포커스와 글자가 유지된다. list·grid 는 `..` 와 이름 입력 줄(grid 는 그 둘이 놓인 첫 줄)을 늘 그리고, detail 은 그 행이 화면 밖이면 그린 행의 위치와 행 번호로 입력 자리를 계산해 표 밖에서 그린다.
  - 타입어헤드·새 항목 고르기처럼 화면 밖 항목으로 가야 할 때는 그 항목의 자리를 행 높이로 계산해 가운데로 스크롤한다.
  - list·grid 의 항목 위젯 id 는 화면 위치가 아니라 항목 경로에 묶는다. 스크롤해도 누름·hover 상태가 같은 항목에 남는다. detail 은 공용 `Table` 의 `virtual_rows`·`scroll_to_row` 를 쓴다.
  - Find 로 이름을 거르면 맞는 항목의 목록 번호를 한 번 만들어 두고(`find.rs` 의 `refresh_filter`, Find 바를 그릴 때) 검색어와 목록이 그대로인 동안 다시 쓴다. 목록은 `set_entries` 로 바꿀 때마다 세대가 오르고, 거른 결과는 그 세대·목록 주소·길이와 검색어가 모두 같을 때만 쓴다. 맞지 않는 프레임에는 그 자리에서 다시 거른다. 그래서 거르는 동안 목록 전체의 이름을 보는 것은 검색어나 목록이 바뀐 프레임뿐이다. 검색어가 바뀌면 보이지 않게 된 항목을 선택에서 빼는데, 이때는 선택된 항목만 본다. 선택은 읽은 목록과 하위 폴더 검색 결과 안에만 있으므로 지금 폴더 항목은 부모 폴더와 이름을 검색어에 대 보고, 하위 폴더 검색 결과는 결과 목록에서 찾는다. 거르지 않을 때와 하위 폴더 검색 결과는 목록을 훑지 않는다.
- detail 행은 **행 전체가 클릭 타겟**이다 — 파일 이름·Size·Date·Type 글자 위에서도 좌클릭 선택 / Ctrl·Cmd+클릭 토글 / Shift+클릭 범위 선택 / 더블클릭 열기(Navigate·OpenFile) / 우클릭 컨텍스트 메뉴가 동일하게 동작한다. 그 대가로 셀 텍스트를 드래그로 선택·복사할 수는 없다(대체: 우클릭 "경로 복사"). 이 정합은 공용 `Table` 이 selectable 모드에서 셀 라벨 선택성을 끄는 계약으로 보장한다 — [ADR-0037](../../adr/0037-ui-input-motion-and-elevation.md). 헤더 컬럼 제목 클릭(정렬 토글)은 영향을 받지 않는다.

### 주소 입력

`Enter`·Go 의 입력은 `src/adapters/ui/surface/explorer/address.rs` 가 이동할 폴더로 바꾼다. 앞뒤 공백은 지운다. 빈 입력은 아무것도 하지 않는다. 이동하지 않으면 무반응으로 끝내지 않고 이유를 오류 toast 로 알린다.

로컬 explorer:

- 절대 경로는 그대로, 상대 경로는 프로세스 cwd 가 아니라 **현재 보고 있는 폴더(current)** 기준으로 잇는다.
- `~` 와 `~/…`(Windows 는 `~\…` 도)는 로컬 홈 아래로 펼친다. `~name` 은 펼치지 않고 그 이름의 상대 경로로 본다. 홈을 찾지 못하면 거부한다.
- Windows 에서 드라이브만 있는 입력(`C:`, `C:dir`)은 프로세스의 드라이브별 현재 폴더가 아니라 그 드라이브의 루트 기준이다. UNC(`\\server\share\…`)는 절대 경로다.
- `.` 과 `..` 은 글자 그대로 정리해 절대 경로로 확정한다. 링크를 풀지 않으므로 폴더 링크 안에서 `..` 은 링크의 부모다. 루트 위의 `..` 는 루트에 머문다.
- 확정한 경로가 폴더(폴더를 가리키는 링크 포함)일 때만 이동한다. 파일이면 "폴더가 아님", 없으면 "찾을 수 없음", 대상이 없는 링크면 "링크 대상 없음", 권한 거부 등 확인하지 못하면 OS 가 알려 준 이유로 거부한다. 파일 경로를 열거나 그 부모로 가는 동작은 하지 않는다.
- 공백·Unicode 이름은 그대로 경로의 일부다.

mirror(원격) explorer:

- 로컬 파일시스템을 보지 않는다. 입력은 원격 호스트의 경로로 다룬다.
- `~` 와 `~/…`·`~\…` 는 원격 홈 아래다. 원격 홈은 연결(재연결 포함)마다 한 번 조회한다. 빈 경로의 `list_dir` 를 보내면 서버가 자기 홈을 읽고 그 경로를 돌려준다(`src/app/attach_client/home_probe.rs`). 응답 전이거나 조회에 실패하면 원격 홈을 모르므로 거부하고 전체 경로를 적으라고 알린다. 로컬 홈으로 펼치지는 않는다. `~name` 은 펼치지 않고 그 이름의 상대 경로로 본다.
- 절대 경로(`/…`, `\…`, `X:\…`, `X:/…`)는 그대로, 나머지는 현재 원격 폴더 뒤에 잇는다. 잇는 구분자는 원격 현재 경로를 따른다(경로에 `\` 가 있으면 Windows 형식으로 보고 `\`, 아니면 `/`). 파일 선택기의 원격 경로 결합(`join_dir`)과 같은 함수다. 그래서 Windows 클라이언트에서도 POSIX 원격 경로를 `/` 로 잇는다.
- 그 뒤 `.` 과 `..` 을 글자 그대로 정리한다(`explorer/address/remote.rs`). 로컬 OS 의 경로 규칙이 아니라 원격 경로의 형식을 따른다. POSIX 형식은 `/` 루트, Windows 형식(`\` 나 드라이브 접두가 있는 경로)은 드라이브 루트(`C:\`)나 UNC 공유(`\\host\share\`)를 루트로 보고 `/` 도 구분자로 받아 `\` 로 잇는다. 원격 링크는 풀지 않으므로 폴더 링크 안의 `..` 은 링크의 부모다. 루트 위의 `..` 는 루트에 머문다.
- 존재 여부는 이동 뒤 원격 목록 조회가 알려 준다. 원격 조회 실패는 목록 자리의 읽기 오류 화면(`Error`/`NoPermission`)으로 보인다.

### 미리보기 패널

툴바 view 묶음의 미리보기 토글(`COLUMNS` 글리프, Find 다음. 좁은 칸은 More 메뉴의 Show preview·Hide preview)이 목록 오른쪽 패널을 켜고 끈다(시안 `YPreview`, `explorer/preview.rs`). View(`ExplorerView::preview`)가 토글·폭을 바로 바꿔 그리고, 토글하거나 경계선 끌기를 마쳤을 때 한 번 `EngineAction::ExplorerPreview` 로 model(`ExplorerPanel::preview`)에 남긴다. 그래서 레이아웃 snapshot 에 실려 재시작 뒤에도 남는다(위 "저장·복원"). View 는 view 와 model 이 마지막으로 같았던 값을 기억하고, 그릴 때마다 model 값이 그 값과 다르면 model 값을 받아 온다. 처음 그릴 때는 복원한 값을 받는다. 같은 surface id 로 다른 kind 로 바꿨다가 explorer 로 돌아오면 view 는 id 로 남지만 새 `ExplorerPanel` 은 기본값이라, 그 값을 받아 화면과 저장되는 값이 같다. 끌기 중처럼 view 만 바뀐 동안에는 model 이 그대로라 덮어쓰지 않는다. 단축키 `explorer_toggle_preview` 도 같은 경로다.

- **폭**: `explorer_preview_width`(288)에서 시작하고 패널 왼쪽 경계선(잡는 폭은 pane 분할선과 같은 `DIVIDER_HIT_THRESHOLD`)을 끌어 `explorer_preview_min_width`(200)…`explorer_preview_max_width`(460) 사이로 바꾼다. 목록에는 `explorer_list_min_width`(200)를 남긴다. 칸이 패널 하한 + 경계선 + 목록 하한(합 400 + 경계선)보다 좁으면 패널만 숨기고 토글은 켜진 채 둔다.
- **대상**: 선택이 정확히 하나일 때 그 항목. 선택이 없으면 "Select a file" 상태다. 여럿이면 머리에 "N items" 와 파일·폴더 수, 본문에 layers 글리프와 `explorer.preview.multi`("{n} items selected")·`multi_sub`("Select one file to preview it.")를 보인다. 대상이 바뀌면 이전 미리보기를 바로 지우고 Loading 상태를 보인다. 같은 항목이라도 수정 시각이 바뀌면 다시 읽는다.
- **머리**: 높이 40, 이름 · 종류 · 크기(그림은 픽셀 크기도) 두 줄.
- **본문**: 읽기는 App read worker(`local_reads::preview`)가 한다. 텍스트(NUL 이 없는 UTF-8)는 앞 64 KB(`PREVIEW_TEXT_BYTES`)를 mono 로 스크롤해 보인다. 그림(`decodable_image_ext`)은 패널에 맞추되 원래 크기보다 키우지 않는다. worker 가 그림을 패널이 가장 넓을 때의 그림 영역 폭(`explorer_preview_max_width` − 2 × `spacing_md`, 물리 px)과 `PREVIEW_TEXTURE_SIDE`(2048, egui `max_texture_side` 의 보장 하한) 상자에 맞춰 Triangle 필터로 줄여 보낸다. egui-wgpu 텍스처에는 밉맵이 없어 GPU 가 크게 줄이면 앨리어싱이 생기므로, 패널을 좁히거나 넓혀도 GPU 의 추가 축소는 패널 최소 폭에서도 약 2.5 배(scale 1 그림 영역 436 → 176)에 그친다. 배율이 바뀌어 이 폭이 달라지면 다시 읽는다. 썸네일은 더 싼 면적 평균으로 줄인다. 머리의 픽셀 크기는 원본 값이다. 정규 파일만 연다. 폴더·FIFO·장치 파일 같은 특수 파일과 그 밖의 형식은 "No preview for this file type" 이다. 1 MiB(`PREVIEW_MAX_BYTES`, 앱 상한) 초과는 "Too large to preview" 와 "Over 1 MiB." 이다. 상한 문구의 이진 단위는 모두 MiB 이고 숫자와 단위 사이는 NBSP 다. 그림은 디코딩 전에 `image::Limits` 로 한 변 16384px(`MAX_IMAGE_SIDE`)·디코딩 메모리 256 MiB(`MAX_DECODE_ALLOC`)를 넘는지 보고, 넘으면 같은 "Too large to preview" 에 픽셀 상한 문구("Over 16384 px on a side, or needs more than 256 MiB to decode.", 메모리는 이진 단위 MiB)를 붙이고, 머리글에서 읽은 실제 크기를 이유 줄("20000 × 14000 px")과 머리의 종류 뒤("TIF image · 20000 × 14000 · 61.0 MB")에 보인다. 머리글도 읽지 못하면 이유 줄이 없다. 읽기 실패는 "Can't read this file" 과 오류 문구다.
- **원격**: mirror explorer 는 파일 내용을 받을 경로가 없어 대상과 관계없이 지원하지 않는 형식 상태를 보인다.

### Grid 썸네일

Grid 셀은 모두 `explorer_grid_thumb_size`(40) 슬롯을 잡아 썸네일 유무와 관계없이 행 높이가 같다(셀 높이 +24, 시안 "Grid thumbnails", `explorer/thumbs.rs`). 로컬 explorer 의 그림 파일 중 1 MiB 이하인 정규 파일만, 화면에 보인 셀부터 read worker 가 긴 변 80px 로 줄여 만든다. 미리보기와 같은 픽셀 상한을 넘으면 디코딩하지 않는다. 썸네일은 40×40 에 맞추고(키우지 않는다) 1px separator 테두리와 radius-sm 을 둔다. 만드는 동안·상한 초과·디코딩 실패는 16 글리프(accent-info)를 그대로 둔다. 캐시(`ExplorerView::thumbs`)는 경로와 수정 시각으로 맞추고 512 개까지 둔다. 차면 그 프레임에 보이지 않은 항목 중 가장 오래전에 보인 것부터 버리므로 그림이 512 개를 넘는 폴더도 스크롤한 자리의 썸네일을 만든다. 한 프레임에 보이는 셀이 512 개를 넘으면 넘는 셀은 글리프로 둔다. list·detail 보기와 mirror explorer 는 썸네일을 만들지 않는다.

### Properties popup

`explorer_properties` popup(`src/adapters/ui/popup/explorer_properties.rs`, 시안 `YProps`)은 그 explorer surface 에 묶인(`PopupScope::Surface`) headless popup 이다. 바탕은 시안 YProps 와 같은 bg-panel 이다(읽기 실패 화면의 Secondary Retry 가 surface-raised 바탕에 묻히지 않는다). 폭은 `explorer_props_width`(360), 라벨 열은 `explorer_props_label_width`(96, caption text-muted)이고 값은 body 또는 mono caption 이다. 연 대상을 고정해 보이며, 원 explorer 가 사라지면 닫힌다.

- **진입**: 컨텍스트 메뉴 맨 끝 구분선 뒤 "Properties"(id 70, 모든 변형·mirror 에서도 보인다), `explorer_properties` 단축키·Command Palette. 대상 규칙은 [파일 작업 계약](file-operations.md#대상-결정-규칙)이다.
- **종류 문구**(`explorer/kind.rs` 의 `kind_word`·`file_kind_word`): Detail 의 Type 열, Properties 의 Kind, 미리보기 머리가 같은 낱말을 쓴다. 확장자 분류(`core/file_kind.rs` 의 `FileKind`)는 낱말과 Type 정렬이 함께 쓴다.
  - 형식 이름 표: md·markdown "Markdown", txt "Text", html·htm "HTML", json "JSON", toml "TOML", yaml·yml "YAML", csv "CSV", pdf "PDF document", zip·tar·gz·tgz·bz2·xz·zst·7z·rar "Archive", sh·bash·zsh·fish·ps1 "Shell script". HTML·JSON·TOML·YAML·CSV 는 모든 언어에서 같은 낱말이라 번역 키가 없다.
  - 표에 없는 그림 확장자(`is_image_ext`)는 `explorer.kind.image`("PNG image"), 그 밖은 `explorer.kind.ext_file`("RS file"), 확장자가 없으면 "File", 폴더는 "Folder".
  - 링크는 Type 열과 미리보기 머리에서 `explorer.kind.link_to`("Link to Markdown")이고 크기는 대상의 크기다. 링크의 종류는 링크 이름의 확장자와 대상이 폴더인지로 정한다. 대상이 없는 링크는 `explorer.kind.broken_link`("Broken link")다. Properties 의 Kind 는 lstat 결과대로 "Symbolic link" 다.
  - Type 열은 좁아 낱말이 넘치면 말줄임하고 호버에 전체 낱말을 보인다. Type 열 정렬(`core/fs_list.rs` 의 `type_key`)은 보이는 낱말과 같은 묶음이 붙어 있도록 링크 여부(링크 아님 · 링크 · 끊긴 링크), `FileKind` 선언 순서, 확장자, 이름 순으로 비교한다. 번역문을 비교하지 않으므로 언어와 무관하다(예: gz 와 zip 은 Archive 로 붙는다).
- **파일**: Kind · Size(사람이 읽는 크기와 바이트 수) · Modified · Created · Location(Copy) · Permissions(Unix 는 `rwxr-xr-x` 와 read-only, 그 밖은 read-only 만).
- **링크**: 제목 글리프 link · Kind "Symbolic link" · Link target(Copy) · Location. 링크를 따라가지 않는다.
- **폴더**: Size 자리에 하위 항목 수와 크기를 read worker 가 배경에서 세며 Spinner 를 보인다. 링크는 따라가지 않고 읽지 못한 하위 폴더는 건너뛴다. popup 을 닫으면 세기를 멈춘다.
- **여러 항목**: 머리 "N items", Kinds(파일·폴더 수) · Total size · 공통 Location.
- **읽기 실패**: 팝업 틀을 그대로 두고, 제목은 그 항목의 글리프(탐색기 목록에서 본 종류)와 이름이다. 본문은 alertTriangle 과 `explorer.properties.unreadable`("Can't read properties", explorer-error-fg), mono muted OS 이유, Retry(secondary sm, 같은 항목을 다시 읽는다)다.
- **제목 굵기**: 제목은 14 regular 다. 테마에 semibold UI 글꼴이 없어 크기로 제목을 구분한다.
- **원격(mirror)**: 원격 목록에 있는 Kind · Size · Modified · Location 만 보이고 그 아래 muted 안내를 붙인다. 원격 파일시스템을 다시 읽지 않는다.
- **오류**: 정보를 읽지 못하면 머리에 `alertTriangle` 글리프와 첫 대상 이름, "Can't read" 라벨 한 줄에 오류 문구를 mono 로 보인다.

### 새 폴더 · 새 파일

툴바의 명령 묶음(`commands.rs`, 공용 `tasty_ui_widgets::explorer_commands`)이 주소창과 보기 전환 사이에 create 묶음(New folder·New file)과 1px 구분선, view 묶음(Find·Preview 토글)을 icon-only sm 버튼으로 둔다. 칸 폭이 `explorer-toolbar-compact-below`(440)보다 좁으면 두 묶음을 More(`…`) 하나로 접고, 누르면 `ExplorerAction::MoreMenu` → `PendingNativeMenu::ExplorerMore` 로 같은 명령의 네이티브 메뉴를 연다(`src/view/main/explorer_create.rs`, New folder 80 · New file 81 · Find 82 · Preview 83). 네이티브 메뉴에는 체크 표시가 없으므로 view 묶음의 두 행은 누르면 할 동작을 쓴다. Find 바가 열려 있으면 `explorer.more.find_close`("Close find"), 아니면 "Find"이고, 미리보기는 `explorer.more.preview_show`·`preview_hide` 다. mirror explorer 는 create 묶음을 숨긴다. 로컬 explorer 는 폴더를 읽을 때 읽기 worker 에 쓰기 가능 여부를 함께 묻고(`local_reads::writable`, Unix 는 `access(W_OK)`, 그 밖의 OS 는 확인하지 않고 쓸 수 있다고 본다), 쓸 수 없으면 두 버튼을 비활성으로 두고 툴팁을 `explorer.command.cannot_write` 로 바꾼다. 화면 스레드는 파일시스템을 읽지 않는다.

진입점은 툴바 버튼, More 메뉴, 컨텍스트 메뉴(빈 영역 메뉴의 첫 묶음, 단일 폴더 메뉴의 잘라내기·붙여넣기 뒤 묶음 — id 80·81), 단축키 `explorer_new_folder`·`explorer_new_file` 이다. 모두 이름 입력만 연다(`MainViewState::start_explorer_create` → `ExplorerView::start_create`). 대상 폴더는 시작할 때 고정한다. 폴더 메뉴는 그 폴더, 나머지는 지금 보는 폴더다. mirror 는 `remote_write_unsupported` 토스트, 쓸 수 없다고 확인한 현재 폴더는 `cannot_write` 토스트로 거절한다.

이름 입력(`create.rs`, 공용 `explorer_name_row`)은 목록 맨 위(`..` 다음)에 열린다. Detail 은 표에 빈 이름의 자리 행을 하나 넣고 그 위에 겹쳐 그리며, 자리 행은 선택·클릭·메뉴 대상이 아니다. List 는 행 높이 28 의 줄, Grid 는 칸 글리프 아래 160 폭 입력이며 입력이 목록의 보이는 영역 밖으로 나가면 안으로 민다. 처음 프레임에 입력에 포커스를 주고 기본 이름을 고르며 그 줄로 스크롤한다.

- 기본 이름: 폴더 `explorer.new.folder_default` 전체 선택, 파일 `explorer.new.file_default` 확장자 앞까지 선택. 대상 폴더의 이미 아는 이름(지금 목록, 다른 폴더면 사이드바 트리가 읽은 자식)과 겹치면 `"{stem} 2{.ext}"` 부터 비는 번호를 붙인다.
- 입력하는 동안 검사: 빈 이름(공백만인 이름 포함), 금지 글자(모든 OS 에서 `/`·NUL, Windows 는 `<>:"/\|?*` 와 제어 문자), 앞뒤 공백(모든 OS, `explorer.name.edge_space`. 떼지 않고 거절한다. 사용자가 일부러 넣었을 수도 있어 조용히 바꾸면 입력한 이름과 달라진다), 예약 이름(모든 OS 의 `.`·`..`, Windows 의 CON·PRN·AUX·NUL·COM1~9·LPT1~9 — 확장자가 붙어도 같다 — 와 끝이 점인 이름. Windows 는 끝의 점을 떼고 만들어 다른 이름이 되기 때문이다). 이 검사는 새 항목 이름 입력과 이름 바꾸기 대화상자(`rename_name_error`)가 함께 쓴다. 대화상자는 이유를 입력 아래에 보이고 Save 를 막으며, 같은 프레임의 Enter 도 확정할 때 다시 검사해 보내지 않는다. Enter 에서 아는 이름과 겹치는지 본다. 오류는 입력 아래 상자(`explorer_name_error`)로 보이고 입력은 열린 채 글자를 유지한다. 글자를 바꾸면 "이미 있음" 오류는 사라진다.
- Enter 는 확정, Esc 는 취소(디스크 변화 없음), 포커스를 잃으면 유효할 때 확정·아니면 취소한다.
- 확정은 `ExplorerAction::Create { dir, name, folder }` 를 내고 `apply_explorer_action` 이 `explorer_files::Operation::Create` 를 요청한다. worker 는 이름이 한 파일 이름인지 다시 확인하고 `create_dir` 또는 `create_new` 로 만들어, 같은 이름이 생겨 있으면 덮어쓰지 않고 실패한다. 실패는 다른 파일 작업과 같은 오류 토스트와 다시 읽기다.
- 성공하면 요청한 explorer 가 아직 그 폴더를 보고 있을 때만 새 항목을 선택하고, 다시 읽은 목록에 나타난 프레임에 그 자리로 스크롤한다. 같은 폴더를 보는 다른 explorer 는 다시 읽기만 한다.
- 입력이 열려 있는 동안 타입어헤드를 끈다.

### 찾기 · 하위 폴더 검색

툴바 view 묶음의 Find 토글(좁은 칸은 More 메뉴의 Find·Close find, id 82)과 explorer 포커스의 `find` 단축키가 툴바 아래에 Find 바(`find.rs`, 공용 `explorer_find_bar`, 높이 `explorer-search-bar-height` 36, 칸 전체 폭)를 연다. 토글은 열려 있으면 닫고, `find` 단축키는 이미 열린 바에 포커스만 준다. 바 상태(`FindState`: 연 폴더·검색어·Subfolders·하위 폴더 검색)는 `ExplorerView::find` 에 두고 저장하지 않는다.

- **거르기**: 입력하는 즉시 지금 보는 폴더의 항목을 이름의 부분 문자열(대소문자 무시, `match_range`)로 거른다. 맞는 부분은 Detail·List·Grid 모두 `explorer-match-fg` 로 칠한다(List 는 공용 `tree_row_matching`). 바 오른쪽에 `explorer.find.count`("{shown} of {total}"), 상태줄에 `explorer.find.status` 를 보인다. 거르는 동안에도 `..` 행은 목록 맨 위에 남는다. 이동용 행이라 이름을 맞춰 보지 않고 개수에도 넣지 않는다. 맞는 항목이 없으면 바는 "0 of N" 을 그대로 두고, 목록 자리에 search 글리프와 `explorer.find.none_filter`("No names match “{query}”", 보조 줄 없음) 상태 화면을 보인다. mirror explorer 도 거르기를 쓴다.
- **보이는 항목만 다룬다**: 검색어나 Subfolders 가 바뀌면 보이지 않게 된 항목을 선택에서 뺀다. 전체 선택·Shift 범위 선택·타입어헤드는 보이는 항목만 대상으로 한다. 숨은 항목에 명령이 닿지 않게 하기 위해서다.
- **Subfolders**(로컬만, mirror 는 체크박스를 숨긴다): 켜면 같은 바가 지금 폴더부터의 하위 폴더 검색이 된다. 로컬 읽기 worker(`local_reads::search`)가 너비 우선으로 폴더를 읽어 맞는 항목을 100ms 마다 묶어 보내고 화면을 깨운다. 링크로 된 폴더는 따라 들어가지 않는다. 결과는 목록을 대신하고 `..` 행을 두지 않으며 지금 정렬로 정렬한다. 새 묶음만 정렬해 이분 탐색으로 자리에 끼워 넣으므로 받을 때마다 전체를 다시 정렬하지 않는다. 상태줄·바·상태 화면은 보이는 항목을 복사하지 않고 세거나 참조한다. Detail 은 Type 대신 Name 뒤에 Folder 열(`explorer-search-folder-col-width` 160, mono caption, 시작 폴더 기준 상대 경로, 시작 폴더 자신은 ".")을 둔다. List·Grid 는 행 모양을 바꾸지 않고 든 폴더를 툴팁으로 보인다. 결과 하나를 고르면 상태줄에 시작 폴더 기준 경로를 보인다. 열기·복사·잘라내기·이름 변경·휴지통은 결과의 실제 경로를 쓴다.
- **검색 상태**: 진행 중은 Spinner·`explorer.find.searching`·Stop, 끝나면 `explorer.find.found`, Stop 으로 멈추면 `explorer.find.stopped`(그때까지의 결과 유지). 결과가 5,000 개(`SEARCH_MAX_HITS`)에 닿으면 worker 가 더 읽지 않고 멈춘다. stopped 는 상한에 닿은 뒤 더 맞는 항목이나 읽지 않은 폴더가 남았을 때 쓴다. 그래서 정확히 5,000 개로 끝나도 읽지 않은 폴더가 남았으면 stopped 다. 바는 `explorer.find.capped`("5,000+ found · stopped"), 상태줄은 `explorer.find.capped_hint`(검색어를 더 입력하라는 안내)이고 찾은 결과는 남는다. 읽지 못한 하위 폴더는 건너뛰고 `explorer.find.skipped`(하나면 `explorer.find.skipped_one` "1 folder skipped")를 accent-warning 으로 덧붙이며 툴팁에 그 폴더들을 적는다. 결과가 없으면 `explorer.find.none` 과 보조 줄 `explorer.find.none_sub`, 시작 폴더를 읽지 못하면 error 톤 `explorer.find.failed` 와 OS 이유, Retry(새로고침 — 새로고침은 하위 폴더 검색을 처음부터 다시 한다) 상태 화면이다.
- **닫기**: 입력의 첫 `Esc` 는 글자를 지우고 둘째 `Esc` 는 바를 닫는다. ×·토글로도 닫는다. 연 폴더를 떠나면(뒤로·앞으로·위로·주소창·폴더 열기) 닫는다. Subfolders 를 끄면 지금 폴더 거르기로 돌아간다. 바를 닫거나 검색어를 바꾸면 이전 검색의 영수증을 버려 worker 가 다음 폴더를 읽기 전에 멈춘다.
- 입력에 포커스가 있는 동안 타입어헤드와 explorer 목록 단축키를 끈다(`text_input_active`).

### 컨텍스트 메뉴 · 파일 조작

진입점별 대상 결정, 작업별 결과·피드백, 지원하지 않는 작업은 [파일 작업 계약](file-operations.md)에 있다.

**들어 있는 폴더에서 보기**(id 90, `view/main/explorer_show_in_folder.rs`): 대상이 하나이고 로컬 explorer 이며 `ExplorerView::enclosing_target`(`explorer/enclosing.rs`)이 폴더를 돌려줄 때 메뉴 맨 앞에 행과 구분선을 둔다. 링크(lstat 이 symlink)는 링크 자신만 한 번 푼 대상이 든 폴더다(`link_target`: `read_link` 결과를 링크가 든 폴더에 이어 붙이고 `.`·`..` 만 글자로 정리한다). 상위 경로의 링크는 풀지 않으므로 `~/proj` 가 다른 디스크로 링크돼 있어도 `~/proj/…` 경로로 열린다. 대상이 다시 링크면 그 링크가 든 폴더에서 그 링크를 고른다(체인을 끝까지 따라가지 않는다). 하위 폴더 검색 결과는 결과가 든 폴더다. 대상이 없는 링크와 그 밖의 항목에는 행이 없다. 고르면 툴바 이동과 같은 `EngineAction::Explorer { Navigate }` 를 보내(히스토리에 남는다) 그 폴더를 열고, `reveal_after_load` 가 Find 를 닫은 뒤 그 폴더 목록에 항목이 나타나는 프레임에 항목 하나만 고르고 그 자리로 스크롤한다. 깜박임 같은 강조는 없다. 키보드 현재 항목(cursor ring)은 이 기능이 놓지 않는다.

우클릭 컨텍스트 메뉴는 **2-단계 네이티브 메뉴 패턴**([context-menu](../../dev-guide/context-menu.md))을 따른다: 렌더 중 우클릭을 감지하면 `ExplorerAction::ContextMenu { target, cwd, x, y }` 를 모으고, `apply_explorer_action` 이 이를 `PendingNativeMenu::Explorer`/`ExplorerFavorite` 슬롯에 선점한다.
비-terminal 컨텍스트 메뉴는 winit 이 만들지 않고 egui 프레임이 단일 생산자다 — explorer 메뉴는 같은 egui 프레임 안에서 `apply_explorer_action`(렌더 루프 종료 직후)이 generic surface fallback(`emit_surface_menu_fallback`)보다 **먼저** 슬롯을 선점하므로, fallback은 `is_none()` 확인 후 건너뛰어 explorer 전용 메뉴를 유지한다.
이후 `MainView::process_pending_native_menu` 가 `open_native_menu` 로 OS 네이티브 메뉴를 띄우고, 선택 id 를 조작으로 번역하는 처리는 continuation 으로 예약된다(Linux 는 메뉴가 닫힌 뒤 프레임에 실행 — [context-menu](../../dev-guide/context-menu.md) · [ADR-0036](../../adr/0036-overlay-scope-and-lifetime.md)).
메뉴를 열 때 대상 경로·현재 폴더와 그 surface 의 세대(`SurfaceBinding`)를 고정한다. 고른 항목을 실행할지 정하는 규칙과 rename 팝업의 확인은 [파일 작업 계약](file-operations.md#경로-출처와-작업-대상-공통-계약)에 있다. 즐겨찾기 행 메뉴도 같은 규칙을 쓰며 그중 이 폴더로 루트 설정만 파일을 바꾸지 않는 항목으로 본다.

대상(target)은 우클릭 위치/선택 상태로 결정한다(design §3.3 target rule): 선택 안의 항목 → 선택 전체, 선택 밖 → 그 항목으로 선택 리셋, 빈 영역 → 현재 폴더(current). variant 4종(빈 영역 / 파일 / 폴더 / 다중). 좌측 사이드바 트리 폴더 우클릭도 **단일 폴더 target 을 직접 구성**해(선택집합 미조작) 동일 메뉴를 띄운다.

**surface의 나머지 영역**: 위 위치별 핸들러가 처리하지 못한 우클릭(툴바/주소창/내부 탭바/상태줄/빈 사이드바 등 chrome 영역)은 `draw_explorer` 끝의 **표면 전체 rect catch-all** 이 `Empty`(현재 폴더) target 으로 처리한다. 하위 위젯이 이미 `action`을 만들었으면 건너뛰므로 파일/폴더/다중 선택 메뉴를 유지한다. 이로써 generic surface fallback("터미널 ID 복사")이 explorer 표면 어디에서도 뜨지 않는다(불가침 원칙 §1·§2). 예외: 권한 거부 루트(`LoadState::NoPermission`)는 붙여넣기가 무의미하므로 catch-all 을 건너뛴다(content 빈영역 규칙과 동일).

- **새 폴더 / 새 파일** (빈 영역·단일 폴더, mirror 에서 숨김) — 위 "새 폴더 · 새 파일".
- **경로 복사** (`copy_path`, 다중은 개행 결합) → OS 텍스트 클립보드 + `toast.copied_path` 토스트(단축키/Command Palette/우클릭 메뉴 모두 동일).
- **복사 / 잘라내기 / 붙여넣기** — explorer 내부 파일 클립보드(`MainViewState::explorer_clipboard`, 창마다 단일 슬롯·세션 종료 시 폐기)에 경로+cut 플래그를 담고, 붙여넣기에서 소비한다.
  파일 복사·이동·휴지통·이름 변경·시스템 열기는 View가 고정 경로와 원 surface/View identity를 요청으로 넘기고 App의 `explorer_files` worker가 실행한다. 파일 이동 헬퍼는 `src/app/explorer_files/ops.rs`에 있으며 충돌 시 `(copy)` 접미사를 붙이며 목적지 공개는 OS의 덮어쓰기 금지 rename으로 수행한다. 복사는 목적지의 전용 임시 디렉터리에서 준비하고 실패 시 제거한다. 심볼릭 링크는 따라가지 않고 링크로 복사하며 별칭을 해소한 실제 하위 디렉터리로의 복사는 거부한다. cut은 교차 파일시스템 오류일 때만 copy+remove로 전환한다.
  View별 대기 요청은 8개, 요청 경로·이름 자료는 1MiB 이내이며 App은 한 작업씩 실행한다. 이 상한에 걸린 사용자 요청은 나누어 보내지 않고 받지 않으며, 그 칸에 경고 카드로 이유를 보인다(대기열이 차면 `explorer.request.queue_full`, 자료가 1MiB 를 넘으면 `explorer.request.too_large`, [파일 작업](file-operations.md)). 시작 전 원 대상과 mirror 제한을 다시 검사한다. 완료 뒤 원 View/surface가 살아 있을 때만 목록 갱신을 요청하고, 변경된 선택이나 새 클립보드는 지우지 않는다. cut은 전부 성공한 원 클립보드만 소진하며 부분 성공은 기존 목록을 유지한다.
  종료는 신규 실행을 막고 최대 5초 실제 worker join을 관측한다. 기한이 지나도 작업 취소나 완료로 기록하지 않으며 남은 worker를 경고한다. 로컬 목록·트리 읽기는 이 worker 와 분리된 읽기 worker 가 맡아 대형 복사가 목록 조회를 막지 않는다.
  잘라내기는 이동 성공 시 클립보드를 비운다.
  우클릭 메뉴뿐 아니라 키보드 단축키(기본 `copy`/`cut`/`paste` 바인딩, explorer 포커스 시)로도 동일하게 동작한다 — `handle_explorer_shortcut`(`src/adapters/ui/input/shortcuts/copy_paste.rs`)가 선택 항목을 모아 컨텍스트 메뉴와 같은 `explorer_menu_set_clipboard`/`explorer_menu_paste` 를 호출하므로 fs 동작이 두 경로에서 갈라지지 않는다.
  단축키 붙여넣기 대상은 현재 폴더(current)다(선택된 폴더 안으로의 paste-into 는 컨텍스트 메뉴 전용).
  **복사(cut=false)** 는 fs 접근이 없어 mirror explorer 에서도 그대로 동작하지만, **잘라내기(cut=true)/붙여넣기**는 mirror 에서 메뉴·단축키 모두 차단된다(위 "mirror(attach) explorer 의 파일 변경 차단" 참고).
  클립보드는 경로와 함께 그 경로의 출처(`ExplorerPathSource`: 로컬 또는 mirror workspace)를 기록한다(`src/state/explorer_path.rs`). mirror explorer 에서 복사한 경로는 원격 호스트의 경로이므로, 로컬 explorer 의 붙여넣기는 메뉴에 나오지 않고 단축키로 실행해도 `explorer.state.remote_paste_unsupported` toast 로 거부한다. 같은 문자열의 로컬 파일을 대신 복사하지 않는다.
- **휴지통으로 이동** (`delete`) — `trash` 크레이트로 OS 휴지통에 보낸다(가역적이라 확인 모달 없음). mirror 에서 차단.
- **이름 변경** (`rename`, 단일만) — 공용 rename 팝업(`PopupDef`)을 재사용한다. 이름은 드라이브 접두어·경로 구분자 없는 단일 파일명이어야 하며 기존 항목을 덮어쓰지 않는다. mirror 에서 차단(가드가 먼저 막아 팝업 자체가 열리지 않는다).
- **OS 기본 앱으로 열기** (`open_in_system`, 단일 폴더만) — `platform::reveal::open_path`(Windows `explorer` / macOS `open` / Linux `xdg-open`). mirror 에서 차단.
- **즐겨찾기 추가** (`add_to_favorites`, 단일 폴더 또는 빈 영역) — 아래 참조. mirror 에서 차단.
- **새 탭으로 열기** (`open_in_new_tab`, 단일 폴더) — 그 폴더를 cwd 로 하는 새 explorer 를 **Pane 탭**(explorer 내부 탭이 아님)으로 연다. 우클릭 대상 surface 의 **소유 pane** 에 추가해(`RequestContext::add_kind_tab_by_owner`) focused pane 이 아니어도 올바른 pane 에 열린다. 기존 explorer 는 불변. mirror 에서 메뉴 자체가 숨겨지고 핸들러도 막는다(위 "mirror(attach) explorer 의 파일 변경 차단").
- **이 폴더로 루트 설정** (`set_as_root`, 단일 폴더) — **현재 explorer** 의 cwd 를 그 폴더로 이동한다(`RequestContext::set_explorer_cwd` 가 `EngineAction::ExplorerCwd` 를 보내고 App 이 `ExplorerTab::set_cwd` 를 적용: 좌측 트리 루트·current 이동 + 히스토리 초기화 + 뷰 리로드). 파일시스템을 바꾸지 않으므로 mirror 에서도 그대로 동작.

### 경로 출처와 작업 대상

파일 작업이 공유하는 규칙(경로의 출처, 요청 시점의 대상 고정, 진입점별 원격 쓰기 거부)은 [파일 작업 계약](file-operations.md#경로-출처와-작업-대상-공통-계약) 한 곳에 있다.

### 심볼릭 링크

목록은 `read_dir_entries`(`src/core/fs_list.rs`)가 읽는다. 링크 항목은 대상의 metadata 로 종류·크기·수정 시각을 정하고 `DirEntryInfo::link` 에 상태를 남긴다(`NotALink` / `Valid` / `Broken`).

- **폴더를 가리키는 링크**는 폴더로 보이고 트리에도 나온다. 들어가면 경로는 링크 자신의 경로다. 주소창·히스토리·뒤로/위로는 대상의 실제 경로가 아니라 링크 경로를 쓴다. 따라서 링크 폴더에서 위로 가면 링크가 있던 폴더로 돌아간다.
- **대상이 없는 링크**는 폴더로 보지 않으며 크기·수정 시각은 링크 자신의 값이다. 열면 파일을 찾지 못한 것처럼 보이지 않도록 `explorer.state.broken_link` 오류 toast 로 대상이 없다는 원인을 알린다. 주소창 입력도 같은 이유로 거부한다.
- **목록 표시**(`explorer/link.rs`, Detail · List · Grid 공통): 대상이 있는 링크는 대상의 글리프(폴더·파일·그림)를 쓰고 이름 뒤 `space-xs` 자리에 link 글리프 12를 붙인다. 세 보기 모두 꼬리 폭(`space-xs` + 글리프)을 먼저 빼고 이름을 그 앞에서 자른다. 그래서 이름이 길어도 글리프는 보인다. Detail 은 이름을 말줄임하고, List 는 이름 칸을 그 폭만큼 줄여 클립한다(공용 `tree_row_with_tail`). Grid 는 이름의 마지막 줄 끝에 붙이고, 그 폭만큼 이름 줄 폭을 줄인다. 대상이 없는 링크는 항목 자리에 link 글리프를 accent-warning 으로 그리고 이름은 보통 색(취소선 없음)이다. 호버하면 `explorer.link.target_missing`("Target not found: {path}")으로 `read_link` 가 돌려준 대상 경로를 보인다(호버할 때만 읽는다). 색·크기는 `explorer-link-glyph`(→ text-muted)·`explorer-link-glyph-size`(→ icon-size-xs 12)·`explorer-link-broken-fg`(→ accent-warning) 토큰이다. Type 열 낱말은 위 종류 문구다.
- **링크 자체에 대한 조작**: 복사·붙여넣기는 링크를 링크로 복사한다. 이름 변경과 cut 의 교차 파일시스템 정리(`remove_path`)는 링크만 옮기거나 지운다. 휴지통(`trash` 크레이트)은 모든 OS 에서 부모 경로만 canonicalize 하고, Linux(freedesktop) 구현은 링크 항목 자체를 휴지통으로 옮긴다. macOS·Windows 의 링크 휴지통 동작은 실 기기에서 확인하지 않았다. 어느 조작도 대상 폴더나 그 내용을 바꾸지 않는다.
- 원격 목록 응답은 링크 상태를 싣지 않는다. 원격 서버도 같은 함수로 목록을 만들므로 원격의 폴더 링크도 폴더로 보이지만, 끊긴 링크 구분은 원격 항목에 없다.

### 즐겨찾기 (favorites)

전역(surface 무관)·영속 즐겨찾기 — **로컬 client 파일시스템 경로 전용**. `~/.tasty/explorer-favorites.toml`(`[[favorite]]` 배열, label+path)에 저장된다. 프로세스에 원본 하나(`RuntimeRegistries::explorer_favorites`)를 두고 윈도우마다 있는 engine 은 그리기용 사본만 가진다. 추가·제거는 `EngineAction` 이 `EngineRuntime::explorer_favorites.change` 로 원본을 바꾸고 원본 전체를 파일에 쓴다. 그래서 한 윈도우의 저장이 다른 윈도우에서 추가한 항목을 지우지 않고, 그리기 밖에서 적용되는 제거(Favorites 행 네이티브 메뉴)도 다른 윈도우에 바로 보인다. 사본 갱신·다른 윈도우 다시 그리기·파일 재읽기 시점은 [App·Engine·View 상태 소유권](../../dev-guide/app-state-ownership.md#윈도우가-함께-쓰는-영속-목록)을 따른다. 메모리 mutator(`add`/`remove`)는 순수하고 디스크 반영은 호출처가 `save()` 로 한다(테스트가 디스크를 건드리지 않게 분리). mirror(attach 원격 점유) explorer 의 경로는 원격 호스트의 경로라 이 전역 목록에 섞일 수 없다 — "즐겨찾기 추가"는 mirror explorer 에서 차단된다(위 "mirror(attach) explorer 의 파일 변경 차단" 참고).

- **추가**: 컨텍스트 메뉴 "Add to favorites" → rename 팝업과 동일 골격의 입력 팝업(`RenameTarget::ExplorerAddFavorite`, 확정 버튼 라벨만 "Add")으로 라벨을 받아 등록(같은 경로 재등록 시 라벨만 갱신).
- **표시/이동**: 사이드바 하단 고정(pin) "Favorites" 영역(사이드바 본문이 240px 이상이면 캡션 **항상 표시**)에 **채운 별(STAR_FILL) + accent-warning(골드)** 행으로 나열, 클릭 시 해당 경로로 이동. 현재 폴더인 즐겨찾기는 surface-active 하이라이트. 이 영역은 위 "사이드바 트리"에 서술한 고정 높이로 보이며 자체 스크롤된다. 본문이 240px 미만인 낮은 칸에서는 영역 전체를 숨긴다.
- **여러 윈도우**: 한 윈도우에서 추가·제거한 결과는 다른 윈도우의 Favorites 에도 바로 보이고, 재시작 뒤에도 모두 남는다.
- **빈 상태(empty state)**: 즐겨찾기가 0개여도 섹션이 사라지지 않는다(발견성) — 흐린 별(opacity 0.55) + `explorer.sidebar.favorites_empty`("No favorites yet") + 우클릭 힌트(`favorites_empty_hint`, "Add to favorites" 스팬만 text-muted, 나머지 text-placeholder)를 표시한다(design `FavoritesEmpty`).
- **제거/열기/루트 설정**: 즐겨찾기 행 우클릭 → `PendingNativeMenu::ExplorerFavorite`(우클릭 explorer 의 `surface_id` 동봉) → "새 탭으로 열기" / "이 폴더로 루트 설정" / "즐겨찾기에서 제거". 제거는 전역이라 경로만으로 하지만, "루트 설정" 은 `surface_id` 로 대상 explorer 를 지정한다.
- 즐겨찾기 목록은 `EngineRead` 가 빌려 주는 읽기 전용 slice 로 `draw_explorer` 에 전달된다. 화면은 목록을 바꾸지 않고 추가·제거를 요청만 한다.

### 사용자 트리거 (단축키 — [KeybindingSettings](../../features/keybindings/index.md))

모든 단축키는 `KeybindingSettings` 로 노출되며 하드코딩하지 않는다. `convert_to_explorer` 외에는 explorer 포커스에서만 동작:

| 액션 | 필드 | Tasty 프리셋 기본 |
|------|------|------|
| 새로고침 | `explorer_refresh` | `F5` |
| 상위 폴더로 | `explorer_go_up` | `Alt+Up` |
| 미리보기 패널 켜기·끄기 | `explorer_toggle_preview` | (기본 미할당) |
| Properties 열기 | `explorer_properties` | (기본 미할당) |
| 전체 선택 | `select_all` | `Ctrl+A` / `Alt+A` |
| 경로 복사 | `copy_path` | `Alt+Shift+C` |
| 새 폴더 | `explorer_new_folder` | (기본 미할당) |
| 새 파일 | `explorer_new_file` | (기본 미할당) |
| 현재 항목 이동 (위·아래·왼쪽·오른쪽·처음·끝·한 화면 위·아래) | `explorer_cursor_*` | 방향키 · `Home` · `End` · `PageUp` · `PageDown` |
| 선택 넓히기 (같은 이동) | `explorer_extend_*` | 위 키에 `Shift+` |
| 이름 변경 | `explorer_rename` | `F2` |
| 휴지통으로 이동 | `explorer_trash` | `Delete` / `Alt+Backspace`(macOS 에서 ⌘⌫). macOS 프리셋은 `Alt+Backspace` 만, Windows·Linux 프리셋은 `Delete` 만 |
| 찾기 | `find` | `Ctrl+F` / `Alt+F` |
| explorer 로 변환 | `convert_to_explorer` | (기본 미할당) |

직접 키 매칭은 `explorer_refresh`·`explorer_go_up`·`explorer_new_folder`·`explorer_new_file`·`explorer_toggle_preview`·`explorer_properties`·`find`·`convert_to_explorer`(포커스 surface 무관) 가 `keybinding.rs`, `select_all`·`copy_path`·`explorer_rename`·`explorer_trash` 가 `copy_paste.rs`, 이동·넓히기가 `explorer_keys.rs` 다. 이동·넓히기·이름 변경·휴지통은 전역 목록이 아닌 탐색기 목록(`EXPLORER_LIST_BINDING_FIELDS`, [단축키](../../features/keybindings/index.md#탐색기-목록-바인딩--explorer_list_binding_fields))에 있다. action-id/Command Palette `dispatch.rs` 는 열 모두를, 더블탭 `double_tap.rs` 는 `convert_to_explorer` 만 받는다. `find` 는 터미널 검색과 같은 바인딩이며, 포커스가 explorer 면 터미널 검색 대신 Find 바를 연다([터미널 검색](../../features/terminal-search/index.md)). 설정 UI 서브탭은 `explorer_refresh`·`explorer_go_up`·`explorer_new_folder`·`explorer_new_file`·`explorer_rename`·`explorer_trash`·`explorer_toggle_preview`·`explorer_properties` = **Explorer**, `select_all`·`copy_path` = **Clipboard**, `convert_to_explorer` = **Surface**.
주소창·새 항목 이름 입력·Find 입력이 키를 받는 동안(`ExplorerView::text_input_active`)에는 explorer 목록 단축키(`keybinding.rs` 의 explorer 묶음, `copy_paste.rs` 의 전체 선택·경로 복사·복사·잘라내기·붙여넣기·이름 변경·휴지통)가 키를 소비하지 않고 글자 편집에 양보한다.

**새 탭으로 탐색기 열기(`open_explorer`, 기본 미할당)는 포커스와 무관하다** — 위 표와 달리 explorer 포커스를 요구하지 않는다. `Intent::NewTab { kind: "explorer" }` 를 발생시키므로 CLI 의 `new tab --type explorer` 와 같은 도메인 인텐트(`CreateTab`)를 쓰되 선택은 다르다 — 단축키는 새 탭을 선택하고, 에이전트(CLI/IPC)는 선택하지 않는다([ADR-0059](../../adr/0059-id-targets-and-view-owned-selection.md)). 이 액션은 경로를 안 실으므로 홈에서 열린다(명시 경로는 CLI 의 `--path` 가 받는다). 설정 UI 는 **Tab** 서브탭이다 — `open_markdown` 옆, 둘 다 새 탭 열기라서. 이 액션은 `keybinding.rs`·`double_tap.rs`·`dispatch.rs` 세 진입점 전부에서 처리한다.

**드래그 반전 modifier(`explorer_drag_flip_modifier`)** 는 키 조합이 아니라 드래그 중 누르고 있는 modifier 다. 값은 `tab_switch_modifier` 와 같은 modifier 조합 문자열이고, 기본은 macOS `option`, 다른 OS `ctrl` 이다. 누르고 있으면 드래그 놓기의 기본 동작(같은 디스크 이동, 다른 디스크 복사)을 뒤집는다([드래그 앤 드롭](file-operations.md#드래그-앤-드롭)). 설정 › 단축키 › General 서브탭 끝(엔트리 아래 구분선 뒤)의 한 행에서 고르거나 config.toml 의 `[keybindings]` 에서 바꾼다. 단축키 가져오기는 이 값도 그대로 옮기며, macOS 에서 내보낸 `option` 은 다른 OS 에서 Win·Super 를 누른 채 끄는 동작이 된다([key-mapping](../../design/policies/key-mapping.md#이식-시-option-처리)).

### 타입어헤드로 항목 선택

목록에 포커스가 있을 때 영숫자를 입력하면 그 글자로 시작하는 항목이 선택되고 그 항목이 보이도록 스크롤한다. grid·list·detail 세 뷰 모두 같다. 같은 글자를 이어서 누르면 후보를 순환하고 마지막 다음에는 처음으로 돌아간다. 다른 글자를 이어서 입력하면 그 글자를 합친 접두사로 찾는다. 마지막 입력 후 1초가 지나면 버퍼를 비우고 다음 글자를 새 검색으로 읽는다. 일치하는 항목이 없으면 선택과 버퍼가 그대로 남아, 오타 한 글자가 그 뒤의 입력을 막지 않는다. 비교할 때 대소문자는 구분하지 않으며, `..`는 화면에만 있는 행이라 대상이 아니다.

이 동작에는 `KeybindingSettings` 항목이 없다. 키 조합에 액션을 연결하는 것이 아니라 문자 입력을 그대로 소비하는 방식이라 위 표와는 범주가 다르다. 다만 사용자가 수식 키 없이(또는 shift만 붙여) 영숫자를 단축키로 등록하면 그 글자는 타입어헤드가 소비하지 않고 단축키에 넘긴다. 한 번의 키 입력으로 둘이 함께 동작하기 때문이다. 텍스트 이벤트가 단축키 처리보다 먼저 egui 큐에 들어가고 그것을 되돌릴 방법이 없다([key-mapping](../../design/policies/key-mapping.md)의 "텍스트 입력과 단축키의 우선순위"). Tasty 기본 프리셋에는 그런 단축키가 없다.

다음 네 조건 중 하나라도 해당하면 그 프레임의 글자를 무시하고 버퍼도 비운다. 버퍼를 남겨두면 다시 돌아왔을 때 예전 접두사가 이어져, 사용자가 입력하지 않은 글자로 검색하게 된다.

- 그 explorer surface가 포커스를 가지고 있지 않다. egui 이벤트 큐는 창 전체가 공유하므로, 이 조건이 없으면 한 번의 입력이 열려 있는 모든 explorer의 선택을 동시에 움직인다.
- 모달·팝업·입력 다이얼로그가 떠 있다(`keyboard_overlay_open`).
- 주소창을 편집 중이다(`addr_editing`). 그 글자는 `PathField`가 받는다.
- 목록이 `LoadState::Ok`가 아니거나 비어 있다.

폴더나 정렬이 바뀌어 목록을 다시 읽어 오거나(`sync`), cwd나 내부 탭을 바꾸는 액션이 적용되면(주소창 편집을 취소하는 것과 같은 자리) 버퍼를 비운다. 인덱스가 가리키는 항목이 달라지기 때문이다.

### 현재 항목 · 영역 선택

**현재 항목**(`view/cursor.rs`)은 선택과 따로 있는 키보드 위치다. 이동 키를 누르면 현재 항목을 옮기고 그 항목만 고른다. 넓히기 키(기본 Shift+이동)는 기준 항목(`anchor`)부터 새 현재 항목까지 고른다. 시작 위치는 키로 움직이는 중이면 현재 항목, 아니면 클릭·타입어헤드가 정한 기준 항목이고, 둘 다 목록에 없으면 첫 항목(End 는 마지막)이다.

- Detail·List 의 위·아래는 한 항목, 왼쪽·오른쪽은 움직이지 않는다. Grid 의 위·아래는 한 줄(열 수)씩, 왼쪽·오른쪽은 한 칸씩이며 줄을 넘어 이어진다. 아래 줄에 같은 열 칸이 없으면 마지막 항목으로 간다.
- 한 화면 위·아래는 목록 높이에 온전히 들어가는 줄 수 × 열 수만큼 간다. 처음·끝은 목록 양 끝이다.
- 옮긴 항목이 지난 프레임에 온전히 보이지 않았으면 그 항목을 화면 가운데로 스크롤한다(타입어헤드와 같은 `scroll_to` 경로). 보이던 항목이면 스크롤하지 않는다.
- 키로 움직인 뒤에만 그 칸 안쪽에 1px 테두리를 그린다(design `explorer-cursor-ring`, 선택 채움 위, 모서리 radius-sm). 목록을 누르면 다음 키까지 숨는다.
- 주소창·이름 입력·Find 입력이 키를 받는 동안과 영역 선택 중에는 움직이지 않는다.

**영역 선택**(`view/marquee.rs`)은 목록의 빈 곳(항목 칸도, 누를 수 있는 다른 위젯도 아닌 곳)을 누르고 끌 때 시작한다. 클릭만 하면 선택을 바꾸지 않는다.

- 사각형에 걸친 항목을 고른다. Ctrl·Cmd·Shift 를 누르고 시작하면 그때의 선택에 더하고, 아니면 새로 고른다. 기준 항목은 걸친 항목의 첫 항목이다.
- 시작점은 목록 좌표로 둔다. 스크롤해도 처음 누른 자리에 남고, 화면 밖 항목도 칸 배치(`view/list_layout.rs` 의 `ListLayout`: 첫 칸 자리·칸 간격·칸 크기·열 수·첫 줄 앞 `..`·이름 입력 칸 수)로 계산해 고른다. 칸 사이 간격에만 걸치면 고르지 않는다. 배치는 세 보기가 그릴 때 기록한다(Grid `note_grid`, List `note_rows`, Detail 은 처음 그린 이름 칸으로 `note_detail`).
- 사각형은 accent-primary 의 12% 채움과 36% 1px 테두리, 모서리 없음으로 목록 영역 안에만 그린다(design `explorer-marquee-bg`·`-border`).
- 포인터가 목록 위·아래 끝 24px 띠(design `explorer-autoscroll-zone`, 표시 없음) 안에 있으면 그 쪽으로 스크롤하고, 끝에 가까울수록 빠르다(띠 바깥 경계 0, 가장 바깥 800 px/s). 목록 밖으로 나가면 멈춘다. egui ScrollArea 가 포인터가 안에 있을 때만 스크롤 입력을 받기 때문이다. 스크롤 양은 다음 프레임 목록 ScrollArea 앞에서 휠 입력처럼 넣는다(`preview::split`).
- 영역 선택 중에는 항목 드래그(파일 끌어 놓기)를 시작하지 않는다. 다른 위젯이 먼저 끌기를 가져갔거나 popup·modal 이 떠 있으면 시작하지 않는다.

### 폰트

Appearance → **Explorer** 서브탭에서 surface 폰트를 오버라이드한다 (`appearance.plugin_font_overrides["explorer"]`, `effective_font_for_kind("explorer")` 가 읽음).

## 검증 기준

- Given 윈도우 두 개 When 각 윈도우의 explorer 에서 다른 폴더를 즐겨찾기에 추가한다 Then 두 윈도우의 Favorites 에 둘 다 보이고 재시작 뒤에도 둘 다 남는다(`engine_action.rs` 의 `favorites_added_in_two_windows_both_survive_a_restart`).
- Given 윈도우 두 개에 같은 즐겨찾기가 보인다 When 한 윈도우에서 즐겨찾기를 제거한다 Then 다른 윈도우의 Favorites 에서도 바로 사라진다(`view_frame.rs` 의 `a_shared_list_change_redraws_every_window_once`).
- Given 로컬 explorer When 하위 폴더로 이동한다 Then 목록이 `Loading` 을 거쳐 새 폴더의 항목으로 바뀌고, 이전 폴더의 늦은 결과는 반영되지 않는다(`src/app/local_reads.rs` 의 `replaced_directory_receipt_cannot_publish_its_old_result`, mirror 는 `view.rs` 의 `apply_remote_list_dir_result_ignores_stale_request_id`).
- Given 사이드바 트리에서 하위 폴더를 펼친 로컬 explorer When 외부 프로그램이 그 하위 폴더에 폴더를 만든 뒤 `F5` 를 누른다 Then 목록과 펼친 트리가 함께 다시 읽혀 새 폴더가 트리에도 보인다(`view.rs` 의 `an_explicit_reload_of_the_same_folder_rereads_the_tree`).
- Given 포커스를 가진 탭의 로컬 explorer When 다른 프로그램이 현재 폴더에서 항목 하나를 지우고 하나를 만든다 Then 다음 확인에서 보이던 목록을 둔 채 다시 읽어 새 목록으로 바뀌고, 선택은 사라진 항목만 빠진다(`view/poll_tests.rs` 의 `a_changed_folder_is_reread_while_the_old_list_stays`).
- Given 포커스를 가진 탭의 로컬 explorer When 폴더가 바뀌지 않은 채 확인한다 Then 목록을 다시 읽지 않고 창도 다시 그리지 않는다(`an_unchanged_folder_is_not_read_again`). 펼친 트리 폴더 둘 중 하나만 바뀌면 그 폴더만 다시 읽고, 결과가 올 때까지 보이던 하위 목록을 둔다(`only_a_changed_expanded_tree_folder_is_read_again`). 트리에 펼친 현재 폴더가 바뀌면 목록만 한 번 읽고 그 결과로 트리 항목도 바꾼다(`the_current_folder_in_the_tree_is_read_once_and_its_node_is_replaced_in_place`).
- Given 다른 탭에 가려진 explorer 와 mirror explorer When 주기가 지난다 Then 확인하지 않고 App 도 그 때문에 깨어나지 않는다. 그 탭이 포커스를 얻으면 바로 확인한다(`a_tab_without_focus_is_never_checked`, `gaining_focus_checks_at_once_and_then_every_interval`, `a_panel_not_drawn_this_frame_stops_waking`, `a_mirror_explorer_is_not_checked`).
- Given 포커스 탭에 로컬 explorer 가 있는 창 When 다른 앱이 포커스를 가져간 동안 주기가 지난다 Then 확인하지 않고 깨어나지도 않는다. 창이 포커스를 되찾은 프레임에 주기를 기다리지 않고 확인한 뒤 주기를 다시 시작한다(`view/poll_tests.rs` 의 `only_the_focused_tab_of_the_focused_window_is_checked_and_regaining_focus_checks_at_once`).
- Given 로컬 explorer 가 A/B/C 를 보고 있다 When B 가 통째로 지워져 읽기 오류 화면에서 "상위 폴더로" 를 누른다 Then A 로 간다(`local_reads.rs` 의 `a_missing_folder_names_its_nearest_existing_ancestor`, `view.rs` 의 `go_up_from_a_vanished_folder_skips_vanished_parents`).
- Given 읽기 권한이 없는 폴더 When 들어간다 Then 권한 거부 화면이 나오고 툴바·트리는 그대로 쓸 수 있다.
- Given 폴더를 우클릭한다 When "새 탭으로 열기"를 고른다 Then 우클릭한 surface 의 pane 에 그 폴더를 cwd 로 하는 explorer Pane 탭이 생기고 원래 explorer 는 바뀌지 않는다.
- Given mirror explorer When 폴더를 탐색하고 파일을 더블클릭한다 Then 목록은 원격 조회로 오고, 파일은 원격에 탭으로 열리거나 열 수 없다는 토스트가 나온다. 로컬 파일시스템은 읽지 않는다.
- Given mirror explorer When 잘라내기·붙여넣기·휴지통·이름 변경·새 탭 열기를 단축키나 메뉴로 시도한다 Then 메뉴에 없거나 `remote_write_unsupported` 토스트가 나오고 아무것도 바뀌지 않는다.
- Given 붙여넣기가 진행 중이다 When 그 surface 를 닫는다 Then 이미 시작한 작업은 끝까지 실행되고 닫힌 surface 에 결과 카드나 목록 갱신을 내지 않는다. 아직 시작하지 않은 요청은 실행되지 않는다. 이름 충돌 답을 기다리던 작업은 취소되고 기존 항목은 그대로다.
- Given 로컬 explorer 의 폴더에 "New folder" 가 있다 When 툴바의 New folder 를 누른다 Then 목록 맨 위에 "New folder 2" 가 전체 선택된 입력이 열리고, Enter 를 누르면 그 폴더가 생겨 정렬 자리에서 선택된다(`explorer_files/tests.rs` 의 `a_created_entry_is_selected_only_while_its_folder_is_still_shown`, `create/tests.rs`).
- Given 이름 입력이 열려 있다 When 이미 있는 이름으로 Enter 를 누른다 Then 입력은 열린 채 "already exists" 오류가 보이고 디스크는 바뀌지 않는다. Esc 를 누르면 아무것도 만들지 않고 닫힌다.
- Given 쓸 수 없는 폴더 When 툴바를 본다 Then New folder·New file 이 비활성이다. Given mirror explorer When 툴바·메뉴를 본다 Then 두 명령이 없다.
- Given 로컬 explorer 에 Report.pdf·report-draft.txt·notes.md 가 있다 When Find 를 열고 "re" 를 입력한다 Then 두 항목만 남고 맞는 부분이 강조되며 바에 "2 of 3" 이 보인다. 첫 `Esc` 는 글자를 지우고 둘째 `Esc` 는 바를 닫는다(`find/tests.rs`).
- Given 거르기 전 notes.md 를 골랐다 When 그 항목이 걸러진다 Then 선택에서 빠져 이후 명령이 닿지 않는다(`find/tests.rs` 의 `the_filter_hides_rows_that_do_not_match_and_counts_what_is_shown`).
- Given 하위 폴더에 맞는 파일과 읽을 수 없는 폴더가 있다 When Subfolders 를 켠다 Then 결과가 들어오는 대로 채워지고 Detail 에 Folder 열이 생기며, 끝나면 "{n} found · 1 folder skipped" 가 보인다. 링크로 된 폴더는 들어가지 않는다(`local_reads/search/tests.rs`).
- Given 칸 폭이 440 보다 좁다 When 툴바를 그린다 Then New folder·New file·Find·Preview 가 모두 More 하나로 접히고, 메뉴의 view 행은 지금 상태에 따라 Find·Close find, Show preview·Hide preview 를 쓴다(`commands.rs` 의 `preview_sits_in_the_view_group_and_folds_into_more_with_it`, `explorer_create.rs` 의 `more_rows_name_the_action_they_take`).
- Given 하위 폴더 검색이 5,000 개보다 많이 맞는다 When 검색이 끝난다 Then 5,000 개만 남고 바에 "5,000+ found · stopped", 상태줄에 범위를 좁히라는 안내가 보인다(`local_reads/search/tests.rs` 의 `a_search_stops_at_the_hit_cap_and_keeps_what_it_found`, `find/tests.rs` 의 `one_skipped_folder_and_the_hit_cap_have_their_own_words`).
- Given 거르기 검색어에 맞는 이름이 없다 When 목록을 그린다 Then 바는 "0 of N", 목록 자리는 "No names match “{query}”" 이다(`find/tests.rs`).
- Given 거르는 중이다 When 검색어와 목록이 그대로인 채 프레임이 이어진다 Then 다시 거르지 않고, 검색어나 목록이 바뀌면 다시 거른다(`find/tests.rs` 의 `the_filter_is_not_run_again_while_the_query_and_the_list_stay`, `a_new_query_or_a_new_list_filters_again`).
- Given 지금 폴더 항목 둘과 하위 폴더 검색에서 고른 항목 하나가 선택돼 있다 When 검색어를 바꾼다 Then 새 검색어에 맞는 지금 폴더 항목만 선택에 남고, 하위 폴더 검색 중에는 그 결과에 든 항목만 남는다(`find/tests.rs` 의 `a_new_query_keeps_only_the_selected_items_it_still_shows`, `subfolder_results_keep_only_the_selected_hits`).
- Given Find 바가 열려 있다 When 하위 폴더로 이동한다 Then 바가 닫힌다(`find/tests.rs` 의 `leaving_the_folder_closes_the_bar`).
- Given 항목 20,000개 폴더 When detail·list·grid 로 본다 Then 화면에 걸친 항목만 그리고(200개 미만), 15,000번째 항목을 고르거나 타입어헤드로 18,765번째로 가면 그 항목이 화면 가운데로 오고 클릭하면 그 항목이 선택된다. 이름 입력을 연 채 목록 끝으로 스크롤해도 입력은 열린 채 포커스를 유지한다(`explorer/virtual_tests.rs`).
- Given 항목 20,000 개 폴더 When 세 보기에서 End 를 누른다 Then 마지막 항목만 선택되고 화면으로 스크롤된다. 그 뒤 PageUp 은 한 화면만큼 가고, Shift+Up 두 번은 두 줄(Grid 는 열 수 × 2)을 더한다. Detail·List 의 오른쪽 키는 움직이지 않고 Grid 는 한 칸 간다(`virtual_tests/keyboard_select.rs`).
- Given 키로 현재 항목을 옮겨 테두리가 보인다 When 다른 항목을 클릭한다 Then 테두리가 숨고 다음 아래 키는 클릭한 항목 다음으로 간다.
- Given 항목 10 개인 Detail·List When 목록 아래 빈 곳에서 7 번째 줄까지 끈다 Then 7~9 번째가 선택되고 놓은 뒤에도 남는다.
- Given 항목 20,000 개 Grid When 오른쪽 빈 곳에서 아래 끝 띠로 끌고 머문다 Then 목록이 스크롤되며 선택이 화면 밖 항목까지 늘고, 처음 누른 줄은 선택에 남는다.
- Given 내부 탭 둘을 열고 정렬을 바꾼 explorer When 재시작한다 Then 탭·cwd·current·뷰 모드·정렬이 복원되고 히스토리와 선택은 비어 있다.

## 관련

- [work-area](../../features/work-area/index.md)(Surface/Tab/Pane 계층) · [file-handler](../../features/file-handler/index.md)(파일 열기 위임) · [convert-surface](../../features/convert-surface/index.md)(explorer 로/에서 변환) · [keybindings](../../features/keybindings/index.md) · [settings](../../features/settings/index.md)(폰트/단축키 탭)
