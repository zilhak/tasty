# 공개 사이트 (GitHub Pages)

`docs/` 트리와 랜딩 페이지를 정적 HTML 로 발행하는 경로. 산출물은 GitHub Pages 로 배포된다.

- 생성기: `site/` (Rust, **workspace exclude**)
- 산출물: `_site/` (gitignore)
- 배포: `.github/workflows/pages.yml` (main 푸시 시 자동)

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

# 깨진 상대 링크를 실패로 승격 (CI 가 쓰는 형태). stale 번역은 실패 사유가 아니다.
cargo run --manifest-path site/Cargo.toml -- --strict

# 번역 파일에 원본 해시 스탬프 (아래 "번역 모델")
cargo run --manifest-path site/Cargo.toml -- --stamp docs/en/index.md

# 로컬 확인
python3 -m http.server 8899 --directory _site
```

## URL 구조

| 경로 | 내용 |
|------|------|
| `/` | 영어 랜딩 |
| `/ko/` | 한국어 랜딩 |
| `/docs/**` | `docs/**.md` 를 1:1 로 전사한 문서 (한국어, canonical) |
| `/en/docs/**` | 영어 문서 트리. `docs/en/**.md` 가 있으면 번역, 없으면 한국어 본문 + 배너 |
| `/assets/` | `style.css` · `site.js` · 로고 · `search-index.json` · `search-index.en.json` |

## 번역 모델

한국어(`docs/`)가 canonical 이고 영어(`docs/en/`)가 번역이다. 다른 다국어 문서
프로젝트(Kubernetes · Vue · React 문서)와 같은 골격 — 방향만 반대다.

- **경로 1:1** — `docs/en/<rel>` 이 `docs/<rel>` 을 그대로 미러한다.
  `docs/features/terminal/index.md` 의 번역은 `docs/en/features/terminal/index.md`.
- **폴백** — 번역이 없는 문서도 `/en/docs/` 에 발행된다. 한국어 본문 위에
  "아직 번역되지 않았다" 배너가 붙는다. 따라서 영어 트리는 **항상 완전**하고,
  번역이 채워지는 만큼 배너가 사라진다. 100% 를 기다릴 필요가 없다.
- **스탬프** — 번역 파일 첫 줄에 원본의 내용 해시를 박는다:

  ```
  <!-- source-hash: 90acc4edaa45 -->
  # Tasty Documentation
  ```

  원본이 바뀌어 해시가 달라지면 그 페이지에 "원문보다 오래됐다" 배너가 뜨고,
  생성기가 `stale: docs/en/…` 목록을 출력한다. 스탬프가 없는 번역은 stale 로 본다.
- **링크는 원본과 동일하게 쓴다.** 영어 페이지의 상대 링크는 *한국어 위치 기준*으로
  해석된다. `docs/en/dev-guide/x.md` 에서 `../../CLAUDE.md` 라고 쓰면
  (`docs/en/` 이 한 단계 깊은데도) 저장소 루트의 `CLAUDE.md` 로 간다. 번역자는
  원본의 링크를 손대지 않고 그대로 둔다.
- **고아 번역** — 대응하는 한국어 원본이 없는 `docs/en/` 파일은 경고를 내고
  발행되지 않는다. 원본을 옮기거나 지웠으면 번역도 함께 옮기거나 지운다.

### 번역 절차

```bash
# 1. 원본을 같은 상대경로에 번역해 쓴다 (링크는 그대로)
$EDITOR docs/en/features/terminal/index.md

# 2. 스탬프를 찍는다 (첫 줄에 삽입하거나, 이미 있으면 갱신)
cargo run --manifest-path site/Cargo.toml -- --stamp docs/en/features/terminal/index.md

# 3. 생성해서 stale/untranslated 집계를 확인한다
cargo run --manifest-path site/Cargo.toml
#   translations: 12/276 pages, 0 stale, 264 untranslated
```

원본을 고친 뒤에는 번역도 갱신하고 다시 `--stamp` 한다. 갱신 없이 스탬프만
찍으면 stale 표시만 사라지고 내용은 어긋난 채 남으므로, `--stamp` 는 **번역을
실제로 손본 직후에만** 실행한다.

**모든 내부 링크는 상대경로다.** 프로젝트 페이지의 base path(`/tasty/`) 든
커스텀 도메인이든 그대로 동작하며, `_site/` 를 로컬 파일로 열어도 깨지지 않는다.

## 문서 변환 규칙

생성기는 원본 마크다운을 고치지 않는다. 변환은 전부 렌더 단계에서 일어난다.

| 원본 | 산출 |
|------|------|
| `docs/` 안의 `*.md` 링크 | 같은 상대경로 + `.html` |
| `docs/` 를 벗어나는 링크 (`../CLAUDE.md`, `crates/…`) | GitHub blob URL |
| 존재하지 않는 대상 | 경고 출력, `--strict` 면 실패 |
| `##` · `###` | 우측 목차 + 앵커. slug 는 GitHub 규칙(한글 보존) |
| 표 | 가로 스크롤 컨테이너로 감싼다 |
| 코드 펜스 | syntect 로 스코프 클래스(`hl-*`) 부여 — 색은 CSS 가 정한다 |

사이드바 카테고리와 순서, 언어별 라벨은 `site/src/main.rs` 의 `CATEGORIES` 에
있다. `docs/` 최상위에 새 디렉토리를 만들면 이 배열에도 추가해야 하며, 빠뜨리면
생성 중 `skipping uncategorised docs/…` 경고가 뜬다. `docs/en/` 은 번역 트리라
카테고리가 아니다.

## 테마

사이트 색은 앱이 실제로 쓰는 테마에서 그대로 가져온다.

- 다크(기본): `crates/tasty-themes/themes/mocha.toml`
- 라이트: `crates/tasty-themes/themes/latte.toml`

`site/static/style.css` 상단의 `:root` / `:root[data-theme="light"]` 블록이 두 파일의
값을 1:1 로 옮긴 것이다. **테마 TOML 을 고치면 이 블록도 함께 갱신한다.**
코드 하이라이팅 색도 같은 토큰(`--mauve` · `--green` · `--peach` …)을 참조하므로
테마 토글에 함께 반응한다.

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

선택은 `localStorage` 에 남고, 저장값이 없으면 `prefers-color-scheme` 을 따른다.
첫 페인트 전에 인라인 스크립트가 적용해 깜빡임이 없다.

## 배포

`main` 에 `docs/**` · `site/**` · 루트 `Cargo.toml` 변경이 푸시되면 워크플로가 돈다
(수동 실행도 가능). GitHub 저장소 설정에서 **Pages > Source 를 GitHub Actions** 로
두어야 동작한다.

배포 자체는 `--strict` 로 생성하므로, 깨진 문서 링크가 있으면 배포되지 않는다.
