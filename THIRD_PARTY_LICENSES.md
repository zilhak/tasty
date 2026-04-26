# Third-Party Licenses

Tasty 자체 코드는 `LICENSE` 파일에 명시된 MIT 라이선스를 따릅니다.
다만 Tasty는 별도 라이선스를 가진 다음 third-party 자산을 번들합니다.

## 폰트

### D2Coding (D2Coding ligature)

- 출처: NAVER Corp. — https://github.com/naver/d2codingfont
- 버전: Ver 1.3.2 (2018-05-24)
- 적용 범위: `assets/fonts/D2Coding-ligature-Regular.ttf`, `assets/fonts/D2Coding-ligature-Bold.ttf`
- 라이선스: SIL Open Font License, Version 1.1 (OFL 1.1)
- 라이선스 본문: [`LICENSES/D2Coding-OFL.txt`](LICENSES/D2Coding-OFL.txt)
- Reserved Font Name: `D2Coding`

OFL 1.1은 폰트의 사용·변경·재배포(상용 포함)를 허용하지만 다음을 요구합니다.

- 라이선스 본문(`LICENSES/D2Coding-OFL.txt`)을 함께 배포할 것.
- Reserved Font Name(`D2Coding`)을 보존할 것. 폰트 파일을 수정·재포장하여 재배포하려는 경우 파생물에 다른 이름을 사용해야 합니다. 본 저장소는 NAVER 공식 ttf를 그대로 번들하므로 원래 이름을 사용할 수 있습니다.

## 릴리스 시 체크리스트

릴리스 산출물(zip/installer/tar 등)을 생성할 때 다음 파일이 반드시 함께 포함되어야 합니다.

- `LICENSES/D2Coding-OFL.txt`
- `THIRD_PARTY_LICENSES.md` (본 문서)

`.github/workflows/`에 릴리스 자동화가 추가되는 시점에 위 두 파일이 에셋으로 업로드되도록 설정합니다.
