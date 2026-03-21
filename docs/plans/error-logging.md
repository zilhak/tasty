# 에러 처리/로깅 전략

## 에러 처리 전략

### 크레이트 선택

- 라이브러리 레이어: `thiserror` — 구조화된 에러 타입 정의
- 애플리케이션 레이어: `anyhow` — 에러 전파 및 컨텍스트 첨부
- 경계: 라이브러리(termwiz 래퍼, GPU 렌더러 등)는 `thiserror`로 구체적 에러 타입을 정의하고, 앱 최상위는 `anyhow`로 수집한다

### 에러 카테고리

| 카테고리 | 처리 방식 | 예시 |
|---------|----------|------|
| Fatal | 에러 다이얼로그 표시 후 종료 | GPU 초기화 실패, 셰이더 컴파일 실패 |
| Recoverable | 사용자에게 알림, 기능 비활성화 | 시스템 알림 API 실패 → 인앱 알림만 사용 |
| Transient | 자동 재시도 | IPC 연결 끊김, 포트 스캔 실패 |
| Silent | 로그만 남김 | 메타데이터 수집 실패, 폰트 폴백 |

### PTY 에러 처리

- PTY 생성 실패: 사용자에게 에러 메시지를 표시하고 다른 패인은 영향 없이 유지한다
- PTY 종료 (셸 exit): 패인 자동 닫기 또는 "프로세스 종료됨" 메시지 표시 (설정 가능)
- PTY 읽기 에러: 재연결을 시도하고, 실패 시 사이드바에 에러 상태를 표시한다

### GPU 에러 처리

- wgpu 장치 분실 (Device Lost): 자동 재초기화 시도
- 셰이더 컴파일 실패: 폴백 셰이더 사용 (효과 없는 기본 렌더링)
- 텍스처 할당 실패: 글리프 아틀라스 축소 후 재시도

## 로깅 전략

### 크레이트: `tracing`

구조화된 로깅을 위해 `tracing` + `tracing-subscriber`를 사용한다.

- 레벨: ERROR, WARN, INFO, DEBUG, TRACE
- 스팬 기반 컨텍스트 (어떤 워크스페이스, 어떤 패인에서 발생했는지 추적)

### 로그 출력

| 환경 | 출력 대상 | 레벨 |
|------|----------|------|
| 일반 실행 | `~/.local/share/tasty/logs/tasty.log` | WARN+ |
| `TASTY_LOG=debug` | 파일 + stderr | DEBUG+ |
| `TASTY_LOG=trace` | 파일 + stderr | TRACE |

- 로그 로테이션: `tracing-appender`의 일별 로테이션, 최대 7일 보관
- 구조화 필드: timestamp, level, workspace_id, pane_id, component

### 예시

```rust
use tracing::{info, warn, error, instrument};

#[instrument(skip(self), fields(workspace = %self.id))]
fn spawn_shell(&self) -> Result<()> {
    info!("spawning shell");
    let pty = portable_pty::native_pty_system()
        .openpty(size)
        .context("PTY 생성 실패")?;
    // ...
}
```

## 크래시 리포팅

- panic hook 설치: `std::panic::set_hook`으로 panic 정보를 캡처한다
- 크래시 로그 저장: `~/.local/share/tasty/crash/crash-{timestamp}.log`
- 재시작 시 크래시 로그를 감지하여 "이전 세션이 비정상 종료되었습니다" 알림을 표시한다
- 선택적 텔레메트리: Sentry 등은 사용자 동의 시에만 활성화한다 (기본 비활성)

## 디버그 도구

- `tasty --debug`: 디버그 모드 실행 (DEBUG 레벨 로깅)
- `tasty doctor`: 환경 진단 (GPU, PTY, 폰트, IPC 등)
- 내부 디버그 패널: 프레임 타이밍, GPU 메모리, PTY I/O 통계 (개발자 모드에서만)
