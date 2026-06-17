# 설치

Tasty 는 GitHub Releases 에서 OS·아키텍처·설치 형태별 산출물로 배포된다. 자동 설치 스크립트나 패키지 매니저 통합은 제공하지 않는다 — 모든 설치는 [릴리스 페이지](https://github.com/zilhak/tasty/releases)에서 직접 받는다.

## 산출물 매트릭스

| OS | 아키텍처 | 산출물 | 비고 |
|----|---------|--------|------|
| Linux | x86_64 | `tasty-{ver}-linux-x64.tar.gz` | 정적 바이너리 |
| Linux | x86_64 | `tasty_{ver}-1_amd64.deb` | Debian/Ubuntu/Mint |
| Linux | x86_64 | `tasty-{ver}-1.x86_64.rpm` | Fedora/RHEL/openSUSE |
| Linux | x86_64 | `Tasty-{ver}-x86_64.AppImage` | distro 무관 단일 파일 |
| Linux | aarch64 | `tasty-{ver}-linux-arm64.tar.gz` / `_arm64.deb` / `.aarch64.rpm` / `-aarch64.AppImage` | ARM64 |
| macOS | universal | `Tasty-{ver}-macos.dmg` | App Bundle 드래그 설치 |
| Windows | x86_64 | `tasty-{ver}-windows-x64.zip` | 바이너리만 압축 |
| Windows | x86_64 | `tasty-{ver}-windows-x64.msi` | 시작 메뉴/제거 등록 인스톨러 |

릴리스에는 에이전트용 [reference/](reference/index.md) 문서(IPC/CLI 레퍼런스)도 함께 첨부된다.

## Linux

```bash
# .deb (Debian/Ubuntu/Mint)
sudo apt install ./tasty_{ver}-1_amd64.deb      # 또는 dpkg -i + apt-get install -f

# .rpm (Fedora/RHEL/openSUSE)
sudo dnf install ./tasty-{ver}-1.x86_64.rpm

# .AppImage (distro 무관, 설치 불요)
chmod +x Tasty-{ver}-x86_64.AppImage && ./Tasty-{ver}-x86_64.AppImage
# FUSE 미지원 환경: ./Tasty-*.AppImage --appimage-extract-and-run

# .tar.gz (수동)
tar -xzf tasty-{ver}-linux-x64.tar.gz && ./tasty-linux-x64/tasty
```

- `.deb`/`.rpm`: `tasty` 가 PATH 등록 + 데스크톱 메뉴 아이콘 등록. 의존성은 패키지 메타데이터(`libfreetype6`/`libfontconfig1`/`libgtk-3`/`libwebkit2gtk-4.1` 등)로 자동 분석.
- `.AppImage`: 의존 라이브러리를 모두 번들. 데스크톱 메뉴 등록은 수동(`appimaged` 또는 `.desktop` 을 `~/.local/share/applications/`).
- `.tar.gz`: PATH·메뉴 등록 사용자 직접. 편한 등록을 원하면 `.deb`/`.rpm`/`.AppImage` 권장.

## macOS

`.dmg` 더블클릭 → `Tasty.app` 을 `Applications` 로 드래그. 첫 실행 시 Gatekeeper 경고가 나오면 시스템 설정 > 개인정보 보호 및 보안에서 허용(코드 사인 미인증 빌드 한정).

## Windows

```powershell
# .msi (권장) — 더블클릭 → 설치 마법사. 시작 메뉴 바로가기 + 프로그램 추가/제거 등록
# .zip (수동)
Expand-Archive tasty-{ver}-windows-x64.zip; .\tasty\tasty.exe
```

빌드 측면은 [dev-guide/build](dev-guide/build.md) "Windows MSI".

## 검증

```bash
tasty --version       # 버전
tasty list info       # GUI 인스턴스가 떠 있을 때 시스템 정보 IPC
```

## 설치 위치

| OS | 바이너리 | 사용자 데이터 |
|----|---------|--------------|
| Linux | `/usr/bin/tasty`(.deb/.rpm) 또는 추출 위치 | `~/.tasty/` |
| macOS | `/Applications/Tasty.app/Contents/MacOS/tasty` | `~/.tasty/` |
| Windows | `%LOCALAPPDATA%\tasty\tasty.exe`(.msi) | `~/.tasty/` |

`~/.tasty/` 에 `tasty.port`(IPC 포트)·`config.toml`·세션 등 사용자 데이터가 들어간다(전체 지도: [design/systems/storage](design/systems/storage.md), OS별 경로: [reference/environments](reference/environments.md)).

## 패키지 빌드 (메인테이너)

[dev-guide/build](dev-guide/build.md) · [dev-guide/dist-build](dev-guide/dist-build.md) · [dev-guide/release-runners](dev-guide/release-runners.md).
