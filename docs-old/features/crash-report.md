# Crash Report & 진단

- **Status**: Implemented

### Panic Hook (Release + Debug)
- `std::panic::set_hook`으로 커스텀 panic handler 등록
- panic 발생 시 `~/.tasty/crash-reports/crash-YYYY-MM-DDTHH-MM-SS.log` 파일에 자동 저장
- 리포트 내용: 타임스탬프, 버전, OS/아키텍처, panic 메시지 및 위치, 전체 스택트레이스
- stderr에도 동일 내용 출력 (fallback)
- 정상 동작 중 성능 영향 없음

### Debug 전용: 상세 파일 로깅
- debug 빌드에서 `~/.tasty/debug.log`에 모든 tracing 이벤트 기록
- 로그 레벨: `debug` (wgpu 관련은 `warn`)
- 매 실행 시 파일을 초기화하여 무한 증가 방지
- `#[cfg(debug_assertions)]`으로 release 빌드에서 완전히 제거

### Debug 전용: 에러 루프 감지 (ErrorLoopDetector)
- 동일 에러가 1초 내 100회 이상 반복되면 panic을 발생시켜 crash report로 기록
- GPU 렌더 에러, 셸 셋업 에러, 셸 respawn 에러에서 자동 호출
- `record_error()` 글로벌 함수로 호출 (release에서는 no-op)
- `#[cfg(debug_assertions)]`으로 release 빌드에서 완전히 제거
