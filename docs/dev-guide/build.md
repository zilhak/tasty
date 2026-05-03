# 빌드 가이드

Tasty의 워크스페이스 구조, 빌드 프로필, 빌드 시간 최적화 정책.

## 워크스페이스 구조

Tasty는 cargo workspace로 구성된 멀티 크레이트 프로젝트다.

```
tasty/
├── Cargo.toml            # 본 바이너리 + workspace 정의
├── src/                  # tasty 바이너리 크레이트 (UI/window/state/ipc 등)
└── crates/
    ├── tasty-core/       # 공용 데이터 타입 (model, theme, i18n, paths, color)
    ├── tasty-settings/   # 설정 스키마/직렬화 (appearance/keybindings/general/...)
    ├── tasty-font/       # 폰트 atlas, 글리프 래스터라이징 + 내장 D2Coding
    ├── tasty-terminal/   # PTY/VTE 파싱 (termwiz 래퍼)
    ├── tasty-hooks/      # Surface Hook 매니저
    └── tasty-tui-simulator/  # E2E TUI 테스트용 시뮬레이터
```

본 바이너리(src/)에서는 `pub use tasty_core::{model, theme, i18n, paths};`,
`pub use tasty_settings as settings;`, `pub use tasty_font as font;` 식으로
재수출하므로 `crate::model::X` 같은 기존 경로가 그대로 동작한다.

## 빌드 프로필

| 프로필 | LTO | 용도 | clean 빌드 시간 (참고치) |
|--------|-----|------|-------------------------:|
| `dev` (기본) | off | 일상 개발, `cargo build` | ~40s |
| `release` | thin | 빠른 최적화 빌드, `cargo build --release` | ~58s |
| `dist` | full | 배포용 산출물, `cargo build --profile dist` | ~3:22 |

> 시간은 32-core / 64GB / Windows 11 환경 기준. 절대값은 머신마다 다르니 비율로 보면 된다.

### dev

기본 프로필. 컴파일 빠르고 디버그 심볼 포함. 코드 변경 후 검증할 땐 항상 이걸 사용한다.

### release

`lto = "thin"`. 크레이트 단위 IR 요약을 공유해 cross-crate inlining을
**병렬로** 적용한다. full LTO의 95-99% 최적화 효과를 1/3 시간에 얻는다.

일상적인 "릴리즈 모드 검증"은 모두 이 프로필을 쓴다.

### dist

`release` 상속 + `lto = true`. 모든 크레이트 IR을 단일 LLVM 모듈로 통합 후
다시 최적화한다. 단일 스레드 단계가 길어 약 3.5배 느리다.

**오직 배포용 바이너리 (DMG/MSIX/AppImage 등) 빌드 시에만 사용한다.**

```bash
cargo build --profile dist
./scripts/build-macos-dmg.sh    # 자동으로 --profile dist 사용
```

## LTO란?

LTO = Link-Time Optimization. 컴파일러가 링크 시점에 *crate 경계를 넘어*
추가 최적화(inlining, dead code elimination 등)를 수행하는 단계.

| 종류 | 동작 | 빌드 시간 | 런타임 성능 |
|------|------|-----------|------------|
| off | 각 crate 독립 최적화 | 가장 빠름 | 기준 |
| **thin** | summary 기반 cross-crate inlining, 병렬 | 약간 느림 | full의 95-99% |
| full (`true`) | 모든 IR을 단일 모듈로 합쳐 다시 최적화 | 매우 느림 (단일 스레드) | 100% (이론상 최고) |

GPU/IO 위주 워크로드에선 thin과 full의 런타임 차이를 체감하기 어렵다.
Tasty는 단일 핫 루프 라이브러리가 아니므로 thin이 sweet spot이다.

## 자주 쓰는 명령

```bash
# 일상 개발
cargo build                          # debug
cargo check                          # 타입 검사만 (가장 빠름)
cargo build --release                # thin LTO로 최적화 검증

# 워크스페이스 한 크레이트만
cargo check -p tasty-settings
cargo build -p tasty-font

# 배포
cargo build --profile dist           # dist 프로필
./scripts/build-macos-dmg.sh         # macOS .app + .dmg
./scripts/build-linux.sh             # Linux tar.gz (uname -m으로 x64/arm64 자동 감지)
./scripts/build-windows.ps1          # Windows zip + msi (cargo-wix + WiX 3.x 필요)
./scripts/build-windows.ps1 -SkipMsi # Windows zip만
```

