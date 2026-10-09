# 디자인 시스템 vendor

Claude Design 프로젝트 **Tasty Design System**에서 받아온 디자인 사본이다. 사이트의 랜딩 데모와 가이드 그림은 이 사본의 컴포넌트를 사용한다. 실제 앱 구현과 자동으로 동기화되지는 않는다.

디자인 수정은 원격 킷에 먼저 반영한 뒤 이 사본을 갱신한다. **이 디렉토리의 파일을 직접 고치지 않는다.** 아래 "원본과 다르게 둔 자리"의 예외와 이 README만 직접 수정할 수 있다.

## vendor 갱신 절차

디자인이 바뀌면 사이트 사본도 갱신한다. 빌드와 아래 비교 검사는 일부 차이만 검출하므로, 통과했다고 사본이 최신인 것은 아니다.

앱 토큰 사본의 갱신 절차는 `crates/tasty-design-tokens/README.md`에 있다. 앱은 `tokens/tasty.tokens.json` 한 파일을 가져오지만, 사이트는 원격 파일 목록을 비교하고 트리 전체를 가져온 뒤 로컬 변형을 다시 적용한다. 결정 이유와 대안은 `docs/adr/0035-shared-design-and-theme.md`에 있다.

1~3단계는 DesignSync로 원격 프로젝트에 접근할 수 있어야 한다. 접근할 수 없으면 갱신하지 못한 범위를 보고한다. 4단계부터는 로컬 사본만으로 실행할 수 있다.

1. **파일 목록을 비교한다.** `DesignSync.list_files`로 받은 원격 경로와 `find site/vendor -type f`로 얻은 로컬 목록을 비교한다. 새 파일, 삭제된 파일, 이름이 바뀐 파일을 확인한다. 아래 "원본에서 제외한 것"에 해당하는 경로는 비교에서 뺀다.
2. **변경된 파일을 받는다.** `DesignSync.get_file`로 파일을 하나씩 받아 같은 상대 경로에 쓴다. 원격에서 삭제된 파일은 사본에서도 지운다. `get_file`의 256 KiB 상한을 넘는 파일은 별도로 처리하고 그 사실을 기록한다.
   - 사본에는 이번 갱신 작업을 시작한 뒤 받은 `get_file` 결과만 쓴다. 같은 세션에서 앞선 작업이 받은 결과는 원격의 옛 판일 수 있다.
   - 같은 경로를 두 번 이상 받았다면 시간순으로 마지막 결과를 쓰고, 중복된 경로와 횟수를 출력해 확인한다. 결과를 파일에서 다시 모으는 방식은 시간순을 보장하지 않으므로 순서를 직접 확인한다.
3. **로컬 변형을 다시 적용한다.** 아래 "원본과 다르게 둔 자리"의 세 가지(로컬 문서 인용, 옛 ADR 번호 인용, 출처 메타데이터 제거)를 적용한다. `cargo test -p tasty-doc-guards --test no_todo_file_citation`으로 로컬 작업 문서 인용이 남지 않았는지 확인한다.
   - 그다음 사본을 원격과 대조한다. 범위는 최소한 디자인 작업 결과(`.DONE`)가 적은 파일과, 그 파일을 쓰는 갤러리 페이지가 불러오는 파일이다. 가능하면 vendor 전체를 원격과 바이트 단위로 비교한다.
   - 다른 곳이 있으면 각 차이가 "원본과 다르게 둔 자리"의 변형인지 줄 단위로 확인한다. 변형으로 설명되지 않는 차이는 옛 판이 남은 것이므로 원격 판으로 다시 받는다.
4. **토큰과 아이콘을 비교한다.** 다음 검사를 실행한다.

   ```sh
   cargo test -p tasty-doc-guards --test site_vendor_tokens_track_the_app_export --test site_vendor_icons_match_the_app_transcription
   ```

   - 토큰 검사는 앱 사본과 사이트 사본의 **토큰 이름 집합** 차이를 명부와 비교한다. 갱신으로 해소된 차이는 명부에서 지운다.
   - 아이콘 검사는 `icons/*.svg`, `components/core/Icon.jsx`의 `ICON_PATHS`, 앱의 `crates/tasty-icons/src/lib.rs`에 있는 기하를 비교한다. 이름 대응은 명부를 사용한다. 킷의 `list`는 앱의 `log`에, `listView`는 앱의 `list`에 해당한다. 아이콘 추가·삭제 시 명부도 갱신한다.
   - `<svg>`의 `viewBox`, 선 굵기, cap/join도 비교한다. 의도적인 색(`stroke`/`fill`) 차이는 사유와 함께 명부에 남기고, 해소되면 지운다.
   - 이 검사들은 토큰·아이콘을 바꾸지 않는 문구·구성·컨트롤 변경을 검출하지 않는다. **최신 여부는 1~3단계에서 원격과 직접 비교한다.**
