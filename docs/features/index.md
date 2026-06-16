# 기능 (Features)

각 기능은 폴더 하나다 — `features/<f>/index.md`(기획, 내부 동작 · 1순위) + `screens/<s>.md`(화면, 투영 · 0..N). 모델·작성 규칙은 [documentation-model.md](../documentation-model.md), 작성 전 [identity.md](../identity.md) 필독.

양식: [`_feature.template.md`](_feature.template.md) (기획) · [`_screen.template.md`](_screen.template.md) (화면). 새 기능은 양식을 복사해 채운다.

> **재작성 중** — 검증된 기능만 아래에 등재한다. 옛 명세는 [`docs-old/features/`](../../docs-old/features/) 참고(현재 상태와 다를 수 있음).

## 카탈로그

| 기능 | 주체 | 화면 |
|------|------|------|
| [main-view](main-view/index.md) — MainView (메인 윈도우) | 로컬 사용자 · AI Agent · 원격 | [전체 레이아웃](main-view/screens/main-view.md) |
| [sidebar](sidebar/index.md) — 사이드바 (MainView 좌측 패널) | 로컬 사용자 | [화면](sidebar/screens/sidebar.md) |
| [tools-menu](tools-menu/index.md) — 도구 메뉴 (사이드바 도구 버튼) | 로컬 사용자 | [메뉴](tools-menu/screens/tools-menu.md) |
| [settings](settings/index.md) — 설정 창 (사이드바 설정 버튼) | 로컬 사용자 | [창](settings/screens/settings.md) |
| [plugin-system](plugin-system/index.md) — 플러그인 관리 (사이드바 플러그인 버튼) | 로컬 사용자 · AI Agent | [창](plugin-system/screens/plugins-window.md) |
| [command-palette](command-palette/index.md) — 명령 팔레트 (도구 메뉴 항목) | 로컬 사용자 | [화면](command-palette/screens/command-palette.md) |
| [ssh-tool](ssh-tool/index.md) — SSH 프로필 (도구 메뉴 항목) | 로컬 사용자 · AI Agent | [창](ssh-tool/screens/ssh-tool.md) |
| [listening-ports](listening-ports/index.md) — 리스닝 포트 뷰어 | 로컬 사용자 | [팝업](listening-ports/screens/listening-ports.md) |
