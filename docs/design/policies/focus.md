# 포커스 정책 (운영 상세)

> 정체성 차원의 근거는 [identity §2.3 포커스 독립성](../../identity.md). 본 문서는 *현재 운영 동작* 만 기술한다. 계층 용어(View)는 [concepts/hierarchy](../../concepts/hierarchy.md).

**포커스(활성 윈도우/탭/워크스페이스/Pane/Surface)는 사용자의 것**이다 — 사용자가 지금 무엇을 보고 어디에 입력하는지의 시점. 에이전트 행동(IPC/CLI)은 포커스를 바꾸지 않으며, release 엔 포커스 변경 API 가 없다.

## 계층

```
Engine
└── View (여러 개, HashMap<WindowId, …>)
    ├── ModalView    — 활성 시 모든 입력 독점 (엔진 전역 최대 1개)
    └── 그 외 (MainView / PresetView 등) — Modal 없을 때 OS 네이티브 포커스
        └── Pane / Surface — View 내부 포커스
```

## Modal 포커스 차단

앱(ViewRegistry)이 `active_modal_id: Option<WindowId>` 를 보유한다.

- **Modal 없음**(`active_modal_id == None`): 각 View 는 OS 네이티브 포커스를 따른다. View 들은 독립적으로 포커스를 받고(z-order 독립), 사이에 다른 앱 창이 있을 수 있다.
- **Modal 있음**(`active_modal_id == Some(id)`): 이벤트 디스패처가 각 View 에 `modal_active: bool` 을 전달한다. Modal 이 아닌 View 는 입력 이벤트를 무시하고(`Resized`/`RedrawRequested`/`ScaleFactorChanged`/`ModifiersChanged`/`Focused` 만 통과), Modal View 만 `modal_active: false` 로 받아 정상 동작한다. Modal 을 닫으면 기존 포커스로 자연 복귀한다.

| 상태 | 구현체 | 동작 |
|----------|--------|------|
| Modal | `SettingsView` · `PluginsView` · `QuitView` (`ModalView`) | 전체 입력 차단, 닫기 전까지 다른 조작 불가 |
| Modeless | `MainView` · `PresetView` (`View` + `sealed::Sealed` 직접 구현) | 독립 포커스, 다른 윈도우와 공존 |

**OS 네이티브 윈도우 비활성화(Win32 `EnableWindow` 등)는 쓰지 않는다** — 플랫폼별 동작 차이로 크로스플랫폼 일관성이 깨진다. 앱 레벨 `modal_active` 게이트로 처리한다.

## View 내부 포커스

Modal/View 레벨과 별개로, 각 View 내부에서 Pane 간·Surface 간 포커스 이동과 탭 전환이 일어난다 (단축키/클릭). 단축키는 [`KeybindingSettings`](key-mapping.md) — 하드코딩 아님. 이 내부 포커스는 그 View 가 OS 포커스를 갖고 Modal 이 비활성일 때만 동작한다.

**탭바 클릭 → 그 pane 으로 focus 이동**: 콘텐츠 영역 클릭과 대칭으로, 비-focused pane 의 탭바(탭 본체·탭이 없는 빈 영역·스크롤 화살표·"+"/split/search 버튼)를 primary click 하면 그 pane 으로 focus 가 이동한다. 탭바는 그 pane 을 직접 조작하는 사용자 행위이므로 클릭 대상 pane 과 focus 가 어긋나면 안 된다(비-focused pane 의 탭을 클릭해도 탭 전환만 일어나고 focus 는 그대로 남는 것은 결함). 빈 영역 클릭은 탭 전환 없이 focus 만 옮긴다. 우클릭 컨텍스트 메뉴(탭/pane/새 탭 버튼)는 대상 `pane_id`/`tab_index` 를 메뉴 항목에 직접 실어 나르므로 focus 이동이 필요 없다 — 우클릭은 조회/메뉴-오픈이지 조작 commit 이 아니다. 구현: `src/adapters/ui/tab_bar.rs` `TabBarAction::focus_target_pane` + `src/adapters/ui/tab_bar/apply.rs` `apply_tab_bar_actions`.

이 규칙은 **사용자 마우스 클릭**에 의한 focus 이동이므로 아래 "CLI/IPC 포커스 독립 원칙"(에이전트/명령 유래 focus 강제 금지)과 별개다 — 혼동 금지. 그 원칙은 IPC/CLI 명령이 focus 를 대상 결정 수단으로 쓰거나 강제 변경하는 것을 막는 것이지, 사용자가 GUI 를 직접 클릭했을 때 그 결과로 focus 가 따라가는 것을 막지 않는다.

