# 레이아웃 영속화 (Layout persistence)

- **Status**: Implemented
- **주체**: 로컬 사용자 (설정 토글)
- **ADR**: [ADR-0017](../../adr/0017-workspace-identity-and-focus.md) — 슬롯 점유 모델
- **코드**: `src/core/layout_persistence/`, `~/.tasty/layouts/NN.json` · `~/.tasty/scrollback/<id>.bin`
- **화면**: 없음 (앱 시작 시 자동 복원)

## 목적

`general.restore_layout`(기본 on)이 켜져 있으면 워크스페이스·페인·탭·서피스 구성을 `~/.tasty/layouts/NN.json` 슬롯 파일에 저장한다. 다음 실행에서 슬롯을 읽어 배치를 복원한다. 파일 읽기와 쓰기는 실패할 수 있으며 아래의 보호 규칙을 적용한다.

## 내부 동작

### 저장 대상 / 타이밍

다음 정보를 저장한다.

- 워크스페이스 이름·부제·설명, 페인 분할 방향과 비율, 탭 이름과 선택 상태
- 서피스 배치와 종류별 복원 정보: Terminal의 cwd·`restore.command`·`scrollback_ref`, Markdown·Image의 path, Explorer의 root, Html의 url
- 활성 워크스페이스와 포커스 페인 인덱스

구조가 바뀌면 dirty로 표시하고 500ms 디바운스 뒤 저장한다. 종료 시에는 디바운스를 건너뛰며, `restore_surface_content`가 켜져 있으면 구조가 바뀌지 않았어도 저장을 시도한다.

저장 함수 `save_slot`은 파일 I/O를 동기로 수행하지만 성공 여부를 호출자에게 반환하지 않는다. 호출자는 함수를 부른 뒤 dirty를 지우며 저장을 생략한 경우에도 `LayoutSaved`를 반환한다. 따라서 이 이벤트나 종료 flush 완료가 파일 저장 성공을 뜻하지는 않는다. 실패 로그와 실제 파일을 함께 확인해야 한다.

레이아웃 복원은 engine을 만들 때 한 번 수행한다. 앱의 첫 윈도우뿐 아니라 나중에 여는 윈도우도 해당한다. 파일이 없거나 읽지 못하면 기본 `Workspace 1`로 시작한다. 개별 surface를 복원하지 못하면 해당 항목은 건너뛰고, 복원한 workspace가 하나도 없으면 기본 workspace를 만든다.

복원할 레이아웃이 있는 engine에는 기본 workspace를 미리 만들지 않는다. 기본 터미널의 셸이 복원 후 소속 없이 남는 것을 피하기 위해서다. 복원이 끝난 뒤 view 상태를 구성한다.

### 슬롯 파일

슬롯 파일 하나에 engine 하나의 전체 레이아웃을 저장한다. 파일명의 숫자로 목록과 순서를 정하며 별도 인덱스 파일은 없다. 번호는 최소 2자리(`01.json`)로 표시하고 100 이상은 필요한 자릿수를 사용한다.

`NN.json.tmp`에 쓴 뒤 rename으로 슬롯 파일을 교체한다. 쓰기나 rename이 실패하면 로그를 남긴다. 이 방식이 디스크 오류나 전원 중단 뒤의 저장까지 보장하는 것은 아니다.

각 engine은 자신이 점유한 슬롯에 저장한다. 종료 flush도 윈도우별 파일에 나누어 쓴다. 같은 `TASTY_HOME`을 공유하는 여러 앱 인스턴스의 동시 저장까지 보호하는 모델은 아니다([슬롯 설계](../../adr/0017-workspace-identity-and-focus.md)).

**레거시 마이그레이션** — 단일 파일 시절의 `~/.tasty/layout.json` 은 부팅 1회 `layouts/01.json` 으로 **이동**(rename)된다. `layouts/` 가 이미 있으면 이동하지 않고 남은 레거시 파일을 로그로 알린다.

### 읽지 못한 슬롯

파일이 없는 경우와 읽지 못한 경우를 구분한다. 읽기 실패를 빈 슬롯으로 취급하면 이어지는 저장이 사용자의 기존 레이아웃을 덮어쓸 수 있기 때문이다.

| 상태 | 그 슬롯에 저장 | 원본 |
|------|----------------|------|
| 없음 | 한다 | — |
| 파싱 실패 | 한다 — **저장 직전에** `NN.json.bak` 으로 옮긴 뒤 쓴다 | 백업에 남는다 |
| 파싱 실패 + 백업 실패 | **안 한다** | 그 자리에 그대로 |
| 읽기 실패 (권한 · IO) | **안 한다** | 그 자리에 그대로 |
| version 이 이 빌드보다 높음 | **안 한다** | 그 자리에 그대로 |

