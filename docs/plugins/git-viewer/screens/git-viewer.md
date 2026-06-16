# Git Viewer popup 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `design-system/` 의 git viewer popup 디자인(있으면), vendor 예정.

도구 메뉴/IPC 로 뜨는 git status/log/diff 읽기 전용 popup.

## 트리거

[도구 메뉴](../../../features/tools-menu/screens/tools-menu.md) git viewer 항목 또는 IPC.

## UI 요소 인벤토리

- **status** — 변경된 파일 목록.
- **log** — 커밋 히스토리.
- **diff** — 선택 항목의 변경 내용.
- 전부 **읽기 전용**(액션 버튼 없음).

## 상태별 시각

- repo / 비-repo(빈·에러) / detached.

## 시각 소스

popup 치수·섹션 배치는 design-system(vendor 후 링크). popup 구현은 `PopupDef` 시스템(dev-guide).
</content>
