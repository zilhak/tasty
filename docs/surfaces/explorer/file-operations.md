# Explorer 파일 작업 — 대상과 결과의 계약

- **Status**: Partial — 새 폴더·새 파일·이름 변경·복사·잘라내기·붙여넣기·휴지통 이동은 동작한다. 속성 조회와 미리보기 패널·Grid 썸네일은 파일을 바꾸지 않는 읽기로 동작한다. 드래그 놓기·충돌 선택·진행 취소·실행 취소·검색은 없다.
- **주체**: 로컬 사용자 ([주체](../../concepts/actors.md)). 에이전트(IPC/CLI)는 이 문서의 파일 작업을 호출하지 않는다.
- **ADR**: [ADR-0022](../../adr/0022-remote-mirror-content-and-queries.md) (mirror explorer 는 파일을 바꾸지 않는다)
- **코드**: 메뉴 구성 `build_explorer_context_menu`·메뉴 핸들러 `explorer_menu_*` (`src/view/main/redraw.rs`), 단축키·Command Palette 진입 `handle_explorer_shortcut`·`run_explorer_action` (`src/adapters/ui/input/shortcuts/copy_paste.rs`), 실행 worker `src/app/explorer_files.rs`·`src/app/explorer_files/ops.rs`
- **화면**: [Explorer](index.md) 의 컨텍스트 메뉴·rename 팝업·토스트

## 목적

탐색기의 파일 작업은 진입점이 여럿이다. 우클릭 메뉴, 키보드 단축키, Command Palette 가 같은 작업을 부른다. 이 문서는 진입점마다 대상과 결과가 같아야 한다는 규칙과, 작업마다 대상·결과·사용자 피드백·상태를 정한다. 지원하지 않는 작업은 그 사실을 적는다. 화면 요소와 탐색 동작은 [Explorer](index.md) 에 있다.

## 내부 동작 (headless-valid)

### 경로 세 가지

| 이름 | 모델 | 쓰는 곳 |
|---|---|---|
| 고정 루트 (cwd) | `ExplorerTab::cwd()` | 사이드바 Files 트리의 뿌리, 스폰 cwd, 탭 이름. 탐색으로 바뀌지 않고 "이 폴더로 루트 설정"만 바꾼다 |
| 현재 폴더 (current) | `ExplorerTab::current()` (필드 `root`) | 오른쪽 목록과 주소창. 뒤로·앞으로·상위·주소창 이동이 바꾼다 |
| 선택 항목 | `ExplorerView::selected` | 현재 폴더 목록 안에서 클릭·Ctrl/Cmd+클릭·Shift+클릭(범위)·Ctrl/Cmd+Shift+클릭(범위 추가)·전체 선택으로 고른 항목. 폴더가 바뀌면 비운다 |

파일 작업은 고정 루트를 대상으로 쓰지 않는다. 대상은 선택 항목이거나 현재 폴더다.

### 대상 결정 규칙

| 진입 | 대상 |
|---|---|
| 우클릭 — 선택 안의 항목 | 선택 전체 |
| 우클릭 — 선택 밖의 항목 | 그 항목 하나. 선택도 그 항목으로 바꾼다 |
| 우클릭 — 목록 빈 영역과 툴바·주소창·내부 탭바·상태줄 | 현재 폴더 |
| 우클릭 — 사이드바 Files 트리의 폴더 | 그 폴더 하나. 선택은 바꾸지 않는다 |
| 우클릭 — Favorites 행 | 그 즐겨찾기 경로 |
| `explorer_properties` 단축키·Command Palette | 선택 항목(목록 순서). 선택이 없으면 현재 폴더 |
| 단축키·Command Palette 의 복사·잘라내기·경로 복사 | 선택 항목. 선택이 없으면 아무것도 하지 않는다 |
| 단축키·Command Palette 의 붙여넣기 | 현재 폴더 |
| 메뉴 "Paste into" (단일 폴더) | 그 폴더 |

단축키와 Command Palette 는 같은 함수(`run_explorer_action`)를 거치고, 복사·잘라내기·붙여넣기는 메뉴와 같은 핸들러(`explorer_menu_set_clipboard`·`explorer_menu_paste`)를 부른다. 같은 대상이면 진입점이 달라도 결과가 같다. 대상 surface 는 포커스된 explorer surface 이고, 메뉴는 우클릭한 surface 의 id 를 지닌다.

