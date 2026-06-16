# Clipboard History popup 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `design-system/` 의 clipboard history popup 디자인(있으면), vendor 예정.

도구 메뉴/단축키로 뜨는 클립보드 히스토리 popup.

## 트리거

[도구 메뉴](../../../features/tools-menu/screens/tools-menu.md) `Clipboard history` 또는 토글 단축키(`shortcut.toggle_clipboard_viewer`).

## UI 요소 인벤토리

- **항목 목록** — 최근 클립보드 항목들(텍스트 미리보기). 각 항목 클릭 → 다시 복사.
- (빈 상태) 항목 없음 표시.

## 상태별 시각

- 항목 있음 / 빈 목록.

## 시각 소스

popup 치수·행 배치는 design-system(vendor 후 링크). popup 구현은 `PopupDef` 시스템(dev-guide).
</content>
