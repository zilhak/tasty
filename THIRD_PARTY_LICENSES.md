# Third-Party Licenses

Tasty 자체 코드는 [`LICENSE`](LICENSE) 파일에 명시된 MIT 라이선스를 따릅니다.
다만 Tasty는 별도 라이선스를 가진 다음 third-party 자산을 번들합니다 — upstream 프로젝트
넷, 라이선스 셋(OFL 1.1 · MIT · BSD-3-Clause)입니다.

| upstream | 버전 | 라이선스 | 들어가는 곳 | 라이선스 본문 |
|---|---|---|---|---|
| D2Coding ligature | Ver 1.3.2 | OFL 1.1 | 본체 바이너리 (폰트 2 개) | [`LICENSES/D2Coding-OFL.txt`](LICENSES/D2Coding-OFL.txt) |
| mermaid | 11.16.1 | MIT | markdown plugin 바이너리 | [`LICENSES/mermaid-MIT.txt`](LICENSES/mermaid-MIT.txt) |
| highlight.js | 11.10.0 | BSD-3-Clause | markdown plugin 바이너리 | [`LICENSES/highlightjs-BSD-3-Clause.txt`](LICENSES/highlightjs-BSD-3-Clause.txt) |
| KaTeX | 0.18.4 | MIT | markdown plugin 바이너리 (JS · CSS · 폰트 20 개) | [`LICENSES/KaTeX-MIT.txt`](LICENSES/KaTeX-MIT.txt) |

`LICENSES/` 의 본문은 upstream 이 둔 라이선스 파일을 한 글자도 고치지 않고 옮긴 것입니다.
mermaid · highlight.js · KaTeX 는 **그 버전의 태그**에 둔 `LICENSE` 파일입니다. D2Coding 은
예외입니다 — 번들한 Ver 1.3.2 의 태그(`VER1.3.2`)와 그 릴리스 zip 에는 라이선스 파일이 없어서,
upstream 이 라이선스 파일을 처음 둔 다음 태그 `VER1.3.3` 의 `OFL.txt` 를 옮겼습니다(아래 D2Coding
절). 저작권 줄이 프로젝트마다 다르므로 일반 라이선스 템플릿으로 대신하지 않습니다.

## 폰트

### D2Coding (D2Coding ligature)

- 출처: NAVER Corporation — https://github.com/naver/d2-coding-font (옛 이름 `naver/d2codingfont` 는 이 주소로 넘어갑니다)
- 버전: Ver 1.3.2 (2018-05-24)
- 적용 범위: `crates/tasty-font/assets/D2Coding-ligature-Regular.ttf`, `crates/tasty-font/assets/D2Coding-ligature-Bold.ttf`
- 라이선스: SIL Open Font License, Version 1.1 (OFL 1.1)
- 라이선스 본문: [`LICENSES/D2Coding-OFL.txt`](LICENSES/D2Coding-OFL.txt) — upstream 태그 `VER1.3.3`
  (커밋 `d95bc36438113099566bb39064334608955c2612`)의 `OFL.txt` 와 바이트 동일
  (`https://raw.githubusercontent.com/naver/d2-coding-font/VER1.3.3/OFL.txt`, sha256
  `1807e8dec4d65f474cbf9be39f5e2254ecb81702babc320749e272ea66ffcc69`). 태그 `VER1.3.2` 에는
  라이선스 파일이 없고, 그 시점 README 가 라이선스로 링크한 upstream wiki 의 `Open-Font-License`
  문서도 같은 두 저작권 줄을 적고 있습니다.
- 저작권: Copyright (c) 2015, NAVER Corporation — 위 파일 머리의 두 줄을 그대로 따릅니다.
- Reserved Font Name: `D2Coding`, `D2Coding-Bold`

OFL 1.1은 폰트의 사용·변경·재배포(상용 포함)를 허용하지만 다음을 요구합니다.

- 라이선스 본문(`LICENSES/D2Coding-OFL.txt`)을 함께 배포할 것.
- Reserved Font Name(`D2Coding` · `D2Coding-Bold`)을 보존할 것. 폰트 파일을 수정·재포장하여 재배포하려는 경우 파생물에 다른 이름을 사용해야 합니다. 본 저장소는 NAVER 공식 ttf를 그대로 번들하므로 원래 이름을 사용할 수 있습니다.

## markdown plugin 의 렌더링 엔진

markdown plugin(`crates/tasty-plugin-markdown`)은 아래 세 엔진을 **컴파일 시점에 plugin
바이너리 안으로** 임베드합니다(`include_str!` 4 개 · `include_bytes!` 20 개, 파일로 24 개).
실행 중 네트워크로 받는 것은 없습니다. 각 파일의 출처 URL · sha512 · 갱신 절차는
[`crates/tasty-plugin-markdown/assets/NOTICE.md`](crates/tasty-plugin-markdown/assets/NOTICE.md)
가 정본이고, 이 절은 배포 의무에 필요한 것만 옮깁니다.

### mermaid

- 출처: https://github.com/mermaid-js/mermaid (npm `mermaid`)
- 버전: 11.16.1
- 적용 범위: `crates/tasty-plugin-markdown/assets/mermaid.min.js`
- 라이선스: MIT — Copyright (c) 2014 - 2022 Knut Sveidqvist
- 라이선스 본문: [`LICENSES/mermaid-MIT.txt`](LICENSES/mermaid-MIT.txt) (태그 `mermaid@11.16.1` 의 `LICENSE`)
- 번들 안에 다른 MIT 조각(jQuery 이벤트 모듈 등)의 고지가 주석으로 들어 있고, 그 주석은
  `mermaid.min.js` 안에 그대로 보존돼 있습니다.

