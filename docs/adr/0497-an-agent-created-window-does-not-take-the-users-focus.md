# ADR-0497: 에이전트가 만든 창은 사용자의 포커스를 가져가지 않는다

- **Status**: Accepted
- **Date**: 2026-09-22
- **Tags**: focus, window, multi-window, ipc, cli, user-agent-separation, identity, winit, x11, wayland

## Context

main 창을 등록하는 자리는 `App::register_window` 하나다. 이 결정 전에는 그 함수가 등록한 창을
**무조건** `focused_view_id` 로 세웠다. 창을 만드는 경로는 둘로 갈린다.

- 사용자 경로: 부팅 첫 창 · 단축키 · 명령 팔레트 · CSD 버튼 · 트레이 · macOS dock.
- 에이전트 경로: IPC `window.create` / `view.create`. CLI `tasty new window` 와 hook 의
  `ipc_sequence` 도 이 경로로 들어온다.

그래서 에이전트가 `tasty new window` 를 부르면 두 가지가 일어났다.

- `window.list` 의 `focused` 가 새 창으로 옮겨 갔다.
- 뒤이은 대상 없는 요청(`tasty new workspace` 등)이 사용자가 보던 창이 아니라 에이전트의 새 창에
  떨어졌다. IPC 라우터(`src/app/ipc/routing.rs`)가 대상이 없으면 `focused_view_id` 로 폴백하기
  때문이다.

그 폴백은 "대상 없는 요청은 사용자가 보는 창으로 간다" 는 약속이고, 그 창이 어디인지를 에이전트가
바꿔 버린 것이다. [정체성 원칙](../identity.md) 1(에이전트 행동의 부수효과가 사용자 포커스에 닿지
않는다)과 3(포커스는 사용자의 것이다)을 어긴다.

`create_new_window` 는 이미 `WindowRequestOrigin { User, Agent }` 를 받고 있었다. 창 생성 실패를
누구에게 알릴지 가르던 축이다([ADR-0117](0117-window-and-modal-creation-failure-policy.md) ·
[ADR-0122](0122-winit-scheduled-fallible-ipc-returns-outcome.md)). 포커스를 옮기느냐도 같은
물음(이 창은 누가 원했나)이다.

OS 포커스는 별개 층이다. winit 0.30.13 의 `WindowAttributes::with_active(false)` 는 새 창이
포커스를 받지 않게 요청한다. 문서는 Android · iOS · X11 · Wayland · Orbital 에서 이것을
"Unsupported" 라 적는다. 실측(winit 0.30.13 소스)으로 X11 백엔드는 이 값을 읽지 않고, 창이
visible 이면 그대로 map 한다.

## Decision

**창을 만든 주체가 포커스를 정한다. 그 주체 값은 `WindowRequestOrigin` 하나다.**

- `register_window` 는 origin 을 받는다. 등록 뒤 `focused_view_id` 는 순수함수
  `focus_after_register` 가 정한다.
  - `User` 면 새 창으로 옮긴다. 부팅 첫 창도 `User` 다 — 사용자가 앱을 띄운 결과이고, 옮겨 갈
    이전 포커스도 없다.
  - `Agent` 면 옮기지 않는다. 단 가리키던 창이 없으면(main 창이 0 개였으면) 새 창을 잡는다.
    빼앗을 사용자 포커스가 없고, `None` 으로 두면 이후 대상 없는 IPC 가 전부 실패한다.
- 에이전트 창의 `WindowAttributes` 에는 `with_active(false)` 를 준다. 사용자 창은 `true`(winit
  기본값)다. 이 호출에는 `#[cfg]` 가 없다 — 지원하지 않는 플랫폼에서는 winit 이 무시한다.
- 사용자가 에이전트 창을 직접 고르면 기존 `WindowEvent::Focused(true)` 추적 경로
  (`App::handle_window_focused`)가 `focused_view_id` 를 옮긴다. 이 결정은 그 경로를 바꾸지 않는다.

### 플랫폼별 결과

| 플랫폼 | tasty 의 `focused_view_id` | OS 포커스 |
|---|---|---|
| macOS | 안 옮김 | `with_active(false)` — 새 창을 key window 로 만들지 않는다. 미측정 |
| Windows | 안 옮김 | `with_active(false)` — 활성화 없이 표시한다. 미측정 |
| X11 | 안 옮김 | winit 이 무시한다. 창 관리자가 정한다(대개 새 창에 포커스를 준다) |
| Wayland | 안 옮김 | winit 이 무시한다. 컴포지터가 정한다 |

첫 열(tasty 가 대상 없는 요청을 보낼 창)은 모든 플랫폼에서 같다. 원칙 3 이 지키려는 것이 그
값이다. OS 포커스가 새 창으로 가도 사용자가 그 창을 실제로 조작하면(`Focused(true)`) 그때
옮겨 간다 — 사용자가 한 일이니 옳다.

### X11 · Wayland 의 한계

winit 0.30.13 의 X11 확장(`WindowAttributesExtX11`)이 주는 것은 visual · screen · name ·
override-redirect · window type · base size · embed parent 다. 포커스 힌트
(`_NET_WM_USER_TIME` · `WM_HINTS.input` 류)를 주는 것은 없고, 백엔드도 `_NET_WM_USER_TIME` 을
쓰지 않는다.

