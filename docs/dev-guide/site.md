# 공개 사이트 (GitHub Pages)

랜딩 페이지와 **사용자 가이드**를 정적 HTML 로 발행하는 경로. 산출물은 GitHub Pages 로 배포된다.

- 생성기: `site/src/` (Rust, **workspace exclude**)
- 콘텐츠: `site/content/` (한국어 정본) · `site/content/en/` (영어 번역)
- 산출물: `_site/` (gitignore)
- 배포: `.github/workflows/pages.yml` (main 에 `site/**` 변경이 푸시되면 자동)

**`docs/` 는 발행하지 않는다.** `docs/` 는 코드를 고치는 사람과 에이전트를 위한 명세·설계·ADR 이고,
사이트는 Tasty 를 받아서 쓰는 사람을 위한 것이라 독자가 다르다. 사이트가 실을 내용은 전부
`site/content/` 에 사용자 시점으로 따로 쓴다. 명세가 바뀌면 가이드도 같은 커밋에서 손본다
([`CLAUDE.md`](../../CLAUDE.md) "문서 갱신").

## 왜 workspace 밖인가

생성기는 `pulldown-cmark` · `syntect` 에 의존하는데, 본체와 공유하는 코드가 하나도 없다.
workspace member 로 두면 그 의존성이 `cargo build` / `cargo test --workspace` 의
의존성 해석에 섞여 cold build 가 느려진다. 루트 `Cargo.toml` 의 `exclude` 에 넣어
완전히 분리했다 (`crates/tasty-plugin-sdk-wasm` 과 같은 이유).

따라서 `cargo build --workspace` · `cargo clippy --workspace` · `cargo test --workspace`
는 `site/` 를 **보지 않는다.** 생성기를 만질 때는 `--manifest-path` 를 명시한다.

## 빌드

```bash
# _site/ 로 생성
cargo run --manifest-path site/Cargo.toml

# 출력 위치 지정
cargo run --manifest-path site/Cargo.toml -- --out /tmp/preview

# 깨진 상대 링크·ORDER 에 없는 페이지를 실패로 승격 (CI 가 쓰는 형태). stale 번역은 실패 사유가 아니다.
cargo run --manifest-path site/Cargo.toml -- --strict

# 번역 파일에 원본 해시 스탬프 (아래 "번역 모델")
cargo run --manifest-path site/Cargo.toml -- --stamp site/content/en/index.md

# 로컬 확인
python3 -m http.server 8899 --directory _site
```

## URL 구조

| 경로 | 내용 |
|------|------|
| `/` | 영어 랜딩 |
| `/ko/` | 한국어 랜딩 |
| `/guide/**` | 영어 가이드. `site/content/en/**.md` 가 있으면 번역, 없으면 한국어 본문 + 배너 |
| `/ko/guide/**` | 한국어 가이드 (`site/content/**.md`, canonical) |
| `/assets/` | `style.css` · `site.js` · 로고 · `search-index.json` (en) · `search-index.ko.json` |

영어가 기본 URL 인 이유는 공개 사이트의 첫 방문자 대부분이 영어권이고 랜딩도 `/` 가 영어라서다.
정본이 한국어인 것과는 별개다 — 작성은 한국어로, 노출 기본은 영어로.

## 콘텐츠 구조

`site/content/` 의 디렉토리가 사이드바 섹션이고, 페이지 순서는 `site/src/main.rs` 의 `ORDER` 가
정한다. 파일 이름 순이 아니다 — 읽는 순서가 의도이므로 배열로 박는다.

```
site/content/
  index.md                      가이드 홈
  getting-started/  install · first-look
  using/            workspaces · panes-tabs-splits · terminal · files
  customize/        keybindings · settings · themes · scripts
  agents/           cli · claude-codex · tasks · hooks-notifications
  remote/           attach
  plugins/          index
  help/             troubleshooting
  en/               위 트리의 1:1 미러 (번역)
```

- 새 페이지를 추가하면 `ORDER` 에도 넣는다. 빠뜨리면 경고와 함께 맨 뒤에 붙고 `--strict` 는 실패한다.
- 새 디렉토리(섹션)를 만들면 `SECTIONS` 에 (디렉토리, 한국어 라벨, 영어 라벨) 을 추가한다.
  등록되지 않은 디렉토리의 파일은 `skipping …` 경고와 함께 발행되지 않는다.
- 페이지 첫 줄 `# 제목` 이 사이드바 라벨이다 (괄호 · 대시 이후는 잘린다).

### 집필 규칙

