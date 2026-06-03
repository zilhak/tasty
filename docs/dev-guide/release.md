# 릴리스 절차

Tasty의 릴리스는 Git 태그 push로 트리거된다. 아래 절차를 순서대로 따른다.

## 1. 버전 올리기

`Cargo.toml` (workspace root)의 `version` 필드를 올린다.

```toml
version = "0.3.1"  # 패치 버전 +1
```

`cargo build`를 실행하여 `Cargo.lock`을 갱신한다.

## 1-A. CHANGELOG 갱신

릴리스 직전:

1. `CHANGELOG.md`의 `## [Unreleased]` 절을 새 버전 헤더로 옮긴다:
   ```markdown
   ## [0.3.1] - 2026-05-12
   ```
2. 새 `## [Unreleased]` 절을 비어 있는 상태로 다시 추가한다 (`tests/changelog_unreleased.rs`가 강제).
3. `crates/tasty-plugin-protocol/CHANGELOG.md`에 schema 관련 변경이 있었다면 동일 처리.
4. break/deprecation 항목이 있는지 한 번 더 확인. 분류 기준은 [`ipc-stability.md`](ipc-stability.md).

### 패치 promotion 예시 (0.7.x)

0.7.x 패치는 *fix 만* 포함한다 (Conventional Commits 의 `fix:`). 새 메서드 / break /
`Cargo.toml` major 변경은 모두 minor 또는 major 로.

CHANGELOG.md 의 변환 예 — 한 PR 로 처리:

```diff
-## [Unreleased]
-
-- `fix(resize)`: 터미널 그리드 재계산 누락
-
-## [0.7.0] - 2026-06-04
+## [Unreleased]
+
+(no changes yet)
+
+## [0.7.1] - 2026-06-12
+
+### Fixed
+- `fix(resize)`: 터미널 그리드 재계산 누락
+
+## [0.7.0] - 2026-06-04
```

`crates/tasty-plugin-protocol/CHANGELOG.md` 에 schema 변경이 *없으면* 본 파일은 건드리지 않는다 (빈 `[Unreleased]` 그대로).

## 2. 커밋 작성

`Cargo.toml` + `Cargo.lock`을 함께 커밋한다. **커밋 body에 체인지로그를 작성**한다.
릴리스 워크플로가 `git log -1 --format=%b`로 body를 추출하여 GitHub Release 노트로 사용하기 때문이다.

```
chore: bump version to X.Y.Z

## What's Changed

### Features
- feat(xxx): 설명

### Bug Fixes
- fix(xxx): 설명

### Chores
- chore(xxx): 설명

### Docs
- docs: 설명
```

이전 태그 이후 커밋 목록은 다음으로 확인한다:

```bash
git log v<이전버전>..HEAD --oneline
```

## 3. 태그 생성

```bash
git tag v0.3.1
```

태그 이름은 반드시 `v` 접두사 + Cargo.toml 버전과 일치해야 한다.
워크플로가 태그 버전과 Cargo.toml 버전을 비교하여 불일치 시 실패한다.

## 4. Push

```bash
git push origin main --tags
```

## 5. 워크플로 확인

`release.yml`이 자동 트리거된다. 순서:

1. **create-release**: 버전 검증 → draft release 생성 (body를 릴리스 노트로)
2. **build-macos / build-windows / build-linux-x64 / build-linux-arm64**: 각 플랫폼 빌드 및 아티팩트 업로드
3. **publish-release**: `docs/agent-guide/*` 업로드 → draft 해제 (공개)

GitHub Actions 탭에서 모든 job이 성공했는지 확인한다.

## 6. 검증

- GitHub Releases 페이지에서 릴리스 노트와 아티팩트 확인
- 각 플랫폼별 바이너리가 모두 업로드되었는지 확인:
  - macOS: `Tasty-X.Y.Z-macos.dmg`
  - Windows: `tasty-X.Y.Z-windows-x64.zip`, `tasty-X.Y.Z-windows-x64.msi`
  - Linux x64: `.tar.gz`, `.deb`, `.rpm`, `.AppImage`
  - Linux arm64: `.tar.gz`, `.deb`, `.rpm`, `.AppImage`
  - `docs/agent-guide/*` 문서들

## 버전 정책

버전 형식은 `MAJOR.MINOR.PATCH` (예: `0.6.0`).

- **패치** (0.0.X): 빌드 요청 시 AI 가 자동으로 올림
- **마이너** (0.X.0): 사용자가 직접 지정. AI 가 임의로 올리지 않음
- **메이저** (X.0.0): 사용자가 직접 지정. AI 가 임의로 올리지 않음

### AI 가 패치 버전을 자동으로 올리는 조건

**사용자가 빌드를 요청했을 때** 다음을 모두 만족하면 패치 버전을 1 올린다:

1. 마지막 빌드 (= 마지막 버전 bump 커밋) 이후 새 커밋이 있다.
2. 사용자가 "버전 올리지 말라" 고 명시하지 않았다.

절차:

1. `Cargo.toml` 의 patch 번호를 `+1` 한다.
2. `cargo build` 를 실행한다 (`Cargo.lock` 이 자동 갱신된다).
3. `Cargo.toml` + `Cargo.lock` 을 함께 커밋한다 (`chore: bump version to X.Y.Z`).
4. 본격 릴리스 절차는 위의 1-A (CHANGELOG) 부터 이어간다.

### AI 가 자동으로 올리지 않는 경우

