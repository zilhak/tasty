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

## 시각 소스

popup 치수(480×360)·좌우 분할(`splitter` Horizontal 0.3)은 design-system(vendor 후 링크). popup 구현은 `PopupDef` 시스템(dev-guide).
