# 시스템 트레이 정책 (운영 상세)

> 결정 근거·대안·재검토 조건은 [`adr/0001-system-tray-best-effort.md`](../../adr/0001-system-tray-best-effort.md). 본 문서는 *현재 운영 동작* 만 기술한다.

tasty 는 GUI 환경에서 백그라운드로 갈 때 **가능한 모든 OS 에서 트레이/상태 영역으로 들어간다 (best-effort)**. 트레이가 없는 환경은 조용히 생략하고 태스크바/도크 최소화로 폴백한다(graceful degradation). 구현은 `tray-icon` 0.22 단일 크레이트 + OS별 `cfg` 분기다.

## 트레이 생성

- **한 번만 생성하고 앱 생존 동안 유지**한다(`tray_icon.is_none()` 가드). macOS 는 백그라운드 시 윈도우를 파기·재생성하므로, 트레이를 매번 다시 만들지 않도록 이 가드가 필요하다.
- 생성 실패(미가용 환경)면 `create_tray_icon()` 이 **`None` 을 반환**하고 경고 로그만 남긴다 — 앱을 중단시키지 않는다. 이후 동선은 태스크바/도크 최소화 폴백.
- 메뉴 항목: **Show Window · New Window · Quit**.
- 아이콘은 임베드 PNG(`app_icon::tray_icon()`), macOS 는 `with_icon_as_template(true)` 로 메뉴 바 라이트/다크 틴팅에 맡긴다(타 OS 는 no-op).

## 백그라운드 진입 / 복귀 (OS별)

| OS | 백그라운드 진입 | 복귀("Show Window") |
|----|----------------|---------------------|
| **Windows** | 트레이 있으면 윈도우 `set_visible(false)` 로 숨김(생존 유지), 없으면 `set_minimized(true)` 태스크바 | `TrayShowWindow` → `set_visible(true)` + `set_minimized(false)` |
| **Linux** | Windows 와 동일 (트레이 있으면 숨김, 없으면 최소화) | Windows 와 동일 |
| **macOS** | 기존 모델 유지 — 윈도우 **파기 + state 파킹**(dock reopen 시 복원). 트레이와 무관하게 동작 | 트레이 "Show Window" 는 `CreateWindow` 로 라우팅 → 파킹된 state 를 꺼내 복원(dock reopen 과 동일 경로). macOS 엔 숨겨진 윈도우가 없기 때문 |

- **New Window**: 세 OS 모두 `CreateWindow`.
- **Quit**: 세 OS 모두 `Shutdown`.
- 메뉴 클릭은 매 이벤트 루프 tick 에서 `poll_menu_event()`(`MenuEvent::receiver().try_recv()`)로 폴링한다.

## graceful degradation

트레이가 없거나 tasty 가 지원하지 못하는 환경(미니멀 WM, AppIndicator 호스트 없음, 디스플레이 없음 등)에서는 트레이 등록을 **조용히 생략**하고 백그라운드 동선이 자동으로 태스크바/도크 최소화로 떨어진다. 사용자에게 에러를 띄우지 않는다 — "최대한 활용하되, 없으면 없는 대로" 가 원칙.

## Linux 특이사항 (GTK)

`tray-icon` 의 Linux 백엔드(StatusNotifierItem/AppIndicator)는 **GTK 가 초기화돼 있고 GTK 이벤트 루프가 같은 스레드에서 도는 것**을 전제한다. tasty 는 전용 GTK 메인 루프를 두지 않고:

- 트레이 생성 직전 `gtk::init()` 을 **지연 호출**한다. 실패(디스플레이/GTK 없음)하면 bail → `None` → 폴백.
- winit 이벤트 루프의 매 tick(`about_to_wait`)에서 **비차단** `gtk::main_iteration_do(false)` 로 GTK 이벤트를 펌프한다(`pump_gtk_events()`). 처리할 게 없으면 즉시 반환하므로 렌더 루프를 막지 않는다.
- 런타임 의존: `libgtk-3`, `libappindicator3`(또는 `libayatana-appindicator3`), `libxdo`. 빌드 의존은 각 `-dev` 패키지.

### DE 가용성 (best-effort 범위)

| 데스크톱 환경 | 트레이 |
|--------------|--------|
| KDE Plasma | SNI 네이티브 — 동작 |
| GNOME / Ubuntu | AppIndicator 확장 있으면 동작 |
| XFCE / Cinnamon / MATE | 동작 |
| 미니멀 WM / 트레이 없는 환경 | 미등록(폴백) |

## 스레드 제약

- **macOS**: 트레이는 메인 스레드에서, 이벤트 루프가 이미 도는 상태에서 생성해야 한다. tasty 의 생성 지점은 winit 메인 스레드 윈도우 셋업(루프 가동 후)이라 충족.
- **Linux**: GTK 소유 스레드(= winit 메인 스레드)에서 생성하고 같은 스레드에서 펌프한다.

## 코드 위치

- `src/platform/system_tray.rs` — `create_tray_icon()`(미가용 시 `None`), `poll_menu_event()`, `pump_gtk_events()`(Linux), `TrayMenuIds`. `cfg(all(any(windows, macos, linux), feature = "gui"))`.
- `src/app/event_handler.rs` — 생성(1회 가드)·백그라운드 진입(OS별)·복귀·메뉴 폴링·GTK 펌프 배선.
- `src/app/event.rs` — `TrayShowWindow` 이벤트.
- `src/platform/app_icon.rs` — 트레이 아이콘.
</content>