## CLI/IPC 포커스 독립 원칙

release IPC/CLI에는 사용자 포커스를 바꾸는 API가 없다.
요청은 대상 ID가 가리키는 engine에서 실행하고, 대상을 지정했는데 찾지 못하면 오류를 반환한다.
포커스된 창의 다른 대상으로 바꾸어 실행하지 않는다. 대상을 받지 않는 생성 요청이나 호환용 기본값은
아래 예외를 따르며, 명시 ID를 주는 방법을 우선한다.

### 목록 조회

창별 자원 목록은 모든 main·parked engine에서 모은다. `src/app/dispatch/list_global.rs`가 처리한다.
새 목록을 추가할 때 메서드 이름이 list로 끝나는지만 보지 말고 실제로 읽는 컬렉션과 대상 인자를 확인한다.
필터 인자는 대상 지정과 다르다. tree와 workspace.list는 같은 workspace 집합을 반환해야 한다.

| 자원 | 조회 방식 |
|---|---|
| workspace, pane, tab, surface, tree | 전체 engine 합산 |
| attach | 한 번의 engine 순회에서 surface·workspace 점유 배열을 함께 합산 |
| hook, global hook | 공유 ID 카운터로 ID를 보장한 뒤 합산 |
| notification | 전체 생성 ID 역순 최대 50개. 병합은 생성 ID 유지, UI·읽음·보존은 창별 |
| approval | 창 생성 때 같은 approval_store Arc를 공유하므로 공유 목록 조회 |
| image.list | plugin이 host-call로 반환한 요청도 host 합산 경로 사용 |

합산한 active는 각 창의 활성 상태이므로 여러 개가 true일 수 있다.
일반 자원 ID는 IdGenerator의 engine 간 공유 카운터로 유일하게 만든다.
예외인 예약 카테고리 normal(ID 0)은 한 행으로 합친다. rename·delete·move 대상이 아니므로
창을 별도로 지정할 필요가 없다. count는 전 창 합, 위치는 고정값, collapsed는 모든 창에서 접혔을 때만 true다.

분류 명부는 `crates/tasty-doc-guards/tests/window_owned_lists_are_classified.rs`다.
명부는 기존 분류 누락을 검사하지만 새 종류의 목록을 자동 발견하지 않는다.
공유 여부는 CoreState 생성자뿐 아니라 `App::ensure_engine_and_plugins`의 Arc 전달·교체도 읽어 판단한다.

### 대상 해소

라우터는 surface·workspace·pane·tab·PTY·hook·global hook·output observer·preset source·split 대상을 해소한다.
같은 메서드의 핸들러와 같은 표현·우선순위로 읽는다. 예를 들어 split은 숫자 문자열을 먼저 해석하고
nickname은 메모리 저장소에서 해소한다. nickname 매핑은 창에 종속되지 않는다.
발신자 from_surface_id, 호출자 caller_surface_id, stream client ID는 대상 ID가 아니다.

명시한 대상이 없으면 무엇을 찾지 못했는지 오류를 반환한다. headless도 같은 검사를 수행한다.
headless의 사전 검사는 host 예약 prefix에 한정한다. plugin이 처리할 요청을 미리 거절하지 않기 위해서다.

순서 변경은 대상 ID와 목적지 to_index로 표현한다. tab.move는 pane_id와 탭 인덱스로 pane을 먼저 지정한다.
workspace.move·workspace_category.move의 옛 index-only 입력은 호환상 focused 창을 사용하지만,
ID와 옛 index를 동시에 주면 거절한다.

terminal.kill·release·respawn·broadcast의 parent 생략은 현재 engine에 parent가 정확히 하나일 때만 허용한다.
main 창이 둘 이상이면 engine 자체가 불분명하므로 --surface를 요구한다.
TASTY_SURFACE_ID는 호출자가 있는 surface이며 사용자 포커스와 다르다. CLI가 지원하는 --surface 기본값으로 사용할 수 있다.

### 검토 기준

활성 상태를 응답으로 조회하는 것은 허용한다. 요청 대상과 포커스는 별도로 취급한다.
생성·삭제에 내부적인 임시 focus 변경이 필요하면 원래 focus를 복원해야 한다.
새 대상 키는 `every_id_key_a_handler_reads_is_routed_or_exempt`의 대조 대상이며,
대상이 아닌 키는 구체적인 사유와 함께 제외한다.

## 폴백으로 가는 메서드는 이름과 사유로 남는다

`src/source_guards/unrouted_dispatch_reasons.rs`는 라우팅 대상 키가 드러나지 않는 메서드를 분류한다.
명부에는 주인 창이 정해지지 않아도 답이 올바른 이유를 적는다.

