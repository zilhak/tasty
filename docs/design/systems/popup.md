# 내부 팝업 시스템

Popup 은 View 내부에 존재하는 가상 창이다 — 터미널과 공존하며 포커스를 독점하지 않는다. 모든 내부 팝업은 **`PopupManager` + `PopupDef`** 로 관리된다(`src/adapters/ui/popup.rs`). 용어 구분은 [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md), 팝업을 *추가하는 법* 은 [dev-guide/popup-implementation](../../dev-guide/popup-implementation.md). 이 문서는 시스템 *동작 모델* 이다.

## 8대 규칙

1. **타이틀 + 콘텐츠** — 상단 타이틀 영역(높이 토큰 고정) + 하단 콘텐츠.
2. **타이틀바** — 제목 중앙 정렬 + 우측 X(닫기) 버튼(호버 시 빨강). 제목이 close 버튼과 겹칠 만큼 길면 가용 폭 기준으로 말줄임(`…`) 처리(상세 → [popup-implementation §타이틀 길이 처리](../../dev-guide/popup-implementation.md#타이틀-길이-처리-elide)).
3. **드래그 이동** — 팝업이 선언한 **드래그 핸들**(`drag_handle`) 영역을 드래그로 팝업 전체 이동. 타이틀바 팝업은 타이틀 영역이 핸들(기본). 타이틀바 없는 headless 패널 팝업(`port_scanner`·`remote_tool`)은 뷰가 렌더 시점의 **실측 헤더 rect(전체폭 × 실제 높이)** 를 매니저에 보고해 헤더 영역 전체를 이동 핸들로 쓴다(정적 `DragHandle::Region` 띠는 open 첫 프레임 폴백). 헤더 텍스트 라벨은 비선택(`selectable_labels=false`)이라 글자 위에서도 드래그가 텍스트 선택으로 새지 않는다. **위젯 우선 중재**(`is_using_pointer`)로 핸들이 위젯(검색 입력·버튼)과 겹쳐도 위젯 클릭이 항상 우선되어 충돌하지 않는다(8번 입력 우선순위 참조).
4. **커서** — 드래그 핸들 위에서 grab 커서. 리사이즈 가능 팝업의 테두리에서는 엣지별 리사이즈 커서.
5. **배경 구분** — 팝업 배경은 `surface0`, 타이틀바는 `mantle` — 터미널 focused(검정)/unfocused 배경과 달라 위에 떠 있음이 시각적으로 구분된다.
6. **경계 제한** — 팝업의 어떤 부분도 스코프 밖으로 못 나간다. 리사이즈 시 자동 재배치.
7. **다중 + z-order** — 여러 개 동시 가능. 나중에 열리거나 클릭된 것이 앞. 겹친 영역의 마우스 이벤트는 최상단 팝업만 받는다.
8. **리사이즈 + 입력 우선순위** — `resizable` 팝업은 테두리 8방향 드래그로 크기 조절(`min_size` 하한 + 스코프 경계 클램프). 입력 우선순위는 **(egui)위젯 > close 버튼 > 리사이즈 엣지 > 드래그 핸들 > 콘텐츠**. 이동/리사이즈 START 는 콘텐츠 렌더 뒤 `is_using_pointer()` 게이트로 판정해, 위젯이 프레스를 가져간 프레임에는 발동하지 않는다(이동이 사실상 최후순위). 사용자가 리사이즈한 뒤에는 `sizer` 가 크기를 되돌리지 않는다(close 시 리셋).

(모든 색·치수는 Theme 토큰 — [theme.md](theme.md).)

## 구조

- **`PopupDef`** — 정적·데이터 지향 정의(id, title_key/title_fn, default_size/sizer, default_scope, close_on_outside_click, headless, sticky_focus, drag_handle, resizable, min_size, draw_fn). 전부 `src/adapters/ui/popup/defs.rs::all_defs()` 에 모은다. 필드 상세 → [popup-implementation](../../dev-guide/popup-implementation.md).
- **`PopupManager`** — 공통 동작(z-order, 드래그, 리사이즈, 타이틀바, clamp, 포커스) 중앙 관리. `register_def` / `open*` / `close` / `toggle` / `draw`.
- **`PopupState`** — 개별 인스턴스 상태(id, title, pos, size, open, focused, scope, dragging/resizing, size_user_overridden).
- **범용 render 루프** — `src/adapters/ui/popup/frame.rs::draw_popup_layer`(등록된 모든 `PopupDef` 순회 + close 경로별 `on_close` 훅 drain). toast/banner/modifier-hint/tutorial 오버레이 체인은 개념이 다르므로(ADR-0024) `src/adapters/ui/overlay.rs` 로 분리돼 있다. 진입점 `src/adapters/ui.rs::draw_popups` 가 둘을 z-order 순서로 호출한다.

등록된 팝업은 `all_defs()` 가 단일 출처다(예: `notifications`, `convert_surface`, `rename`, `search_bar`, `tools_menu` …). 새 팝업 = 테이블 항목 1개 + draw 함수 하나. 단, plugin 이 소유하는 팝업(예: markdown 파일열기·large-file 확인)은 host `PopupDef` 가 아니라 plugin 매니페스트 `[[contributes.popup]]`(egui-mesh) 로 등록된다.

## 수명 계약 (open → close → 뒷정리)

팝업이 닫히는 경로는 6개다: draw_fn 이 `PopupAction::Close` 반환 / X 버튼·바깥 클릭(`PopupManager::draw` 내장 포인터 처리) / `UiIntent::ClosePopup` / 이미 열린 채로의 `UiIntent::TogglePopup` / App 계층의 직접 `close()` 호출 / debug IPC(`debug.host_popup.close`, 구조적으로 `ClosePopup` 과 동일). 이 6개 전부가 **`PopupManager::close()`** 로 수렴한다 — `PopupState.open` 을 `false` 로 세팅하는 유일한 지점이다.

상태를 가진 팝업(draft 버퍼, 대상 id 등)은 draw_fn 안에서만 정리하면 안 된다 — draw_fn 을 거치지 않는 나머지 5개 경로에서 정리가 샌다. 대신 **`PopupDef.on_close`** 훅을 선언한다: `close()` 가 대상을 `closed_queue` 에 쌓고, `popup::frame::draw_popup_layer` 가 다음 draw 시점에 이 큐를 drain 하며 등록된 훅을 정확히 한 번 호출한다(재진입 지원 — 훅이 다른 popup 을 닫으면 그 close 도 같은 drain 안에서 처리되고, 상호 재오픈 등 논리 오류에 대비해 라운드 상한을 둔다). 상태가 없거나(`notifications`) 남아도 무해하다고 판단했으면(`tutorial_topics`) `on_close: None` 옆에 근거를 남긴다.

절차·필드 상세는 [popup-implementation §닫힘 정리](../../dev-guide/popup-implementation.md#닫힘-정리).

## 발화 정책 (CRITICAL)

**Popup 은 사용자 행동(키보드 단축키 / 마우스 / 메뉴)에서만 발사된다.** release 의 시스템·에이전트·도메인 cascade 어느 경로도 popup 을 자동으로 띄울 수 없다([toast.md](toast.md) 와 동일 원칙, [identity](../../identity.md) 원칙 1).

- ✅ 단축키/마우스/메뉴 → `UiIntent::OpenPopup` 발화
- ✅ popup A 의 *사용자 액션* cascade → popup B (origin 전파)
- ❌ release 의 IPC/CLI/Plugin 에서 popup 발화
- ❌ 시스템 조건(PTY 종료·시간 경과)으로 자동 popup — 대신 *Domain Intent 로 데이터만 변경*(NotificationStore push 등)하고 UI 가 수동 표시
- ✅ debug 의 `debug.popup.*` — *사용자 입력 재현* 한정 ([debug-ipc](../../dev-guide/debug-ipc.md))

**타입 차원 강제**: Core/Domain 핸들러는 `UiIntent`(`OpenPopup`/`ClosePopup`/`TogglePopup`)를 발화하는 메서드를 갖지 않는다 — GUI adapter(단축키 핸들러·메뉴 콜백·popup draw)에서만 발화 가능. `state.popups.open*` 직접 호출도 금지(Intent 경유). 디스패치 상세는 [`design/flows/action-dispatch.md`](../flows/action-dispatch.md).

## 포커스

팝업은 **포커스 상태**를 가진다. 포커스된 팝업이 있으면 키보드 입력이 터미널로 안 간다. 클릭 → 포커스(다른 팝업 언포커스), 바깥 클릭 → 전체 언포커스(터미널 복귀), 닫기 → 자동 언포커스. `PopupManager::has_focused()` 로 확인.

Modal 의 전역 입력 독점과 다르다 — 팝업 포커스는 **키보드만** 차단하고, 마우스는 [입력 계층](../../architecture/input-layer.md)에 따라 팝업이 소비한다.

### sticky_focus

`true` 면 바깥 클릭에도 키보드 포커스가 유지된다(닫기로만 해제). 마우스는 터미널에 정상 전달. 검색 바처럼 *키보드는 항상 자기가 받되 터미널 마우스 조작(스크롤/선택)은 허용* 해야 하는 오버레이용.

### close_on_outside_click

`false`(기본): 바깥 클릭 시 언포커스만(팝업 유지, 예: 알림 패널). `true`: 바깥 클릭 시 닫음(예: surface 타입 전환).

## 스코프

팝업은 소속 범위(`PopupScope`)를 가지며 가시성·경계가 결정된다. enum: `Window` / `Workspace(usize)` / `Pane(u32)` / `Tab(u32, usize)` / `Surface(u32)`.

| 스코프 | 가시성 | 경계 clamp |
|--------|--------|-----------|
| Window | 항상(워크스페이스 전환 무관) | 윈도우 |
| Workspace | 해당 워크스페이스 활성 시 | 워크스페이스 영역 |
| Pane | 해당 pane 보일 때 | pane |
| Tab | 해당 탭이 활성 탭일 때 | 탭 소속 pane |
| Surface | 해당 surface 보일 때 | surface |

`PopupManager::draw()` 가 `LayoutContext` 를 받아 스코프별 가시성 필터 + clamp rect 를 결정한다.

## Modal 과의 차이

| 항목 | Popup | Modal |
|------|-------|-------|
| 입력 차단 | 키보드: 포커스 시 / 마우스: 입력 계층 | 전역 독점 |
| 동시 열기 | 여럿 | 최대 1 |
| 위치 | 자유 이동 | 중앙 고정 |
| 구현 | PopupManager | 별도 OS 윈도우(View) |

## 관련

- [toast.md](toast.md) — 휘발성 알림 (별도 시스템)
- [banner.md](banner.md) — parent 상단 info+action 오버레이 (별도 시스템)
- [dev-guide/popup-implementation](../../dev-guide/popup-implementation.md) — 팝업 추가 절차
- [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md) — Window/Modal/Popup/Toast/Banner 구분