5. **사이트를 빌드한다.** `cd site && npm run build`. 변환기(`scripts/vendor-to-esm.mjs`)가 처리하지 못하는 파일 형식이 없는지 확인한다. 변환기는 파일 전체를 감싼 IIFE, `window.TastyKit`의 구조 분해와 `<window.TastyKit.X />`, `window.TastyKit && window.TastyKit.X` 로드 순서 가드, `const X = window.<전역>.Y;` 한 줄 별칭, 전역 전체를 받는 `const X = window.<전역>;`(네임스페이스 import), 갤러리 파일이 `window.<이름> = <이름>;`으로 내보낸 컴포넌트를 모듈 import로 바꾼다. 킷 파일이 `window.<전역>.Y`를 직접 참조하면서 같은 이름을 로컬로 선언했다면 import에 별칭을 붙인다. 다른 페이지가 내보내기만 쓰려고 불러오는 페이지의 `if (!window.__플래그) Gallery.mount(` 조건은 지우고 페이지 정의를 내보낸다. 새 갤러리 페이지는 `site/src/lib/design.ts`의 목록과 소개 문구, `site/src/pages/design/gallery/<페이지>/index.astro` 경로 파일을 함께 추가한다. 갤러리의 Stage는 화면 근처에 올 때만 그리므로 빌드 성공만으로 예제가 그려진다고 보지 않는다. 새 절은 `npm run preview`로 띄워 해당 앵커로 이동해 확인한다.
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
- **`tokens/tasty.tokens.json`**: DTCG 원본이다. 앱이 가져오는 사본은 `crates/tasty-design-tokens/dtcg/tasty.tokens.json`에 두고(갱신 절차는 `crates/tasty-design-tokens/README.md`), 사이트는 CSS 토큰만 쓴다. `tokens/fonts.css`는 위 웹폰트 항목으로 제외한다.
- **컴포넌트 보조 파일**: `components/**/*.prompt.md`(디자인 도구용 사용 안내)와 `components/*/*.card.html`(디자인 시스템 패널 카드)은 사이트가 쓰지 않는다.
- **`changelog/`**: 원격 디자인 결정문이다. 저장소의 결정은 `docs/`와 ADR에 둔다.
- **루트 문서와 도구 파일**: `readme.md`, `FEATURES.md`, `CLAUDE.md`, `SKILL.md`, `TOKENS.md`, `Canvas.dc.html`, `thumbnail.html`, `.thumbnail`, `support.js`, `_ds_manifest.json`, `_adherence.oxlintrc.json`. 원격 프로젝트 운영용이며 사이트가 쓰지 않는다. 진입점 `styles.css`만 가져온다.
- **작업 폴더**: `design-request/`, `design-tasks/`, `explorations/`, `scraps/`, `screenshots/`, `uploads/`. 요청·작업 기록과 시안 초안이다.
- **`fonts/`, `assets/icons/`**: 폰트 원본은 웹폰트 항목으로 제외한다. `assets/icons/`는 앱 아이콘 원본(`.icns`·`.ico`·PNG·멜론 SVG)과 보조 SVG라 사이트가 쓰지 않는다. `assets/screens/`는 `screens/`로 가져온다.

## 원본과 다르게 둔 자리

기본적으로 원격 파일을 그대로 가져온다. 갱신할 때는 다음 예외를 다시 적용한다.

- **커밋되지 않는 로컬 문서 인용**: 원격 주석·노트에 있는 로컬 작업 폴더와 티켓 인용은 뜻만 남기고 고친다. `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`가 검사한다. 렌더링 구조와 값은 유지한다.
  - `ui_kits/terminal/overlays/settings_window.jsx`: Hook Handlers 서브탭 설명의 "see … todo"를 "not yet built"로 바꾼다.
  - `gallery/components.jsx`: AutoComplete 노트의 "tracked as a separate implementation TODO"를 "tracked as separate implementation work"로 바꾼다.
  - `tokens/semantic.css`: `--tasty-text-disabled` 주석 끝의 원격 changelog 인용("see changelog/…")을 지운다.
  - 원격 요청문서 인용(`design-request/…`)을 지운다. 날짜가 함께 적혀 있으면 날짜만 남긴다("Design request: … (날짜 answer)." 줄은 "Designed 날짜."로 바꾼다). 대상은 다음과 같다.
    - `ui_kits/terminal/overlays/info_modal.jsx`: 머리 주석, Title bar buttons 주석, Narrow popups 주석.
    - `ui_kits/terminal/overlays/settings_window.jsx`: File Extension Mapping 주석.
    - `ui_kits/terminal/overlays/html_script_banner.jsx`, `ui_kits/terminal/overlays/macos_permissions.jsx`: 머리 주석의 "Design request:" 줄.
    - `ui_kits/terminal/overlays/dag_view.jsx`, `gallery/dag.jsx`, `ui_kits/terminal/overlays/kb_import_export.jsx`, `ui_kits/terminal/overlays/kb_plugins_subtab.jsx`: 머리 주석.
    - `ui_kits/terminal/overlays/port_scanner.jsx`: FAVORITES 주석.
- **옛 ADR 번호 인용**: 원격 사본은 저장소의 ADR을 재정리하기 전 번호를 인용한다. 이 번호는 지금 저장소의 ADR과 맞지 않으므로 인용을 지우거나 현재 ADR·기능 문서로 바꾸고 뜻은 남긴다. `gallery/components.jsx`, `gallery/layouts.jsx`, `gallery/overlays-windows.jsx`, `ui_kits/terminal/overlays/settings_window.jsx`, `ui_kits/terminal/overlays/plugins_window.jsx`에 있다. 갱신할 때는 `git diff`로 이전 사본의 처리 방식을 확인해 같은 방식으로 적용한다.
- **출처 메타데이터**: 렌더링에 쓰지 않는 base64 C2PA 매니페스트가 사본 크기를 늘리므로 `<metadata>` 요소와 `xmlns:c2pa` 속성을 제거한다. 렌더링 구조와 값은 유지한다. `grep -rl c2pa site/vendor/ --exclude=README.md`의 출력이 없어야 한다. README는 절차 설명이므로 이 검사와 사본 수정일 계산에서 제외한다.

## 통합 시 주의

`ui_kits/terminal/app.jsx`는 마운트할 때 `document.documentElement.dataset.theme`을 자신의 state로 덮어쓴다. 사이트는 별도 래퍼에서 테마를 props로 전달해 사이트 테마 토글과 충돌하지 않도록 한다.