Windows MSI는 [`cargo-wix`](https://github.com/volks73/cargo-wix)로 만든다:

```powershell
cargo install cargo-wix              # 빌드 머신에 한 번만
# WiX Toolset 3.14는 별도 설치 (winget install WiXToolset.WiXToolset, 관리자 권한 필요)
```

템플릿은 `wix/main.wxs`에 들어 있다. UpgradeCode GUID(`722A590A-...`)는
업그레이드 식별자이므로 **절대 변경하지 말 것** — 바뀌면 새 제품으로 인식되어
구버전과 공존하게 된다. MSI는 시작 메뉴 바로가기, "프로그램 추가/제거" 등록,
사용자 선택 기반 PATH 추가 기능을 포함한다.

GitHub Actions(`.github/workflows/release.yml`)는 self-hosted runner로 동작한다.
Linux 빌드는 아키텍처별 라벨로 분기:

| 잡 | 러너 라벨 | 산출물 |
|----|----------|--------|
| `build-linux-x64` | `[self-hosted, Linux, X64]` | `tasty-{ver}-linux-x64.tar.gz` |
| `build-linux-arm64` | `[self-hosted, Linux, ARM64]` | `tasty-{ver}-linux-arm64.tar.gz` |
| `build-windows` | `[self-hosted, Windows]` | `tasty-{ver}-windows-x64.zip` + `.msi` |

`workflow_dispatch`로 태그 없이 수동 검증 빌드도 가능 (release는 만들지 않음).

## 빌드 시간 측정

`--timings` 플래그로 어느 크레이트가 시간을 잡아먹는지 본다.

```bash
cargo clean
cargo build --release --timings
# → target/cargo-timings/cargo-timing-<TIMESTAMP>.html
```

HTML 리포트에서 각 단위의 frontend / codegen 시간을 확인할 수 있다.
**한 단위가 전체의 50% 이상을 잡는다면 그 크레이트가 너무 크다는 신호다** —
모듈 분리를 고려한다.

## 미사용 의존성 검사

```bash
cargo install cargo-machete   # 한 번만
cargo machete                 # 워크스페이스 전체 검사
```

새 크레이트를 추가하거나 의존성을 정리한 뒤 주기적으로 돌린다.
미사용 deps는 컴파일 시간을 그대로 비용으로 가져간다.

## 모듈 의존성 분석

크레이트 분리를 검토할 때 사용:

```bash
cargo install cargo-modules cargo-depgraph

# 본 바이너리의 내부 모듈 그래프 (graphviz dot)
cargo modules dependencies --package tasty --no-externs --no-sysroot \
  --no-fns --no-traits --no-types > module-graph.dot

# 워크스페이스 크레이트 간 그래프
cargo depgraph --workspace-only > workspace-graph.dot
```

graphviz `dot`이 있으면 PNG로 렌더링 가능. 없으면 텍스트 검사로도
충분히 사이클/hub 모듈을 찾을 수 있다.

## 크레이트 분리 가이드

빌드 시간이 거슬리면 본 바이너리의 큰 leaf 모듈을 별 크레이트로 떼어낸다.
적절한 후보 조건:

- **out-degree가 작다** (다른 src/ 모듈을 거의 참조하지 않음). 외부 deps만
  쓰면 best.
- **사이클 없다.** 사이클이 있으면 분리 전에 풀어야 한다.
- **충분히 크다** (1000줄 이상이면 효과 체감).

분리 절차:

1. `crates/tasty-<name>/Cargo.toml` 생성
2. `git mv src/<module>/ crates/tasty-<name>/src/` (또는 `src/<file>.rs` →
   `crates/tasty-<name>/src/lib.rs`)
3. 내부 `crate::model::...` → `tasty_core::model::...` 등 경로 갱신
4. 본 `Cargo.toml`의 `[dependencies]`에 워크스페이스 경로 추가
5. `src/main.rs`에서 `mod <name>;` 제거하고
   `pub use tasty_<name> as <name>;`로 재수출 (backward path 유지)
6. `cargo check` → `cargo build` 검증
7. private 필드가 binary 크레이트에서 접근 불가하면 `pub(crate)` → `pub`로
   승격 (필요한 것만)

기존 `crate::model::Foo` 같은 경로가 모두 그대로 동작하므로 reverse
import 갈아끼우기가 불필요하다는 점이 핵심이다.