| 분류 | 이유 |
|---|---|
| NotWindowOwned | 저장소가 창에 속하지 않음 |
| AggregatedList | 모든 engine의 목록을 합산함 |
| CreatesWithoutATarget | 새 대상을 생성함 |
| ScopedObservation | 창별 관측이며 응답에 소유 ID와 scope를 표시함 |
| TargetReadByDeserializer | serde 구조체가 대상을 읽어 단순 키 검사가 찾지 못함 |
| RoutedOutsideRequestTarget | 다른 단계에서 대상 해소 |
| DebugOnly | 사용자 조작 재현용 debug 기능 |
| PerWindowOpenDefect | 아직 해결되지 않은 창별 처리 문제 |

현재 명부의 PerWindowOpenDefect 항목은 없다. 이것이 모든 IPC 관측 문제의 해결을 뜻하지는 않는다.
system.info는 engine count/index를 유지하고 scope=engine, workspace_ids, active_workspace_id,
layout_slot을 함께 반환한다. window.list는 OS ID와 함께 이 값을 제공하고 전역 workspace 목록은 workspace.list가 담당한다.
버전은 프로세스 전역 값이다.

Git 조회는 창별 큐를 수집한 뒤 전역 attach 세션에서 local_surface_id를 찾는다.
따라서 큐 위치만 보고 조회 대상 오류라고 판단하지 않는다.
recent.query는 state.db의 공유 캐시에서 종류별 최근 항목 최대 10개를 읽으며 순서와 focus를 변경하지 않는다.
파일 열기는 명시 origin의 engine·pane을 비동기 완료와 picker 선택까지 유지하고,
대상이 사라졌으면 다른 창에 열지 않는다. 사용자 파일 열기와 에이전트 파일 열기의 선택 차이는 아래 절을 따른다.

## 라우팅 아래에도 층이 하나 더 있다 — 그 층은 대상을 안 고른다

위의 폴백은 **창을 고르는** 한 층이다. 창이 정해진 뒤 핸들러가 그 창의 활성 포인터를
다시 읽는 층이 하나 더 있고, 그 사실이 여기 적혀 있지 않아 읽는 사람이 "핸들러가
`active_workspace` 를 읽는다" 를 위반으로 볼지 설계로 볼지 가를 수 없었다.

**가른 결과: 대상을 포커스로 고르는 핸들러는 없다.** 출하 코드에서 에이전트가 닿는
IPC 핸들러(`src/adapters/ipc/`)가 활성 포인터를 읽는 자리를 전수로 뽑아 쓰임새별로
가르면 다섯 부류이고, 어느 것도 요청의 **대상**을 포커스로 정하지 않는다.

| 부류 | 하는 일 | 판정 |
|---|---|---|
| 보고 | 응답에 활성 상태를 싣는다(`"focused"` · `"active"` · `active_workspace`) | 위 "활성 상태 *조회* 는 허용" 그대로 |
| 기본값 채우기 | 대상은 인자로 지목됐고, **미지정 인자**만 포커스가 채운다 — 새 탭·split 의 cwd 상속, `surface_id` 를 안 실은 `workspace.create` 의 cwd 상속, `telemetry.record` 의 workspace, `approval.request` 의 workspace(단 `surface_id` 를 줬으면 그 surface 의 워크스페이스라 포커스를 안 읽는다) | 호출자가 명시하면 안 읽는다. 명시하지 않으면 같은 인자로 두 번 불러도 값이 다를 수 있다 |
| 계측 태그 | 공통 `check_request`가 audit 귀속에 engine의 활성 workspace를 읽는다. telemetry는 같은 engine의 첫 workspace를 태그로 쓴다 | **판정에는 안 쓰인다** — 게이트가 빌린 engine의 관측 문맥이며, 요청이 작용한 대상이나 사용자의 포커스를 증명하지 않는다 |
| 알림 배치 | cap 임계·이상 탐지·승인 요청 알림이 활성 워크스페이스에 뜬다 | 사용자에게 보이라고 두는 자리라 에이전트 대상 결정이 아니다 |
| 효과 scope | `debug.host_popup.open` 의 `workspace_scope` | debug 전용. 사용자 조작 재현이라 창 종속이 뜻 자체다 |

두 번째 부류가 이 축에서 유일하게 관측 가능한 흔들림이다 — **인자를 명시하지 않은
호출은 재현 가능하지 않다.** 금지가 아니라 성질이고, 재현이 필요하면 인자를 준다.
요청이 이미 대상을 댄 자리에서는 그 대상이 귀속을 정하고, 아무것도 대지 않은 자리에만
포커스 기본값이 남는다. 자리별 갈래와 남긴 이유는 [ADR-0017](../../adr/0017-workspace-identity-and-focus.md).

