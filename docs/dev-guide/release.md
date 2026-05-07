# 릴리스 절차

Tasty의 릴리스는 Git 태그 push로 트리거된다. 아래 절차를 순서대로 따른다.

## 1. 버전 올리기

`Cargo.toml` (workspace root)의 `version` 필드를 올린다.

```toml
version = "0.3.1"  # 패치 버전 +1
```

`cargo build`를 실행하여 `Cargo.lock`을 갱신한다.

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

- **패치** (0.0.X): 빌드 요청 시 AI가 자동으로 올림
- **마이너** (0.X.0): 사용자가 직접 지정
- **메이저** (X.0.0): 사용자가 직접 지정
