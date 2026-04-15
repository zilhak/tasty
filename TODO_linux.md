# Linux: New Window 접근성 개선 TODO

## 1. 마지막 윈도우 닫기 동작

Windows와 동일. `close_behavior: "minimize"` 일 때 마지막 윈도우를 최소화하여 태스크바에 유지.

- Linux도 윈도우 없이 백그라운드에 남으면 사용자가 복구할 방법이 없음
- 최소화로 태스크바/독에 유지 → 클릭으로 복원

## 2. .desktop 파일에 Actions 추가

앱 아이콘 우클릭 시 "New Window" 표시 (GNOME, KDE 모두 지원).

```ini
[Desktop Action new-window]
Name=New Window
Exec=tasty new window
```

- 코드 변경 없이 패키징 시 `.desktop` 파일에 추가하면 됨
- `tasty new window` CLI가 IPC로 `window.create`를 호출하여 기존 인스턴스에 새 윈도우 생성

## 3. System Tray 아이콘 (선택사항)

Windows와 동일. 백그라운드 실행 시 시스템 트레이에 아이콘 표시.

- DE마다 트레이 지원이 다름 (GNOME은 확장 필요, KDE는 네이티브)
- 우선순위 낮음