- **테스트/검증을 위한 빌드** (AI 가 스스로 수행하는 `cargo build`, `cargo test` 등): 버전을 올리지 않는다.
- **사용자가 명시적으로 거부**: "버전 올리지 마" 라는 요청이 있었던 경우.
- **이미 직전 커밋이 버전 bump 커밋**: 마지막 빌드 이후 새 커밋이 없으면 올리지 않는다.

### 사용자가 major 버전을 지시할 때 (0.7.0 등)

major bump 는 *API stability 선언* 의 성격을 가지므로 patch/minor 보다 절차가 추가된다. AI 는 임의로 올리지 않으며, 사용자가 명시적으로 `X.0.0` 을 지시한 경우에만 다음 순서를 따른다.

1. **0.7 freeze 체크리스트 충족 확인** — [`ipc-stability.md`](ipc-stability.md) 의 *0.7 freeze 진입 체크리스트* 6 항목을 점검. 보안 예외 운영 경험 같은 항목은 *완화* 결정으로 기록 가능 (사건 기반 정착으로 0.7.x 에 위임).
2. **`crates/tasty-ipc/src/alias.rs::ALIASES` 비우기** — deprecation 기간이 충분히 운영된 transitional alias 를 모두 제거. 호출 사이트는 그대로 두고 빈 배열만 유지 (다음 alias 등장 시 재사용). 내부 단위 테스트가 alias 동작을 가정하면 빈-배열 noop 검증으로 교체.
3. **CHANGELOG.md `[Unreleased]` → `[X.0.0]` 헤더 변환** — 빈 `[Unreleased]` 절을 위에 다시 추가 (`tests/changelog_unreleased.rs` 보호). 헤더에 `(BREAK) X.0.0 — API stability 선언` 캡션. `Removed` 절에 alias 제거 항목 명시.
4. **README.md 갱신** — major 시기의 *현재 상태* (기능 목록 / 아키텍처 / crate 수 배지) 가 GitHub 첫 페이지에 반영되도록.
5. **`docs/agent-guide/api-reference.md` + `docs/dev-guide/plugin-development.md` 정합 확인** — 실제 IPC 와 문서가 어긋나면 같은 PR 에서 갱신.
6. **`docs/dev-guide/cli-naming.md` namespace 메서드 수 재계산** — 새 IPC 추가가 누적된 namespace 의 카운트를 실측 (`grep -E '"namespace\.' crates/tasty-ipc/src/method_meta.rs`) 와 일치시킴.
7. **plugin-protocol baseline 결합 시점 기록** — `crates/tasty-plugin-protocol/CHANGELOG.md` 의 baseline 섹션에 *"tasty X.0.0 부터 api_version=N 안정 선언"* 노트 1줄 추가.
8. **§1 ~ §3 의 일반 릴리스 절차** (Cargo.toml bump → cargo build → 커밋 → tag → 산출물 빌드) 진행.

이후 minor (X.Y.0) 부터는 추가만 가능하고, break 는 다음 major 까지 보류한다.

## 0.7.x 패치 release 가드

0.7.0 이후 0.7.x 동안 외부 표면이 *모르게* break 되는 것을 차단하기 위해 다음
4 개 테스트가 강제된다. 모두 `cargo test --workspace` 와 새 `.github/workflows/test.yml`
에서 자동 실행된다.

| test | 위치 | 가드 내용 |
|------|------|---------|
| `all_baseline_methods_still_registered` | `tests/api_baseline_0_7.rs` | `tests/fixtures/method_baseline_0_7.txt` 의 0.7.0 메서드가 `METHOD_TABLE` 에서 사라지면 fail. |
| `root_unreleased_has_no_break_when_major_unchanged` | `tests/changelog_unreleased.rs` | `CHANGELOG.md` `[Unreleased]` 의 bullet entry 가 `(BREAK)` 토큰을 포함하면서 Cargo.toml major 가 직전 release 와 동일하면 fail. |
| `plugin_protocol_unreleased_has_no_break_when_major_unchanged` | `tests/changelog_unreleased.rs` | plugin-protocol CHANGELOG 에 동일 가드. `HOST_API_VERSION` major 보존을 강제. |
| `cli_naming_namespace_counts_match_method_table` | `tests/cli_naming_count_drift.rs` | `docs/dev-guide/cli-naming.md` 의 `<!-- count-table:host-namespaces -->` 표가 METHOD_TABLE 실측과 일치하지 않으면 fail. |

원칙: *추가만 가능, 제거 금지*.

- **메서드 추가** (patch/minor 모두 허용): `METHOD_TABLE` 에 새 row → `cli-naming.md` 표 카운트 +1 → baseline fixture 는 *건드리지 않는다* (baseline 은 *제거 금지* 만 강제 — 추가는 통과).
- **메서드 제거 / 이름변경**: 0.7.x 에서는 baseline 가드가 차단. major bump (2.0.0) 가 필요. 만약 의도된 major 이라면 `tests/fixtures/method_baseline_0_7.txt` 를 갱신 후 같은 PR 에서 진행.
- **break 누적**: `[Unreleased]` 에 `- (BREAK) ...` bullet 이 들어가는 순간 break guard 가 fail → minor / major bump 까지 머지 차단. 0.7.x 안에서 break 가 필요하면 *deprecation 으로 전환* 하여 0.7.x 동안 두 표면을 유지하고, 다음 major 에서 제거.

분기 결정 트리:

```
변경이 새 메서드 추가만? → patch 또는 minor (사용자가 minor 가치 평가)
변경이 동작 변경 (semantic 변경)? → minor (1.X.0)
변경이 메서드 제거 / 시그니처 BREAK? → major (X.0.0). 0.7.x 가 아닌 1.1.0 이나 2.0.0 으로 분기.
```

