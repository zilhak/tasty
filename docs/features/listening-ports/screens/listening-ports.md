# 리스닝 포트 팝업 (화면)

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `design-system/ui_kits/terminal/overlays/port_scanner.jsx` (claude design — vendor 예정)

## 트리거

사이드바 하단 **Tools 메뉴** → `Listening ports…` 클릭. Window 스코프 팝업으로 열린다.

## UI 요소 인벤토리

- **팝업 프레임**: 660×520 (디자인 canonical), headless.
- **헤더 행**: leading 포트 아이콘 + 제목 "Listening ports" + accent Tag(`{listening} listening` / `scanning…`) + 단일 라인 검색 입력 + Refresh 아이콘 버튼(상시 노출) + close(`×`).
- **필터 행**: `전체 보기 (system)` 체크박스 — scope 토글 (Tasty ↔ System).
- **테이블 (7컬럼)**: Port / Proto / Address / Process / Workspace / Tab / State.
  - 정렬 가능 헤더(Port/Address/Process/Workspace/Tab) 클릭 시 `▲`/`▼` 인디케이터. Proto/State 헤더는 비정렬.
  - State 셀: 상태 dot(색 + pulse) + 상태 텍스트.
  - 행 클릭: 선택 토글(선택 행 강조). 브라우저 오픈 없음.
- **footer**: `{shown} of {total} ports` 카운터 + `Copy address`(선택 없으면 disabled) + `Close`.

## 상태별 시각

부모 기획의 스캔 상태 → 화면 표현:

- **Loading**: 본문 중앙 `Spinner` + "Scanning…", footer 에 동일 메시지.
- **Ready**: 테이블 렌더.
- **Failed**: 에러 메시지 (재스캔은 헤더 Refresh 버튼으로).
- **빈 결과**: search_zero / system_empty / tasty_empty 각각의 메시지 (부모 기획의 3분기).

## 시각 소스

`design-system/ui_kits/terminal/overlays/port_scanner.jsx` — 팝업 치수·색·dot·레이아웃 수치의 단일 출처. 스크린샷: `design-system/assets/screens/port_scanner-*.png`. (design-system vendor 후 링크 resolve.)
