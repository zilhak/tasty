# 내부 팝업 시스템

Popup 은 View 내부에 존재하는 가상 창이다 — 터미널과 공존하며 포커스를 독점하지 않는다. 모든 내부 팝업은 **`PopupManager` + `PopupDef`** 로 관리된다(`src/adapters/ui/popup.rs`). 용어 구분은 [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md), 팝업을 *추가하는 법* 은 [dev-guide/popup-implementation](../../dev-guide/popup-implementation.md). 이 문서는 시스템 *동작 모델* 이다.

## 8대 규칙

1. **타이틀 + 콘텐츠** — 상단 타이틀 영역(높이 토큰 고정) + 하단 콘텐츠.
2. **타이틀바** — 제목 중앙 정렬 + 우측 버튼군. 버튼은 항상 X(닫기, 호버 시 빨강)이고, **전체화면 무대를 선언한 popup**(`PopupDef.fullscreen_stage`)에만 그 왼쪽에 전체화면 버튼(디자인 `fit` 글리프, 호버 시 tooltip)이 하나 더 붙는다 — 선언하지 않은 popup 의 타이틀바는 X 위치까지 그대로다. 제목이 버튼군과 겹칠 만큼 길면 **버튼군 좌변** 기준 가용 폭으로 말줄임(`…`) 처리한다(상세 → [popup-implementation §타이틀 길이 처리](../../dev-guide/popup-implementation.md#타이틀-길이-처리-elide)). 전체화면 버튼을 누르면 [무대](fullscreen-stage.md)가 뜨고 **원본 popup 은 열린 채 남는다** — 무대에 올라가는 것은 이 popup 인스턴스가 아니라 같은 형상의 별개 콘텐츠다.
3. **드래그 이동** — 팝업이 선언한 **드래그 핸들**(`drag_handle`) 영역을 드래그로 팝업 전체 이동. 타이틀바 팝업은 타이틀 영역이 핸들(기본). 타이틀바 없는 headless 패널 팝업(`port_scanner`·`remote_tool`)은 뷰가 렌더 시점의 **실측 헤더 rect(전체폭 × 실제 높이)** 를 매니저에 보고해 헤더 영역 전체를 이동 핸들로 쓴다(정적 `DragHandle::Region` 띠는 open 첫 프레임 폴백). 헤더 텍스트 라벨은 비선택(`selectable_labels=false`)이라 글자 위에서도 드래그가 텍스트 선택으로 새지 않는다. **위젯 우선 중재**(`is_using_pointer`)로 핸들이 위젯(검색 입력·버튼)과 겹쳐도 위젯 클릭이 항상 우선되어 충돌하지 않는다(8번 입력 우선순위 참조).
4. **커서** — 드래그 핸들 위에서 grab 커서. 리사이즈 가능 팝업의 테두리에서는 엣지별 리사이즈 커서.
5. **배경 구분** — 팝업 배경은 `surface0`, 타이틀바는 `mantle` — 터미널 focused(검정)/unfocused 배경과 달라 위에 떠 있음이 시각적으로 구분된다.
6. **경계 제한** — 팝업의 어떤 부분도 스코프 밖으로 못 나간다. 리사이즈 시 자동 재배치.
7. **다중 + z-order** — 여러 개 동시 가능. 나중에 열리거나 클릭된 것이 앞. 겹친 영역의 마우스 이벤트는 최상단 팝업만 받는다(판정: [§Host ↔ Plugin popup z-order](#host--plugin-popup-z-order) 의 "마우스 소유권").
8. **리사이즈 + 입력 우선순위** — `resizable` 팝업은 테두리 8방향 드래그로 크기 조절(`min_size` 하한 + 스코프 경계 클램프). 입력 우선순위는 **(egui)위젯 > close·전체화면 버튼 > 리사이즈 엣지 > 드래그 핸들 > 콘텐츠** — 두 타이틀바 버튼은 매니저가 직접 페인팅한 같은 층이고, 둘 다 드래그 핸들(타이틀바)과 겹치므로 핸들보다 먼저 판정해야 버튼을 눌러 끌어도 팝업이 따라오지 않는다. 이동/리사이즈 START 는 콘텐츠 렌더 뒤 `is_using_pointer()` 게이트로 판정해, 위젯이 프레스를 가져간 프레임에는 발동하지 않는다(이동이 사실상 최후순위). 사용자가 리사이즈한 뒤에는 `sizer` 가 크기를 되돌리지 않는다(close 시 리셋).

(모든 색·치수는 Theme 토큰 — [theme.md](theme.md).)

## 구조

- **`PopupDef`** — 정적·데이터 지향 정의(id, title_key/title_fn, default_size/sizer, default_scope, close_on_outside_click, headless, sticky_focus, drag_handle, resizable, min_size, fullscreen_stage, draw_fn). 전부 `src/adapters/ui/popup/defs.rs::all_defs()` 에 모은다. 필드 상세 → [popup-implementation](../../dev-guide/popup-implementation.md).
- **`PopupManager`** — 공통 동작(z-order, 드래그, 리사이즈, 타이틀바, clamp, 포커스) 중앙 관리. `register_def` / `open*` / `close` / `toggle` / `draw`.
- **`PopupState`** — 개별 인스턴스 상태(id, title, pos, size, open, focused, scope, dragging/resizing, size_user_overridden).
- **범용 render 루프** — `src/adapters/ui/popup/frame.rs::draw_popup_layer`(등록된 모든 `PopupDef` 순회 + close 경로별 `on_close` 훅 drain). toast/banner/modifier-hint/tutorial 오버레이 체인은 개념이 다르므로(ADR-0024) `src/adapters/ui/overlay.rs` 로 분리돼 있다. 진입점 `src/adapters/ui.rs::draw_popups` 가 둘을 z-order 순서로 호출한다.

등록된 팝업은 `all_defs()` 가 단일 출처다(예: `notifications`, `convert_surface`, `rename`, `search_bar`, `tools_menu` …). 새 팝업 = 테이블 항목 1개 + draw 함수 하나. 단, plugin 이 소유하는 팝업(예: markdown 파일열기·large-file 확인)은 host `PopupDef` 가 아니라 plugin 매니페스트 `[[contributes.popup]]`(egui-mesh) 로 등록된다.

## Host ↔ Plugin popup z-order

Plugin 이 egui-mesh 로 그리는 popup(예: markdown 파일열기, `src/plugin_bridge/popup_render.rs`)은 host `PopupManager` 소속이 아니지만 규칙 7 의 "나중에 열리거나 클릭된 것이 앞" 을 host popup 과 함께 지킨다. 양쪽 모두 자신의 open/click 시점에 공유 전역 시퀀스(`tasty_host_plugin::next_popup_z_seq()`)에서 번호를 받아 기록하고(host: `PopupState.z_seq`, plugin: `PopupInstance.z_seq`), 매 프레임 두 진영의 열린 popup 중 최댓값끼리 비교해 이긴 쪽을 위로 강제한다(`src/gfx/gpu/egui_bridge.rs::host_popup_should_render_on_top`).

- **Shell(배경/테두리/제목)**: 둘 다 raw `ctx.layer_painter()` 로 그려 `Areas::order` 에 자연 등록되지 않는다([input-layer.md (c)](../../architecture/input-layer.md) 참고) — `enforce_host_plugin_popup_z_order` 가 진 쪽을 이긴 쪽의 `ctx.set_sublayer()` 자식으로 강제 편입해 순서를 고정한다.
- **Content(plugin 전용 GPU mesh)**: plugin popup 콘텐츠는 별도 `wgpu::Renderer` pass(`render_egui_mesh_popups`)로 host egui pass(`render_egui_pass`)와 독립 합성된다 — `render_egui_pass_and_mesh_popups`(`src/gfx/gpu/render_pass.rs`)가 같은 승패 결과로 두 pass 의 호출 순서를 뒤바꾼다. plugin popup 은 콘텐츠가 shell 보다 먼저 그려지는 경우에도 자기 shell 이 자기 콘텐츠를 덮지 않도록, shell 배경을 콘텐츠 영역을 제외한 4분할 사각형으로 그린다(`paint_shell_background_excluding_content`).
- **마우스 소유권(규칙 7 후반)**: "겹친 영역의 마우스 이벤트는 최상단 팝업만 받는다" 는 렌더 순서와 별개로 `src/adapters/ui/popup/occlusion.rs::point_ownership` 가 판정한다. 한 좌표에 대해 `Mine`(내 rect 안 + 위에 아무도 없음) / `OccludedByHigher`(나보다 z 가 높은 popup 이 덮음) / `OutsideAll`(어떤 popup 에도 안 속함) 3-상태를 내고, host/plugin 양쪽이 같은 함수를 쓴다 — click-to-front 는 `Mine` 일 때만, outside-click dismiss 는 `OutsideAll` 일 때만 일어난다. 그래서 위에 열린 popup 안을 클릭해도 아래 popup 이 "바깥 클릭" 으로 닫히거나 앞으로 튀어나오지 않는다. host popup 은 hover/close 버튼/리사이즈/드래그 히트테스트 전체가, plugin popup 은 focus-bump·dismiss·포인터 이벤트 forward 가 이 판정을 거친다. 아래 2-그룹 제약과 달리 이 판정은 **popup 쌍마다 z_seq 를 직접 비교**하므로 host↔plugin·plugin↔plugin 을 모두 정확히 가른다.
- **판정 재료의 방향별 신선도**: plugin 판정이 보는 host rect 는 같은 프레임 값이다(`draw_popups` 가 `draw_plugin_popups` 보다 먼저 돈다). 반대로 host 판정이 보는 plugin 셸 rect(`AppState.plugin_popup_hittest`)는 **1 프레임 stale** 이다 — 방금 닫힌 plugin popup 이 바깥 클릭 한 번을 더 삼킬 수 있지만, 반대 방향 오판(가려진 popup 이 잘못 닫히는 것)보다 회복이 쉬운 쪽을 택한 결과다.
- **범위**: host popup 묶음 대 plugin popup 묶음의 2-그룹 비교만 지원한다. plugin popup 이 여러 개 열려 있을 때 그들끼리의 상대 순서는 z_seq 로 정렬돼 콘텐츠(GPU mesh push 순서)에는 반영되지만, shell 레이어끼리는 `set_sublayer` 의 1단 들여쓰기 제약(아래 [input-layer.md (d)](../../architecture/input-layer.md)) 때문에 서로 엮이지 않는다 — host 묶음과의 상대 위치만 보정 대상이다.

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

`Workspace` 스코프의 clamp 는 실제로는 윈도우 전체다 — 워크스페이스가 윈도우를 통째로
차지하므로 둘이 같은 사각형이다. 따라서 이 스코프가 실질적으로 결정하는 것은 **가시성**이다.

**스코프는 `PopupDef` 에 못 박히지 않는다.** `Workspace(usize)` / `Pane(u32)` 처럼 대상을
런타임에야 아는 스코프는 `default_scope` 에 안전한 기본만 두고, 여는 쪽이
`OpenPopupMode::WithScope(scope)` 로 실제 값을 주입한다. 현재 소비자:

| 팝업 | 스코프 | 여는 쪽이 주입하는 값 |
|------|--------|----------------------|
| [DAG 목록](../../features/agent-collaboration/screens/dag-list-popup.md) (`dag_list`) | Workspace | 여는 시점의 활성 workspace 인덱스 |
| 변환(`convert_surface`) · 검색바(`search_bar`) | Surface | 포커스 surface id |

`Workspace` 스코프 팝업은 워크스페이스를 옮기면 **그리지 않는다** — 상태는 그대로 남아 있어
돌아오면 보던 화면이 그대로 복원된다. 그 상태의 수명은 스코프가 아니라 `on_close` 가 정한다.

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
- [fullscreen-stage.md](fullscreen-stage.md) — 타이틀바 전체화면 버튼이 여는 창 전체 무대 (별도 시스템)
- [dev-guide/popup-implementation](../../dev-guide/popup-implementation.md) — 팝업 추가 절차
- [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md) — Window/Modal/Popup/Toast/Banner 구분
