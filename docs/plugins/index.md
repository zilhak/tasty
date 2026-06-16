# 번들 플러그인 (Bundled plugins)

tasty 에 **동봉되어 첫 부팅 시 자동 install** 되는 공식 플러그인들(배포 축 = bundled, [plugins 개념](../concepts/plugins.md)). 각 플러그인은 외부 플러그인과 동일한 라이프사이클(활성/비활성/제거/권한)을 따른다 — 관리 UI 는 [plugin-system](../features/plugin-system/index.md).

각 플러그인은 폴더 하나다 — `plugins/<id>/index.md`(동작) + `screens/`(UI 가 있으면). 이 영역은 [features/](../features/index.md) 와 구조가 같되, **host 가 아니라 플러그인이 제공**하는 동작이라 분리되어 있다. 양식은 features 템플릿을 그대로 쓴다.

> **재작성 중** — 검증된 항목만 등재한다. 옛 명세는 [`docs-old/`](../../docs-old/) 참고.

## 카탈로그 (`BUILTINS`)

| 플러그인 (id) | 무엇 | 주요 기여 |
|---------------|------|-----------|
| [explorer](explorer/index.md) — `com.tasty.explorer` | 파일 탐색기 | surface_kind(plugin-rendered) · settings_page · commands |
| [markdown](markdown/index.md) — `com.tasty.markdown` | 마크다운 뷰어 | surface_kind(host-rendered) · 파일 핸들러 · cli · settings_page |
| [image](image/index.md) — `com.tasty.image` | 이미지 뷰어 / 그림판 | surface_kind(host-rendered) · 파일 핸들러 · cli |
| [html](html/index.md) — `com.tasty.html` | HTML 뷰어 | surface_kind(webview) · 파일 핸들러 · cli |
| [clipboard-history](clipboard-history/index.md) — `com.tasty.clipboard-history` | 클립보드 히스토리 | 도구 메뉴 · popup |
| [git-viewer](git-viewer/index.md) — `com.tasty.git-viewer` | git status/log/diff 뷰어 | 도구 메뉴 · popup |
| [claude](claude/index.md) — `com.tasty.claude` | Claude Code CLI 통합 | cli · ipc · 멀티에이전트 |
| [codex](codex/index.md) — `com.tasty.codex` | Codex CLI 통합 | cli · ipc · 멀티에이전트 |

## 관련

- 개념·분류 축·권한: [concepts/plugins](../concepts/plugins.md)
- 관리/설치/권한 UI: [features/plugin-system](../features/plugin-system/index.md)
- 도구 메뉴 기여 항목의 진입점: [features/tools-menu](../features/tools-menu/index.md)
</content>
