# 알림 (Notifications)

- **Status**: Implemented
- **주체**: 로컬 사용자 (에이전트도 `notification.create` 로 발행 가능)
- **ADR**: 없음
- **코드**: `NotificationStore` (notification 모델); IPC `notification.{list,create}`
- **화면**: 알림 패널 popup (Window 스코프) · 사이드바 배지

## 목적

터미널이 보낸 OSC 알림 시퀀스와 시스템 이벤트를 모아 **인앱 알림 패널 + 시스템 OS 알림 + 사운드 + surface 하이라이트 + 사이드바 배지**로 노출한다.

## 내부 동작

### OSC 시퀀스 감지

termwiz Parser 의 OSC 액션을 인터셉트해 알림 이벤트 생성 — OSC 9(iTerm2/ConEmu), OSC 99(Kitty), OSC 777(rxvt), BEL. (OSC 7=cwd 변경, OSC 0/2=타이틀 변경은 알림이 아닌 별도 처리.)

**벨(BEL) 토글**: BEL 의 "Bell" 토스트는 전역 `notification.enabled` 위에 벨 전용 `general.bell_notification`(기본 on)을 한 겹 더 얹어 게이트한다. off 면 토스트를 억제하되, 사용자가 등록한 `bell` 훅은 그대로 발화한다(훅=명시적 자동화 → 수동 반응인 토스트와 분리). `cascade_terminal_bell_ring` 참조.

### NotificationStore

VecDeque FIFO(최대 100, 초과 시 `pop_front` O(1)). **병합(coalescing)**: 같은 source 에서 설정 간격(기본 500ms) 내 연속 알림은 기존에 합침. 워크스페이스별 unread 카운트, 개별/전체 읽음 처리. 신규 알림 발화 시 그 source surface 를 highlight 발동한다 — toast 는 highlight 의 **producer 중 하나**이며, highlight 상태 자체는 NotificationStore 가 아니라 producer 중립 공유 primitive(CoreState `highlighted_surfaces`)에 있다. 상세 [`surface-highlight`](../surface-highlight/index.md).

### 시스템 알림 + 사운드

윈도우 비활성 시 OS 네이티브 알림(notify-rust, 초당 1회 rate limit). `notification.sound` 가 true 면 신규 알림 발화 시 OS beep 1회(macOS `NSBeep` / Windows `MessageBeep` / Linux `paplay→aplay→\a` 3단 폴백, headless 는 Noop). coalesce 로 묶인 알림은 host event 미생성이라 자동 비음. 터미널 `\a`(Bell)는 OS 가 자체 beep 할 수 있어 안전 default 로 skip.

### 시각 표시

- **surface 하이라이트**: 알림 발생 surface 에 파란 테두리, 포커스 시 자동 해제 — 또는 그
  surface 발 알림을 읽음 처리(개별/모두 읽음)했을 때 그 surface 에 남은 안읽음 알림이 없으면
  해제(같은 surface 의 다른 알림이 아직 안읽음이면 유지). 상세 [`surface-highlight`](../surface-highlight/index.md).
- **사이드바 배지**: 하이라이트 surface 가 있는 워크스페이스에 `!` 배지(확장=이름 우측, 축소=번호 버튼 강조). 모두 방문하거나 읽음 처리하면 소멸.
- **알림 패널** (Popup, Window 스코프): 최신순 목록, 워크스페이스·제목·본문·경과시간 + "Jump" 버튼. 열 때 전체 읽음, "Mark all read". Popup 이라 터미널 입력을 차단하지 않고 워크스페이스 전환과 무관하게 보임([popup](../../design/systems/popup.md)).

## 인터페이스

- **사용자**: 단축키로 알림 패널 토글, Jump/Mark all read.
- **AI Agent / CLI**: `notification.list` / `notification.create`(workspace_id/surface_id 라우팅, 포커스 비의존).

## 비-목표

- **busy indicator**(실행 중 표시)·**도구 메뉴**·**토스트** 는 별도 — 각각 [busy-indicator](../../design/policies/busy-indicator.md)·[tools-menu](../tools-menu/index.md)·[toast](../../design/systems/toast.md).

## 관련

- [design/systems/popup](../../design/systems/popup.md) · [design/systems/toast](../../design/systems/toast.md) · [settings](../settings/index.md)