가까워 보이는 것들도 답이 아니다.

- `with_override_redirect(true)` 는 창 관리자를 통째로 우회한다. 장식 · 스태킹 · 포커스 관리를
  전부 잃는다.
- window type(utility · notification 등)은 창의 의미를 바꾼다.
- startup-notify 의 activation token 과 Wayland 의 xdg-activation 은 반대 방향이다. 포커스를
  **받으려는** 장치다.

남는 길은 raw window handle 로 X window id 를 얻어 프로퍼티를 직접 쓰는 것이다. 이 결정은 그것을
구현하지 않는다(재검토 조건 참조).

## Consequences

- **얻은 것**:
  - 에이전트가 창을 만들어도 대상 없는 요청이 떨어지는 창이 바뀌지 않는다. 모든 플랫폼에서
    같다.
  - macOS · Windows 에서는 OS 포커스도 안 옮겨 간다.
  - 포커스를 가르는 축이 실패 안내 축과 같은 값 하나라, 한쪽만 갈리는 사고가 안 난다.
- **잃은 것**:
  - `tasty new window` 직후 대상 없는 명령으로 새 창을 조작하던 스크립트는 이제 원래 창을
    조작한다. 새 창을 다루려면 `window.create` 응답의 `window_id` 로 대상을 지정한다. 원칙 3 이
    원래 요구하던 형태다.
  - X11 · Wayland 에서는 OS 포커스와 tasty 의 `focused_view_id` 가 잠시 어긋날 수 있다. 창
    관리자가 새 창에 포커스를 줘도 winit 이 `Focused(true)` 를 전하면 추적 경로가 따라가므로
    어긋남은 그 이벤트 전까지다.
- **운영 비용 / 유지 부담**:
  - 창을 만드는 새 경로는 `WindowRequestOrigin` 을 정해 넘겨야 한다. `register_window` 의
    인자라 빠뜨리면 컴파일이 안 된다.

## Alternatives Considered

- **(b) OS 동작을 받아들인다** — 새 창이 포커스를 가져가는 것을 플랫폼 관례로 두고 tasty 는
  손대지 않는다. 사용자가 기각했다. OS 포커스 쪽은 X11 · Wayland 에서 실제로 이 상태가 남지만,
  `focused_view_id` 까지 따라가게 두면 에이전트의 창 생성이 대상 없는 요청의 목적지를 바꾼다.
  그것은 OS 가 강제하지 않는 tasty 자신의 선택이다.
- **X11 에 `_NET_WM_USER_TIME` 을 직접 쓴다** — winit 밖에서 raw handle 로 프로퍼티를 쓰는
  구현이다. 창 관리자마다 해석이 달라 효과를 이 머신(Xvfb, 창 관리자 없음)에서 잴 수 없다. 이번
  범위에서 제외했다.
- **에이전트 창은 `focused_view_id` 가 `None` 이어도 안 잡는다** — 규칙은 더 단순하다. 그러나
  main 창이 0 개인 상태(macOS 에서 전부 파킹 · 트레이)에서 에이전트가 창을 만들면 이후 대상 없는
  IPC 가 전부 실패한다. 빼앗을 포커스가 없는 자리에서 원칙이 지키는 것이 없다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- winit 을 올렸고, 새 버전이 X11 또는 Wayland 에서 `with_active` 를 지원하거나 X11 확장에 user
  time · 포커스 힌트 setter 를 더했다. 재는 법: 새 버전 소스의 `WindowAttributes::with_active`
  문서의 플랫폼 절과 `platform/x11.rs` 의 `WindowAttributesExtX11` 목록.
- 창을 만드는 경로가 새로 생겼는데 사용자/에이전트 둘로 안 갈린다(예: plugin 이 IPC 를 거치지
  않고 창을 만든다).

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- X11 · Wayland 사용자가 에이전트 창이 포커스를 가져가 타이핑이 새 창으로 샌다고 보고한다.
  재는 법: 창 관리자가 있는 X11 · Wayland 세션에서 한 창에 타이핑하는 중에 `tasty new window`
  를 부르고, 키 입력이 어느 창에 들어가는지 본다.
- macOS · Windows 에서 `with_active(false)` 창이 그래도 포커스를 가져간다. 재는 법: 같은 절차를
  그 OS 에서.

## References

- [포커스 정책](../design/policies/focus.md) — 에이전트 창 생성과 포커스
- [멀티 윈도우 아키텍처](../architecture/multi-window.md)
- [정체성](../identity.md) — 원칙 1 · 3
- [ADR-0117](0117-window-and-modal-creation-failure-policy.md) ·
  [ADR-0122](0122-winit-scheduled-fallible-ipc-returns-outcome.md) — 같은 `WindowRequestOrigin`
  축이 실패 안내 채널을 가른다
- 코드 근거(결정이 실현된 현재 위치): `App::register_window` · `App::create_new_window` ·
  `focus_after_register`(`src/app/window_lifecycle.rs`), `WindowRequestOrigin`(`src/app/event.rs`),
  `App::handle_window_focused`(`src/app/event_handler.rs`)
