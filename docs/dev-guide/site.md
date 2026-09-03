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

# 깨진 상대 링크를 실패로 승격 (CI 가 쓰는 형태)
cargo run --manifest-path site/Cargo.toml -- --strict

# 로컬 확인
python3 -m http.server 8899 --directory _site
```

## URL 구조

| 경로 | 내용 |
|------|------|
| `/` | 영어 랜딩 |
| `/ko/` | 한국어 랜딩 |
| `/docs/**` | `docs/**.md` 를 1:1 로 전사한 문서 (한국어) |
| `/assets/` | `style.css` · `site.js` · 로고 · `search-index.json` |

레퍼런스 문서는 한국어로만 존재한다. 랜딩만 두 언어로 작성하고, 문서 트리는
한 벌만 발행한다 — 같은 내용을 두 경로로 중복 노출하지 않기 위해서다.
영어 랜딩은 문서가 한국어라는 사실을 명시한다.

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

사이드바 카테고리와 순서는 `site/src/main.rs` 의 `CATEGORIES` 에 있다.
`docs/` 최상위에 새 디렉토리를 만들면 이 배열에도 추가해야 하며, 빠뜨리면
생성 중 `skipping uncategorised docs/…` 경고가 뜬다.

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

## 배포

`main` 에 `docs/**` · `site/**` · 루트 `Cargo.toml` 변경이 푸시되면 워크플로가 돈다
(수동 실행도 가능). GitHub 저장소 설정에서 **Pages > Source 를 GitHub Actions** 로
두어야 동작한다.

배포 자체는 `--strict` 로 생성하므로, 깨진 문서 링크가 있으면 배포되지 않는다.
