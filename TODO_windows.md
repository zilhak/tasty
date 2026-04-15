# Windows: New Window 접근성 개선 TODO

## 1. ~~마지막 윈도우 닫기 동작~~ (구현 완료)

`set_minimized(true)`로 태스크바에 유지하도록 구현됨. Windows/Linux에서는 윈도우를 파괴하지 않고 최소화.

## 2. 태스크바 Jump List에 "New Window" 추가

Windows 태스크바 아이콘 우클릭 시 "New Window" 항목 표시.

- `ICustomDestinationList` COM API 사용
- 클릭 시 `tasty new window` CLI 명령 실행 (이미 IPC로 `window.create` 지원됨)
- 별도 COM 코드 필요, 작업량 있음

## 3. System Tray 아이콘 (선택사항)

백그라운드 실행 시 시스템 트레이에 아이콘 표시.

- "Show Window", "New Window", "Quit" 메뉴 제공
- `tray-icon` 또는 유사 크레이트 필요
- Jump List만으로 충분할 수 있으므로 우선순위 낮음