세 번째 부류는 **감사 로그를 workspace 로 조회할 때만 드러난다.** 행의 workspace 는
요청의 대상이 아니므로, 그 열로 "이 워크스페이스에서 무슨 일이 있었나" 를 물으면
답이 어긋난다.

### 재는 명령

    # 출하되는 줄만 남긴 사본을 만들고(테스트 코드가 같은 이름을 쓴다)
    cargo run -q -p tasty-doc-guards --bin strip-cfg-test -- \
        --blank-test-only-files <out> . src/adapters/ipc src/app
    # 다섯 포인터를 센다
    grep -rn 'focused_view_id\|active_workspace\|focused_pane\|active_tab\|focused_surface' <out>

    # 합산 집합의 소속 판정(ADR-0017) — 창 소유 컬렉션을 순회하는 핸들러를 뽑아
    # `src/app/dispatch/list_global.rs` 의 arm 과 대조한다. 대상 인자가 있는 것
    # (`surface_id` 등을 받는 것)은 라우터가 주인 창을 푸니 합산 대상이 아니다.
    grep -rn 'for ws in &engine.workspaces\|engine.workspaces.iter()' <out>/src/adapters/ipc
    grep -oE '"[a-z_.]+" =>' src/app/dispatch/list_global.rs

`focused_view_id` 는 핸들러 층에 **0** 이어야 한다 — 창을 고르는 것은 라우터의 일이고,
핸들러는 이미 정해진 창 안에서만 산다. 그 값이 0 이 아니게 되면 층이 섞인 것이다.

`approval.request`의 기록은 명시 workspace_id, 지정 surface의 workspace, 활성 workspace 순서로 귀속한다.
외부 요청의 없는 surface는 라우터가 먼저 거절한다. 핸들러를 직접 부른 내부 경로는 같은 검사를 중복하지 않는다.
telemetry.record와 record_batch는 workspace_id를 생략하면 활성 workspace를 사용하므로 재현 가능한 기록에는 값을 명시한다.

비용 상한은 agent와 metric의 누적값이며 여러 workspace의 이벤트가 섞인다.
마지막 이벤트가 전체 비용의 소속이라고 볼 수 없어 자동 승인과 알림은 현재 활성 workspace를 사용한다.
영속 승인 기록까지 그 scope에 남는 한계가 있다. workspace별 상한 또는 신뢰할 수 있는 원인 surface가 생기면
이 귀속을 다시 정한다. audit의 활성 workspace 태그와 IPC telemetry의 첫 workspace 태그 역시 요청 대상의 증거가 아니다.

## 삭제로 인한 인덱스 이동에서도 포커스 대상은 보존된다

**시야가 움직이는 경우는 하나뿐 — 사용자가 보고 있던 대상 *자체* 가 사라졌을 때다.** 보고 있지 않은 워크스페이스/탭/pane 이 닫혔는데 화면이 바뀌면 결함이다. 근거 [ADR-0017](../../adr/0017-workspace-identity-and-focus.md).

활성 포인터 셋 중 둘은 **인덱스**가 진실 소스다 — `AppState::active_workspace` 와 `Pane::active_tab`. 인덱스는 앞쪽 원소가 빠지면 손대지 않아도 **가리키는 대상이 바뀐다.** 그래서 범위 초과 clamp 만으로는 부족하고, 제거 위치를 기준으로 함께 당겨야 한다.

| 계층 | 포인터 | 제거가 앞쪽일 때 | 제거된 것이 보던 대상일 때 |
|---|---|---|---|
| workspace | `active_workspace`(인덱스) | 한 칸 당김 — 같은 워크스페이스 유지 | 그 자리로 밀려 들어온 워크스페이스(마지막이었으면 직전) |
| tab | `Pane::active_tab`(인덱스) | 한 칸 당김 — 같은 탭 유지 | 그 자리로 밀려 들어온 탭(마지막이었으면 직전) |
| pane | `Workspace::focused_pane`(id) | 그대로 — id 는 밀리지 않는다 | 생존 pane 으로 재배정 |