독자는 **설치해서 쓰는 사람**이다. 다음은 가이드에 넣지 않는다: 소스 경로 · ADR · IPC 메서드
이름(CLI 명령으로 대신) · 내부 타입명 · Status/주체 같은 명세 머리 항목 · 수용 기준 · 구현 히스토리 ·
`docs/` 로의 링크. 메뉴 · 버튼 · 설정 항목은 `lang/ko.toml` 의 실제 라벨을 쓰고, 처음 등장할 때 영어
라벨을 HTML 주석으로 붙인다(`**설정** <!-- en: Settings -->`) — 번역이 그대로 쓴다.
확인 못 한 사실은 쓰지 말고 `<!-- TODO verify: … -->` 로 남긴다.

**문체는 합니다체다** — 마이크로소프트 한국어 스타일 가이드를 따른다. 사이트에 나가는 한국어는
가이드 본문도 랜딩 문구도 UI 문자열도 전부 `합니다` / `합니다체` 로 끝맺고, 사용자에게 시키는
문장은 `하십시오` 가 아니라 `하세요` 로 쓴다. 명사에 `수행` · `실행` · `제공` 을 붙이는 번역투는
동사로 푼다(`개요를 제공합니다` → `설명합니다`). 표 셀이나 목록의 명사구 조각은 그대로 둔다 —
종결어미가 있는 완전한 문장에만 해당한다. `docs/` 개발 문서는 이 규칙 밖이며 한다체를 유지한다
(독자가 다르다).

## 번역 모델

한국어(`site/content/`)가 canonical 이고 영어(`site/content/en/`)가 번역이다.

- **경로 1:1** — `site/content/en/<rel>` 이 `site/content/<rel>` 을 그대로 미러한다.
- **폴백** — 번역이 없는 페이지도 `/guide/` 에 발행된다. 한국어 본문 위에
  "아직 번역되지 않았습니다" 배너가 붙는다. 영어 트리는 **항상 완전**하다.
- **스탬프** — 번역 파일 첫 줄에 원본의 내용 해시를 박는다:

  ```
  <!-- source-hash: 90acc4edaa45 -->
  # Tasty guide
  ```

  원본이 바뀌어 해시가 달라지면 그 페이지에 "원문보다 오래됐습니다" 배너가 뜨고,
  생성기가 `stale:` 목록을 출력한다. 스탬프가 없는 번역은 stale 로 본다.
- **링크 경로는 원본과 동일하게, 앵커는 영어 제목의 slug 로 쓴다.** 영어 페이지의 상대 링크는
  *한국어 위치 기준*으로 해석되므로 파일 경로는 그대로 둔다. 반면 `#앵커` 는 제목 텍스트에서
  만들어지므로 번역된 페이지의 앵커는 영어 slug 다(`install.md#설치-위치` → `install.md#install-locations`).
  생성기가 언어별 산출 트리에서 모든 `#앵커` 의 존재를 검사해 없으면 깨진 링크로 집계한다
  (`--strict` 실패).
- **고아 번역** — 대응하는 한국어 원본이 없는 `en/` 파일은 경고를 내고 발행되지 않는다.

### 번역 절차

```bash
# 1. 원본을 같은 상대경로에 번역해 쓴다 (링크 경로는 그대로, 앵커는 영어 slug)
$EDITOR site/content/en/using/terminal.md

# 2. 스탬프를 찍는다 (첫 줄에 삽입하거나, 이미 있으면 갱신)
cargo run --manifest-path site/Cargo.toml -- --stamp site/content/en/using/terminal.md

# 3. 생성해서 stale/untranslated 집계를 확인한다
cargo run --manifest-path site/Cargo.toml
#   translations: 18/18 pages, 0 stale, 0 untranslated
```

`--stamp` 는 **번역을 실제로 손본 직후에만** 실행한다. 갱신 없이 스탬프만 찍으면 stale 표시만
사라지고 내용은 어긋난 채 남는다.

**모든 내부 링크는 상대경로다.** 프로젝트 페이지의 base path(`/tasty/`) 든
커스텀 도메인이든 그대로 동작하며, `_site/` 를 로컬 파일로 열어도 깨지지 않는다.

## 문서 변환 규칙

생성기는 원본 마크다운을 고치지 않는다. 변환은 전부 렌더 단계에서 일어난다.

| 원본 | 산출 |
|------|------|
| `site/content/` 안의 `*.md` 링크 | 같은 상대경로 + `.html` |
| 콘텐츠 트리를 벗어나는 링크 (`../../../CHANGELOG.md`) | GitHub blob URL |
| 존재하지 않는 대상 · 없는 `#앵커` | 경고 출력, `--strict` 면 실패 |
| `##` · `###` | 우측 목차 + 앵커. slug 는 GitHub 규칙(한글 보존) |
| 표 | 가로 스크롤 컨테이너로 감싼다 |
| 코드 펜스 | syntect 로 스코프 클래스(`hl-*`) 부여 — 색은 CSS 가 정한다 |

## 테마

사이트 색은 앱이 실제로 쓰는 테마에서 그대로 가져온다.

