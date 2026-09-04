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

**탭바 클릭 → 그 pane 으로 focus 이동**: 콘텐츠 영역 클릭과 대칭으로, 비-focused pane 의 탭바(탭 본체·탭이 없는 빈 영역·스크롤 화살표·"+"/split/search 버튼)를 primary click 하면 그 pane 으로 focus 가 이동한다. 탭바는 그 pane 을 직접 조작하는 사용자 행위이므로 클릭 대상 pane 과 focus 가 어긋나면 안 된다(비-focused pane 의 탭을 클릭해도 탭 전환만 일어나고 focus 는 그대로 남는 것은 결함). 빈 영역 클릭은 탭 전환 없이 focus 만 옮긴다. 우클릭 컨텍스트 메뉴(탭/pane/새 탭 버튼)는 대상 `pane_id`/`tab_index` 를 메뉴 항목에 직접 실어 나르므로 focus 이동이 필요 없다 — 우클릭은 조회/메뉴-오픈이지 조작 commit 이 아니다. 구현: `src/adapters/ui/tab_bar.rs` `TabBarAction::focus_target_pane` + `apply_tab_bar_actions`.

이 규칙은 **사용자 마우스 클릭**에 의한 focus 이동이므로 아래 "CLI/IPC 포커스 독립 원칙"(에이전트/명령 유래 focus 강제 금지)과 별개다 — 혼동 금지. 그 원칙은 IPC/CLI 명령이 focus 를 대상 결정 수단으로 쓰거나 강제 변경하는 것을 막는 것이지, 사용자가 GUI 를 직접 클릭했을 때 그 결과로 focus 가 따라가는 것을 막지 않는다.

## CLI/IPC 포커스 독립 원칙

**focus 는 사용자의 시선·관심을 나타내는 독립 행위이며, IPC/CLI 명령의 대상을 결정하는 수단이 아니다.** focus 를 대상 결정에 쓰면 race condition 이 내재한다(명령 발행 후 실행 전 사용자가 focus 를 옮기면 엉뚱한 대상에 실행; 에이전트가 다른 작업으로 focus 를 옮기면 의도 붕괴).

따라서:

- **IPC/CLI 로 focus 를 변경할 수 없다.** focus 변경 API(`surface.focus` / `pane.focus` / `workspace.select` / `focus.direction`)는 release 에 없다(제거됨). focus 는 오직 사용자 행위(단축키·마우스)로만 바뀐다.
- 모든 명령은 대상을 **ID 로 직접 지정**한다. `list` 는 **전 워크스페이스 순회**(활성 상태 비의존).
- 활성 상태 *조회* 는 허용(`focused` 필드 등). 활성 상태에 *의존* 하는 동작은 금지.
- target 미지정 명령은 **에러 + 사용법 안내**(silent fallback 금지). 호출자는 조회(`list surfaces` 등)로 ID 를 확인해 전달한다. 리소스 생성 명령은 응답에 생성된 ID 를 포함한다.
  - `terminal.kill`/`terminal.release`/`terminal.respawn`/`terminal.broadcast` 는 `--surface`(parent) 를 생략하면 host 가 "현재 engine 에 등록된 parent 가 정확히 1개"일 때만 그 parent 로 폴백한다(단일 윈도우 세션의 하위 호환). main window 가 **2개 이상** 열려 있는데 이 4개 메서드가 `--surface` 없이 호출되면, 어느 window 를 봐야 하는지 자체가 정해지지 않으므로 focused window 로 조용히 새지 않고 명시적 에러로 거부한다(`src/app/request_owner.rs` `find_request_owner`/`ambiguous_parent_fallback_requires_surface`).
- 리소스 생성/삭제 명령이 내부적으로 focus 를 일시 이동해야 하면 작업 후 **원래 focus 를 복원**한다.
- `TASTY_SURFACE_ID` 환경변수(= "내가 있는 surface")는 focus 와 다르다. CLI `--surface` 기본값으로 쓸 수 있다.

## 삭제로 인한 인덱스 이동에서도 포커스 대상은 보존된다

**시야가 움직이는 경우는 하나뿐 — 사용자가 보고 있던 대상 *자체* 가 사라졌을 때다.** 보고 있지 않은 워크스페이스/탭/pane 이 닫혔는데 화면이 바뀌면 결함이다. 근거 [ADR-0113](../../adr/0113-close-preserves-the-focused-target.md).

활성 포인터 셋 중 둘은 **인덱스**가 진실 소스다 — `AppState::active_workspace` 와 `Pane::active_tab`. 인덱스는 앞쪽 원소가 빠지면 손대지 않아도 **가리키는 대상이 바뀐다.** 그래서 범위 초과 clamp 만으로는 부족하고, 제거 위치를 기준으로 함께 당겨야 한다.

| 계층 | 포인터 | 제거가 앞쪽일 때 | 제거된 것이 보던 대상일 때 |
|---|---|---|---|
| workspace | `active_workspace`(인덱스) | 한 칸 당김 — 같은 워크스페이스 유지 | 그 자리로 밀려 들어온 워크스페이스(마지막이었으면 직전) |
| tab | `Pane::active_tab`(인덱스) | 한 칸 당김 — 같은 탭 유지 | 그 자리로 밀려 들어온 탭(마지막이었으면 직전) |
| pane | `Workspace::focused_pane`(id) | 그대로 — id 는 밀리지 않는다 | 생존 pane 으로 재배정 |

