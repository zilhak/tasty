# 접근성(Accessibility) 전략

## 개요

2026년 기준 접근성은 선택이 아닌 필수다.
단, tasty는 터미널 에뮬레이터이므로 일반 GUI 앱과 접근성 범위가 다르다.
터미널 내용 자체는 셸/CLI 앱이 관리한다. tasty는 "터미널 밖" UI의 접근성을 담당한다.

## 키보드 내비게이션

모든 UI 요소(사이드바, 명령 팔레트, 설정, 알림 패널)를 키보드만으로 조작 가능해야 한다.

- Tab/Shift+Tab으로 포커스 이동
- Enter/Space로 활성화
- Escape로 닫기/취소
- 포커스 인디케이터 항상 표시 (egui 기본 지원)

## 고대비 모드

OS 고대비 설정을 감지한다.

- Windows: `SystemParametersInfo(SPI_GETHIGHCONTRAST)`
- macOS: `NSWorkspace.accessibilityDisplayShouldIncreaseContrast`
- Linux: `prefers-contrast` 미디어 쿼리 (GTK 설정)

고대비 활성 시 사이드바/UI 색상을 고대비 팔레트로 전환한다.
최소 대비 비율은 4.5:1 (WCAG AA)을 충족해야 한다.
설정에서 수동 토글도 가능하다.

## 스크린 리더

터미널 에뮬레이터에서 스크린 리더 지원은 매우 어려운 과제다.
현실적으로 단계별 접근을 취한다.

- **Phase 1**: UI 요소(사이드바, 팔레트, 설정)에만 접근성 레이블 제공
- **Phase 2**: 터미널 내용의 접근성 (선택 텍스트 읽기, 알림 텍스트 음성 출력)

OS별 접근성 API:

- Windows: UI Automation (UIA)
- macOS: NSAccessibility
- Linux: AT-SPI2 (D-Bus)

egui는 AccessKit 통합을 지원하여 기본적인 접근성 트리를 제공한다.
AccessKit이 각 OS의 접근성 API로 자동 브릿지한다.

## 폰트 크기/줌

- Ctrl+/- (또는 Alt+/-)로 폰트 크기 조정
- 설정에서 기본 폰트 크기 변경
- 사이드바/UI 폰트 크기도 연동 (또는 독립 설정)
- 최소 8pt, 최대 72pt 범위

## 애니메이션 감소

OS의 "애니메이션 줄이기" 설정을 감지한다.

- Windows: `SystemParametersInfo(SPI_GETCLIENTAREAANIMATION)`
- macOS: `NSWorkspace.accessibilityDisplayShouldReduceMotion`
- Linux: `prefers-reduced-motion`

활성 시 글로우 애니메이션, 전환 효과, 포커스 애니메이션을 비활성화한다.
설정에서 수동 토글도 가능하다.

## 색맹 지원

알림 링/상태 표시에 색상만 사용하지 않고 형태(아이콘/패턴)도 병용한다.

- 작업 중 = 초록 + 스피너
- 대기 = 파랑 + 느낌표
- 에러 = 빨강 + x 아이콘

색맹 시뮬레이션 모드 (Protanopia, Deuteranopia, Tritanopia)를 부가 기능으로 제공한다.

## 구현 우선순위

| 순위 | 항목 | 시기 |
|------|------|------|
| 1 | 키보드 내비게이션 | MVP |
| 2 | 포커스 인디케이터 | MVP |
| 3 | 고대비 모드 | MVP 직후 |
| 4 | 폰트 크기 조정 | MVP |
| 5 | 애니메이션 감소 | MVP 직후 |
| 6 | 색맹 대응 | 이후 |
| 7 | 스크린 리더 (UI) | 이후 |
| 8 | 스크린 리더 (터미널) | 장기 |