- 이 보정은 **origin 으로 분기하지 않는다.** 대상 기준 보정은 사용자 경로(컨텍스트 메뉴로 앞쪽 탭 닫기)에서도 옳다. origin 게이트는 "에이전트가 새로 만든 것으로 포커스를 옮기지 않는다"(`cascade_workspace_created` · `cascade_surface_split`)처럼 이동 여부가 정책적으로 갈리는 곳에만 쓴다.
- 카테고리 quick-switch 착지점(`AppState::category_last_active`)은 인덱스가 아니라 **워크스페이스 id** 를 값으로 든다. 그래서 제거·재정렬 어느 쪽으로도 밀리지 않는다 — 보정 대상이 아니다. 착지 시점에 id 로 워크스페이스를 찾고, 사라졌거나 다른 카테고리로 옮겨졌으면 그 카테고리의 first 로 폴백한다.
- 원격 attach 로 forward 된 구조 변경(`execute_forwarded_structural_op`)과 mirror 워크스페이스 teardown 도 같은 close 경로를 타므로 같은 규칙이 적용된다.

구현: tab 은 `Pane::remove_tab_preserving_active`(`crates/tasty-model/src/pane.rs`), workspace 는 `active_index_after_removal` + `AppState::fix_workspace_pointers_after_removal`(`src/state/workspace.rs`), pane 은 각 close 경로의 `was_focused` 가드. 제거 위치는 `CoreEvent::SurfaceClosed { workspace_purged }` 로 cascade 에 전달된다 — Core 는 `active_workspace` 를 모르고, cascade 시점엔 워크스페이스가 이미 사라져 위치를 알 수 없기 때문이다. 워크스페이스를 제거하는 **새 경로**를 추가하면 그 헬퍼를 함께 태운다.

## 자기 자신 닫기 보호 (Self-Close Protection)

**명령으로 자신이 속한 리소스를 닫을 수 없다** — 에이전트가 target ID 를 잘못 지정해 자기 터미널을 종료하는 사고 방지.

- ID 지정 close(`close surface --surface <ID>` / `close pane --pane <ID>` / `close tab --tab <ID>`)에서 caller 자신이 속한 대상을 지정하면 거부한다.
- **자기 자신을 닫는 유일한 방법은 `tasty close self`** (`TASTY_SURFACE_ID` 로 자신을 식별, 해당 surface 만 닫음).
- 워크스페이스를 통째로 정리할 때는 `tasty close workspace --id <W>` 를 쓴다 — 안의 surface 를 하나씩 닫을 필요가 없다. 자기 자신 닫기 보호는 여기에도 걸린다: caller 의 surface 가 그 워크스페이스 안에 있으면 거부한다.

## 에이전트 닫기와 포커스

에이전트가 **보고 있지 않은** 대상을 닫아도 사용자 화면은 움직이지 않는다.

- `active_workspace` 는 인덱스라 앞쪽 워크스페이스가 빠지면 통째로 밀린다. `workspace.close` 도 위 "삭제로 인한 인덱스 이동" 과 **같은 헬퍼**를 지난다 — 제거 직후 `AppState::fix_workspace_pointers_after_removal` 이 제거 위치를 기준으로 인덱스를 보정하므로, 손대지 않은 포인터가 계속 같은 워크스페이스를 가리킨다. 워크스페이스를 제거하는 새 경로를 추가하면 그 헬퍼를 반드시 함께 태운다.
- **활성 워크스페이스 자신을 닫을 때만** 이웃으로 이동한다.
- 에이전트가 닫은 것은 사용자의 "닫은 항목" 되돌리기 스택에 쌓이지 않는다. 사용자 경로와 에이전트 경로의 차이는 `close_workspace_at` 의 `WorkspaceCloseOrigin` **하나**로 표현하고, 갈리는 부수효과(되돌리기 스택 · plugin `surface.closed` 의 reason · close 계측 경로값)를 전부 거기서 파생시킨다 — 같은 축을 나타내는 값을 여럿 두면 그중 하나만 갈리는 사고가 난다.
- 파일 열기도 같은 형태다 — `FileDispatchOrigin` **하나**가 사용자/에이전트를 가르고, 결과 탭을 선택하는지와 `None` 분기가 발화하는 intent 의 출처가 거기서 파생된다. **전송 채널이 아니라 행위의 성질로 정한다**: plugin 이 사용자의 클릭을 `file_handler.dispatch` 로 중계하는 경로가 있어(markdown 문서 안의 링크 · 파일열기 팝업) "IPC 로 들어왔는가" 는 좌변이 아니다. plugin 이 그 호출에 **자기 popup** 을 `owner_popup_instance` 로 실으면, host 는 호출자가 그 popup 의 소유 plugin 이고 그 popup 이 사용자의 확정형 입력(포인터 버튼 · 키 누름)을 받았을 때만 사용자로 친다 — 외부 IPC 호출자는 같은 키를 실어도 에이전트다([ADR-0031](../../adr/0031-file-handler-routing.md)). webview 에서 오는 중계(markdown 문서 안의 링크)는 plugin 이 통지받은 navigation 의 URL 을 `user_navigation_url` 로 되대고, host 는 native 엔진이 그 시도를 사용자 제스처로 보고했고 그 surface 의 지금 페이지를 소유 plugin 이 썼으며 그 plugin 에 통지한 마지막 시도일 때만 그 한 번을 사용자로 친다 — 근거는 plugin 의 자기 신고가 아니라 host 가 직접 본 두 사실(엔진의 보고 · 페이지 작성자)이다. `webview.set_url` 은 에이전트에게도 열려 있어, 에이전트가 쓴 페이지 위의 사람 클릭은 근거가 되지 않는다(재지 않은 예외 하나: 그 클릭의 시도가 소유 plugin 이 되찾은 프레임의 drain 뒤에야 도착하는 순서 — [파일 열기 가이드의 사용자 동작 판정](../../features/file-handler/index.md)). macOS 는 엔진이 그 값을 주지 않아 에이전트로 도착한다 ([ADR-0031](../../adr/0031-file-handler-routing.md)). 그 값이 `IntentOrigin` 과 별개인 이유는 파일 식별이 워커 스레드를 왕복하면서 발화 당시 intent 를 잃기 때문이다 ([ADR-0031](../../adr/0031-file-handler-routing.md)).
- `workspace.closed` host event 는 origin 과 무관하게 발화한다. 워크스페이스가 사라졌다는 사실 자체는 누가 닫았든 같기 때문이다. 워크스페이스를 제거하는 경로는 셋(GUI·IPC 닫기 · Core cascade · 인라인 cascade)이고, 발화는 각 경로가 아니라 그 셋이 공유하는 초크포인트 `AppState::after_workspace_removed`(`src/state.rs`)가 한다 — 경로마다 각자 쏘던 때 인라인 cascade 하나가 실제로 빠져 있었다. 워크스페이스를 제거하는 새 경로를 추가하면 그 초크포인트를 반드시 지나게 한다.

