# Tasty 가이드

Tasty는 나와 AI 에이전트가 함께 일하는 GPU 가속 터미널입니다. Windows, macOS, Linux에서 프로젝트별로 워크스페이스를 나누고, 여러 터미널을 나란히 열어 작업할 수 있습니다. 에이전트도 `tasty` CLI로 터미널을 만들고 명령을 실행하며 결과를 확인합니다.

처음이라면 설치와 첫 화면 둘러보기부터 시작하세요. 이미 사용 중이라면 아래에서 필요한 기능을 골라 살펴보세요.

## 시작하기

- [설치](getting-started/install.md) — OS 별 설치 파일, 설치 절차, 첫 실행, 업데이트와 제거.
- [첫 화면 둘러보기](getting-started/first-look.md) — 사이드바 · 워크스페이스 · 페인 · 탭 · 분할 · 상태바가 각각 무엇인지.

## 사용하기

- [워크스페이스](using/workspaces.md) — 만들기, 이름 붙이기, 카테고리로 묶기, 전환, 닫기, 복원.
- [페인 · 탭 · 분할](using/panes-tabs-splits.md) — 화면 나누기, 옮기기, 종류 바꾸기, 전체화면, 레이아웃 저장.
- [터미널 다루기](using/terminal.md) — 복사/붙여넣기, 검색, 링크 열기, 스크롤, 마우스 캡처, 셸 통합.
- [파일 열기](using/files.md) — 탐색기 · 마크다운 · 이미지 · HTML · git 보기 등 터미널이 아닌 화면.

## 내 취향대로

- [단축키](customize/keybindings.md) — 기본 단축키 표, 프리셋, 바꾸는 법.
- [설정](customize/settings.md) — 설정 윈도우와 `~/.tasty/config.toml` 의 주요 항목.
- [테마](customize/themes.md) — 번들 테마, 전환, 직접 만들기.
- [Lua 스크립트](customize/scripts.md) — 스크립트 등록, 단축키와 이벤트로 자동 실행.

## AI 에이전트와 함께

- [tasty CLI 로 터미널 조작하기](agents/cli.md) — `list` / `send` / `read` / `mark` / `notify` 기본 패턴.
- [Claude · Codex 와 함께 쓰기](agents/claude-codex.md) — 훅 설치, 자식 인스턴스 spawn, tell, 완료 알림.
- [작업 순서 관리](agents/tasks.md) — 할 일을 의존 관계로 묶어 순서대로 실행하고 진행을 그래프로 보기.
- [훅 · 알림 · 웹훅](agents/hooks-notifications.md) — 서피스 훅, 글로벌 훅, 알림, 외부 HTTP 트리거.

## 원격 · 플러그인

- [원격 작업 이어가기](remote/attach.md) — 프로필과 SSH 로 다른 컴퓨터의 Tasty 를 내 화면에 비추기.
- [플러그인](plugins/index.md) — 설치, 권한, 번들 플러그인 소개.

## 도움말

- [문제 해결](help/troubleshooting.md) — macOS 권한, 로그 파일 위치, 포트 파일, 자주 묻는 것.
