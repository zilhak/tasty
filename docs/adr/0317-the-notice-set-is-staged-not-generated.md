# ADR-0317: 고지 세트는 생성하지 않고 저장소의 세 파일을 산출물마다 스테이징한다

- **Status**: Accepted
- **Date**: 2026-09-20
- **Tags**: release, packaging, licensing, third-party, linux, appimage, deb, rpm, ci

## Context

이 저장소는 제3자 자산을 하나 번들한다 — D2Coding ligature 폰트이고 OFL 1.1 이다. OFL 1.1 은
**라이선스 본문을 폰트와 함께 배포할 것**을 요구한다. 그 의무를 적은 문서
(`THIRD_PARTY_LICENSES.md`)는 폰트가 들어온 커밋(2026-04-26)에 함께 생겼고, 릴리스 체크리스트
절에 "`.github/workflows/` 에 릴리스 자동화가 추가되는 시점에 위 두 파일이 에셋으로
업로드되도록 설정합니다" 라고 적었다.

**그 조건은 닷새 뒤 충족됐다** — 릴리스 워크플로가 2026-05-01 에 들어왔다. 그런데 후속이 안
왔다. 이 결정을 쓰는 시점까지 넉 달 반 동안, 배포 입력 어디에도 고지가 없었다:
`scripts/build-linux.sh` · `scripts/build-macos-dmg.sh` · `scripts/build-windows.ps1` ·
`.github/workflows/release.yml` · 루트 `Cargo.toml` 의 deb/rpm asset 목록이 전부 licence 도
notice 도 0 회 언급한다. 유일한 예외가 `wix/main.wxs` 로, MSI 는 MIT 본문(`wix/License.rtf`)을
설치 동의 화면과 설치 디렉토리 양쪽에 넣는다 — 다만 그것은 Tasty 자신의 라이선스이고 폰트의
OFL 은 아니다.

즉 **문서가 자기 조건을 스스로 만족시킨 뒤에도 이행이 안 된 상태**였고, 그 사실을 아무 게이트도
보지 않았다. 고칠 때 먼저 정해야 하는 것은 "무엇을 넣을 것인가" 다 — 그 답이 플랫폼 넷의 입력을
동시에 정하기 때문이다.

## Decision

**고지 세트는 저장소에 이미 있는 세 파일이고, 릴리스 시점에 생성하지 않는다.**

- `LICENSE` — Tasty 자체 코드의 MIT 본문
- `LICENSES/D2Coding-OFL.txt` — 번들 폰트의 OFL 1.1 본문
- `THIRD_PARTY_LICENSES.md` — 무엇이 번들되고 무슨 의무가 따르는지

생성 단계를 두지 않는 근거는 **세는 대상이 하나**라는 것이다. 번들하는 제3자 자산은 폰트
하나이고 `LICENSES/` 의 파일도 하나다. 사람이 관리할 수 있는 크기이며, 생성기를 두면 빌더마다
그 도구를 깔아야 하고(릴리스 러너는 self-hosted 다) 출력이 도구 버전에 따라 흔들린다.

**자리는 산출물의 관례를 따르고, 그래서 플랫폼마다 다르다.** `tar.gz` 는 패키지 매니저가 없어
최상단, deb 은 Debian 관례대로 `/usr/share/doc/tasty/`, rpm 은 RPM 관례대로
`/usr/share/licenses/tasty/`, AppImage 는 후자와 같은 배치를 쓴다. `LICENSES/` 하위 경로는
deb·tar.gz·AppImage 에서 유지한다 — `THIRD_PARTY_LICENSES.md` 안의 상대 링크가 설치된 트리에서
그대로 풀리게 하기 위함이다. 정본 표는 그 문서가 갖고, 이 ADR 은 그 표를 복제하지 않는다.

**이행은 검증 가능한 플랫폼부터 한다.** 이 결정과 함께 착지한 것은 릴리스 에셋 업로드와 Linux
넷이고, 둘 다 Linux 빌더에서 끝까지 확인했다. Windows `.zip` 과 macOS `.dmg` 는 그 빌드 환경이
아니면 산출물을 열 수 없어 **의도적으로 미이행으로 남겼다** — 배선만 하고 "동작한다" 를 못
말하는 상태를 만들지 않는다.

## Consequences

- **얻은 것**: Linux 산출물 넷과 릴리스 에셋이 OFL 1.1 이 요구하는 형태를 만족한다. 스테이징이
  `build-linux.sh` 의 한 함수(`stage_notice`)라 새 Linux 산출물이 생겨도 한 줄로 붙는다.
  같은 스크립트의 sanity check 가 tar.gz 셋·deb·rpm 의 고지 유무를 함께 본다.
- **잃은 것 (중요)**: 이 세트는 **정적 링크되는 Rust 의존 크레이트의 저작권 고지를 담지
  않는다.** `Cargo.lock` 에 이름이 844 개 있고 그중 MIT·Apache-2.0·BSD 계열은 바이너리 배포 시
  저작권 문구 재현을 요구한다. 그 의무의 현재 충족률은 **0** 이다 — 이 결정 전에도 0 이었고 이
  결정이 그것을 바꾸지 않는다. `deny.toml` 의 licenses allowlist 는 **정책 게이트**이지 고지
  산출물이 아니다.