### 경로 출처와 작업 대상 (공통 계약)

탐색기의 파일 작업(지금의 복사·잘라내기·붙여넣기·휴지통·이름 변경·시스템 열기와, 앞으로 더할 만들기·OS 파일 클립보드·드래그·속성 조회)은 아래 규칙을 함께 따른다.

- **경로는 출처와 함께 다룬다.** explorer 경로는 로컬 경로이거나 mirror workspace 에 보이는 원격 호스트의 경로다(`ExplorerPathSource`). 같은 문자열이라도 출처가 다르면 다른 파일이다. 경로를 보관해 나중에 쓰는 자료(클립보드 등)는 출처를 함께 기록하고, 출처가 원격이거나 알 수 없는 경로를 로컬 파일 작업의 입력으로 해석하지 않는다.
- **대상은 요청할 때 고정한다.** 메뉴·단축키·팝업이 대상 경로와 surface 를 정한 순간의 값을 쓴다. 그 사이 포커스·explorer 내부 탭·선택이 바뀌어도 대상은 바뀌지 않는다. 사용자가 그 사이 다른 폴더로 가도 대상은 바뀌지 않는다.
- **실행 직전에 surface 세대를 다시 확인한다.** 메뉴는 열 때 그 surface 의 세대(`SurfaceBinding`)를 고정하고, 고른 항목은 `explorer_menu_admits`(`src/state/explorer_menu.rs`)가 허용할 때만 실행한다. 파일을 바꾸는 항목은 세대 전체(surface activation·자원·mirror projection)가 그대로여야 한다. 파일을 바꾸지 않는 항목(경로 복사·복사·이 폴더로 루트 설정)은 같은 explorer surface 이기만 하면 되고, mirror projection 이 다시 만들어진 것은 따지지 않는다. 이름 변경은 메뉴의 세대를 rename 팝업에 그대로 실어, 팝업이 떠 있는 사이 surface 가 바뀌면 팝업을 닫고 확정해도 실행하지 않는다.
- **worker 도 다시 확인한다.** View 는 대상 경로와 원 surface·View identity 를 고정해 App 의 `explorer_files` worker 에 넘긴다. worker 는 시작 직전에 원 대상과 mirror 제한을 다시 확인하고, 완료 뒤 원 View 와 surface 가 남아 있을 때만 목록 갱신을 요청한다. 사용자가 그 사이 바꾼 선택이나 새로 담은 클립보드는 건드리지 않는다. 대기 요청 상한과 종료 처리는 [Explorer](index.md#컨텍스트-메뉴--파일-조작) 에 있다.
- **원격 쓰기는 진입점마다 같은 방식으로 거부한다.** mirror explorer 에서는 메뉴 항목을 숨기고, 단축키와 메뉴 핸들러는 `explorer.state.remote_write_unsupported` 로 거부하며, worker 는 대상이 mirror 면 시작하지 않는다. 원격에서 복사한 경로는 로컬 explorer 의 메뉴에 붙여넣기로 나오지 않고, 단축키 붙여넣기는 `explorer.state.remote_paste_unsupported` 로 거부한다. 같은 경로의 로컬 파일을 대신 복사하지 않는다. 외부 파일 드롭은 파일 작업이 아니라 로컬 파일 열기(`DispatchFile`)로 처리하므로 원격 파일시스템에 쓰지 않는다.
- **주소 입력과 링크**는 [Explorer](index.md) 의 [주소 입력](index.md#주소-입력)·[심볼릭 링크](index.md#심볼릭-링크) 절을 따른다. 로컬 경로는 절대 경로로 확정해 다루고, 원격 경로는 로컬 파일시스템으로 확인하지 않는다.

### 작업별 계약

| 작업 | 진입점 | 대상 | 결과 | 사용자 피드백 |
|---|---|---|---|---|
| 새 폴더 | 툴바(좁은 칸은 More 메뉴), 메뉴(빈 영역·단일 폴더), `explorer_new_folder` 단축키 | 지금 보는 폴더, 폴더 메뉴는 그 폴더. 시작할 때 고정한다 | 목록 맨 위 인라인 입력으로 이름을 받아 `create_dir` 로 만든다. 같은 이름이 있으면 덮어쓰지 않고 실패한다 | 입력 아래 이름 오류 상자. 쓰기 실패는 오류 토스트와 다시 읽기. 성공하면 그 폴더를 아직 보고 있을 때 새 항목을 선택한다 |
| 새 파일 | 툴바(좁은 칸은 More 메뉴), 메뉴(빈 영역·단일 폴더), `explorer_new_file` 단축키 | 새 폴더와 같다 | 같은 입력으로 빈 파일을 `create_new` 로 만든다 | 새 폴더와 같다 |
| 이름 변경 | 메뉴 (단일 항목) | 그 항목 | 같은 폴더 안에서 이름만 바꾼다. 경로 구분자·드라이브 접두어가 든 이름과 이미 있는 이름은 거부한다 | rename 팝업. 실패하면 오류 토스트를 띄우고 선택을 유지한 채 목록을 다시 읽는다 |
| 복사 | 메뉴, `copy` 단축키, Command Palette | 선택 항목 | 창 단위 explorer 파일 클립보드에 경로를 담는다. 디스크는 바꾸지 않는다 | 없음 |
| 잘라내기 | 메뉴, `cut` 단축키, Command Palette | 선택 항목 | 클립보드에 잘라내기 표시와 함께 담는다 | 붙여넣기 전까지 잘라낸 항목을 grid·list·detail 에서 흐리게 그린다 (`cut_pending_opacity`) |
| 붙여넣기 | 메뉴 (빈 영역·"Paste into"), `paste` 단축키, Command Palette | 현재 폴더 또는 메뉴의 폴더 | 복사는 목적지 임시 디렉터리에서 준비한 뒤 덮어쓰기 금지 rename 으로 공개한다. 이동은 rename 하고, 다른 파일시스템이면 복사 뒤 원본을 지운다 | 부분 성공이면 실패 경로를 오류 토스트로 보인다 |
| 드래그 놓기 | 외부 파일을 창에 놓기 | — | 탐색기로 복사·이동하지 않는다. 놓은 파일은 [파일 핸들러](../../features/file-handler/index.md)로 연다 | — |
| 휴지통 이동 | 메뉴 | 선택 항목 또는 우클릭 항목 | OS 휴지통으로 보낸다. 확인 모달은 없다. 영구 삭제 경로는 없다 | 실패하면 오류 토스트를 띄우고 목록을 다시 읽는다 |
| 실행 취소 | 없음 | — | 지원하지 않는다. 휴지통 복원은 OS 에서 한다 | — |
| 검색·필터 | 없음 | — | 지원하지 않는다. 타입어헤드는 선택만 옮긴다 | — |
| 속성 | 메뉴 맨 끝 "Properties"(모든 변형, mirror 포함), `explorer_properties` 단축키, Command Palette | 메뉴의 대상, 또는 위 단축키 규칙 | 디스크를 바꾸지 않는다. 열 때의 대상을 고정해 보이고, 폴더 크기는 read worker 가 배경에서 센다. 닫으면 세기를 멈춘다 | 탐색기 칸에 묶인 Properties popup ([Explorer](index.md#properties-popup)) |
| 미리보기 | 툴바 토글, `explorer_toggle_preview` 단축키, Command Palette | 선택이 하나일 때 그 항목 | 디스크를 바꾸지 않는다. 로컬 파일만 read worker 로 읽는다 | 목록 오른쪽 미리보기 패널 ([Explorer](index.md#미리보기-패널)) |
| Grid 썸네일 | 없음 (Grid 보기에 자동) | 화면에 보인 로컬 그림 파일 | 디스크를 바꾸지 않는다 | 셀의 40 슬롯 ([Explorer](index.md#grid-썸네일)) |

이름 변경과 휴지통 이동에는 키보드 단축키가 없다. 디자인 원본의 컨텍스트 메뉴 견본은 이 두 항목에 `F2`·`Del` 을 표시하지만 `KeybindingSettings` 에 대응 필드가 없어 메뉴에도 단축키를 표시하지 않는다.

### 상태별 결과

| 상태 | 결과 |
|---|---|
| 정상 | 아래 "작업 뒤 목록 갱신" 범위의 목록을 다시 읽는다. 잘라내기를 붙여넣어 모두 성공했으면 그 클립보드를 비운다 |
| 빈 폴더 | 빈 영역 우클릭은 현재 폴더를 대상으로 한다. 클립보드가 있으면 붙여넣기를 보인다 |
| 오류 | 오류 토스트. 실패한 이름 변경·휴지통 이동은 선택을 유지한다. 권한 거부 폴더에서는 빈 영역 메뉴를 띄우지 않는다 |
| 부분 성공 | 실패 경로를 토스트로 보인다. 잘라내기 클립보드는 남긴다. 성공한 항목을 되돌리지 않는다 |
| 이름 충돌 | 붙여넣기는 묻지 않고 `(copy)` 접미사를 붙인 새 이름으로 둔다. 기존 항목을 덮어쓰지 않는다 |
| 취소 | 진행 중인 작업을 취소하는 수단이 없다. 앱 종료는 새 작업을 막고 worker 를 최대 5초 기다리며, 기한이 지나도 작업을 취소로 기록하지 않는다 |
| 원격 제한 | mirror explorer 는 쓰기 항목을 메뉴에서 숨기고, 단축키로 부르면 `explorer.state.remote_write_unsupported` 토스트를 띄운다. 복사(클립보드에 담기만 함)·경로 복사·이 폴더로 루트 설정은 그대로 된다. mirror explorer 에서 복사한 클립보드는 원격 출처로 기록되어, 로컬 explorer 에서는 붙여넣기 메뉴가 나오지 않고 단축키는 `explorer.state.remote_paste_unsupported` 토스트만 띄운다 |

### 작업 뒤 목록 갱신

작업이 끝나면 성공·실패·부분 성공과 관계없이 실제 상태를 다시 읽는다. 요청한 surface 가 그 사이 닫혔어도 다른 explorer 는 갱신한다. 대상은 모든 윈도우의 로컬 explorer 이며 mirror explorer 는 제외한다.

| 작업 | 다시 읽는 폴더 | 사라질 수 있는 경로 |
|---|---|---|
| 복사 후 붙여넣기 | 붙여넣은 폴더 | 없음 |
| 잘라내기 후 붙여넣기 | 붙여넣은 폴더, 각 원본의 부모 폴더 | 각 원본 |
| 휴지통 이동 | 각 항목의 부모 폴더 | 각 항목 |
| 이름 변경 | 항목의 부모 폴더 | 원래 이름의 경로 |
| 외부 프로그램으로 열기 | 없음 | 없음 |

- 현재 폴더가 다시 읽는 폴더이거나 사라질 수 있는 경로(또는 그 안)이면 목록을 다시 읽는다. 사라진 폴더 안을 보던 explorer 는 읽기 오류 화면을 보이며 다른 경로로 옮기지 않는다. 그 화면의 "상위 폴더로" 는 남아 있는 가장 가까운 상위 폴더로 간다.
- 현재 폴더가 해당하지 않아도 사이드바 트리에 그 폴더의 하위 목록이 있으면 그 항목만 지워 다시 읽게 한다.
- 경로는 문자열 그대로 비교한다(정규화하지 않는다). 심볼릭 링크를 거쳐 들어간 같은 폴더처럼 다른 경로 문자열로 보는 explorer 는 갱신 대상이 아니며, 새로고침(`F5`)으로 맞춘다.
- 보던 폴더의 이름이 바뀐 경우도 원래 경로가 사라진 것으로 본다. 그 안을 보던 explorer 는 읽기 오류 화면을 보이며 새 이름을 따라가지 않는다.
- 다시 읽어도 선택은 비우지 않는다. 폴더가 그대로면 남아 있는 항목의 선택이 유지되고, 다시 읽은 목록에 없는 경로는 선택에서 빠진다(상태줄의 선택 수도 그만큼 준다).

### 사용자 작업과 에이전트 작업의 경계

- 이 문서의 파일 작업은 사용자 입력으로만 시작한다. release IPC 는 컨텍스트 메뉴를 열거나 그 항목을 누르는 메서드를 제공하지 않는다 ([debug IPC](../../dev-guide/debug-ipc.md)).
- 에이전트는 셸에서 파일을 직접 다룬다. 탐색기 전용 파일 API 는 없다.
- 작업 완료가 포커스를 가져가지 않는다. 다른 surface 의 선택·스크롤도 바꾸지 않는다 ([포커스 정책](../../design/policies/focus.md)).

## 인터페이스

- **AI Agent (IPC/CLI)**: 없음. 위 경계 절을 따른다.
- **사용자 트리거**: 컨텍스트 메뉴, `copy`·`cut`·`paste` 바인딩, Command Palette 의 Copy·Cut·Paste. 단축키 표는 [Explorer](index.md#사용자-트리거-단축키--keybindingsettings) 에 있다.
- **원격 / 점유**: mirror explorer 는 위 원격 제한을 따른다.

## 비-목표 (Out of scope)

- 원격 호스트의 파일 쓰기·전송. mirror explorer 는 탐색과 열기만 한다 (ADR-0022).
- surface 이동. [surface-move](../../features/surface-move/index.md) 가 맡는다.

## 수용 기준

- Given 폴더 A 를 연 탐색기에서 하위 폴더 B 로 이동했고 선택이 없다, When `paste` 를 누른다, Then 붙여넣기 대상은 B 다.
- Given 세 항목을 선택했다, When 그중 하나를 우클릭해 Copy 를 고르거나 `copy` 를 누르거나 Command Palette 에서 Copy 를 고른다, Then 세 경우 모두 클립보드에 같은 세 경로가 담긴다.
- Given 선택 밖의 항목을 우클릭했다, When 메뉴를 연다, Then 선택이 그 항목 하나로 바뀌고 메뉴 대상도 그 항목이다.
- Given 사이드바 Files 트리의 폴더를 우클릭했다, When 메뉴를 연다, Then 목록의 선택은 바뀌지 않는다.
- Given 붙여넣을 위치에 같은 이름이 있다, When 붙여넣는다, Then 기존 항목은 그대로이고 새 항목은 `(copy)` 접미사 이름이다.
- Given mirror explorer 다, When `cut` 또는 `paste` 를 누른다, Then 로컬 파일시스템은 바뀌지 않고 원격 쓰기 미지원 토스트가 뜬다.
- Given 두 윈도우에서 같은 폴더를 연 로컬 explorer 가 있다, When 한쪽에서 항목을 휴지통으로 보낸다, Then 두 explorer 모두 그 폴더를 다시 읽어 항목이 사라진다.
- Given 한 explorer 가 폴더 A 안을 보고 있다, When 다른 explorer 에서 A 를 휴지통으로 보낸다, Then 앞의 explorer 는 읽기 오류 화면을 보이고 경로는 A 안 그대로다.
- Given 붙여넣기가 진행 중이다, When 사용자가 다른 폴더로 이동한다, Then 작업 대상은 요청 시점의 폴더이고 포커스는 바뀌지 않는다.
- Given mirror explorer 에서 항목을 복사했다, When 로컬 explorer 에서 우클릭하거나 `paste` 를 누른다, Then 메뉴에 붙여넣기가 없고 단축키는 원격 붙여넣기 미지원 토스트만 띄우며 로컬 파일시스템은 바뀌지 않는다.
- Given 컨텍스트 메뉴가 열려 있다, When 그 사이 surface 가 닫히거나 다른 surface 로 바뀐 뒤 항목을 고른다, Then 아무 작업도 실행하지 않는다. 경로 복사·복사·이 폴더로 루트 설정은 mirror projection 이 다시 만들어지기만 했으면 그대로 실행한다.
- Given rename 팝업이 떠 있다, When 그 사이 surface 가 다른 surface 로 바뀐다, Then 팝업이 닫히고 확정해도 이름 변경 요청이 나가지 않는다.
- Given 새 이름 입력을 확정했다, When worker 가 만들기 전에 같은 이름이 생겼다, Then 기존 항목은 그대로이고 오류 토스트가 뜬다(`explorer_files/tests.rs` 의 `create_makes_a_folder_or_an_empty_file_and_never_replaces_an_entry`).
