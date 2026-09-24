# 환경 노트 (에이전트 부트스트랩)

AI 에이전트가 tasty 를 조작하기 전 알아야 할 OS별 경로·실행 확인·접속 패턴. 메서드 표는 [api.md](api.md).

## 경로 (모든 OS)

| 파일 | 경로 | 설명 |
|------|------|------|
| 포트 파일 | `~/.tasty/tasty.port` | IPC 동적 포트. 실행 시 생성 |
| 설정 | `~/.tasty/config.toml` | 사용자 설정 |

(Windows 는 `~` = `%USERPROFILE%`. debug 빌드는 루트 자체가 분리되어 `~/.tasty-debug/` 아래에 같은 파일들을 둔다 — 격리 상세 [dev-guide/self-verification 독립 검증](../dev-guide/self-verification.md#독립-검증--개발도-agent-가-스스로-확인할-수-있어야-한다).)

## 실행 여부 확인

`tasty list info`의 IPC 응답으로 대상 인스턴스가 요청을 처리하는지 확인한다.
프로세스 이름 검색이나 포트 파일의 존재만으로 준비가 끝났다고 판단하지 않는다.
조회 실패만으로 포트 파일을 삭제하지 않는다.

## 실행 / 대기 / 종료

검증에는 전용 `TASTY_HOME`을 가진 인스턴스를 직접 시작하고 그 PID를 기록한다.
준비 대기는 재시도 횟수나 전체 기한을 제한하고 PID 생존도 확인하며, 종료도 그 PID에만 요청한다.
구체적인 실행 절차는 [격리 인스턴스 검증](../dev-guide/self-verification.md#tasty-에서-직접-검증)을 따른다.
사용 중인 인스턴스를 이름이나 명령줄 패턴으로 찾아 종료하지 않는다.

## IPC 직접 (Python)

[API 접속 예제](api.md#접속)를 사용한다. 요청과 응답은 개행으로 구분하며 TCP의 한 번의
`recv`가 JSON 하나를 반환한다고 가정하지 않는다. debug 또는 격리 인스턴스에 연결할 때는
예제를 실행하는 프로세스에도 같은 `TASTY_HOME`을 지정한다.

## 스크린샷

`tasty screenshot --path <png>` (IPC `ui.screenshot {path}`) 는 **GUI 모드**에서 동작하는 정식
release 기능이다(focus 독립 — `--surface <id>` 로 터미널 surface 를 오프스크린 캡처하거나
`--window <id>` 로 창 프레임 캡처). 결과 PNG. 상세 [screenshot-methods](../ai-verification/screenshot-methods.md).

## 관련

- [api.md](api.md) — 주요 IPC/CLI · [dev-guide/self-verification](../dev-guide/self-verification.md) — 검증 시나리오
