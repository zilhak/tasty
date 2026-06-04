# Plugin Categories

Tasty 의 viewer / plugin 은 **3 카테고리** 로 분류된다. 본 문서는 각 카테고리의 정의·차이·결정 기준과 함께, 다른 docs 가 사용하는 "builtin / built-in / 기본 제공 plugin / BUILTINS" 같은 표현이 어느 카테고리를 가리키는지 정리한다.

## 카테고리 한눈에

| 카테고리 | 한글 | 영문 | 위치 | plugin 목록 표시 | 설치 | 사용자 교체 |
|----------|------|------|------|-------------------|------|-------------|
| 1 | 기본 내장 | host-native | host 코드 (본 바이너리) | ✗ | tasty binary 자체 | 불가 |
| 2 | 기본 plugin | bundled plugin | `~/.tasty/plugins/<id>/` | ✓ | `BUILTINS` 자동 install | ✓ (disable / remove) |
| 3 | 사용자 plugin | user plugin | `~/.tasty/plugins/<id>/` | ✓ | `tasty plugin install <path>` | ✓ |

## 용어 매핑 (기존 docs 와의 호환)

기존 docs 의 다음 표현은 *모두 카테고리 2 (기본 plugin)* 를 가리킨다. 카테고리 1 (host-native) 과 같은 영어 단어 ("builtin") 가 쓰여 있지만 의미는 다르다.

| 기존 표현 / 심볼 | 등장 위치 | 가리키는 카테고리 |
|-------------------|----------|-------------------|
| "builtin" / "built-in" / "기본 제공 plugin" | `docs/features.md`, `docs/dev-guide/plugin-ecosystem.md`, `docs/dev-guide/plugin-development.md` | 2 (기본 plugin) — `BUILTINS` 묶음 |
| `BUILTINS` const | `crates/tasty-host-plugin/src/builtin.rs:33` (windows) / `:93` (non-windows) | 2 의 *전체 목록* |
| `is_builtin_plugin(id)` | `crates/tasty-host-plugin/src/builtin.rs:152` | 2 판정 함수 |
| `removed_builtins` (in `plugins.toml`) | `docs/features.md`, `docs/dev-guide/plugin-ecosystem.md` §6.3 | 2 의 disable 트래킹 |
| `plugin.upgrade_builtins` IPC, `tasty plugin upgrade-builtins` CLI | `docs/agent-guide/plugins.md`, `docs/dev-guide/plugin-ecosystem.md` §6 | 2 묶음 운용 API |
| `builtin:<name>` (tool menu key prefix) | `docs/agent-guide/plugins.md` § tool menu | **별개 의미** — tool menu 키스페이스 (현재 등록 항목 0 개). 카테고리 1/2 와 무관한 *내부 정렬용 키* |

호환을 위해 본 문서는 *기존 본문 표현을 바꾸지 않는다*. 본 문서의 매핑 표를 통해 "builtin" 이라는 단어가 어느 의미인지 1 곳에서 판별 가능.

## 1. 기본 내장 (host-native)

본 바이너리 안에 코드로 박혀 있는 viewer / 기능. plugin 메커니즘 자체를 거치지 않는다.

- **위치**: host 코드 (본 바이너리 `src/` 또는 host 도메인 crate)
- **plugin 목록 표시**: ✗ — `tasty plugin list` 에 등장하지 않음
- **설치**: tasty binary 일부 — 사용자 작업 0
- **교체 / 제거**: 불가 (코드 수정 필요)
- **현재 등록 항목**: 0 개 (모든 viewer 가 카테고리 2 로 이전됨 — `docs/agent-guide/plugins.md` § tool menu 가 "현재 등록된 빌트인 항목 없음" 으로 명시)
- **결정 기준**: tasty 의 *기본 UX* — 사용자가 plugin 인지 모르고 써야 하며, 사용자가 교체할 여지를 *원천적으로 두지 않는* 기능. plugin 추상화 비용을 들이지 않을 만큼 host 와 강결합인 경우.

