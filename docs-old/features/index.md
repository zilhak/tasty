# Feature Spec 인덱스

기능 단위 *행동 명세 + Acceptance Criteria* 문서. 신규 작성은 [`template.md`](template.md) 양식.

> 옛 `docs/features.md` 단일 파일을 H2 섹션마다 개별 파일로 분할했다. 분할 파일은 본문(현재 상태 상세)을 그대로 보존하며 Status 만 부여한다. 시드 3 개(아래 "행동 명세" 표)에는 Acceptance Criteria 까지 작성되어 있고, 나머지 분할 파일의 Acceptance Criteria 는 후속 작업으로 채운다.

## 행동 명세 (Acceptance Criteria 포함)

| 문서 | 설명 |
|------|------|
| [closed-tab-restore.md](closed-tab-restore.md) | 닫힌 항목 복원 (Ctrl+Shift+T) — *사용자가 닫은 것만* 복원 (사용자/에이전트 분리) |
| [convert-surface.md](convert-surface.md) | Surface 타입 전환 (Alt+') — SurfaceKindRegistry 동적 enumerate 팝업 |
| [terminal-link-click.md](terminal-link-click.md) | 터미널 내 링크 hover·클릭 오픈 — 수식키+클릭, CLI/IPC 비노출 |

## 카테고리별 분할 파일 (현재 상태 상세)

| 카테고리 | 문서 |
|----------|------|
| 터미널 엔진 | [terminal-engine.md](terminal-engine.md) |
| 윈도우 크롬 (CSD 타이틀바) | [window-chrome.md](window-chrome.md) |
| 워크스페이스 & 탭 | [workspace-tabs.md](workspace-tabs.md) |
| 작업영역 StatusBar | [workspace-status-bar.md](workspace-status-bar.md) |
| 레이아웃 프리셋 | [layout-presets.md](layout-presets.md) |
| 알림 시스템 | [notifications.md](notifications.md) |
| 휴먼 핸드오프 (Approval) | [human-handoff.md](human-handoff.md) |
| 에이전트 텔레메트리 | [telemetry.md](telemetry.md) |
| 협업 primitive | [collaboration-primitives.md](collaboration-primitives.md) |
| 공유 컨텍스트 | [shared-context.md](shared-context.md) |
| 설정 시스템 | [settings.md](settings.md) |
| 클립보드 | [clipboard.md](clipboard.md) |
| CLI 도구 & 소켓 API | [cli-and-socket-api.md](cli-and-socket-api.md) |
| 에이전트 자동화 | [agent-automation.md](agent-automation.md) |
| Crash Report & 진단 | [crash-report.md](crash-report.md) |
| 단위 테스트 | [unit-tests.md](unit-tests.md) |
| 국제화 (i18n) | [i18n.md](i18n.md) |
| 이미지 뷰어 & 그림판 | [image-viewer.md](image-viewer.md) |
| 터미널 검색 | [terminal-search.md](terminal-search.md) |
| 레이아웃 영속화 | [layout-persistence.md](layout-persistence.md) |
| Plugin 시스템 | [plugin-system.md](plugin-system.md) |
| 파일 핸들러 시스템 | [file-handler-system.md](file-handler-system.md) |
| Lua Hooks | [lua-hooks.md](lua-hooks.md) |
| 리스닝 포트 뷰어 | [listening-ports.md](listening-ports.md) |
| 명령 팔레트 | [command-palette.md](command-palette.md) |
| 자동 업데이트 확인 | [auto-update.md](auto-update.md) |
| 접근성 (Accessibility) | [accessibility.md](accessibility.md) |
| Git 뷰어 | [git-viewer.md](git-viewer.md) |