- **운영 비용 / 유지 부담**: 새 제3자 자산이 들어오면 움직일 자리가 **다섯**이다 —
  `LICENSES/` · `THIRD_PARTY_LICENSES.md` · `build-linux.sh` 의 `stage_notice` ·
  `[package.metadata.deb]` · `[package.metadata.generate-rpm]`. **그 다섯이 함께 움직이는지
  재는 채널은 없다.** 만들지 않은 이유는 이 회차가 새 게이트를 만들지 않기로 한 회차이기
  때문이고, 없다는 사실을 여기 적는 것으로 대신한다.

## Alternatives Considered

- **A: 의존 크레이트 고지를 생성한다** (`cargo about` · `cargo-bundle-licenses` 류) — 위
  "잃은 것" 을 실제로 닫는 유일한 길이다. 이번에 안 고른 이유는 셋이다. 첫째, 이 머신에도
  릴리스 러너에도 그 도구가 없어서 **이 회차에서 출력을 확인할 수 없다**(설치 자체가 새 공급망
  입력이다). 둘째, 844 개짜리 출력의 정확성은 도구 버전에 딸려 있어 "배선했다" 와 "맞다" 가
  갈린다. 셋째, 그것은 고지 세트의 **내용을 바꾸는 결정**이라 폰트 하나를 넣는 이 이행과 크기가
  다르다. **따로 결정할 일로 남긴다** — 위 "잃은 것" 이 그 입구다.
- **B: 전 플랫폼을 한 자리(`usr/share/licenses/tasty/`)로 통일한다** — 배치가 하나라 표가 짧아
  진다. 안 고른 이유는 deb 이 그 경로를 쓰지 않기 때문이다. Debian 패키지 문서는
  `/usr/share/doc/<pkg>/` 이고, 거기 cargo-deb 이 만드는 `copyright` 가 이미 산다. 관례를
  거스르면 배포판 도구가 보는 자리와 실제 자리가 갈린다.
- **C: 릴리스 에셋으로만 올리고 산출물에는 안 넣는다** — 워크플로 한 스텝으로 끝난다. 안 고른
  이유는 OFL 1.1 이 요구하는 것이 **함께 배포되는 것**이라서다. 따로 내려받을 수 있는 것은 그
  요구를 대신하지 못한다. 다만 에셋 업로드 자체는 유용해서 **함께** 한다.
- **D: 미이행 플랫폼까지 한 번에 배선한다** — Windows·macOS 도 같은 회차에 넣는 안. 안 고른
  이유는 이 빌더에서 그 산출물을 열 수 없어 확인이 원리적으로 불가능하기 때문이다. 확인 없이
  넣으면 다음 사람이 그 자리를 "돌아간다" 로 읽는다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- 번들하는 제3자 자산이 둘 이상이 된다. 좌변은 `LICENSES/` 의 파일 수(오늘 1)이고, 세는 법은
  `ls LICENSES | wc -l`. 손으로 관리하는 근거가 "하나뿐" 이므로 그 수가 자라면 생성기(대안 A)의
  대가 계산이 뒤집힌다. **오늘 이 좌변을 읽는 가드는 없다.**
- 고지 세트를 스테이징하는 자리가 위 "운영 비용" 의 다섯을 넘어선다. 새 산출물 종류가 생겼다는
  뜻이고, 그때는 세트를 파일 목록 하나로 뽑아 소비처가 이름으로 부르는 형태가 싸진다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- Windows·macOS 빌더에서 미이행 두 자리를 확인할 수 있게 된다. 재는 법: 그 환경에서
  `.zip`/`.dmg` 를 풀어 고지 세트 세 파일이 있는지 본다.
- 정적 링크 의존의 저작권 고지를 실제로 요구받는다(배포 채널·법무 검토). 재는 법: 그 요구의
  형태(목록인지 본문 전문인지)를 먼저 확정한 뒤 대안 A 를 다시 연다.

## References

- `THIRD_PARTY_LICENSES.md` — 고지 세트와 산출물별 자리의 정본 표. (결정이 실현된 현재 위치)
- `scripts/build-linux.sh` 의 `stage_notice` — Linux 넷의 스테이징. (결정이 실현된 현재 위치)
- 루트 `Cargo.toml` 의 `[package.metadata.deb]` · `[package.metadata.generate-rpm]` asset 목록. (결정이 실현된 현재 위치)
- `.github/workflows/release.yml` 의 publish 잡. (결정이 실현된 현재 위치)
- `wix/main.wxs` 의 `Component Id='License'` — MSI 가 이미 MIT 본문을 넣는 자리. (결정 시점의 기록)
- [`docs/dev-guide/dist-build.md`](../dev-guide/dist-build.md) — 빌더가 읽는 절차.
