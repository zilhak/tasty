# Third-Party Licenses

Tasty 자체 코드는 [`LICENSE`](LICENSE) 파일에 명시된 MIT 라이선스를 따릅니다.
다만 Tasty는 별도 라이선스를 가진 다음 third-party 자산을 번들합니다.

## 폰트

### D2Coding (D2Coding ligature)

- 출처: NAVER Corp. — https://github.com/naver/d2codingfont
- 버전: Ver 1.3.2 (2018-05-24)
- 적용 범위: `crates/tasty-font/assets/D2Coding-ligature-Regular.ttf`, `crates/tasty-font/assets/D2Coding-ligature-Bold.ttf`
- 라이선스: SIL Open Font License, Version 1.1 (OFL 1.1)
- 라이선스 본문: [`LICENSES/D2Coding-OFL.txt`](LICENSES/D2Coding-OFL.txt)
- Reserved Font Name: `D2Coding`

OFL 1.1은 폰트의 사용·변경·재배포(상용 포함)를 허용하지만 다음을 요구합니다.

- 라이선스 본문(`LICENSES/D2Coding-OFL.txt`)을 함께 배포할 것.
- Reserved Font Name(`D2Coding`)을 보존할 것. 폰트 파일을 수정·재포장하여 재배포하려는 경우 파생물에 다른 이름을 사용해야 합니다. 본 저장소는 NAVER 공식 ttf를 그대로 번들하므로 원래 이름을 사용할 수 있습니다.

## 고지 세트

배포되는 고지는 다음 세 파일이며, 전부 저장소에 그대로 들어 있습니다. 릴리스 시점에 새로
생성하는 단계는 없습니다 — 번들하는 제3자 자산이 폰트 하나뿐이라 목록이 사람이 관리하는
크기입니다.

- `LICENSE` — Tasty 자체 코드의 MIT 본문
- `LICENSES/D2Coding-OFL.txt` — 번들 폰트의 OFL 1.1 본문
- `THIRD_PARTY_LICENSES.md` — 본 문서 (무엇이 번들되고 무슨 의무가 따르는지)

## 산출물별 위치

| 산출물 | 고지 세트의 자리 |
|---|---|
| GitHub 릴리스 | 세 파일이 릴리스 에셋으로 그대로 올라갑니다 |
| Windows `.msi` | MIT 본문을 설치 동의 화면과 설치 디렉토리에 `License.rtf` 로 넣습니다. **제3자 고지(OFL)는 아직 안 들어갑니다.** |
| Linux `tar.gz` | 압축을 풀면 나오는 디렉토리 최상단 |
| Linux `.deb` | `/usr/share/doc/tasty/` (Debian 관례. `LICENSES/` 하위 경로를 그대로 유지합니다) |
| Linux `.rpm` | `/usr/share/licenses/tasty/` (RPM 관례라 deb 과 배치가 다릅니다) |
| Linux `.AppImage` | `usr/share/licenses/tasty/` |
| Windows `.zip` · macOS `.dmg` | **아직 안 들어갑니다.** |

산출물 자체에 동봉되는 것이 OFL 1.1 이 요구하는 형태입니다 — 릴리스 에셋으로 따로
내려받을 수 있는 것은 그 요구를 대신하지 못합니다.
