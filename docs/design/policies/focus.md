# 포커스 정책 (운영 상세)

> 정체성 차원의 근거는 [identity §2.3 포커스 독립성](../../identity.md). 본 문서는 *현재 운영 동작* 만 기술한다. 계층 용어(View/Modality)는 [concepts/hierarchy](../../concepts/hierarchy.md).

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

| Modality | 구현체 | 동작 |
|----------|--------|------|
| Modal | `SettingsView` · `PluginsView` · `QuitView` (`ModalView`) | 전체 입력 차단, 닫기 전까지 다른 조작 불가 |
| Modeless | `MainView`(`TerminalHostView`) · `PresetView`(`EditorView`) | 독립 포커스, 다른 윈도우와 공존 |

**OS 네이티브 윈도우 비활성화(Win32 `EnableWindow` 등)는 쓰지 않는다** — 플랫폼별 동작 차이로 크로스플랫폼 일관성이 깨진다. 앱 레벨 `modal_active` 게이트로 처리한다.

## View 내부 포커스

Modal/View 레벨과 별개로, 각 View 내부에서 Pane 간·Surface 간 포커스 이동과 탭 전환이 일어난다 (단축키/클릭). 단축키는 [`KeybindingSettings`](key-mapping.md) — 하드코딩 아님. 이 내부 포커스는 그 View 가 OS 포커스를 갖고 Modal 이 비활성일 때만 동작한다.

## CLI/IPC 포커스 독립 원칙

**focus 는 사용자의 시선·관심을 나타내는 독립 행위이며, IPC/CLI 명령의 대상을 결정하는 수단이 아니다.** focus 를 대상 결정에 쓰면 race condition 이 내재한다(명령 발행 후 실행 전 사용자가 focus 를 옮기면 엉뚱한 대상에 실행; 에이전트가 다른 작업으로 focus 를 옮기면 의도 붕괴).

따라서:

- **IPC/CLI 로 focus 를 변경할 수 없다.** focus 변경 API(`surface.focus` / `pane.focus` / `workspace.select` / `focus.direction`)는 release 에 없다(제거됨). focus 는 오직 사용자 행위(단축키·마우스)로만 바뀐다.
- 모든 명령은 대상을 **ID 로 직접 지정**한다. `list` 는 **전 워크스페이스 순회**(활성 상태 비의존).
- 활성 상태 *조회* 는 허용(`focused` 필드 등). 활성 상태에 *의존* 하는 동작은 금지.
- target 미지정 명령은 **에러 + 사용법 안내**(silent fallback 금지). 호출자는 조회(`list surfaces` 등)로 ID 를 확인해 전달한다. 리소스 생성 명령은 응답에 생성된 ID 를 포함한다.
- 리소스 생성/삭제 명령이 내부적으로 focus 를 일시 이동해야 하면 작업 후 **원래 focus 를 복원**한다.
- `TASTY_SURFACE_ID` 환경변수(= "내가 있는 surface")는 focus 와 다르다. CLI `--surface` 기본값으로 쓸 수 있다.

## 자기 자신 닫기 보호 (Self-Close Protection)

**명령으로 자신이 속한 리소스를 닫을 수 없다** — 에이전트가 target ID 를 잘못 지정해 자기 터미널을 종료하는 사고 방지.

- ID 지정 close(`close surface --surface <ID>` / `close pane --pane <ID>` / `close tab --tab <ID>`)에서 caller 자신이 속한 대상을 지정하면 거부한다.
- **자기 자신을 닫는 유일한 방법은 `tasty close self`** (`TASTY_SURFACE_ID` 로 자신을 식별, 해당 surface 만 닫음). 상위(pane/workspace)를 정리하려면 안의 다른 surface 를 먼저 닫고 마지막을 `close self` 로 닫으면 상위가 연쇄 정리된다.

## 코드 위치

- `active_modal_id` / `modal_active` 게이트: `src/app/event_handler.rs`(`self.view.active_modal_id`), View 디스패치.
- focus 대상 해석 / `TASTY_SURFACE_ID` / `this`: `crates/tasty-cli/src/request.rs`.
- `tasty close self`: `crates/tasty-cli/src/commands/new_close.rs`(`CloseCommands::CloseSelf`).
</content>