workspace.close는 마지막 workspace, mirror workspace, hard 점유 surface가 포함된 workspace를 거절한다.
호출자 자신을 포함한 대상도 자기 닫기 보호를 따른다. mirror는 attach 해제로 정리한다.
GUI 창 종료는 window.close를 사용하되 headless에는 이 API가 없어 마지막 workspace를 닫을 수 없다.
확인용 force 플래그는 요구하지 않지만 되돌릴 수 없는 동작임을 help와 사용자 문서에 표시한다.
일부 점유 surface만 남기는 부분 workspace.close는 수행하지 않는다.

## 에이전트가 만든 창과 포커스

에이전트가 창을 만들어도 사용자가 보던 창은 그대로 focused 창이다.

- 창을 만든 주체는 `WindowRequestOrigin` **하나**가 정한다.
  - `User`: 부팅 첫 창 · 단축키 · 명령 팔레트 · CSD 버튼 · 트레이 · macOS dock.
  - `Agent`: IPC `window.create` / `view.create`. CLI `tasty new window` 와 hook 의
    `ipc_sequence` 도 여기로 온다.
  - 창 생성 실패를 누구에게 알릴지도 같은 값이 가른다. 새 경로도 이 값을 정해 넘긴다.
- `User` 창은 `focused_view_id` 를 새 창으로 옮긴다. `Agent` 창은 옮기지 않는다. 그래서 뒤이은
  대상 없는 요청(`tasty new workspace` 등)은 사용자가 보던 창에 떨어진다. 새 창에 워크스페이스를
  만들려면 그 창의 surface 를 `workspace.create` 의 `surface_id`(CLI `tasty new workspace --surface`)로
  지목한다 — `window_id` 는 창 자체를 다루는 요청(닫기 · 스크린샷 등)에만 쓴다
  ([ADR-0043](../../adr/0043-cli-errors-and-diagnostic-logs.md)).
  그 `surface_id` 는 `cwd` 를 생략했을 때의 상속 원본도 정한다 — 지목했는데 그 창의 포커스 surface 를
  읽으면 결과가 사용자가 그 창에서 보는 탭에 좌우되기 때문이다
  ([ADR-0043](../../adr/0043-cli-errors-and-diagnostic-logs.md)).
  - 예외: 가리키던 창이 없으면(main 창이 0 개였으면) 에이전트 창이 잡는다. 빼앗을 포커스가 없다.