로드는 파일을 이동하지 않는다. 손상 파일 백업은 저장 직전에 수행한다. GC와 engine 복원 등 여러 경로가 같은 슬롯을 읽으므로, 읽는 중 파일을 옮기면 뒤의 호출자는 실패 원인 대신 파일 없음만 보게 된다.

읽기 오류가 난 파일은 내용이 손상됐는지 확인할 수 없어 옮기지 않는다. 더 높은 version의 파일도 새 빌드가 읽을 수 있으므로 현재 빌드가 백업하거나 덮어쓰지 않는다.

백업 파일명은 `NN.json.bak` 이고 이미 있으면 `NN.json.bak.2` … `NN.json.bak.9` 로 늘어난다.
9개가 차면 백업을 만들지 않고 저장을 막는 쪽을 택한다. 저장이 막힌 창에는 토스트로 알린다.

### 슬롯 배정

윈도우와 연결된 engine이 슬롯 하나를 점유한다. 새 윈도우는 다른 engine이 쓰지 않는 슬롯을 선택한다.

규칙 — **실제 존재하는 슬롯 파일** 중 점유되지 않은 가장 낮은 번호를 쓴다.

- 번호 공백은 채우지 않는다: 파일이 `02.json`·`03.json` 뿐이고 점유가 없으면 답은 `1` 이 아니라 **2** 다. 저장된 레이아웃을 건너뛰고 빈 창을 띄우지 않기 위한 것이다.
- 기존 슬롯 파일이 전부 점유면 `max(파일 슬롯 ∪ 점유 슬롯) + 1` 로 **새 슬롯**을 만든다. 파일이 없으므로 그 창은 기본 워크스페이스 하나로 시작한다.
- 파일도 점유도 없으면(첫 설치) `1`.

저장된 슬롯이 여러 개여도 **부팅 시 창은 1개**다. 나머지 슬롯은 free 로 남아 있다가 창을 더 열면 순서대로 복원된다.

headless 빌드(`--no-default-features`)는 레이아웃을 영속하지 않는다 — 슬롯을 점유하지도, 저장하지도, 복원하지도 않고, 워크스페이스는 프로세스 수명 동안만 산다. `general.restore_layout` 이 켜져 있으면 부팅 때 그 설정이 무시된다는 warn 을 한 줄 남긴다([ADR-0003](../../adr/0003-headless-behavior.md)). (gui 빌드에 `--headless` 를 주면 헤드리스가 아니라 GUI 로 폴백하므로 이 절이 아니라 위 규칙을 따른다.)

### 슬롯 점유

슬롯 점유는 디스크에 기록하지 않는다. 현재 engine들이 가진 슬롯 번호로 판단하며 앱을 다시 시작하면 점유가 초기화된다. 윈도우가 사라져도 parked engine이 남아 있으면 같은 슬롯을 계속 점유한다([멀티 윈도우 구조](../../architecture/multi-window.md)).

#### 점유 조회

`window.list` IPC 또는 `tasty list windows`의 `layout_slot` 필드로 윈도우가 쓰는 슬롯을 조회한다.

```json
[
  { "id": 1, "focused": true,  "title": "Tasty",       "layout_slot": 1 },
  { "id": 2, "focused": false, "title": "Workspace 1", "layout_slot": 2 }
]
```

`layout_slot` 은 슬롯을 잡지 않는 engine 에서 `null` 이다(headless). 순수 read 라 포커스·선택 등 사용자 상태를 건드리지 않는다.

Parked engine은 슬롯을 점유하지만 윈도우 ID가 없어 `window.list`에 포함되지 않는다. 따라서 이 응답만으로 전체 슬롯 점유 집합을 알 수는 없다.

### 창 닫힘 시 슬롯 처리

윈도우를 닫으면서 engine도 제거하면 슬롯 점유가 해제된다. 제거 직전에는 `general.restore_layout`에 따라 파일을 처리한다.

| `restore_layout` | 슬롯 파일 |
|---|---|
| **on** | 500ms 디바운스를 건너뛰고 저장을 시도한 뒤 슬롯을 남긴다. 저장에 성공한 내용은 다음 윈도우에서 복원할 수 있다. |
| **off** | 슬롯 파일 삭제를 시도한다. |

마지막 창을 닫아 engine 이 파킹되는 경우(그리고 macOS 최소화)는 engine 이 살아 있으므로 이 처리를 하지 않는다 — 슬롯 점유가 유지되고 다시 창을 열면 같은 슬롯을 이어쓴다.

