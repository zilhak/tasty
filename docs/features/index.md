# 기능 (Features)

각 기능은 폴더 하나다 — `features/<f>/index.md`(기획, 내부 동작 · 1순위) + `screens/<s>.md`(화면, 투영 · 0..N). 모델·작성 규칙은 [documentation-model.md](../documentation-model.md), 작성 전 [identity.md](../identity.md) 필독.

양식: [`_feature.template.md`](_feature.template.md) (기획) · [`_screen.template.md`](_screen.template.md) (화면). 새 기능은 양식을 복사해 채운다.

> **재작성 중** — 검증된 기능만 아래에 등재한다. 옛 명세는 [`docs-old/features/`](../../docs-old/features/) 참고(현재 상태와 다를 수 있음).

## 카탈로그

| 기능 | 주체 | 화면 |
|------|------|------|
| [main-view](main-view/index.md) — MainView (메인 윈도우) | 로컬 사용자 · AI Agent · 원격 | [전체 레이아웃](main-view/screens/main-view.md) |
| [work-area](work-area/index.md) — 작업 영역 (Workspace/Pane/Tab/Surface 도메인) | 로컬 사용자 · AI Agent · 원격 | [화면](work-area/screens/work-area.md) |
| [workspace-tabs](workspace-tabs/index.md) — 탭 스트립 (Pane 별 탭 바) | 로컬 사용자 | [화면](workspace-tabs/screens/workspace-tabs.md) |
| [window-chrome](window-chrome/index.md) — 윈도우 크롬 (CSD 타이틀바) | 로컬 사용자 | [화면](window-chrome/screens/window-chrome.md) |
| [workspace-status-bar](workspace-status-bar/index.md) — 상태바 (작업영역 하단) | 로컬 사용자 | [화면](workspace-status-bar/screens/workspace-status-bar.md) |
| [sidebar](sidebar/index.md) — 사이드바 (MainView 좌측 패널) | 로컬 사용자 | [화면](sidebar/screens/sidebar.md) |
| [tools-menu](tools-menu/index.md) — 도구 메뉴 (사이드바 도구 버튼) | 로컬 사용자 | [메뉴](tools-menu/screens/tools-menu.md) |
| [settings](settings/index.md) — 설정 창 (사이드바 설정 버튼) | 로컬 사용자 | [창](settings/screens/settings.md) |
| [plugin-system](plugin-system/index.md) — 플러그인 관리 (사이드바 플러그인 버튼) | 로컬 사용자 · AI Agent | [창](plugin-system/screens/plugins-window.md) |
| [command-palette](command-palette/index.md) — 명령 팔레트 (도구 메뉴 항목) | 로컬 사용자 | [화면](command-palette/screens/command-palette.md) |
| [ssh-tool](ssh-tool/index.md) — SSH 프로필 (도구 메뉴 항목) | 로컬 사용자 · AI Agent | [창](ssh-tool/screens/ssh-tool.md) |
| [remote-attach](remote-attach/index.md) — 원격 attach (점유/mirror) | 원격 · AI Agent · 로컬(force-detach) | [GUI mirror](remote-attach/screens/remote-attach.md) |
| [listening-ports](listening-ports/index.md) — 리스닝 포트 뷰어 | 로컬 사용자 | [팝업](listening-ports/screens/listening-ports.md) |
| [keybindings](keybindings/index.md) — 단축키 (KeybindingSettings 도메인) | 로컬 사용자 | [설정 탭](settings/screens/settings.md) |
| [clipboard](clipboard/index.md) — 클립보드 (복사/붙여넣기/선택/히스토리) | 로컬 사용자 | [뷰어 plugin](../plugins/clipboard-history/index.md) |
| [notifications](notifications/index.md) — 알림 (OSC/시스템/패널/배지) | 로컬 사용자 · AI Agent | 패널 popup |
| [auto-update](auto-update/index.md) — 자동 업데이트 확인 (`tasty update`) | 로컬 사용자 | [설정 탭](settings/screens/settings.md) |
| [file-handler](file-handler/index.md) — 파일 핸들러 (식별→디스패치) | 로컬 사용자 · AI Agent · plugin | [설정 탭](settings/screens/settings.md) · picker |
