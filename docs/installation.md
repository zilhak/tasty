# 설치

Tasty는 GitHub Releases에서 OS·아키텍처·설치 형태별 산출물로 배포된다. 자동 설치 스크립트나
패키지 매니저 통합은 현재 제공하지 않는다 — 모든 설치 경로는 GitHub Releases 페이지에서 직접
받는 형태다.

> 릴리스 페이지: https://github.com/zilhak/tasty/releases

## 산출물 매트릭스

| OS | 아키텍처 | 산출물 | 비고 |
|----|---------|--------|------|
| Linux | x86_64 | `tasty-{ver}-linux-x64.tar.gz` | 정적 바이너리 |
| Linux | x86_64 | `tasty_{ver}-1_amd64.deb` | Debian/Ubuntu/Mint |
| Linux | x86_64 | `tasty-{ver}-1.x86_64.rpm` | Fedora/RHEL/openSUSE |
| Linux | x86_64 | `Tasty-{ver}-x86_64.AppImage` | distro 무관 단일 파일 |
| Linux | aarch64 | `tasty-{ver}-linux-arm64.tar.gz` | ARM64 정적 바이너리 |
| Linux | aarch64 | `tasty_{ver}-1_arm64.deb` | ARM64 Debian/Ubuntu |
| Linux | aarch64 | `tasty-{ver}-1.aarch64.rpm` | ARM64 Fedora |
| Linux | aarch64 | `Tasty-{ver}-aarch64.AppImage` | ARM64 distro 무관 |
| macOS | universal | `Tasty-{ver}-macos.dmg` | App Bundle 드래그 설치 |
| Windows | x86_64 | `tasty-{ver}-windows-x64.zip` | 바이너리만 압축 |
| Windows | x86_64 | `tasty-{ver}-windows-x64.msi` | 시작 메뉴/제거 등록 인스톨러 |

릴리스에는 `docs/agent-guide/` 문서들도 함께 첨부된다 (사용자의 AI 에이전트가 Tasty를 조작할 때
참고하는 IPC/CLI 레퍼런스).

## Linux 설치

### `.deb` (Debian / Ubuntu / Mint)

```bash
sudo dpkg -i tasty_{ver}-1_amd64.deb
sudo apt-get install -f      # 의존성이 빠진 경우 자동 보완
```

또는:

```bash
sudo apt install ./tasty_{ver}-1_amd64.deb
```

설치 후: `tasty` 명령이 PATH에 등록되고, 데스크톱 메뉴(GNOME/KDE 등)에 Tasty 아이콘이 등록된다.
의존성은 패키지 메타데이터로 자동 분석된 시스템 라이브러리(`libfreetype6`, `libfontconfig1`,
`libglib2.0-0t64`, `libgtk-3-0t64`, `libwebkit2gtk-4.1-0` 등)를 따른다.

### `.rpm` (Fedora / RHEL / openSUSE)

```bash
sudo dnf install ./tasty-{ver}-1.x86_64.rpm
# 또는: sudo rpm -i tasty-{ver}-1.x86_64.rpm
```

설치 결과는 `.deb`과 동일한 위치(`/usr/bin/tasty`, `/usr/share/applications/tasty.desktop`,
`/usr/share/icons/hicolor/{N}x{N}/apps/tasty.png`)에 들어간다.

### `.AppImage` (distro 무관)

의존 라이브러리 ~100여 개를 모두 번들한 단일 파일이다. 어떤 distro에서도 시스템 라이브러리
버전 차이 없이 동작한다. 설치 없이 실행만 하면 된다.

```bash
chmod +x Tasty-{ver}-x86_64.AppImage
./Tasty-{ver}-x86_64.AppImage
```

데스크톱 메뉴 등록은 자동이 아니다. 등록하려면 `appimaged` 데몬을 별도로 설치하거나, `.desktop`
파일을 수동으로 `~/.local/share/applications/`에 넣는다.

FUSE를 지원하지 않는 환경(일부 컨테이너 등)에선 `--appimage-extract-and-run`으로 실행:

```bash
./Tasty-{ver}-x86_64.AppImage --appimage-extract-and-run
```

### `.tar.gz` (수동 추출)

```bash
tar -xzf tasty-{ver}-linux-x64.tar.gz
cd tasty-linux-x64/
./tasty
```

PATH 등록·데스크톱 메뉴 등록은 사용자가 직접 처리해야 한다 (예: `~/.local/bin/`에 심볼릭
링크, `assets/linux/tasty.desktop`을 `~/.local/share/applications/`에 복사 등).
편한 경로 등록을 원한다면 `.deb`/`.rpm`/`.AppImage` 사용을 권장.

## macOS 설치

`.dmg`를 더블클릭하면 Finder가 마운트한다. `Tasty.app`을 `Applications` 폴더로 드래그한다.

```
Tasty-{ver}-macos.dmg → 더블클릭 → Tasty.app을 Applications에 드래그
```

처음 실행 시 Gatekeeper 경고가 나오면 시스템 환경설정 > 보안 및 개인정보 보호에서 허용한다
(코드 사인 미인증 빌드 한정).

## Windows 설치

### `.msi` (인스톨러, 권장)

```
tasty-{ver}-windows-x64.msi → 더블클릭 → 설치 마법사
```

설치 후 시작 메뉴에 Tasty 바로가기가 추가되고, "프로그램 추가/제거"에 등록된다. 사용자 선택에
따라 PATH에도 추가 가능. 자세한 빌드 측면은 [`dev-guide/build.md`](dev-guide/build.md)의
"Windows MSI" 절 참조.

### `.zip` (수동 추출)

```powershell
Expand-Archive tasty-{ver}-windows-x64.zip
.\tasty\tasty.exe
```

PATH 등록·바로가기 생성은 사용자가 직접. `.msi`와 달리 자동 통합이 없다.

## 검증

설치 후 동작 확인:

```bash
tasty --version              # 버전 출력
tasty list info              # GUI 인스턴스가 떠 있을 때 시스템 정보 IPC 호출
```

GUI 실행 후 동작 확인은 한 워크스페이스에 셸이 정상으로 spawn되는지로 본다.

## 설치 위치 (현재 구현)

| OS | 바이너리 | 설정/포트 |
|----|---------|----------|
| Linux | `/usr/bin/tasty` (.deb/.rpm) 또는 사용자가 둔 곳 (.tar.gz/.AppImage) | `~/.tasty/` |
| macOS | `/Applications/Tasty.app/Contents/MacOS/tasty` | `~/.tasty/` |
| Windows | `%LOCALAPPDATA%\tasty\tasty.exe` (.msi) | `~/.tasty/` |

`~/.tasty/`에는 `tasty.port`(IPC 포트), `config.toml`, 세션/북마크 등 사용자 데이터가 들어간다.
사용자 데이터 디렉토리 정리는 [`agent-guide/linux.md`](agent-guide/linux.md), `windows.md`,
`macos.md`(추후) 의 "파일 경로" 절 참조.

## 패키지 빌드 (메인테이너용)

직접 산출물을 만드는 절차는 [`dev-guide/build.md`](dev-guide/build.md)와
[`dev-guide/release-runners.md`](dev-guide/release-runners.md) 참조.