- 이 보정은 **origin 으로 분기하지 않는다.** 대상 기준 보정은 사용자 경로(컨텍스트 메뉴로 앞쪽 탭 닫기)에서도 옳다. origin 게이트는 "에이전트가 새로 만든 것으로 포커스를 옮기지 않는다"(`cascade_workspace_created` · `cascade_surface_split`)처럼 이동 여부가 정책적으로 갈리는 곳에만 쓴다.
- 카테고리 quick-switch 착지점(`AppState::category_last_active`)도 전역 인덱스를 값으로 들고 있어 같은 보정을 받는다. 제거된 워크스페이스를 가리키던 항목은 삭제되고, 사용 시점에 그 카테고리의 first 로 폴백한다.
- 원격 attach 로 forward 된 구조 변경(`execute_forwarded_structural_op`)과 mirror 워크스페이스 teardown 도 같은 close 경로를 타므로 같은 규칙이 적용된다.

구현: tab 은 `Pane::remove_tab_preserving_active`(`crates/tasty-model/src/pane.rs`), workspace 는 `active_index_after_removal` + `AppState::fix_workspace_pointers_after_removal`(`src/state/workspace.rs`), pane 은 각 close 경로의 `was_focused` 가드. 제거 위치는 `CoreEvent::SurfaceClosed { workspace_index_purged }` 로 cascade 에 전달된다 — Core 는 `active_workspace` 를 모르고, cascade 시점엔 워크스페이스가 이미 사라져 위치를 알 수 없기 때문이다. 워크스페이스를 제거하는 **새 경로**를 추가하면 그 헬퍼를 함께 태운다.

## 자기 자신 닫기 보호 (Self-Close Protection)

**명령으로 자신이 속한 리소스를 닫을 수 없다** — 에이전트가 target ID 를 잘못 지정해 자기 터미널을 종료하는 사고 방지.

- ID 지정 close(`close surface --surface <ID>` / `close pane --pane <ID>` / `close tab --tab <ID>`)에서 caller 자신이 속한 대상을 지정하면 거부한다.
- **자기 자신을 닫는 유일한 방법은 `tasty close self`** (`TASTY_SURFACE_ID` 로 자신을 식별, 해당 surface 만 닫음).
- 워크스페이스를 통째로 정리할 때는 `tasty close workspace --id <W>` 를 쓴다 — 안의 surface 를 하나씩 닫을 필요가 없다. 자기 자신 닫기 보호는 여기에도 걸린다: caller 의 surface 가 그 워크스페이스 안에 있으면 거부한다.

## 에이전트 닫기와 포커스

에이전트가 **보고 있지 않은** 대상을 닫아도 사용자 화면은 움직이지 않는다.

- `active_workspace` 는 인덱스라 앞쪽 워크스페이스가 빠지면 통째로 밀린다. `workspace.close` 도 위 "삭제로 인한 인덱스 이동" 과 **같은 헬퍼**를 지난다 — 제거 직후 `AppState::fix_workspace_pointers_after_removal` 이 제거 위치를 기준으로 인덱스를 보정하므로, 손대지 않은 포인터가 계속 같은 워크스페이스를 가리킨다. 카테고리 quick-switch 착지점도 같이 보정한다. 워크스페이스를 제거하는 새 경로를 추가하면 그 헬퍼를 반드시 함께 태운다.
- **활성 워크스페이스 자신을 닫을 때만** 이웃으로 이동한다.
- 에이전트가 닫은 것은 사용자의 "닫은 항목" 되돌리기 스택에 쌓이지 않는다. 사용자 경로와 에이전트 경로의 차이는 `close_workspace_at` 의 `WorkspaceCloseOrigin` **하나**로 표현하고, 갈리는 부수효과(되돌리기 스택 · plugin `surface.closed` 의 reason · close 계측 경로값)를 전부 거기서 파생시킨다 — 같은 축을 나타내는 값을 여럿 두면 그중 하나만 갈리는 사고가 난다.
- `workspace.closed` host event 는 origin 과 무관하게 발화한다. 워크스페이스가 사라졌다는 사실 자체는 누가 닫았든 같고, surface 를 하나씩 닫아 워크스페이스가 사라지는 cascade 경로도 그렇게 한다.

## 코드 위치

- `active_modal_id` / `modal_active` 게이트: `src/app/event_handler.rs`(`self.view.active_modal_id`), View 디스패치.
- focus 대상 해석 / `TASTY_SURFACE_ID` / `this`: `crates/tasty-cli/src/request.rs`.
- `tasty close self`: `crates/tasty-cli/src/commands/new_close.rs`(`CloseCommands::CloseSelf`).
- 삭제 시 활성 포인터 보정: `Pane::remove_tab_preserving_active`(`crates/tasty-model/src/pane.rs`) · `active_index_after_removal` / `AppState::fix_workspace_pointers_after_removal`(`src/state/workspace.rs`) · cascade 진입점 `cascade_surface_closed`(`src/app/dispatch_domain.rs`, headless 는 `dispatch_domain_stubs.rs`).
- 워크스페이스 close 의 origin 분기: `WorkspaceCloseOrigin`(`src/state/workspace.rs`) — 되돌리기 스택 · plugin close reason · 계측 경로값이 여기서 파생된다.
