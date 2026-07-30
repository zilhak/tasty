# Clipboard Viewer popup 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: Claude Design 프로젝트 `Tasty Design System`(projectId `41fd3f5a-4bb9-4877-999f-db5124dc2925`)
  `ui_kits/terminal/overlays/clipboard_viewer.jsx` — 구조 전사 완료(TODO51).

도구 메뉴/단축키로 뜨는 클립보드 뷰어 popup. header → type-bar → body → footer 4단 수직 스택
(좌측 rail master-detail 레이아웃은 폐기).

## 트리거

[도구 메뉴](../../../features/tools-menu/screens/tools-menu.md) `Clipboard Viewer` 또는 플러그인 커맨드 `open_viewer`(설정 > 단축키 > 플러그인, 기본값 `ctrl+shift+h`).

## UI 요소 인벤토리

- **header** — 클립보드 아이콘 + "Clipboard" 타이틀(14px/600) + `snapshot` 뱃지(default tag) + 우측 close IconButton.
- **type-bar** — 좌측 [`type_switch`]: 가용 타입이 1개면 아이콘 + accent 뱃지(읽기전용), 2개 이상이면 가로 세그먼트 버튼 그룹(rail 없음). 5개 이상(`SEG_COMPACT_AT`)이면 비활성 세그먼트가 아이콘 전용으로 압축되고 hover 시 전체 타입명 툴팁이 뜬다. 우측 슬롯은 메타 텍스트 또는 커스텀 위젯(예: HTML 타입의 Pretty print 체크박스)으로 스왑 가능.
- **body** — well(border+radius+bg-app 스크롤 컨테이너) 안에 타입별 콘텐츠. 현재 Text(mono pre) · Files(아이콘+mono 경로 한 줄씩, 긴 경로는 말줄임) — Image/HTML/기타 포맷은 자매 작업이 추가.
- **footer** — mime 텍스트(mono caption, 좌) + Close 버튼(secondary, 우). host 의 outside-click/Esc 와 기능 중복이지만 디자인이 명시적으로 요구.
- (빈 상태) 아이콘 + 굵은 타이틀 + 옅은 부제 2줄.
- (읽기 실패) 위와 동일 구조, danger 톤.
- (이미 열림) 위와 동일 구조, lock 아이콘.

## 상태별 시각

- 타입 있음(header+type-bar+body+footer 4단) / 빈 클립보드 / 읽기 실패 / 이미 열림(재호출 무시) — 후자 3개는 header 는 그대로 유지하고 본문만 CenterState(아이콘+타이틀+부제)로 교체된다.

## 렌더 경로

popup 은 **egui-mesh**(ADR-0028 / B4)로 그린다. plugin 이 자기 프로세스에서 popup 콘텐츠를
egui 로 tessellate 한 mesh 를 host 가 content 영역에 합성한다. host 는 `popup.set_context` 에
Theme 스냅샷(`ThemeWire`)을 실어 보내고, plugin 은 `Theme::with_colors_and_zoom` 으로 재구성해
디자인 토큰대로 그린다. chrome(scrim/border/outside-click/Esc/단일 인스턴스 셸)은 host 소유 —
plugin 은 header~footer content 영역만 그린다(`cbFrame`/`Scrim` 은 design 의 standalone 프리뷰
전용 목업이며 plugin 이 다시 그리지 않는다).

헤더/푸터의 Close 버튼 클릭은 `view::draw`/`draw_already_open` 이 `bool` 로 반환하고,
`main.rs` 가 그 값을 보고 `popup.close` IPC 로 host 에 닫기를 요청한다(host 가 셸 생애주기를
계속 소유 — [popup-implementation.md](../../../dev-guide/popup-implementation.md)).

아이콘은 빌드타임 SVG→벡터 베이크(`build.rs`, `tasty-plugin-image/build.rs` 정본 패턴)로
`tasty_plugin_sdk::baked_icon::draw` 가 그린다.

## 디자인 토큰 매핑

색·폰트·간격은 전부 host 가 보낸 `Theme` 토큰에서 가져온다(from_rgb/raw px 금지). UI 인벤토리 ↔ 토큰:

| UI 요소 | 토큰 | 비고 |
|---|---|---|
| popup 프레임 | `bg-panel` | 480×360 고정(size_hint), plugin content 도 동일 fill |
| header/type-bar/footer 좌우 인셋 | `spacing-md` | design `var(--tasty-size-14)` 근사(Theme 에 14px 전용 토큰 없음) |
| header 타이틀 | `font-size-max`(14) + `text-primary` | `.strong()` |
| snapshot 뱃지 | `tag`(Default variant) | `tasty_ui_widgets::tag` |
| type-bar 행 배경 | `bg-sidebar` | |
| 단일 타입 뱃지 | `tag`(Accent variant) + `text-muted` 아이콘 | |
| 세그먼트(2개 이상) | `border-default` 그룹 보더 + `corner-radius`, active `accent-primary`/`text-on-accent`, idle `text-secondary` | |
| body well | `bg-app` fill + `separator`+`border-width` + `corner-radius` | `ScrollArea` |
| body 미리보기 텍스트 | `font-size-term-sm`(12) mono + `text-primary` | |
| footer mime 텍스트 | `font-size-caption`(11) mono + `text-muted` | |
| footer Close 버튼 | `tasty_ui_widgets::Button`(Secondary) | |
| CenterState 타이틀 | `font-size-body`(13) + `text-secondary`(또는 danger 시 `accent-danger`) | `.strong()` |
| CenterState 부제 | `font-size-term-sm`(12) + `text-muted` | |
| 읽기 실패 톤 | `accent-danger` | |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/clipboard_viewer.rs` — Overlays › `Clipboard viewer
popup`. header/type-bar(배지)/body(well)/footer 4행(text) + header/type-bar(Text/Files 세그먼트)/
body(아이콘+경로 행)/footer 4행(files) + empty/read-failed/already-open 3 CenterState 를 토큰으로
전사(본체/plugin crate 비의존, 픽셀 동일성 비목표). `SEG_COMPACT_AT`(5) 이상의 압축 세그먼트는
실 데이터가 2종(Text/Files)뿐이라 아직 specimen 에 없다 — [[48/49/50]]이 타입을 늘리면 추가한다.
3자 매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#clipboard-viewer-overlays).

## 시각 소스

Claude Design 프로젝트 `Tasty Design System`(projectId `41fd3f5a-4bb9-4877-999f-db5124dc2925`)
`ui_kits/terminal/overlays/clipboard_viewer.jsx`(구조 전사 소스) ·
`clipboard_viewer.html`(standalone 프리뷰) · `shared.jsx`(`Scrim`/`Icon`/`Spinner` 공용
프리미티브). popup 은 egui-mesh 채널로 plugin 이 자가 렌더한다
([popup-implementation.md](../../../dev-guide/popup-implementation.md), ADR-0028).