- 에이전트 창은 숨긴 채 만들어, 등록 뒤 사용자가 보던 창 **뒤에** 키 포커스 없이 보인다
  (`tasty_platform::window_stacking::show_behind`). OS 마다 할 수 있는 데까지다.
  - macOS · Windows: 사용자 창 바로 아래에 둔다. 키 포커스를 가져가지 않는다.
  - X11: 창 관리자에게 **요청한다** — `_NET_WM_USER_TIME = 0`(포커스를 주지 말라)과
    `_NET_RESTACK_WINDOW`(사용자 창 아래). 창 관리자가 무시하거나 "맨 아래" 로 다룰 수 있다.
    `_NET_WM_USER_TIME` 은 map 된 뒤 지운다 — 사용자가 나중에 그 창을 고를 때 걸림이 없게.
  - Wayland: 할 수단이 없다. 컴포지터가 정한다.
  - 네이티브 호출이 실패하면 경고 뒤 winit 기본 경로로 보인다.
- 사용자가 에이전트 창을 직접 고르면 `WindowEvent::Focused(true)` 추적이 `focused_view_id` 를
  옮긴다.

근거와 플랫폼별 결과는 [ADR-0017](../../adr/0017-workspace-identity-and-focus.md).

## 에이전트가 만든 탭과 선택

에이전트가 탭을 만들어도 그 pane 의 활성 탭은 그대로다 — 위 "에이전트가 만든 창" 의 탭 판이다.

- 선택 여부는 `DomainIntent::CreateTab` 의 `activate` **하나**가 정한다. 각 진입점이 값을 정해
  싣는다.
  - IPC `tab.create`(CLI `tasty new tab`): `false`. 새 탭은 뒤에 붙기만 한다.
  - attach forward 의 `NewTab`: 원격 **사용자**의 손 조작이면 `true`, 원격 에이전트면 `false`
    (복원 스택을 가르는 `ForwardOrigin` 과 같은 축).
  - 파일 열기의 origin 갈래: `FileDispatchOrigin` 에서 파생(위 "에이전트 닫기와 포커스" 의 파일
    열기 항목).
  - `Intent::NewTab`: 발화 origin 이 사용자면 `true`, 아니면 `false`. 에이전트 라벨로 오는
    발화점은 origin 없는 `file_handler.dispatch` 하나이고, 그 갈래는 이제 사용자가 보던 탭을
    바꾸지 않는다. markdown plugin 의 파일열기 팝업(사용자의 `open_markdown`)은 같은 호출에 자기
    popup 을 실어 사용자로 도착하므로 종전대로 새 탭을 선택한다(아래 "에이전트 닫기와 포커스" 의
    파일 열기 항목 · [ADR-0031](../../adr/0031-file-handler-routing.md)).
- terminal kind 는 `activate` 와 무관하게 background 다. 사용자의 새 터미널 탭은 이 인텐트가
  아니라 `AppState::add_tab` 이 연다.
- 응답의 `active_tab` 은 "생성 뒤 그 pane 의 활성 탭" 이다 — 에이전트가 만든 탭이면 사용자가
  보던 탭의 인덱스다. 새 탭은 응답의 `surface_id` 로 다룬다.

근거는 [ADR-0017](../../adr/0017-workspace-identity-and-focus.md).

## 원격이 점유한 surface 는 닫기 요청이 죽이지 않는다

하드 점유(ADR-0021)는 "이 surface 는 지금 원격 사용자가 쓰고 있다" 는 선언이다. 닫기는
비가역이고 — 되돌리기 스택에 남는 것은 살아 있는 PTY 가 아니라 같은 명령으로 새 세션을
여는 레시피다 — 그래서 **닫기 요청은 거절한다.** 에이전트 경로만이 아니라 **사용자
경로도 같다**: 여기서 보호 대상은 로컬 사용자가 아니라 원격 사용자이고, 로컬 사용자는
점유 표시 위의 강제 끊기 버튼으로 점유를 회수한 뒤 닫을 수 있어 갇히지 않는다.

- 규칙의 소유자는 `AppState::refuse_if_hard_occupied` 하나다. 닫기 진입점은 자기가 죽일
  **대상 집합만** 넘긴다 — 워크스페이스 닫기는 그 워크스페이스의 surface 전부, 탭·페인
  닫기는 그 안의 전부, surface 닫기는 그 하나.
- **요청 경로만 이 검사를 지난다.** 셸이 스스로 끝나서 도는 사후 정리는 이미 죽은 프로세스를
  치우는 것이라 거절하면 좀비 surface 가 남는다. 그래서 검사는 공용 cascade 초크포인트가
  아니라 사용자 제스처·에이전트 요청의 진입점에 붙는다.
- IPC 는 토스트가 아니라 사유가 실린 에러로 거절한다. `surface.close_self` 도 예외가 아니다
  — 그 메서드는 호출자를 확인하지 않고 params 의 id 를 받으므로, 예외로 두면 그대로 우회
  통로가 된다.

