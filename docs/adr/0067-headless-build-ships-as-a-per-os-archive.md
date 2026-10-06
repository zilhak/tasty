# ADR-0067: 헤드리스 빌드를 OS별 압축 파일로 릴리스에 함께 올린다

- **Status**: Accepted
- **Date**: 2026-10-06
- **Tags**: release, headless, packaging
- **Group**: rules

## Context

헤드리스 여부는 실행 플래그가 아니라 `gui` feature로 가르는 빌드 구분이다(루트 `Cargo.toml`의 `[features]`).
GUI 바이너리에 `--headless`를 붙여도 창 없이 실행되지 않는다.
릴리스 산출물이 GUI 빌드뿐이어서 서버에서 쓰려면 소스에서 `--no-default-features`로 직접 빌드해야 했다.
사이트 랜딩은 헤드리스 모드를 지원 기능으로 소개하는데 설치 가이드는 배포본에 헤드리스가 없다고 적어 두 문서가 어긋났다.

## Decision

릴리스는 GUI 산출물과 함께 OS별 헤드리스 압축 파일을 올린다.
대상은 Linux x64·arm64(`tar.gz`), macOS Apple Silicon(`tar.gz`), Windows x64(`zip`)이고
이름은 `tasty-headless-<버전>-<OS>-<아키텍처>`다. `.deb`·`.rpm`·`.AppImage`·`.dmg`·`.msi` 같은 설치 형식에는 넣지 않는다.

실행 파일 이름은 GUI와 같은 `tasty`로 둔다. CLI 명령과 원격 attach 프로필의 원격 바이너리 기본값이 `tasty`이기 때문이다.
압축 파일에는 GUI 산출물과 같은 서명된 번들 plugin을 실행 파일 옆 `plugins/`에, 같은 고지 파일을 최상단에 넣는다.
헤드리스 호스트도 plugin을 실행하기 때문이다.

각 OS의 릴리스 빌드 스크립트가 GUI 패키징을 마친 뒤 별도 target 디렉터리(`target/headless`)에서 같은 프로필로
`--no-default-features` 빌드를 하고, 압축 파일 검증과 OS별 `SHA256SUMS` 기재까지 맡는다.
절차는 [릴리스 절차](../dev-guide/release.md)와 [빌드 가이드](../dev-guide/build.md)에 있다.

## Consequences

서버 사용자가 소스 빌드 없이 헤드리스 빌드를 받는다. 랜딩의 헤드리스 소개가 배포본 기준으로도 참이 된다.

릴리스 러너마다 같은 프로필 빌드가 한 번 더 돈다. 별도 target 디렉터리라 GUI 빌드의 의존성 산출물을 공유하지 않는다.
실행 파일 이름이 같아서 GUI 판과 같은 폴더에 풀면 덮어쓴다. 설치 가이드는 다른 폴더에 풀도록 안내한다.

Windows release 바이너리는 feature와 관계없이 `windows_subsystem = "windows"`로 링크된다(`src/main.rs`).
헤드리스판도 부모 콘솔에 붙어 출력한다(`src/boot/os.rs`의 `attach_windows_console_if_needed`).
헤드리스판을 콘솔 서브시스템으로 바꿀지는 이 결정에 포함하지 않았다.
macOS 헤드리스 바이너리는 앱 번들과 같은 identity로 단독 서명하고 공증하지 않는다.

## Alternatives Considered

- 랜딩 문구를 "헤드리스 빌드"로 좁힌다. 서버 사용자가 소스에서 빌드해야 하는 부담이 남는다.
- 랜딩은 빌드 구분을 적지 않는다고 확정한다. 소개한 기능을 배포본에서 찾을 수 없는 어긋남이 남는다.
- 헤드리스용 `.deb`·`.rpm` 등 설치 패키지를 만든다. 같은 `/usr/bin/tasty` 경로를 두고 GUI 패키지와 충돌하며 패키지 이름과 의존성을 따로 관리해야 한다.
- GUI 패키징 뒤 같은 target 디렉터리에서 헤드리스를 빌드한다. 빌드 시간은 줄지만 `target/<프로필>/tasty`가 헤드리스 바이너리로 바뀌어 이후 로컬 실행과 재패키징이 GUI가 아닌 바이너리를 쓴다.
- 실행 파일 이름을 `tasty-headless`로 바꾼다. `tasty` CLI 사용법과 원격 attach 기본값이 맞지 않게 된다.

## Reconsideration Triggers

코드와 설정에서 확인:

- `gui` 구분이 빌드 feature가 아니라 실행 시 선택으로 바뀌면 산출물을 하나로 합친다.
- Windows 헤드리스판의 서브시스템을 콘솔로 바꾸기로 하면 `src/main.rs`의 `windows_subsystem` 조건과 이 문서를 함께 고친다.

실행 결과로 확인:

- 릴리스 러너의 빌드 시간이 배포 주기를 막을 만큼 늘면 target 공유나 프로필을 다시 검토한다.
- 헤드리스판을 패키지 매니저나 설치 형식으로 받아야 하는 요청이 생기면 설치 형식 제외를 다시 검토한다.

## References

- [릴리스 절차](../dev-guide/release.md)
- [빌드 가이드](../dev-guide/build.md)
- [ADR-0051](0051-release-artifacts-and-versioning.md) — 릴리스 번들의 서명·고지문
- [ADR-0058](0058-headless-without-local-views.md) — 헤드리스가 지원하는 실행 범위
- `scripts/build-linux.sh`, `scripts/build-macos-dmg.sh`, `scripts/build-windows.ps1`, `.github/workflows/release.yml`