## 2. 기본 plugin (bundled plugin)

Tasty 바이너리에 *동봉되어 배포* 되는 plugin. 첫 부팅 시 `~/.tasty/plugins/<id>/` 로 자동 install 되며, 이후엔 외부 plugin 과 동일한 라이프사이클 (활성/비활성/제거/권한 grant 등) 을 따른다.

- **위치**: `~/.tasty/plugins/<id>/` (host-owned 디렉토리)
- **plugin 목록 표시**: ✓
- **설치**: 부팅 시 `install_builtins_if_needed` 가 `BUILTINS` 목록을 자동 install. semver 기반 자동 upgrade (`docs/dev-guide/plugin-ecosystem.md` §6)
- **교체 / 제거**: ✓ 사용자가 disable / remove 가능. 제거 시 `removed_builtins` 에 박혀 자동 재설치되지 않음
- **현재 등록 항목** (`crates/tasty-host-plugin/src/builtin.rs:33,93`):
  - `com.tasty.explorer` (파일 탐색기)
  - `com.tasty.markdown` (마크다운 viewer)
  - `com.tasty.html` (HTML viewer)
  - `com.tasty.image` (이미지 viewer / 그림판)
  - `com.tasty.claude` (Claude Code 통합)
  - `com.tasty.codex` (Codex CLI 통합)
  - `com.tasty.clipboard-history` (클립보드 히스토리)
  - `com.tasty.git-viewer` (git diff / log viewer)
- **결정 기준**: tasty 가 *기본 제공* 하지만 사용자가 교체·비활성화할 여지를 남기는 기능. plugin 추상화로 host 와 분리 가능한 경우. 외부 ecosystem 의 reference 구현 역할도 겸함.

자동 upgrade / `removed_builtins` / `restart_running` 등 운용 절차 상세: [`docs/dev-guide/plugin-ecosystem.md` §6](../dev-guide/plugin-ecosystem.md).

## 3. 사용자 plugin (user plugin)

사용자가 직접 install 한 외부 plugin. 동일 디렉토리 (`~/.tasty/plugins/<id>/`) 에 살지만 host 가 *자동 install 대상* 으로 인지하지 않는다.

- **위치**: `~/.tasty/plugins/<id>/`
- **plugin 목록 표시**: ✓
- **설치**: `tasty plugin install <path>` (CLI / Plugin 관리 모달의 "Add plugin" 탭)
- **교체 / 제거**: ✓
- **예시**: 외부 ecosystem 의 plugin. 사용자가 import 한 임의의 매니페스트 + 바이너리.
- **결정 기준**: tasty 핵심과 분리된 *외부 ecosystem*. host 가 라이프사이클을 책임지지 않음.

## 신규 추가 결정 가이드

| 질문 | 결과 |
|------|------|
| 사용자가 항상 사용 + 교체 여지를 두지 않아야? + plugin 추상화 비용이 비대? | 1 (기본 내장) |
| 기본 제공이지만 사용자가 교체·비활성화할 여지를 남기고 싶은가? | 2 (기본 plugin) |
| 외부 ecosystem 의 plugin? | 3 (사용자 plugin) |

판단이 어렵다면 *2 (기본 plugin) 를 기본값으로 검토* — disable 가능성을 열어 두는 편이 사용자 입장에서 안전하다. 1 은 "사용자가 plugin 으로 인식하지 않게 해야" 가 결정적 사유일 때만.

## 관련 문서

- [agent-guide/plugins.md](../agent-guide/plugins.md) — plugin 설치 / 관리 CLI · IPC
- [dev-guide/plugin-development.md](../dev-guide/plugin-development.md) — plugin 제작 가이드
- [dev-guide/plugin-ecosystem.md](../dev-guide/plugin-ecosystem.md) — 생태계 정책 + `BUILTINS` 자동 upgrade (§6)
- [features.md "Plugin 시스템"](../features.md) — 현재 구현된 기본 plugin 목록 및 라이프사이클