윈도우 정리에서 scrollback `.bin`을 따로 지우지는 않는다. 남아 있는 슬롯의 참조와 다음 부팅의 GC 결과에 따라 정리된다.

### Surface 내용 복원 (현재: 터미널 scrollback)

`general.restore_surface_content`(기본 on)가 켜져 있으면 터미널의 scrollback과 화면 라인을 `~/.tasty/scrollback/<persist_id>.bin`에 저장한다. 파일 magic은 `TSSB`다. 복원 시 이전 scrollback과 화면 내용을 새 터미널의 출력 앞에 넣는다.

`persist_id`는 `TerminalStore.scrollback_persist_ids`(`src/core/terminal_store.rs`)에 보관하고 다음 저장에서 같은 파일을 교체한다. 옵션이 꺼져 있으면 새 scrollback을 캡처하지 않는다. 설정 화면에서 on→off로 바꾸면 기존 scrollback 디렉터리를 정리한다. 복원은 저장된 `scrollback_ref`와 읽을 수 있는 파일을 사용한다.

Surface 닫기에서는 해당 `.bin`을 삭제한다. 부팅 GC는 `list_slots_in`으로 열거한 슬롯의 `scrollback_ref` 합집합을 구해 참조되지 않는 `.bin`을 지운다. 열거한 슬롯 하나라도 로드하지 못하면 그 회차 GC를 건너뛴다.

이 보호는 디렉터리 열거 실패까지 포함하지 않는다. `read_dir` 실패는 빈 목록으로 반환하고 개별 항목 오류는 건너뛴다. 따라서 열거에서 빠진 슬롯의 참조는 보호 집합에 들어가지 않을 수 있다. 빈 목록이면 빈 참조 집합으로 GC가 실행된다.

### Plugin surface 복원 (hello 창)

Markdown·Image처럼 플러그인이 제공하는 surface는 hello와 kind 등록이 끝나기 전에 레이아웃 복원이 시작될 수 있다. 이때 kind와 snapshot을 가진 placeholder로 위치를 남겨 둔다. kind가 등록되면 화면 갱신 경로에서 실제 surface로 바꾼다.

plugin이 시작하지 못하거나 해당 kind를 더 이상 제공하지 않으면 placeholder를 유지한다.
화면에는 빈 자리로 보이고, `tasty list tree`에는 원래 kind와 `ready: false`,
`pending_reason: "plugin_not_loaded"`가 표시된다. surface가 존재하는 것과 사용할 준비가
된 것을 구분하며, 화면과 API를 수정할 때 이 의미가 일치하도록 유지한다.

### TUI 세션 복원 (`restore.command`)

플러그인이 세션 시작 훅에서 surface meta의 `restore.command`를 기록하면 호스트는
그 문자열을 터미널 복원에 사용한다. 에이전트 종류나 명령 내용은 호스트가 해석하지 않는다.
Claude 프로필이 붙은 경우 `claude -r <id> --settings "<프로필 경로>"`처럼 기록한다.
프로필 정보 복구는 [Claude 가이드](../../plugins/claude/index.md)의 해당 절을 따른다.

호스트는 명령 뒤에 `\r`을 붙여 `TerminalConfig.initial_input`으로 전달한다.
자식 셸을 생성한 뒤 writer 스레드를 시작하기 전에 PTY에 동기로 쓰고 flush한다.
쓰기·flush 실패는 경고를 남기며, 이미 실행 중인 셸의 입력 초기화로 바이트가 사라질 수도 있다.
따라서 첫 stdin 읽기에 반드시 도착하거나 spawn과 동시에 명령이 실행된다는 보장은 없다.

앱 재시작과 [닫힌 항목 복원](../closed-tab-restore/index.md)(Ctrl+Shift+T)에서 사용한다.

## 저장하지 않는 것

실행 중인 PTY 프로세스와 환경변수, 팝업 상태는 저장하지 않는다. `restore.command`는 기존 프로세스를 보존하는 기능이 아니라 새 터미널에서 명령을 다시 실행하기 위한 정보다.

## 관련

- [closed-tab-restore](../closed-tab-restore/index.md) · [layout-presets](../layout-presets/index.md) · [terminal](../terminal/index.md)(scrollback)
- [ADR-0017](../../adr/0017-workspace-identity-and-focus.md) — 슬롯 모델을 고른 이유·대안·재검토 조건
- [멀티 윈도우 아키텍처](../../architecture/multi-window.md) — 창 ↔ engine ↔ 슬롯 1:1 구조
