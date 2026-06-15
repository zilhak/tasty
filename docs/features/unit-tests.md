# 단위 테스트

- **Status**: Implemented

각 모듈에 `#[cfg(test)] mod tests` 블록으로 인라인 단위 테스트를 포함한다.

### tasty-terminal 테스트
- DECSET/DECRST 모드 토글: 애플리케이션 커서 키(모드 1), 커서 가시성(모드 25), 브래킷 붙여넣기(모드 2004), 마우스 트래킹(모드 1000/1003)
- 대체 화면 전환: 모드 1049 진입/퇴장, 모드 47 진입/퇴장, 대체 화면 리사이즈
- 방향키 모드 전환: 일반/애플리케이션 커서 키 모드 확인
- 전체 리셋(RIS): 모든 모드가 기본값으로 복원

### model.rs 테스트
- `Rect::contains`: 내부/외부/경계 포인트 판정
- `Rect::split`: 수직/수평/불균등 비율 분할
- `Rect::approx_eq`: 근사 비교 (1px 허용)
- `PaneNode::compute_rects`: 단일 및 분할 레이아웃
- `PaneNode::find_pane`: ID 기반 탐색
- `PaneNode::all_pane_ids`: 순서 보장 ID 수집
- `PaneNode::next_pane_id` / `prev_pane_id`: 순환 포커스 이동
- `AppState::move_focus_forward` / `move_focus_backward`: 탭 내부 Surface 우선 이동, 단일이면 Pane 간 이동
- `PaneNode::find_divider_at`: 분할 경계선 히트 테스트
- `PaneNode::split_pane_in_place`: 트리 내부 분할 (성공/실패 케이스)
- `PaneNode::close_pane`: 단일 리프 닫기 실패, 분할에서 형제 승격, 중첩 분할에서 닫기, 미발견 대상
- `Pane::close_tab`: 탭 닫기 성공, 마지막 탭 닫기 실패

### notification.rs 테스트
- 알림 추가 및 개수 확인
- 개별 및 전체 읽음 처리
- 워크스페이스별 필터 카운트
- 동일 소스 병합(coalescing)
- 다른 소스 비병합
- FIFO 최대 100개 제한

### tasty-tui-simulator (TUI 시뮬레이터)
고수준 명령을 raw VTE escape sequence로 변환하여 출력하는 VTE 시뮬레이터. 터미널 입장에서 실제 TUI 앱과 동일한 바이트 스트림을 받는다.
- **인터랙티브 모드**: stdin REPL — 외부에서 `surface.send`로 명령을 단계별로 전송. 명령마다 `OK` 응답으로 동기화
- 명령어: cursor, print, sgr, fg/bg, bold/italic/underline, altscreen, scroll-region, erase, raw, esc 등
- 종료 제어: `quit`(정상), `exit-code N`(코드 지정), `crash`(SIGABRT), `panic`(Rust panic)
- 원샷 시나리오: cursor, colors, attrs, altscreen, unicode, scroll-region (수동 확인용)
- `debug.cell_info` / `debug.screen_attrs` IPC와 조합하여 셀 속성 자동 검증 가능

### tasty-hooks 테스트
- `HookEvent::parse` 전체 이벤트 타입
- 디스플레이 문자열 라운드트립
- 이벤트 매칭 (같은 타입, 다른 타입, 정규식)
- HookManager: 등록, 삭제, 조회
- once 훅 실행 후 자동 삭제
- persistent 훅 실행 후 유지

### settings.rs 테스트
- 기본 설정 유효성
- TOML 직렬화/역직렬화 라운드트립
- 부분 TOML 기본값 폴백
- 빈 TOML 전체 기본값

### model.rs Visitor 패턴 테스트
- for_each_terminal: 단일 Pane 순회, 분할된 Pane 순회
- for_each_terminal_mut: mutable 접근 및 수정
- compute_terminal_rect: 기본 계산, 스케일 팩터, 사이드바 클램핑, 사이드바 없음

### ipc/protocol.rs 테스트
- 요청 직렬화/역직렬화
- 성공/에러 응답 생성
- method_not_found 응답
- 응답 라운드트립
