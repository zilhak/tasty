# Manual dist 빌드 시나리오

[`release.md`](release.md) 는 *태그 push → GitHub Actions* 워크플로를 다룬다.
본 문서는 **로컬 머신에서 dist 산출물을 수동으로 빌드** 할 때의 명령 카탈로그다.

빌드 프로필 자체의 정의 (LTO, strip 등) 는 [`build.md`](build.md) 의 "빌드 프로필" 섹션을 참조.

## 공통 사전 조건

- `Cargo.toml` 의 `version` 이 의도한 릴리스 버전인지 확인.
  - 검증 빌드 (코드/빌드 시스템 확인) 에는 버전을 올리지 않는다.
  - 릴리스 빌드는 [`release.md`](release.md) 의 버전 bump 절차를 거친 후 진행.
- 모든 빌드 스크립트는 인자 없이 호출하면 **`--profile dist`** 가 기본.
- 산출물은 **`dist/`** 디렉토리에 누적 (구버전 보존, 동일 버전 재빌드는 silent overwrite).

## macOS

현재 머신이 Darwin arm64 이면 즉시 실행 가능. 도구 (`hdiutil`, `codesign`, `xcrun`) 는 Xcode Command Line Tools 에 포함되어 있다.

### 빌드

```bash
cd /path/to/tasty

# 1. dist 프로필로 워크스페이스 컴파일 (clean 기준 ~3:22)
cargo build --profile dist

# 2. .app 번들 + .dmg 생성
./scripts/build-macos-dmg.sh
```

### 산출물

- `dist/Tasty.app/Contents/MacOS/tasty` — 실행 바이너리
- `dist/Tasty.app/Contents/Info.plist` — `CFBundleVersion` = `Cargo.toml` 의 version
- `dist/Tasty.app/Contents/Resources/icon.icns`
- `dist/Tasty-{version}-macos.dmg` (~18 MB)

### Sanity check

```bash
# 버전 출력 (clap 기본 형식: "tasty {version}")
dist/Tasty.app/Contents/MacOS/tasty --version

# 바이너리 아키텍처 확인
file dist/Tasty.app/Contents/MacOS/tasty
# 기대: Mach-O 64-bit executable arm64

# Info.plist 버전과 Cargo.toml 버전 비교
plutil -extract CFBundleVersion raw dist/Tasty.app/Contents/Info.plist
grep '^version' Cargo.toml | head -1
```

`dist` 는 `release` 를 상속하여 `strip = true` 가 적용된다. 산출물 바이너리에서 `nm` 출력이 거의 비어 보이는 것은 정상 (디버그 심볼 제거).

### 서명 / 공증 상태 진단

dist 빌드 스크립트는 `codesign` / `notarytool` 을 호출하지 않는다. 산출물은 **미서명** 상태로 생성된다.

```bash
# 서명 상태 (미서명이면 "not signed at all")
codesign --display --verbose=2 dist/Tasty.app 2>&1 | head -5

# Gatekeeper 평가 (미서명 → rejected 가 정상)
spctl -a -t exec dist/Tasty.app
```

사용자가 더블클릭으로 실행하면 macOS 가 차단한다. 우회 절차: `Finder` 에서 우클릭 → "열기" → "확인되지 않은 개발자" 다이얼로그에서 강제 실행.

**서명 / 공증 / 배포는 본 문서의 범위 외다.** Apple Developer ID 인증서 + app-specific password 가 필요하며, 절차는 별도 (운영 문서 또는 [`release.md`](release.md) 의 GitHub Actions 잡) 에서 다룬다.

### 산출물 무결성 (선택)

동일 버전 재빌드 시 silent overwrite 가 발생한다. 재현성을 확인하려면 hash 를 보존:

```bash
shasum -a 256 dist/Tasty-*-macos.dmg | tee dist/SHA256SUMS-macos.txt
```

### Out-of-scope

- **Universal binary (arm64 + x86_64)**: 현재 산출물은 *호스트 아키텍처* (arm64 in this env) only. Intel Mac 배포는 별도 작업 (`cargo build --target x86_64-apple-darwin` + `lipo` 결합) 이 필요하며 본 문서 범위 외.
- **codesign / notarization**: 위 §"서명 / 공증 상태 진단" 참조.

