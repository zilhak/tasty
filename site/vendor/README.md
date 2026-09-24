# 디자인 시스템 vendor

Claude Design 프로젝트 **Tasty Design System**에서 받아온 디자인 사본이다. 사이트의 랜딩 데모와 가이드 그림은 이 사본의 컴포넌트를 사용한다. 실제 앱 구현과 자동으로 동기화되지는 않는다.

디자인 수정은 원격 킷에 먼저 반영한 뒤 이 사본을 갱신한다. **이 디렉토리의 파일을 직접 고치지 않는다.** 아래 "원본과 다르게 둔 자리"의 예외와 이 README만 직접 수정할 수 있다.

## vendor 갱신 절차

디자인이 바뀌면 사이트 사본도 갱신한다. 빌드와 아래 비교 검사는 일부 차이만 검출하므로, 통과했다고 사본이 최신인 것은 아니다.

앱 토큰 사본의 갱신 절차는 `crates/tasty-design-tokens/README.md`에 있다. 앱은 `tokens/tasty.tokens.json` 한 파일을 가져오지만, 사이트는 원격 파일 목록을 비교하고 트리 전체를 가져온 뒤 로컬 변형을 다시 적용한다. 결정 이유와 대안은 `docs/adr/0035-shared-design-and-theme.md`에 있다.

1~3단계는 DesignSync로 원격 프로젝트에 접근할 수 있어야 한다. 접근할 수 없으면 갱신하지 못한 범위를 보고한다. 4단계부터는 로컬 사본만으로 실행할 수 있다.

1. **파일 목록을 비교한다.** `DesignSync.list_files`로 받은 원격 경로와 `find site/vendor -type f`로 얻은 로컬 목록을 비교한다. 새 파일, 삭제된 파일, 이름이 바뀐 파일을 확인한다. 아래 "원본에서 제외한 것"에 해당하는 경로는 비교에서 뺀다.
2. **변경된 파일을 받는다.** `DesignSync.get_file`로 파일을 하나씩 받아 같은 상대 경로에 쓴다. 원격에서 삭제된 파일은 사본에서도 지운다. `get_file`의 256 KiB 상한을 넘는 파일은 별도로 처리하고 그 사실을 기록한다.
3. **로컬 변형을 다시 적용한다.** 아래 "원본과 다르게 둔 자리"의 문구 변경 2곳과 출처 메타데이터 제거를 적용한다. `cargo test -p tasty-doc-guards --test no_todo_file_citation`으로 로컬 작업 문서 인용이 남지 않았는지 확인한다.
4. **토큰과 아이콘을 비교한다.** 다음 검사를 실행한다.

   ```sh
   cargo test -p tasty-doc-guards --test site_vendor_tokens_track_the_app_export --test site_vendor_icons_match_the_app_transcription
   ```

   - 토큰 검사는 앱 사본과 사이트 사본의 **토큰 이름 집합** 차이를 명부와 비교한다. 갱신으로 해소된 차이는 명부에서 지운다.
   - 아이콘 검사는 `icons/*.svg`, `components/core/Icon.jsx`의 `ICON_PATHS`, 앱의 `crates/tasty-icons/src/lib.rs`에 있는 기하를 비교한다. 이름 대응은 명부를 사용한다. 킷의 `list`는 앱의 `log`에, `listView`는 앱의 `list`에 해당한다. 아이콘 추가·삭제 시 명부도 갱신한다.
   - `<svg>`의 `viewBox`, 선 굵기, cap/join도 비교한다. 의도적인 색(`stroke`/`fill`) 차이는 사유와 함께 명부에 남기고, 해소되면 지운다.
   - 이 검사들은 토큰·아이콘을 바꾸지 않는 문구·구성·컨트롤 변경을 검출하지 않는다. **최신 여부는 1~3단계에서 원격과 직접 비교한다.**
