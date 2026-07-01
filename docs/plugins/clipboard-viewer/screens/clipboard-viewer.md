# Clipboard Viewer popup 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `design-system/` 의 clipboard viewer popup 디자인(있으면), vendor 예정.

도구 메뉴/단축키로 뜨는 클립보드 뷰어 popup. master-detail 레이아웃.

## 트리거

[도구 메뉴](../../../features/tools-menu/screens/tools-menu.md) `Clipboard Viewer` 또는 토글 단축키(`shortcut.toggle_clipboard_viewer`).

## UI 요소 인벤토리

- **좌측 타입 목록** — 현재 클립보드에 존재하는 타입(예: text)을 Button 으로 나열. 선택된 타입은 primary 강조.
- **우측 미리보기** — 선택된 타입의 내용을 `scroll_v(text_preview)` 로 표시.
- (빈 상태) 클립보드 비어 있음 표시.
- (읽기 실패) read_failed 메시지.

## 상태별 시각

- 타입 있음(선택된 타입 미리보기) / 빈 클립보드 / 읽기 실패 / 이미 열림(재호출 무시).

## 렌더 경로

popup 은 **egui-mesh**(ADR-0028 / B4)로 그린다. plugin 이 자기 프로세스에서 popup 콘텐츠를
egui 로 tessellate 한 mesh 를 host 가 content 영역에 합성한다. host 는 `popup.set_context` 에
Theme 스냅샷(`ThemeWire`)을 실어 보내고, plugin 은 `Theme::with_colors_and_zoom` 으로 재구성해
디자인 토큰대로 그린다. chrome(scrim/border/outside-click/Esc/단일 인스턴스 셸)은 host 소유.

## 디자인 토큰 매핑

색·폰트·간격은 전부 host 가 보낸 `Theme` 토큰에서 가져온다(from_rgb/raw px 금지). UI 인벤토리 ↔ 토큰:

| UI 요소 | 토큰 | 비고 |
|---|---|---|
| popup 프레임 | `bg-panel` | 480×360 고정(size_hint), plugin content 도 동일 fill |
| 좌/우 분할선 | `separator` · `border-width` | ratio 0.3, painter `vline` 1px |
| 타입 버튼(선택) | `accent-primary` fill + `text-on-accent` | `tasty_ui_widgets::Button` primary |
| 타입 버튼(유휴) | `surface-raised` + `border-default` + `text-secondary` | Button secondary(외곽선) |
| 미리보기 본문 | `text-primary` (mono) | `ScrollArea` + selectable Label |
| 빈 클립보드 안내 | `text-muted` | 양축 중앙 한 줄 |
| 읽기 실패 안내 | `accent-danger` | 양축 중앙 한 줄 |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/clipboard_viewer.rs` — Overlays › `Clipboard viewer
popup`. master-detail + empty + read-failed 3 상태를 토큰으로 전사(본체/plugin crate 비의존,
픽셀 동일성 비목표). 3자 매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#clipboard-viewer-overlays).

## 시각 소스

popup 치수(480×360)·좌우 분할(ratio 0.3)은 design-system(vendor 후 링크). popup 은 egui-mesh
채널로 plugin 이 자가 렌더한다([popup-implementation.md](../../../dev-guide/popup-implementation.md), ADR-0028).