- 다크(기본): `crates/tasty-themes/themes/mocha.toml`
- 라이트: `crates/tasty-themes/themes/latte.toml`

`site/static/style.css` 상단의 `:root` / `:root[data-theme="light"]` 블록이 두 파일의
값을 1:1 로 옮긴 것이다. **테마 TOML 을 고치면 이 블록도 함께 갱신한다.**
코드 하이라이팅 색도 같은 토큰(`--mauve` · `--green` · `--peach` …)을 참조하므로
테마 토글에 함께 반응한다.

선택은 `localStorage` 에 남고, 저장값이 없으면 `prefers-color-scheme` 을 따른다.
첫 페인트 전에 인라인 스크립트가 적용해 깜빡임이 없다.

## 랜딩의 제품 창

랜딩 hero 아래의 창은 스크린샷이 아니라 Claude Design 프로젝트(`Tasty Design System`)의
`ui_kits/terminal` 킷을 **구조 전사**한 정적 HTML 이다 — `app.jsx` 의 합성 순서(타이틀바 →
사이드바 | 탭 스트립 → 분할 서피스 → 상태바)와 `chrome.jsx` · `work.jsx` · `components/`
(`Tab` · `StatusDot` · `Badge` · `Kbd`) 의 구조를 그대로 옮겼다. 렌더는 `site/src/landing.rs` 의
`mock()`, 스타일은 `style.css` 의 `.mock*` 블록.

- 치수는 전부 디자인 픽셀 단위 `--u`(컨테이너 폭 ÷ 1040, 상한 1px) 로 적어 창이 컨테이너에
  맞춰 축소된다. 560px 이하에서는 사이드바와 에이전트 서피스를 감춰 포커스된 터미널만 남긴다.
- 색은 `tokens/semantic.css` 의 역할 토큰(`bg-sidebar` · `surface-active` · `text-muted` · `separator`
  …)을 사이트 팔레트 변수에 별칭으로 매핑한다. 아이콘은 `icons/*.svg` 의 canonical 글리프를 인라인.
- 킷을 바꾸면 이 창도 같이 갱신한다 — 다른 방향으로는 흐르지 않는다(사이트가 디자인의 정본이
  아니다).

## 다운로드

랜딩이 제공하는 다운로드 경로는 둘뿐이다. **주 버튼**은 방문자 OS 의 대표 설치 파일을 직접
가리키고, 그 옆 **다른 플랫폼** 버튼은 최신 릴리스 페이지로 보낸다. OS·아키텍처·포맷 조합이 11 개라
랜딩에 다 늘어놓으면 hero 가 무거워지고 릴리스 페이지·[설치 가이드](../../site/content/getting-started/install.md)
와 삼중으로 겹친다 — 조합 선택은 그 두 곳이 맡는다.

자산 파일명에 버전이 들어 있어(`Tasty-0.10.2-macos-arm64.dmg`) 정적 링크로는 최신을 가리킬 수
없으므로, Pages 워크플로가 생성기 실행 전에 `gh release view --json tagName,assets > site/release.json`
으로 최신 릴리스를 받아두고 생성기가 그 파일을 읽는다(`main.rs` 의 `Release`). 로컬 빌드처럼 파일이
없으면 주 버튼도 릴리스 페이지로 폴백하고 생성기가 `release: none` 을 출력한다. 로컬에서 실제
링크를 보려면 같은 명령을 직접 실행해 두면 된다(`.gitignore` 대상).

- 주 버튼은 서버 렌더 시 릴리스 페이지를 가리키고, `site.js` 가 방문자 OS 를 감지해 대응 자산
  (`landing.rs` 의 `PRIMARY_FORMATS`: macOS `.dmg` · Windows `.msi` · Linux x86_64 `.AppImage`)으로
  바꾸고 라벨을 "{os} 용 다운로드" 로 바꾼다. 감지 실패나 JS 없음이면 릴리스 페이지 링크 그대로다.
- 릴리스 워크플로의 파일 이름 규칙([installation](../installation.md) 의 산출물 표)이 바뀌면
  `PRIMARY_FORMATS` 의 접미사도 맞춘다. 접미사가 어긋나면 그 OS 는 조용히 릴리스 페이지로 폴백한다.
- 자산 URL 은 `?v=` 없이 그대로 쓴다 — GitHub 가 제공하는 영구 URL 이다.

## 배포

`main` 에 `site/**` · 루트 `Cargo.toml` · 로고 변경이 푸시되면 워크플로가 돈다
(수동 실행도 가능). GitHub 저장소 설정에서 **Pages > Source 를 GitHub Actions** 로
두어야 동작한다.

배포 자체는 `--strict` 로 생성하므로, 깨진 가이드 링크나 `ORDER` 누락이 있으면 배포되지 않는다.