5. **사이트를 빌드한다.** `cd site && npm run build`. 변환기(`scripts/vendor-to-esm.mjs`)가 처리하지 못하는 파일 형식이 없는지 확인한다.
6. `site/vendor/` 변경, 필요한 명부 갱신과 변환기 수정을 같은 커밋에 담는다. 생성된 `site/src/{ds,kit,gallery}/` 등은 커밋하지 않는다.

## 구성

| 경로 | 내용 |
|------|------|
| `styles.css` | 토큰 진입점. 사용하는 쪽은 이 파일만 링크한다 |
| `tokens/` | 3단계 토큰: `primitives` → `semantic` → `components`, 공통 `base` |
| `components/` | 공용 위젯(Button, Tab, Kbd, Badge, Table 등) |
| `ui_kits/terminal/` | 앱 셸(`app`, `chrome`, `work`, `titlebar/`)과 오버레이 |
| `icons/` | 아이콘 SVG |
| `screens/` | mocha UI 스크린샷. 라이트 테마 대응본은 없음 |
| `guidelines/` | 독립 HTML 디자인 문서. 첫 줄 `@dsCard` 주석에 그룹·이름·뷰포트를 기록함 |
| `gallery/` | 컴포넌트 갤러리. 전역 객체 참조를 모듈로 변환해 사용함. `gallery.css`는 갤러리 페이지에서만 로드함 |

## 원본에서 제외한 것

- **웹폰트(D2Coding, 8.3MB)**: `styles.css`의 `@import url("tokens/fonts.css")`를 제외한다. sans는 시스템 폰트를 사용하고 mono는 `"SF Mono"`, `"Cascadia Code"` 등의 대체 폰트를 사용한다. 브랜드 글꼴 통일이 필요하면 woff2 서브셋 도입을 검토한다.
- **`_ds_bundle.js`**: 브라우저에서 직접 실행하는 전역 번들이다. 사이트는 `components/`를 직접 번들하므로 필요 없다.
- **`icons.json`**: `icons/*.svg`에서 생성한 아이콘 이름·그룹·역할·`paths`·`fill` 명부다. 사이트는 `ICON_PATHS`를 사용하므로 추가 사본을 두지 않는다. 앱 아이콘은 이 명부를 참고해 옮긴 것이며, `crates/tasty-doc-guards/tests/site_vendor_icons_match_the_app_transcription.rs`가 `ICON_PATHS`와 앱 구현을 비교한다.
- **프리뷰 `index.html`**: 원격 킷의 번들에 의존한다. `ui_kits/terminal/overlays/*.html`, `titlebar/*.html`의 단독 미리보기와 킷 `README.md`도 제외한다. 사이트는 `.jsx`를 변환해 사용한다.

## 원본과 다르게 둔 자리

기본적으로 원격 파일을 그대로 가져온다. 갱신할 때는 다음 예외를 다시 적용한다.

- **커밋되지 않는 로컬 문서 인용**: 원격 주석·노트에 있는 로컬 작업 폴더와 티켓 인용은 뜻만 남기고 고친다. `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`가 검사한다. 렌더링 구조와 값은 유지한다.
  - `ui_kits/terminal/overlays/settings_window.jsx`: Hook Handlers 서브탭 설명의 "see … todo"를 "not yet built"로 바꾼다.
  - `gallery/components.jsx`: AutoComplete 노트의 "tracked as a separate implementation TODO"를 "tracked as separate implementation work"로 바꾼다.
- **출처 메타데이터**: 렌더링에 쓰지 않는 base64 C2PA 매니페스트가 사본 크기를 늘리므로 `<metadata>` 요소와 `xmlns:c2pa` 속성을 제거한다. 렌더링 구조와 값은 유지한다. `grep -rl c2pa site/vendor/ --exclude=README.md`의 출력이 없어야 한다. README는 절차 설명이므로 이 검사와 사본 수정일 계산에서 제외한다.

## 통합 시 주의

`ui_kits/terminal/app.jsx`는 마운트할 때 `document.documentElement.dataset.theme`을 자신의 state로 덮어쓴다. 사이트는 별도 래퍼에서 테마를 props로 전달해 사이트 테마 토글과 충돌하지 않도록 한다.