## 재정렬에서도 포커스 대상은 보존된다

워크스페이스를 재정렬하면 인덱스가 가리키는 대상이 바뀐다 — 제거와 같은 종류의 밀림이다.
사용자가 보고 있던 워크스페이스는 재정렬 뒤에도 그대로 보고 있어야 한다.

- 옮겨진 것을 보고 있었으면 포인터가 **따라간다**.
- 옮겨진 구간을 자기 위치가 통과당하면 한 칸 당겨지거나 밀린다.
- 구간 밖이면 그대로다.

재정렬은 두 경로로 들어온다 — 사이드바 드래그·컨텍스트 메뉴가 부르는
`AppState::move_workspace`, 그리고 `CoreEvent::WorkspaceMoved` 의 `cascade_workspace_moved`
(IPC `workspace.move` 도 이쪽). 규칙은 **한 곳에만** 있다: 순수함수
`active_index_after_move` 와 그것을 적용하는 `AppState::fix_workspace_pointers_after_move`.
재정렬하는 새 경로를 추가하면 그 헬퍼를 함께 태운다 — 규칙을 복제하면 어느 경로로
재정렬했느냐에 따라 포커스가 달라진다.

카테고리 quick-switch 착지점은 id 를 들어 이 축의 보정 대상이 아니다(위 참조).

카테고리 안의 표시 순서는 입력 단계에서 전체 workspace 인덱스로 바꾼다.
카테고리 CRUD나 소속 변경만으로 workspace 배열 순서와 활성 인덱스를 바꾸지 않는다.
category_last_active는 ID를 저장해 삭제·이동 때 인덱스 보정이 필요하지 않다.

## 코드 위치

- `active_modal_id` / `modal_active` 게이트: `src/app/event_handler.rs`(`self.view.active_modal_id`), View 디스패치.
- focus 대상 해석 / `TASTY_SURFACE_ID` / `this`: `crates/tasty-cli/src/request.rs`.
- `tasty close self`: `crates/tasty-cli/src/commands/new_close.rs`(`CloseCommands::CloseSelf`).
- 삭제 시 활성 포인터 보정: `Pane::remove_tab_preserving_active`(`crates/tasty-model/src/pane.rs`) · `active_index_after_removal` / `AppState::fix_workspace_pointers_after_removal`(`src/state/workspace.rs`) · cascade 진입점 `cascade_surface_closed`(`src/core/structural_cascade.rs` — 두 빌드가 같은 본문).
- 재정렬 시 활성 포인터 보정: `active_index_after_move` / `AppState::fix_workspace_pointers_after_move`(`src/state/workspace.rs`) · 호출 경로 `AppState::move_workspace` 와 `cascade_workspace_moved`(`src/app/dispatch_domain.rs`, headless 는 `dispatch_domain_stubs.rs`).
- 창 생성의 origin 분기: `WindowRequestOrigin`(`src/app/event.rs`) → `focus_after_register` · `origin_window_attributes`(`src/app/window_lifecycle.rs`) — 등록 뒤 focused 창과 생성 속성(`with_active` · `with_visible`)이 여기서 파생된다. 에이전트 창을 사용자 창 뒤에 보이는 OS 호출은 `crates/tasty-platform/src/window_stacking.rs`.
- 탭 생성의 선택 분기: `DomainIntent::CreateTab` 의 `activate` → `Core::apply_create_tab`(`src/core/impl_tab.rs`) 이 `Pane::add_surface_tab` / `Pane::add_surface_tab_background`(`crates/tasty-model/src/pane.rs`) 중 하나를 고른다. 값을 정하는 진입점은 `structural_exec::create_tab`(`src/core/structural_exec.rs`) 의 호출자 · `src/intent/tab.rs` · `open_surface_tab`(`src/file/dispatch.rs`).
- 워크스페이스 close 의 origin 분기: `WorkspaceCloseOrigin`(`src/state/workspace.rs`) — 되돌리기 스택 · plugin close reason · 계측 경로값이 여기서 파생된다.
- 워크스페이스 제거 후 공통 뒷정리(`workspace.closed` 발화 + workspace scope memory purge): `AppState::after_workspace_removed`(`src/state.rs`).

외부 소켓의 전 창 합산·namespace·App 조기 응답도 일반 handler와 같은 진입 검사와 허용된 요청의 사용량 집계를 한 번 거친다. 검사 완료 요청을 하위 라우터에 전달하므로 라우팅 층 수만큼 예산이 소비되지 않는다. [ADR-0012](../../adr/0012-request-admission-and-isolation.md).
