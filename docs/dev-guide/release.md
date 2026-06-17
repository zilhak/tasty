# 릴리스 절차

릴리스는 **Git 태그 push** 로 트리거된다(`.github/workflows/release.yml`). 버전 형식은 `MAJOR.MINOR.PATCH`. 버전 정책의 권위는 [`../../CLAUDE.md`](../../CLAUDE.md) "버전 정책" — 이 문서는 절차다.

## 버전 자동 bump 규칙 (요약)

CLAUDE.md 정책의 운영 형태:

- **본체** (`Cargo.toml` 루트): 사용자가 *빌드를 요청* 했고, 마지막 빌드 이후 새 커밋이 있으며, 사용자가 막지 않았으면 **patch +1**. AI 자체 검증 빌드(`cargo build`/`test`)는 올리지 않는다.
- **Plugin** (`crates/tasty-plugin-*/Cargo.toml`): 한 커밋에 특정 plugin 디렉토리 파일이 하나라도 staged 되면 그 plugin 의 **patch +1 을 같은 커밋에** 포함(무조건, 명시적 거부 없는 한). 여러 plugin 변경 시 각각 독립 적용. 본체 규칙과 독립.
- **minor / major**: 사용자가 직접 지정. AI 가 임의로 올리지 않는다.

본체 patch bump 절차: `Cargo.toml` patch +1 → `cargo build`(Cargo.lock 갱신) → `Cargo.toml` + `Cargo.lock` 함께 커밋(`chore: bump version to X.Y.Z`) → 아래 릴리스 절차로 이어감.

## 릴리스 단계

### 1. 버전 + CHANGELOG

1. `Cargo.toml`(루트) `version` 갱신, `cargo build` 로 `Cargo.lock` 갱신.
2. `CHANGELOG.md` 의 `## [Unreleased]` 를 새 버전 헤더로 promote 하고, 빈 `[Unreleased]` 를 다시 추가한다 (`tests/changelog_unreleased.rs` 가 빈 절 존재를 강제).
   ```markdown
   ## [Unreleased]

   ## [X.Y.Z] - YYYY-MM-DD
   ### Fixed
   - `fix(...)`: ...
   ```
3. schema 변경이 있었다면 `crates/tasty-plugin-protocol/CHANGELOG.md` 도 동일 처리. break/deprecation 분류는 [api-conventions](api-conventions.md) "안정성 정책".

### 2. Plugin 매니페스트 서명

`crates/tasty-plugin-*/tasty-plugin.toml` 가 이번 사이클에 하나라도 변경됐으면(또는 첫 release 빌드면) 서명을 갱신한다.

```bash
./scripts/sign-bundle.sh --key secrets/dev-private.pem --all-builtins
```

생성/갱신된 `*.toml.sig`(현재 8개)를 bump 커밋에 포함하거나 직전 커밋으로 분리. repo 의 `.sig` 는 *로컬 release 빌드·dev 검증용* — CI 정식 release 는 `TASTY_RELEASE_SIGN_KEY` secret 으로 **재서명**하므로 repo 와 다른 키로 덮어쓰는 게 정상. 알고리즘·키 보관은 [plugin-packaging](plugin-packaging.md).

### 3. 커밋 — body 가 곧 릴리스 노트

`Cargo.toml` + `Cargo.lock` 을 함께 커밋한다. **커밋 body 에 changelog 를 적는다** — 워크플로가 `git log -1 --format=%b` 로 추출해 GitHub Release 노트로 쓴다.

```
chore: bump version to X.Y.Z

## What's Changed
### Features
- feat(...): ...
### Bug Fixes
- fix(...): ...
```

이전 태그 이후 커밋: `git log v<이전>..HEAD --oneline`.

### 4. 태그 + push

```bash
git tag vX.Y.Z          # 'v' 접두사 + Cargo.toml 버전과 정확히 일치 (불일치 시 워크플로 fail)
git push origin main --tags
```

### 5. 워크플로 (release.yml)

1. **create-release** — 버전 검증 → draft release 생성(body = 릴리스 노트).
2. **build-macos / build-windows / build-linux-x64 / build-linux-arm64** — `TASTY_RELEASE_SIGN_KEY` 로 plugin 재서명 → 빌드(`--profile dist`) → 아티팩트 업로드 → 키 파일 wipe.
3. **publish-release** — draft 해제(공개).

`TASTY_RELEASE_SIGN_KEY` 미등록 상태로 tag push 시 build job 이 첫 step 에서 fail.

### 6. 검증

GitHub Releases 에서 노트 + 플랫폼별 아티팩트 확인:

- macOS `*.dmg` / Windows `*.zip`·`*.msi` / Linux x64·arm64 각 `.tar.gz`·`.deb`·`.rpm`·`.AppImage`
- `SHA256SUMS-{macos,windows,linux-x64,linux-arm64}.txt` 4종 (없으면 `tasty update` 가 hard fail)

> **사용자 업그레이드**: publish 되면 호스트 백그라운드 폴러가 1시간 내 감지 → in-app 알림 + Settings의 Updates 탭 표시. 사용자는 `tasty update` 로 다운로드 + SHA256 검증 + atomic swap 후 수동 재시작. (자동 업데이트 기능 문서: [`features/auto-update/`](../features/auto-update/index.md).)

## API 안정성 가드

0.x 라인은 *추가만 가능, 제거 금지* 원칙으로 외부 표면 회귀를 막는다 (메서드 baseline, CHANGELOG break guard 등 `cargo test --workspace` 강제). 분류·major bump 절차는 [api-conventions](api-conventions.md).

## 관련

- [commit-convention.md](commit-convention.md) — bump 커밋 형식
- [build.md](build.md) — `--profile dist` 배포 빌드·패키징 스크립트