## Windows

> **사용자 검증 대기.** Claude 작성 환경 (Darwin) 에서는 직접 실행 불가. 사용자가 Windows 머신에서 아래 절차로 빌드 후 결과를 반영한다.

### 사전 도구 (1회 설치)

```powershell
cargo install cargo-wix
winget install WiXToolset.WiXToolset   # 관리자 권한 필요
```

`winget` 패키지는 `WIX` 환경변수만 등록한다. `build-windows.ps1` 이 MSI 단계에서 `$env:WIX\bin` 을 자동으로 PATH 에 prepend 하므로 추가 PATH 설정은 불필요.

### 빌드

```powershell
cd C:\path\to\tasty

# dist 빌드 + ZIP + MSI
.\scripts\build-windows.ps1

# ZIP 만 (MSI 단계 생략)
.\scripts\build-windows.ps1 -SkipMsi
```

### 산출물

- `dist\tasty-{version}-windows-x64.zip`
- `dist\tasty-{version}-windows-x64.msi`

### 사용자 검증 체크리스트

- [ ] `cargo build --profile dist` 가 warning 0 으로 통과
- [ ] `dist\tasty-*-windows-x64.zip` 생성 + 내부 `tasty.exe` 실행 가능
- [ ] `dist\tasty-*-windows-x64.msi` 설치 → 시작 메뉴 바로가기 생성 → 제어판에서 제거 가능
- [ ] MSI 의 UpgradeCode 가 `722A590A-...` 로 유지 (`wix/main.wxs` 변경 0)

검증 결과는 본 문서의 별도 commit 으로 반영한다.

## Linux

> **사용자 검증 대기.** Claude 작성 환경 (Darwin) 에서는 직접 실행 불가.

### 사전 도구 (1회 설치)

```bash
sudo apt install cmake pkg-config libfreetype6-dev libfontconfig1-dev
cargo install cargo-deb cargo-generate-rpm
curl -fsSL -o ~/.local/bin/linuxdeploy \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$(uname -m).AppImage
chmod +x ~/.local/bin/linuxdeploy
```

도구의 역할 / 패키지 메타데이터는 [`build.md`](build.md) 의 Linux 섹션 참조.

### 빌드

```bash
cd /path/to/tasty
./scripts/build-linux.sh
```

`uname -m` 으로 x64 / arm64 가 자동 감지된다.

### 산출물

- `dist/tasty-{version}-linux-{x64|arm64}.tar.gz`
- `dist/tasty_{version}-1_{amd64|arm64}.deb`
- `dist/tasty-{version}-1.{x86_64|aarch64}.rpm`
- `dist/Tasty-{version}-{x86_64|aarch64}.AppImage` (~83 MB)

### 사용자 검증 체크리스트

- [ ] 4 종 산출물 모두 생성
- [ ] tar.gz 압축 풀고 `./tasty --version` 동작
- [ ] AppImage 실행 권한 부여 후 `./Tasty-*.AppImage --version` 동작
- [ ] `dpkg -i` 또는 `rpm -i` 설치 → `which tasty` → 제거 가능

검증 결과는 본 문서의 별도 commit 으로 반영한다.

## 산출물 카탈로그 (요약)

| 플랫폼 | 빌드 명령 | 산출물 | 크기 (참고) |
|--------|-----------|--------|-------------|
| macOS (arm64) | `./scripts/build-macos-dmg.sh` | `Tasty-{v}-macos.dmg` | ~18 MB |
| Windows (x64) | `.\scripts\build-windows.ps1` | `tasty-{v}-windows-x64.{zip,msi}` | TBD |
| Linux | `./scripts/build-linux.sh` | `{tar.gz, deb, rpm, AppImage}` | ~83 MB (AppImage) |

## 관련 문서

- [`build.md`](build.md) — 빌드 프로필 정의 (`dev` / `release` / `dist`), LTO, 빌드 시간 측정, Linux/Windows 도구 메타데이터
- [`release.md`](release.md) — 릴리스 워크플로 (버전 bump → CHANGELOG → 태그 push → GitHub Actions)
- [`release-runners.md`](release-runners.md) — self-hosted runner 인벤토리 + 운영 명령