### highlight.js

- 출처: https://github.com/highlightjs/highlight.js (cdnjs 의 prebuilt "common" 번들)
- 버전: 11.10.0
- 적용 범위: `crates/tasty-plugin-markdown/assets/highlight.min.js`
- 라이선스: BSD-3-Clause — Copyright (c) 2006, Ivan Sagalaev
- 라이선스 본문: [`LICENSES/highlightjs-BSD-3-Clause.txt`](LICENSES/highlightjs-BSD-3-Clause.txt) (태그 `11.10.0` 의 `LICENSE`)
- 번들 파일 머리의 배너(`(c) 2006-2024 Josh Goebel <hello@joshgoebel.com> and other contributors`)도
  고치지 않고 보존돼 있습니다. BSD-3-Clause 는 바이너리 재배포 시 위 저작권 고지·조건·면책 문구를
  배포물에 함께 넣을 것을 요구하고, 저작권자의 이름을 파생물 홍보에 쓰지 못하게 합니다.

### KaTeX

- 출처: https://github.com/KaTeX/KaTeX (npm `katex`)
- 버전: 0.18.4
- 적용 범위: `crates/tasty-plugin-markdown/assets/katex.min.js`, `crates/tasty-plugin-markdown/assets/katex.min.css`, `crates/tasty-plugin-markdown/assets/fonts/KaTeX_*.woff2` (20 개)
- 라이선스: MIT — Copyright (c) 2013-2020 Khan Academy and other contributors
- 라이선스 본문: [`LICENSES/KaTeX-MIT.txt`](LICENSES/KaTeX-MIT.txt) (태그 `v0.18.4` 의 `LICENSE`)
- 폰트는 같은 저장소 전체의 MIT 가 덮습니다 — 그 `LICENSE` 파일에 폰트만 따로 정한 조항은
  없습니다.

MIT 는 위 저작권 고지와 허가 문구를 "소프트웨어의 모든 사본 또는 상당 부분" 에 포함할 것을
요구합니다. plugin 바이너리가 그 사본이므로, 본문을 산출물에 함께 넣는 것이 그 요구의 형태입니다.

## 고지 세트

배포되는 고지는 다음이며, 전부 저장소에 그대로 들어 있습니다. 릴리스 시점에 새로 생성하는
단계는 없습니다 — 근거는
[ADR-0051](docs/adr/0051-release-artifacts-and-versioning.md).

- `LICENSE` — Tasty 자체 코드의 MIT 본문
- `THIRD_PARTY_LICENSES.md` — 본 문서 (무엇이 번들되고 무슨 의무가 따르는지)
- `LICENSES/` 의 **모든 파일** — 번들 자산의 라이선스 본문. 지금은 위 표의 네 파일입니다.

세트의 셋째 항목은 파일 이름이 아니라 **디렉토리**로 정합니다. 새 자산이 들어와 본문이 하나
늘면 그 파일을 `LICENSES/` 에 넣고 위 표에 한 줄을 더하는 것이 저장소 쪽 할 일의 전부입니다.

## 산출물별 위치

| 산출물 | 고지 세트의 자리 |
|---|---|
| GitHub 릴리스 | 고지 세트를 릴리스 에셋으로 올리도록 해 두었습니다(`LICENSES/` 의 파일은 디렉토리 없이 이름만으로 올라갑니다). **그 설정으로 발행된 릴리스는 아직 없습니다.** |
| Windows `.msi` | 설치 디렉토리(`<설치 폴더>\tasty\`) 최상단, `LICENSES\` 하위 경로 유지. MIT 본문은 설치 동의 화면용 `License.rtf` 로도 한 번 더 들어갑니다. **설치 파일 정의와 빌드 스크립트의 확인을 넣었지만, 그렇게 만든 `.msi` 를 열어 본 적은 아직 없습니다** — Windows 빌더에서만 만들 수 있습니다. |
| Linux `tar.gz` | 압축을 풀면 나오는 디렉토리 최상단 |
| Linux `.deb` | `/usr/share/doc/tasty/` (Debian 관례. `LICENSES/` 하위 경로를 그대로 유지합니다) |
| Linux `.rpm` | `/usr/share/licenses/tasty/` (RPM 관례라 deb 과 배치가 다릅니다 — `LICENSES/` 의 본문도 하위 디렉토리 없이 이 자리에 바로 놓입니다) |
| Linux `.AppImage` | `usr/share/licenses/tasty/` |
| macOS `.dmg` | `Tasty.app/Contents/Resources/` (`LICENSES/` 하위 경로를 그대로 유지합니다). **스크립트에 스테이징과 확인을 넣었지만, 그렇게 만든 `.dmg` 를 열어 본 적은 아직 없습니다** — macOS 빌더에서만 만들 수 있습니다. |
| Windows `.zip` | 압축을 풀면 나오는 최상단(`tasty.exe` 옆). **`.msi` 와 같이 배선만 됐고 열어 본 적은 아직 없습니다.** |

산출물 자체에 동봉되는 것이 OFL 1.1 · MIT · BSD-3-Clause 가 요구하는 형태입니다 — 릴리스
에셋으로 따로 내려받을 수 있는 것은 그 요구를 대신하지 못합니다.
