# 알림 (Notifications)

- **Status**: Implemented
- **주체**: 로컬 사용자 (에이전트도 `notification.create` 로 발행 가능)
- **ADR**: [ADR-0040](../../adr/0040-locale-catalogs-and-display-text.md) (제목은 표시 전용 · `notification.create` 기본 제목은 UI 문구)
- **코드**: `NotificationStore` (notification 모델); IPC `notification.{list,create}`
- **화면**: 알림 패널 popup (Window 스코프) · 사이드바 배지

## 목적

터미널이 보낸 OSC 알림 시퀀스와 시스템 이벤트를 모아 **인앱 알림 패널·소리·surface 하이라이트·사이드바 배지**로 노출한다.

## 내부 동작

### OSC 시퀀스 감지

termwiz Parser 의 OSC 액션을 인터셉트해 알림 이벤트 생성 — OSC 9(iTerm2/ConEmu), OSC 99(Kitty), OSC 777(rxvt), BEL. (OSC 7=cwd 변경, OSC 0/2=타이틀 변경은 알림이 아닌 별도 처리.)

**벨(BEL) 토글**: BEL 알림(제목은 `notification.bell_title` 번역값)은 전역 `notification.enabled` 위에 벨 전용 `general.bell_notification`(기본 on)을 한 겹 더 얹어 게이트한다. off 면 토스트를 억제하되, 사용자가 등록한 `bell` 훅은 그대로 실행한다(훅=명시적 자동화 → 수동 반응인 토스트와 분리). `cascade_terminal_bell_ring` 참조.

### 제목은 표시 전용 — 식별은 별도 필드로

호스트가 스스로 만드는 알림 제목은 전부 `t()` 번역값이라 `general.language` 에 따라 달라진다 — 벨은 `notification.bell_title`, `notification.create` 가 `title` 없이 호출되면(`tasty notify "본문"`) `notification.default_title`. 따라서 제목 문자열은 **기계적 식별자가 아니다**. 알림의 출처를 구분해야 하는 소비자는 식별 필드를 본다 — 훅은 `HookEvent::Bell`(셸 env `TASTY_HOOK_EVENT=bell`, payload 에 제목 없음 — [hooks](../hooks/index.md)), plugin 은 `notification.created` 이벤트의 `source`. 제목 비교(`title == "Bell"`)로 분기하는 코드는 언어를 바꾸는 순간 깨지므로 두지 않는다. 근거·대안: [ADR-0040](../../adr/0040-locale-catalogs-and-display-text.md).

### NotificationStore

`VecDeque`에 최대 100개를 보관하고 초과하면 가장 오래된 항목을 `pop_front`로 제거한다.
같은 source의 알림이 설정 간격(기본 500ms) 안에 이어지면 기존 항목에 합친다.
개별 또는 전체 읽음 처리를 지원한다.

새 알림은 해당 surface의 `AttentionStore`에도 주의 표시를 요청한다. 알림 기록과 주의
표시는 서로 다른 저장소이며, 사이드바 배지는 읽지 않은 알림 수가 아니라 주의 표시가 있는
surface 수(`attention_count`)를 보여준다. 자세한 규칙은 [surface-highlight](../surface-highlight/index.md)를 따른다.

### IPC 목록의 범위·ID·순서·상한

`notification.list`는 모든 main/parked engine의 알림을 합쳐 반환한다. headless는
하나의 engine에 같은 규칙을 적용한다. UI 패널과 저장소는 여전히 창별이고, 각
저장소의 FIFO 보존 상한 100개와 읽음 상태는 공유하지 않는다.

ID는 인스턴스의 공유 IdGenerator에서 발급하는 u64이며 재시작 동안 보존되는 ID가
아니다. 목록은 ID 내림차순, 즉 **신규 생성 순서의 역순**으로 전체 최대 50개다.
창 순회 순서나 포커스는 결과를 바꾸지 않는다. coalescing은 기존 ID와 생성 순서를
유지하므로 최근 내용 갱신이 항목을 앞으로 옮기지는 않는다. 기존 한 engine의 순서와
고정 50개 응답 계약을 유지하며 새로운 limit/필터 인자를 도입하지 않는다.

이는 [목록 합산 원칙](../../adr/0017-workspace-identity-and-focus.md)의
적용이며, GUI 패널을 전역 패널로 바꾸는 결정이 아니다.

<a id="시스템-알림--사운드"></a>

### 알림 소리

`notification.sound`가 true이면 새 알림에 소리를 한 번 재생한다. macOS는 `NSBeep`,
Windows는 `MessageBeep`, Linux는 `paplay` → `aplay` → `\a` 순서로 시도한다.
headless는 소리를 내지 않는다. 기존 항목에 합쳐진 알림과 터미널 Bell은 별도로 소리를
재생하지 않는다. Bell은 OS가 이미 소리를 낼 수 있어 중복을 피하기 위한 예외다.
현재 OS 알림 센터로 보내는 기능은 없다.

### 시각 표시

- **surface 하이라이트**: 알림 발생 surface 에 파란 테두리, 포커스 시 자동 해제 — 또는 그
  surface 발 알림을 읽음 처리(개별/모두 읽음)했을 때 그 surface 에 남은 안읽음 알림이 없으면
  해제(같은 surface 의 다른 알림이 아직 안읽음이면 유지). 단 그 surface 가 **하드 점유
  (attach)** 중이면 해제되지 않는다 — 점유 중 attention 의 해제 주체는 홀더다
  ([ADR-0024](../../adr/0024-attention-ownership-and-clear.md)). **알림 자체의
  읽음 처리는 점유와 무관하게 그대로 동작한다** — 게이트가 걸리는 것은 attention 해제
  하나뿐이다. 상세 [`surface-highlight`](../surface-highlight/index.md).
- **사이드바 배지**: attention surface 가 있는 워크스페이스 행 우측에 `attention_count` 개수 pill
  배지(`paint_workspace_count_badge`, 99 초과 시 "99+" — 확장 사이드바는 이름 우측 숫자 배지, 축소
  사이드바는 dot). 모두 방문하거나 읽음 처리하면 소멸.
- **알림 패널** (Popup, Window 스코프): 최신순 목록, 워크스페이스·제목·본문·경과시간 + "Jump" 버튼. 열 때 전체 읽음, "Mark all read". 출처 워크스페이스가 이미 닫힌 알림은 워크스페이스명 자리에 `notification_panel.unknown_workspace` 번역값을 표시한다. Popup 이라 터미널 입력을 차단하지 않고 워크스페이스 전환과 무관하게 보임([popup](../../design/systems/popup.md)).

## 인터페이스

- **사용자**: 단축키로 알림 패널 토글, Jump/Mark all read.
- **AI Agent / CLI**: `notification.list` / `notification.create`(workspace_id/surface_id 라우팅, 포커스 비의존). `notification.create` 의 `title` 은 선택 — 생략하면 호스트가 `notification.default_title` 번역값을 채운다(`tasty notify <body> [--title ..]`). 기본 제목은 프로토콜 토큰이 아니라 UI 문구이므로 언어에 따라 달라진다.

## 비-목표

- **busy indicator**(실행 중 표시)·**도구 메뉴**·**토스트** 는 별도 — 각각 [busy-indicator](../../design/policies/busy-indicator.md)·[tools-menu](../tools-menu/index.md)·[toast](../../design/systems/toast.md).

## 관련

- [design/systems/popup](../../design/systems/popup.md) · [design/systems/toast](../../design/systems/toast.md) · [settings](../settings/index.md)
