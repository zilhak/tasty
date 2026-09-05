# CI · 훅 게이트 매트릭스 — 어떤 검사가 어디서 도는가

이 문서는 **각 검증 명령이 실제로 어디서 실행되는지**만 기술한다. "누군가 돌리고
있겠지" 로 남는 검사가 있으면 그것이 곧 사각이므로, 각 행은 자동 채널이 없으면 없다고
적는다.

명령의 내용·정책(왜 그 lint 인지, 왜 그 임계값인지)은 각 항목이 가리키는 문서에 있다.

> **이 문서와 레포의 모든 채널 주장은 `.github/workflows/` 의 작업 트리 파일 기준이다.**
> GitHub 이 실제로 읽는 것은 원격에 push 된 파일이라, 트리거나 명령을 고친 뒤 push 전까지
> 두 값이 갈린다. 그 갈림은 아래 ["트리거는 어느 ref 의 것인가"](#트리거는-어느-ref-의-것인가--작업-트리와-원격이-갈린다)
> 에서만 다루고, 거기서도 **현재값이 아니라 재는 명령**으로 적는다. 개별 문장에 "단 아직
> push 되지 않았다" 를 달지 않는 이유는 **push 하는 날 그 문장들이 한꺼번에 거짓이 되기
> 때문**이다 — 실제로 그 일이 일어났다. 결정과 근거는
> [ADR-0142](../adr/0142-channel-claims-are-written-against-the-working-tree.md).

## 자동으로 도는 것

| 검사 | 명령 | 채널 | 트리거 |
|---|---|---|---|
| 포맷 | `cargo fmt --check` (+ `site/` · `crates/tasty-plugin-sdk-wasm/` 매니페스트 각각) | `format-check.yml` (ubuntu-latest) | main push · PR · 수동 |
| SemVer 가드 | `cargo test --locked --no-default-features --test api_baseline_0_7 --test changelog_unreleased --test cli_naming_count_drift` | `test.yml` 의 `semver-guards` (self-hosted Linux X64) | main push · 수동 |
| macOS 컴파일 | `cargo check --workspace --locked` | `crossplatform-check.yml` (self-hosted macOS) | main push · PR · 수동 |
| Windows lint + 단위테스트 | `cargo clippy --workspace --all-targets --locked` · `cargo test --workspace --lib --bins --locked --no-fail-fast` | `crossplatform-check.yml` (self-hosted Windows) | main push · PR · 수동 |
| headless 컴파일 · **전체 스위트** · lint | `cargo check --workspace --no-default-features --locked` · `cargo test --workspace --no-default-features --locked --no-fail-fast -- --skip <1 건>` · `cargo clippy --workspace --all-targets --no-default-features --locked` | `crossplatform-check.yml` 의 `check-headless` (self-hosted Linux X64) | main push · PR · 수동 |
| **not-debug(release) 컴파일 · gui** | `cargo check --workspace --release --locked` | `crossplatform-check.yml` 의 `check-release` (self-hosted Linux X64) | main push · PR · 수동 |
| 문서 가드 | `cargo test -p tasty-doc-guards --locked --no-fail-fast` | `doc-guards.yml` (ubuntu-latest) | main push · PR · 수동 — **경로 필터 없음**([ADR-0138](../adr/0138-doc-guards-live-in-a-dependency-free-crate.md)) |
| 파일 SLOC | `bash scripts/check-file-size.sh` | `complexity-check.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 |
| 동결 총합 래칫 | `bash scripts/check-frozen-sum-ratchet.sh` | `complexity-check.yml` (self-hosted Linux X64, 같은 잡) | main push(문서·site 제외) · PR · 수동 |
| Intent 규율 | `bash scripts/check-intent-discipline.sh` — **`mask-source` 판정기를 먼저 짓는다** | `script-gates.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 |
| 사유 없는 `#[allow]` (**상한 래칫**, 판정기 `mask-source` 선행) | `bash scripts/check-allow-reason.sh` | `script-gates.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 |
| plugin 버전 bump | `bash scripts/check-plugin-version-bump.sh --range <before> <after>` | `plugin-version-check.yml` (self-hosted Linux X64) | main push · PR — **둘 다 `crates/**` 가 바뀐 경우** · 수동. ★ 판정 대상이 plugin 디렉토리가 아니라 **워크스페이스 내부 의존 폐포**이고 그 안에서 **출하되는 내용**만 세기 때문에([ADR-0166](../adr/0166-the-plugin-version-gate-judges-the-artifact-not-the-directory.md)) 경로 필터가 `crates/**` 다 — `tasty-utils`·`tasty-shm` 처럼 이름이 `tasty-plugin-` 으로 시작하지 않는 크레이트가 바뀌어도 plugin 산출물이 달라진다. 잡이 출하 판정기(`strip-cfg-test`)를 먼저 빌드한다 |
| 공급망 | `cargo deny check` | `supply-chain-check.yml` | main push(`paths: Cargo.lock · deny.toml`) · PR · 매주 월 09:00 UTC · 수동 |
| 사이트 생성 — 가이드 링크 · `ORDER` 누락 | `cargo run --release --manifest-path site/Cargo.toml -- --strict` | `pages.yml` 의 `build` (ubuntu-latest) | main push — `site/**` · `Cargo.toml` · 랜딩 아이콘 · 그 워크플로가 바뀐 경우만 · 수동 |

**문서만 담은 push 는 세 크로스플랫폼 잡을 발사하지 않는다.** `crossplatform-check.yml` 의
push 트리거에 `paths-ignore`(`docs/**` · `site/**` · `**/*.md`)가 걸려 있다. 컴파일 입력이
아닌 경로로 러너를 깨우지 않으려는 안전판인데, **문서 가드에는 정확히 거꾸로 작동한다** —
문서를 고치는 push 가 문서를 검사하는 채널을 돌리지 않는다. 소스를 함께 담은 push 에서는
걸러지지 않으므로 실무상 드물게 나타나지만, "문서만 고쳤으니 CI 가 봐 줄 것" 은 성립하지
않는다.

**자동 잡은 push 된 커밋만 본다.** 로컬에 쌓아 둔 커밋은 push 전까지 어느 자동 채널도
보지 않는다 — 채널이 배선돼 있다는 사실과 그 채널이 네 커밋을 봤다는 사실은 다르다.

**채널이 있다는 것은 그 잡이 초록이라는 뜻이 아니다.** 어떤 검사를 "CI 가 본다" 를 근거로
면제하려면 **그 잡이 최근에 실제로 통과했는지**까지 확인해라 — `gh run list` 로 최근 실행의
성패를 보고, 빨간 것이 있으면 `gh run view <id>`(실패 잡의 로그까지 보려면 `--log-failed`)로
어느 잡이 왜 죽었는지 확인한다. 이 조회는 코드 상태 판정이 아니라 실행 이력 조회다.
이 문서는 **배선**을 기술하고, 배선은 건강을 보장하지 않는다. 그리고 특정 시점의 적/녹은
여기 적지 않는다 — 적는 순간 낡기 시작하고, 낡은 시점 정보를 영구 서술로 읽는 것이 이
문서가 막으려는 실패 그 자체다.

**포맷 잡만 PR 을 함께 받는 이유**: `format-check.yml` 은 공용 `ubuntu-latest` 에서 돌아
러너 줄서기가 없다. 나머지 자동 잡은 self-hosted 러너를 쓰고, 특히 Linux X64 는 **한 대**를
`check-headless` · complexity-check · supply-chain-check · release/dist 빌드가 함께 쓴다 —
그래서 semver 가드는 PR 트리거를 붙이지 않았다(트리거가 잡마다 다른 것은 러너가 다르기
때문이지 중요도가 달라서가 아니다).

**self-hosted 잡은 매 회차 콜드로 짓는다 — 캐시가 없다.** `actions/checkout@v4` 는
`clean: true` 가 기본이라 `git clean -ffdx` 를 돌리고, `target/` 은 `.gitignore` 대상이라
**그 명령이 지운다**(로그에 `Removing target/` 이 찍힌다). 그래서 같은 러너의 두 Linux 잡이
같은 작업 디렉토리를 쓰면서도 서로의 산출물을 못 물려받고, 둘 다 `proc-macro2` 부터 다시
짓는다. 이것이 뜻하는 바 둘:

- **디스크는 누적되지 않는다.** 한 회차의 최댓값이 상한이고 급수가 아니다. 그래서 스텝을
  더할 때 재야 할 것은 "몇 회차 뒤에 찬다" 가 아니라 "한 회차 최댓값이 여유 안인가" 다.
- **시간 견적에 warm 수를 쓰면 안 된다.** 로컬에서 잰 증분 빌드 시간은 이 러너에 적용되지
  않는다 — 여기서는 언제나 콜드다.

**실측 한 회차 (2026-09-05, run 33965463122, `check-headless`)** — 분자만 적으면 읽는 쪽이
빠듯한지 넉넉한지 못 판단하므로 모수와 함께 적는다:

| 재는 것 | 값 | 모수 |
|---|---|---|
| 러너 루트 파티션 | 사용 234G · **여유 1.5T** | 전체 1.8T (사용률 14%) |
| 그 회차의 `target` | 18G | 위 여유의 1.2% |
| `~/.cargo` | 2.1G | — |

이 값의 성격은 **한 회차 스냅샷**이다([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md)
분류로는 계보가 붙은 실측치라 적어도 되지만, 회차마다 달라지므로 판단에 쓰기 전에 다시 재라).
gui 유닛 스텝이 `target` 을 얼마나 키우는지는 **아직 러너에서 안 쟀다** — 로컬 측정은
+7.8G 였고, 그 값이면 여유의 0.5% 다. 러너 값은 그 스텝을 담은 첫 회차의 같은 진단 줄에서
나온다. **안 본 채로 초록인 것은 통과가 아니라 미측정이다.**

재는 명령(잡 로그에 `df`/`du` 를 한 줄 넣어두면 회차마다 나온다):

```bash
job=$(gh run view <run-id> --json jobs --jq '.jobs[]|select(.name=="check-headless").databaseId')
gh api "repos/zilhak/tasty/actions/jobs/$job/logs" | grep -A3 'Filesystem'
gh api "repos/zilhak/tasty/actions/jobs/$job/logs" | grep -c 'Compiling '   # 콜드면 700+
```

이 저장소는 PR 을 열지 않고 main 에 직접 push 한다. **"거의" 가 아니라 실측 0 이다** —
최근 200 run 의 이벤트 분포가 `push 48 · schedule 8 · workflow_dispatch 1`, `pull_request`
**0** 이다(2026-09-04 측정). 그래서 PR 전용 트리거는 이 저장소에서 장식이다.

### 트리거는 어느 ref 의 것인가 — 작업 트리와 원격이 갈린다

트리거를 고쳐도 **push 하기 전까지 아무것도 안 바뀐다.** GitHub 은 `origin/main` 에 있는
워크플로 파일을 읽는다. 그래서 "트리거를 붙였다" 와 "그 잡이 돈다" 사이에 push 라는 층이
하나 더 있고, 이 저장소는 로컬 main 이 원격보다 크게 앞서는 기간이 길어 그 층이 실제로
벌어진다.

**이 절은 값을 안 적는다.** 트리와 원격의 차분은 어느 한쪽이 움직일 때마다 바뀌고,
`origin/main` 은 push 마다 움직인다 — 겉보기에 구조적인 수(워크플로 파일 수)라도 **두 ref
의 차분으로 만든 수는 그중 빠른 쪽의 속도로 낡는다.** 실제로 이 자리에 값 넷을 적었다가
하루 만에 넷 다 뒤집혔다.

판정이 필요하면 그 자리에서 센다:

```bash
git fetch origin main
diff <(git show origin/main:.github/workflows/<이름>.yml) .github/workflows/<이름>.yml
ls .github/workflows/*.yml | wc -l; git ls-tree --name-only origin/main .github/workflows/ | wc -l
gh run list --workflow=<이름>.yml --limit 20
```

**이 층이 실재한다는 근거는 값이 아니라 이력이다.**
[ADR-0131](../adr/0131-file-sloc-gate-needs-a-firing-trigger.md) 이 `complexity-check` 에
`push:[main]` 을 넣은 뒤에도, 그 커밋이 원격에 닿기 전까지 그 워크플로는 **등록 이래 run
이력이 0 건**이었다. 트리거를 붙인 것과 그 잡이 도는 것 사이에 push 라는 층이 실제로
벌어진 사례다. (그 뒤 push 되어 처음 발사됐다 — 사례는 과거형이라 안 낡는다.)

**워크플로 파일이 그 push 로 처음 들어가도 GitHub 은 그 push 에 대해 발화시킨다**(실측).
"새 워크플로는 다음 push 부터" 로 추정하지 마라 — `doc-guards.yml` 과
`plugin-version-check.yml` 이 원격에 처음 들어간 그 push 에서 둘 다 돌았다.

**③층을 볼 때 트리거만 대조하면 안 된다 — 잡의 명령 본문도 갈린다.** 같은 관측에서
가장 큰 갈림이 트리거가 아니라 명령이었다: 원격 `check-headless` 가 `--lib --bins` 라
통합 타깃을 하나도 안 보던 기간이 있었고, 그동안 "통합 테스트가 자동으로 돈다" 계열
서술 전부가 ③층에서 거짓이었다. 트리거만 봤으면 그 잡을 살아 있는 채널로 셌을 것이다.
위 명령의 `diff` 가 파일 **전문**을 대조하는 이유가 이것이다.

### 그 잡이 초록인가, 그리고 그 결과가 읽히는가

발화하는 것과 게이트인 것은 다르다. **연속 실패 중인 잡은 게이트가 아니라 소음이다** —
아무도 안 보는 빨강은 다음 빨강을 숨긴다.

그리고 **워크플로 결론은 잡의 상태를 가린다.** 잡이 여럿인 워크플로는 하나만 실패해도
전체가 `failure` 로 보이므로, 그 안의 초록 잡이 주는 채널은 *있지만 안 읽힌다.* 반대로
그 빨강이 어느 잡의 것인지 안 보면 **죽은 채널과 살아 있는 채널을 못 가른다.**

한 관측이 그 둘을 동시에 보여줬다(2026-09-05 관측, 과거형이라 지금 값과 갈라져도 된다).
`crossplatform-check` 이 다섯 번 연속 워크플로 결론 `failure` 였는데, 잡 단위로 열어 보니
셋의 사정이 전부 달랐다.

- `check-headless` — 그 다섯 중 넷이 `success`. 통합 테스트의 자동 채널은 **살아 있었다.**
- `check-macos` — 유일 스텝인 `cargo check` 에서 죽는다. 그 뒤가 없으므로 **채널이 죽었다.**
- `check-windows` — `clippy` 스텝은 통과하고 그 다음 `cargo test` 스텝에서 죽는다. clippy
  채널은 **살아 있지만** 워크플로 결론이 빨개서 읽히지 않는다.

그러니 채널 판정은 워크플로가 아니라 **잡 단위**로 한다 —
`gh run view <run-id> --json jobs`.

그러니 채널 판정에는 층이 다섯이다: **① 그 명령이 그 테스트를 도는가**(아래 배치별 표)
**② 그 잡이 애초에 발화하는가**(트리거 열) **③ 그 트리거와 명령이 원격에 가 있는가**
**④ 그 잡이 실제로 돈 적이 있는가** **⑤ 그 잡이 초록이고 그 결과가 읽히는가.**
①만 보면 PR 트리거를 채널로 세고, ②만 보면 push 안 된 트리거를 채널로 세고, ③까지만
보면 **한 번도 안 돈 잡을 채널로 센다**(④), ④까지만 보면 **연속 실패 중인 잡을 게이트로
센다**(⑤).

**④ 는 ⑤ 와 다르다 — "빨갛다" 가 아니라 "결과가 아예 없다".** 존재하지만 한 번도 돈 적
없는 잡은 채널이 아니고, 그 잡의 소요 시간·통과 여부·그 러너에서 필요한 것(디스플레이 등)
이 전부 **미측정**이다. 실측 예: `test.yml` 의 `test-linux-x64`(전체 스위트)는
`if: github.event_name == 'workflow_dispatch'` 뒤에 있고 최근 100 회차에 그 이벤트가
**0 건**이라, "수동으로 돌리면 된다" 가 한 번도 행사되지 않았다.

```bash
# 한 워크플로가 어떤 이벤트로 실제로 돌았나 — 0 인 이벤트가 곧 ④ 미충족이다.
gh run list --workflow <file>.yml --limit 100 --json event --jq 'group_by(.)[]|"\(length)\t\(.[0].event)"'
# 조건부 잡이라면 잡 단위로. **`conclusion` 을 반드시 함께 봐라** — `if:` 로 걸러진 잡은
# 목록에 `skipped` 로 그대로 나오므로, 이름만 세면 안 돈 잡을 돈 것으로 센다.
for id in $(gh run list --workflow <file>.yml --limit 20 --json databaseId --jq '.[].databaseId'); do
  gh run view $id --json jobs --jq '.jobs[] | "\(.conclusion)\t\(.name)"'
done | sort | uniq -c
# 실측(2026-09-05): 위 20 회차에서 test-linux-x64 는 `20 skipped` — 이름만 세면 20 번
# 돈 것으로 보인다.
```

**④ 가 실제로 무엇을 바꾸는가 — `gui_tests` 33 이 그 첫 사례다.** 이 33 건은 자동 잡
어디서도 안 돌고, 유일하게 그것을 담는 잡이 `test-linux-x64` 다. ④ 를 안 보면 그
33 을 "수동 채널이 있다" 로 세게 된다. ④ 를 보면 다르게 읽힌다 — 그 잡이 한 번도 안
돈 이상 **그 러너에 디스플레이가 있는지조차 모른다.** 즉 33 은 "수동으로 덮이는 것"이
아니라 **"켜면 덮일지 안 덮일지 모르는 것"**이고, 그 잡을 켜는 판단에서 이득 칸이
아니라 위험 칸에 들어간다.

### 규약 — 채널 주장은 **작업 트리 기준**으로 쓰고, ③층은 여기서만 말한다

이 레포의 채널 서술은 수십 곳에 흩어져 있고 ③층은 push 한 번에 통째로 바뀐다. 문장마다
"단 아직 push 되지 않았다" 를 달면 **push 하는 날 그 수십 곳이 한꺼번에 거짓이 된다** —
낡을 자리를 늘리는 형태다. 그래서 규약을 하나로 정한다.

- 개별 문장(이 문서의 배치별 표 · `CLAUDE.md` · ADR · 각 가드의 doc)은 **작업 트리의
  워크플로 파일 기준**으로 쓴다. 그것이 `ci_channel_claims_match_workflows` 가 실제로
  검사하는 층(①②)이기도 하다.
- **③층을 값으로 주장하는 것은 이 절에서만 한다.** 위 두 표가 그 자리이고, 날짜와 재는
  명령을 함께 달아 둔다.
- 다른 곳에서 ③층을 말해야 하면 **값을 옮겨 적지 말고 이 절을 가리킨다.** 옮겨 적은
  값은 여기 값과 독립으로 낡는다.
- 예외는 하나다 — **"실제로 발사됐다/발사된 적이 없다" 처럼 원격의 사실을 주장하는
  문장**은 어디에 있든 관측 좌표(측정 날짜 또는 재는 명령)를 같은 서술에 달아야 한다.
  좌표 없는 원격 주장은 확인할 방법이 없고, 확인할 수 없는 주장은 반증되지 않는다.

### 반대 방향 — 배선돼 있는데 아무 문서도 안 적은 채널

위 표와 [가드](#이-문서와-레포가-어긋나지-않게-하는-것)는 **주장 → 워크플로** 한 방향만
본다. 반대쪽(워크플로에 있는데 아무 문서도 안 적은 채널)은 자동으로 결함이 아니다 —
주장하지 않은 채널은 거짓 주장이 아니다. 그래서 검사를 만들기 전에 **세었다**
(2026-09-05, `.github/workflows/` 작업 트리 기준).

자동 트리거를 가진 잡은 19 개다(브랜치 push · PR · schedule 13 + 태그 push 6). 수동 전용은
5 개다(`build-check.yml` 넷 + `test.yml` 의 `test-linux-x64`). **아무 문서도 안 적은 채널은
0 이었다.** 이 문서의 표에만 없던 것이 8 개였고, 셋으로 갈렸다.

- **배포 파이프라인이라 여기 안 적는다** — `release.yml` 여섯 잡과 `pages.yml` 의 `deploy`.
  산출물을 만들지 검증 술어를 돌리지 않는다. 절차는 [release](release.md) ·
  [release-runners](release-runners.md) · [site](site.md) 가 담는다.
- **빠뜨린 것 하나** — `pages.yml` 의 `build`. `--strict` 가 깨진 상대 링크와 `ORDER` 누락을
  **실패로 승격**하므로 검증 술어이고, 그 스텝이 workspace 밖 `site/` 크레이트를 컴파일하기도
  한다. 위 표에 행을 넣었다.
- **죽은 채널은 0** — 배선만 있고 안 도는 잡은 없었다. 다만 `build-check.yml` 은 지금까지
  실행 이력이 0 이다(수동 전용이니 "쓸 수 있다" 는 주장은 참이다).

**그래서 반대 방향 가드는 만들지 않는다.** 결함이 1 건이고 그 1 건은 문서 한 줄로 닫힌다.
가드로 만들면 "모든 자동 잡이 이 표에 있어야 한다" 는 명부형 판정이 되는데, 배포 잡처럼
정당한 예외가 계속 생겨 **명부 밖에 대상이 없다** 를 함께 단정해야 한다 — 그 단정이 이
표보다 먼저 낡는다.

## "안 돈다" 를 쓰기 전에 두 가지를 갈라라

**① 실행인가 컴파일인가.** 자동 잡의 clippy 는 `--all-targets` 라 `tests/*.rs` 를
**컴파일한다.** 그리고 헤드리스 잡은 전체 스위트를 돌아 **실행도 한다.** 그러므로 통합
테스트를 두고 "CI 가 컴파일조차 안 본다" 도 "어느 자동 잡도 이것을 실행하지 않는다" 도
거짓이다. 정확한 서술은 **어느 조합에서 도는지를 함께 적는 것**이다 — 통합 타깃은
헤드리스 조합에서만 실행되고, 기본 조합의 자동 잡은 `--lib --bins` 라 못 본다. 한쪽
거짓을 고치다 반대쪽 거짓을 심는 것이 이 축에서 가장 흔한 실패다.

**② 강제 수단이 워크플로 안에 있는가.** "워크플로가 안 돌린다" 는 "아무도 안 막는다" 가
**아니다.** clippy `deny`·`#[deny]` 어트리뷰트·pre-commit 훅·타입 시스템은 워크플로 밖에서
막는다. 실례로 복잡도 게이트는 축이 둘인데 채널이 갈린다.

| 복잡도 게이트의 축 | 강제 수단 | 실효 자동성 |
|---|---|---|
| 함수 cognitive | clippy `cognitive_complexity = "deny"` | **있다** — 자동 잡의 컴파일 단계에서 막힌다 |
| 파일 SLOC | `scripts/check-file-size.sh` (`complexity-check.yml`) | **트리거는 붙어 있다** — 다만 그것이 원격에 가 있어야 발화한다. 아래 "트리거는 어느 ref 의 것인가" 의 명령으로 그 자리에서 확인한다 |

**③ 그 채널이 실패할 수 있는가.** 트리거가 붙어 잡이 도는 것과, 그 잡이 문제를 만났을 때
실제로 빨개지는 것은 다른 질문이다. 파일 SLOC 게이트가 그 예였다 — 트리거를 붙인 뒤에도
`tokei` 가 죽거나 빈 결과를 주면 스크립트가 **"게이트 통과" 를 출력하며 exit 0** 이었다.
측정 실패를 위반 없음으로 읽는 형태라, 러너 환경이 어긋나는 순간 그 채널은 영원히 초록이 된다.

지금은 종료코드가 **0(통과) / 1(위반) / 2(측정 실패)** 로 갈리고,
`tests/file_sloc_gate_fails_loudly.rs` 가 스텁 `tokei` 로 그 셋을 고정한다(통합 테스트라
`check-headless` 가 자동으로 돌린다). **채널의 존재 · 그 채널이 대상을 실제로 보는가 ·
그 채널이 실패할 수 있는가 — 셋은 따로 확인해야 한다.**

### 소스 스캔 가드는 지금 어디서 도는가 (인구조사)

소스를 런타임에 읽어 대조하는 가드는 **실행되지 않으면 존재하지 않는 것과 같다** — 컴파일이
통과했다는 사실은 그 가드가 무엇을 발견했는지에 대해 아무것도 말하지 않는다. 그래서 이
부류만 따로 센다(2026-09-06 재측정, 작업 트리 기준).

모수는 **아래 표의 합**이다 — 통합 타깃 중 레포 파일을 런타임에 읽으면서 프로세스는 띄우지
않는 것. 여기에 총합을 따로 적지 않는다. 적으면 같은 수가 두 곳에 있게 되고, 늘어날 때
한 곳만 움직인다 — 실측으로 그 형태가 났다(2026-09-06): 표는 15 에서 17 로 갱신됐는데 위에
적혀 있던 모수는 46 에 멈춰 있었고, 그때 실제 값은 51 이었다. 표의 두 행이 곧 분할이므로,
행이 갱신되면 합도 함께 갱신된다.

**재는 법이 정본이다**(이 수는 커밋마다 바뀐다). 술어를 흉내 내지 말고 그 자리에서 부른다 —
하한을 잠깐 터무니없이 올리면 판정기가 자기가 센 값을 실패 메시지에 적는다:

```bash
# 모수(필터 뒤까지 포함한 순수 스캔 가드 전체)
sed -i 's/MIN_SCANNED: usize = 45/MIN_SCANNED: usize = 999/' \
  crates/tasty-doc-guards/tests/filtered_guards_are_not_totally_blind.rs
cargo test -p tasty-doc-guards --test filtered_guards_are_not_totally_blind
# 첫 행(필터 없는 채널을 가진 것)
sed -i 's/MIN_GUARDED: usize = 12/MIN_GUARDED: usize = 999/' \
  crates/tasty-doc-guards/tests/filter_free_channel_still_exists.rs
cargo test -p tasty-doc-guards --test filter_free_channel_still_exists
```

역-sed 로 되돌린다(`git checkout` 은 같은 파일의 미커밋 작업까지 지운다). 둘째 행은 첫 값에서
첫 행을 뺀 것이다.

| | 개수 | 채널 |
|---|---|---|
| 필터 없는 채널을 가진 것 | 17 | `crates/tasty-doc-guards/tests/` — `doc-guards.yml` 은 경로 필터가 없다 |
| `check-headless` 만 가진 것 | 34 | `crossplatform-check.yml` 의 `paths-ignore` 뒤 |

**이 수는 술어 자신을 세지 않는다.** 술어가 `"Command::new"` 같은 표지를 문자열로 찾는데,
그 표지를 **리터럴로 담은 파일**(술어를 구현한 가드들)은 자기 표지에 걸려 모수에서 빠진다.
실측 17 은 그 둘을 뺀 값이다 — 세는 쪽을 고치려면 `mask-source` 처럼 코드와 문자열을
가르는 판정기가 먼저 있어야 한다.

**필터가 구멍이 되는 것은 그중 1 뿐이다.** 뒤 34 중 28 은 읽는 경로에 무시 대상이 하나도
없고(그 경로가 안 바뀐 push 에서는 판정이 바뀔 수 없다), 4 는 경로 리터럴 없이
`CARGO_MANIFEST_DIR` 로 `crates/**` 의 매니페스트를 읽는다.

**1 은 다른 워크플로가 덮는다.** `changelog_unreleased` 는 읽는 것이 `*.md` 둘뿐이라
이 필터 뒤에 있으면 총체적 사각이어야 하는데, `test.yml` 의 `semver-guards` 가 경로 필터
**없이** main push 마다 `--test changelog_unreleased` 로 이름을 지목한다. 그래서 사각이
아니다 — [ADR-0138](../adr/0138-doc-guards-live-in-a-dependency-free-crate.md) 이 이 가드를
"안 옮긴다" 로 판정한 근거가 그것이고, 그 근거는 지금도 참이다. **옮기면 오히려 깨진다**:
타깃이 본체 패키지를 떠나면 `--test changelog_unreleased` 가 `no test target` 으로 실패한다
(실측).

남은 하나가 `cli_method_table_parity` 이고, 그 입력은 **일부만** 무시 대상이다
(`docs/dev-guide/api-conventions.md` + `crates/tasty-cli/src/**`). 셋 중 유일하게
워크스페이스 크레이트를 링크하는 가드이기도 하다(`tasty_ipc` 의 `METHOD_TABLE` ·
`DEBUG_METHODS` 를 런타임 값으로 읽는다) — 옮기려면 그 링크를 텍스트 판독으로 바꾸는
선행 작업이 필요하다.

★ **그 부류는 비어 가는 중이고, 되감기지 않게 래칫이 걸려 있다.** 한때 셋이었다
(`cli_method_table_parity` · `permission_free_methods_docs_parity` ·
`contributes_gate_docs_parity`). 나머지 둘은 상수를 **소스 텍스트로 읽고** 판독이 진짜 값과
갈리는 위험을 본체 패키지의 교차 대조 가드가 받는 길로 옮겨졌다. 관측자의 `DEP_BEARING`
명부가 그 방향을 양방향으로 고정한다 — 문서를 읽으면서 크레이트를 링크하는 가드가 필터 뒤에
새로 생기면 실패하고, 링크를 끊었는데 명부에 남아 있어도 실패한다. 크레이트 이름 목록은
`crates/` 디렉토리에서 읽으므로 손으로 갱신하지 않는다.

★ **이 분류는 이제 손으로 세지 않는다.** `crates/tasty-doc-guards/tests/filtered_guards_are_not_totally_blind.rs`
가 워크플로에서 `paths-ignore` 를 읽어 필터 뒤 스캔 가드를 매번 다시 분류하고, **읽는 경로가
전부 무시 대상인 것이 생기면 실패한다.** 일부만 무시 대상인 것은 그 파일의
`PARTIALLY_FILTERED` 에 사유와 함께 등재되며 명부는 양방향으로 고정된다(새로 생겨도,
사라졌는데 남아 있어도 실패). `doc-guards.yml` 은 경로 필터가 없으므로 이 관측자는
문서만 담은 push 에서도 돈다.

★ **그 관측자는 한때 자기 채널을 재지 않고 가정했다.**
`filtered_guards_are_not_totally_blind` 는 `crates/tasty-doc-guards/tests/` 를 상수로
**건너뛰었다** — 거기 채널이 있다고 전제한 것이지 확인한 것이 아니다.
그 전제가 깨지는 형태를 변이로 재 봤다(2026-09-05) — ① `doc-guards.yml` 의 `push:` 에
`paths:` 를 달기 ② 그 잡의 호출을 `--test <이름>` 하나로 좁히기 ③ 그 잡을
`if: github.event_name == 'workflow_dispatch'` 로 수동 전용으로 만들기. **셋 다 그때 있던
판정기 전부에서 살아남았다**: 위 관측자는 이 디렉토리를 건너뛰고,
`ci_channel_claims_match_workflows` 의 `automatic_job_bodies` 는 **경로 필터를 아예
모델하지 않으며**(`push:` 만 있으면 자동으로 센다), `src/source_guards` 의
`EXPECTED_TEST_INVOCATIONS` 는 파일별 **호출 건수**만 고정한다(필터가 붙어도, 호출이
좁아져도 건수는 1 그대로다).

`crates/tasty-doc-guards/tests/filter_free_channel_still_exists.rs` 가 그 셋을 닫는다.
판정은 **이름이 아니라 성질**이다 — "`doc-guards.yml` 이 있는가" 가 아니라 "경로 필터
없이 매 push 도는 잡 중 이 패키지를 **좁히지 않고** 돌리는 것이 있는가". 워크플로 이름이
바뀌거나 잡이 다른 파일로 옮겨가도 채널이 남아 있으면 통과한다. ★ **그 판정은 밖에서 부를 수 있다 — 흉내 내지 마라.** `workflow-channels` 판정기
바이너리가 워크플로마다 한 줄(`<파일> push path_filtered tags_only 자동잡 수동전용잡`)과
커버리지 세 줄(`named=` · `packages=` · `whole_workspace=`)을 낸다. 다른 판정기들과 같은
관례다 — `scripts/lib/judge-bin.sh` 의 `resolve_judge` 로 찾고 `--check-fresh` 로 신선도를
묻는다.

```bash
cargo build -p tasty-doc-guards --bin workflow-channels
./target/debug/workflow-channels .
```

**여는 이유는 사본이 갈리기 때문이다.** 실측(2026-09-05): 하루에 세 레인이 각자 이
판정을 흉내 냈고 셋 다 원본과 다른 답을 냈다 — 그중 하나는 `paths-ignore` 를 **가진**
워크플로를 "필터 없음" 으로 냈다. ★ 갈리는 방향은 대체로 **덜 잡는 쪽**이라 조용하다.
`crates/tasty-doc-guards/tests/exposed_judge_agrees_with_the_library.rs` 가 노출본과
라이브러리가 같은 답을 내는지 계속 묻는다(하한 포함) — 그것이 없으면 노출본 자신이 또
하나의 사본이 된다. **다만 그 대조는 갈림만 본다**: 둘이 함께 틀리면 일치하므로 초록이다.
옳음은 내용을 단정하는 가드들이 진다.

트리거 판독과 커버리지 수집은
`tasty_doc_guards::workflow_triggers` 한 벌을 위 관측자와 함께 쓴다 — 주석과 트리거 키를
구조로 가르고(문자열 `contains` 로 세면 다른 워크플로의 필터를 *설명하는* 주석이 필터로
읽힌다. 실측으로 정확히 한 파일이 그 형태였고 하필 `doc-guards.yml` 이었다), 태그 전용
push(`release.yml`)를 매 push 채널로 세지 않는다.

**덮는 채널도 그 관측자가 읽는다.** 경로 필터 없는 워크플로가 `--test <이름>` 으로 지목하거나
`-p <패키지>`(또는 `--workspace`)로 좁힘 없이 돌리는 타깃은 면제된다 — 그 명부 역시 워크플로
파일에서 읽으므로 손으로 갱신하지 않는다. 손 명부는 낡는 순간 **거짓 양성**(이미 덮인 가드를
옮기라고 한다)이 되고, 그 요구를 따르면 위처럼 지목하던 잡이 깨진다. 그래서 이 관측자가
옮기라고 하면 그것은 실제로 옮길 자리다.

**면제를 디렉토리 이름으로 하지 않는다.** 그 관측자는 `crates/tasty-doc-guards/tests/` 를
상수로 건너뛰었는데, 그러면 그 자리의 채널이 사라져도 침묵한다 — 실측으로 확인했다
(2026-09-05): `doc-guards.yml` 의 호출을 `--test` 하나로 좁히는 변이에서 그 디렉토리의 17 개가
실제로 눈멀었는데 그 관측자는 초록이었다. 지금은 덮임을 **계산**하므로 같은 변이에서 함께
발화한다. 채널 자체가 남아 있는지는 `filter_free_channel_still_exists` 가 따로 본다 — 물음이
다르기 때문이다(하나는 "이 타깃이 덮이나", 다른 하나는 "그 채널이 존재하나").

★ **"전부 무시 대상" 과 "일부 무시 대상" 은 처방이 다르다.** 전부인 것은 문서만 담은
push 가 위반의 **유일한 경로**라 필터가 총체적 사각이다 — 그래서 그 셋은 doc-guards 로
옮겼다(ADR-0138). 일부인 것은 코드 쪽 위반이 여전히 잡히고 문서 쪽 위반도 **다음 소스
push 에서** 잡힌다. 실측으로도 그 창은 열리지 않았다.

**push 단위** (2026-09-05 재측정, 창 **25.1 시간** · 재구성된 push 41 구간 — 경로 필터가
없는 `format-check.yml` 의 run 목록으로 push 경계를 복원했고, 그 워크플로가 그보다 오래
살지 않아 이 창이 지금 잴 수 있는 최대다):

    crossplatform-check 가 안 뜨는 push(변경이 전부 무시 대상) : 4
    api-conventions.md 를 담은 push                            : 8
    그 교집합                                                  : 0

**커밋 단위**는 0 이 아니다 — 30 일 창에서 **4**, 전체 이력에서 **10** 이다(변경 파일이
전부 무시 대상이면서 `api-conventions.md` 를 담은 커밋). 커밋 단위는 push 단위의 **상한**
이므로(한 push 안의 커밋 하나라도 소스를 담으면 워크플로가 뜬다) 이 수가 노출을 뜻하지는
않는다. 다만 **0 이 구조가 아니라 묶는 습관에서 나온다**는 것은 말해 준다 — 위 주변부만
놓고 보면 41 push 당 0.8 회쯤 겹칠 자리이고, 실제로 안 겹친 이유는 `api-conventions.md` 가
API 작업과 함께 바뀌어 늘 소스와 같은 push 에 실렸기 때문이다.

그래서 이 하나는 **옮기지 않고 잔여로 적는다.** 다시 볼 조건은 위 관측자가 본다 — 입력이
**전부** 무시 대상이 되는 순간(예: 코드 쪽 입력이 빠지는 리팩터) 그 테스트가 실패한다.
그때의 처방도 이미 재 뒀다: 문서를 읽는 세 테스트 중
`commands_cited_as_alternatives_exist` 는 크레이트 의존이 **아예 없어** 그대로 옮겨지고,
`methods_without_a_cli_entry_point_are_documented` 는 `tasty_doc_guards::method_table`
텍스트 판독으로 대체되며, `debug_methods_without_a_cli_entry_point_are_documented` 만
`DEBUG_METHODS` 판독을 새로 만들어야 한다.

```bash
# 순수 소스 스캔 가드 세기 — 통합 타깃 중 레포 파일을 읽고 프로세스는 안 띄우는 것
for f in tests/*.rs crates/*/tests/*.rs; do
  grep -lq -e CARGO_MANIFEST_DIR -e 'repo_root()' "$f" || continue
  grep -lqE 'Command::new|spawn_diag|TASTY_E2E_BIN|CARGO_BIN_EXE' "$f" || echo "$f"
done | wc -l
```

**②의 실물 하나 더 — 헤드리스 잡의 명명 `--skip`.** libtest 의 `--skip` 은 테스트 경로
전체에 대한 **부분일치**라, 이름이 사라지면 아무것도 안 잡고(과소, 무음) 그 문자열을 품는
이름이 새로 생기면 의도 없이 함께 빠진다(과대, 초록인데 커버리지 감소). 헤드리스 잡은 전체
스위트를 자동으로 도는 유일한 조합이라 여기서 빠지면 어디서도 안 돈다.
`tests/headless_skip_names_are_exact.rs` 가 **워크플로에서 skip 을 읽어와**(목록을 박아두면
만료된다) 각각의 사거리가 하나인지 본다 — 0 건도 2 건 이상도 실패다.

**무엇을 세는지와 무엇을 못 보는지는 그 가드의 모듈 doc 에만 적는다.** 이 문서가 여기에
그 계수 단위를 옮겨 적었다가 낡았다 — "식별자와 맞는지" 로 적혀 있었는데 그 가드는 그
사이 식별자가 아니라 테스트 건수를 세도록 바뀌었고, 사거리 서술도 함께 달라졌다. 가드의
계약은 가드가 바뀔 때 같이 바뀌므로 **복제본은 정의상 가드 밖이다**
([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md) 의
"칸 2 는 복제하지 않는다" 가 수에 대해 말한 것이 서술에도 그대로 적용된다). 이 문서는
그 가드가 **있다는 것과 어느 채널에서 도는지**까지만 말한다.

## 테스트는 **어디 있느냐**로 채널이 갈린다

위 표에서 가장 자주 오해되는 줄이다. 자동 잡이 돌리는 테스트 명령은 **조합마다 다르다** —
기본 조합은 좁혀져 있고(`--lib --bins`, 또는 `--test <이름>` 으로 이름 지목), 헤드리스
조합만 전체 스위트를 돌린다. 그래서 **같은 주제의 두 가드라도 파일이 어디 있느냐에 따라,
그리고 같은 파일이라도 조합에 따라 채널이 갈린다.**

| 테스트가 어디 있나 | 자동 **실행** | 자동 **컴파일** | 실례 |
|---|---|---|---|
| lib 유닛 테스트 (`src/`·`crates/*/src/` 안의 `#[cfg(test)] mod tests`) | **대체로 있다** — 두 조합 모두가 유닛 타깃을 포함한다(Windows 잡은 `--lib --bins`, 헤드리스 잡은 그 상위집합인 전체 스위트). **다만 조합 격자에 빈 칸이 하나 있어 그 칸의 유닛 테스트는 어디서도 안 돈다** — 아래 절 | 있다 | `ui_font_size_tokens_are_integers_at_every_zoom` |
| 통합 테스트 (`tests/*.rs`) | **헤드리스 조합에만 있다** — `check-headless` 가 전체 스위트를 돌린다(`--skip` 1 건 제외). **기본 조합에는 없다** — Windows 잡은 `--lib --bins` 이고 `test.yml` 의 전체 스위트는 `workflow_dispatch` 전용 그리고 `check-headless` 는 `paths-ignore: docs/** · site/** · **/*.md` 뒤에 있어 **문서만 바뀐 push 에서는 이 칸이 통째로 비는 것**에 유의한다 | **있다** — clippy `--all-targets` 가 타깃으로 잡는다 | `tests/design_token_adherence.rs` |
| 문서 가드 통합 테스트 (`crates/tasty-doc-guards/tests/*.rs`) | **있다 — 두 조합과 무관하게** `doc-guards.yml` 이 `-p tasty-doc-guards` 로 돌린다. 그 잡에만 `paths-ignore` 가 없어, 문서만 바뀐 push 에서 도는 **유일한** 테스트 채널이다([ADR-0138](../adr/0138-doc-guards-live-in-a-dependency-free-crate.md)). `check-headless` 의 전체 스위트에서도 함께 돈다 | 있다 | `crates/tasty-doc-guards/tests/no_checkbox_in_docs.rs` |
| SemVer 가드 3종 | **있다** — `semver-guards` 가 `--test` 로 이름을 지목한다 (main push) | 있다 | `api_baseline_0_7` · `changelog_unreleased` · `cli_naming_count_drift` |
| 포맷 | **있다** — `format-check.yml` (main push · PR) + pre-commit | — | `cargo fmt --check` |

### 조합 격자의 빈 칸 — Linux + gui + debug (지금은 채워져 있다)

유닛 테스트가 "두 조합 모두" 라고 말할 때 그 둘은 **Windows + gui + debug** 와
**Linux + headless + debug** 였다. 그래서 **Linux 이면서 gui feature 뒤에 있는** 유닛
테스트는 앞쪽에서 `cfg` 로 사라지고 뒤쪽에서 feature 로 사라져 양쪽 다 안 돌았다.

지금은 `check-headless` 에 `cargo test (linux, gui, unit)` 스텝이 붙어 그 칸을 덮는다
(`--lib --bins`, 기본 feature). **새 잡이 아니라 스텝인 이유**는 self-hosted Linux X64
러너가 한 대뿐이라 잡이 늘면 큐가 직렬로 길어져서다. 아래 표와 절차는 그 칸이 왜
비어 있었는지와 **다시 비었을 때 어떻게 재는지**를 위해 남긴다.

| | debug | release |
|---|---|---|
| macOS + gui | `check-macos`(컴파일) | — |
| Windows + gui | `check-windows`(컴파일 + 유닛) | — |
| Linux + headless | `check-headless`(컴파일 + 전체) | — |
| **Linux + gui** | `check-headless` 의 gui 스텝(컴파일 + `--lib --bins` 유닛) | `check-release`(컴파일) |

빈 칸은 테스트만이 아니라 **컴파일**도 비어 있다. 그 칸에만 있는 코드가 실재한다 —
`src/platform/native_menu/linux.rs` 는 `#[cfg(feature = "gui")]` 아래 Linux 전용이면서
`#[cfg(debug_assertions)]` 함수를 갖는다.

**수를 여기 적지 않는다**([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md))
— 재는 절차를 적는다. 이름을 조합별로 열거해 빼고, 남은 것이 Windows 조합에 실재하는지는
**크로스 빌드한 테스트 바이너리에 바이트로 물어본다.**

```bash
enum() { cargo test --workspace --locked "$@" -- --list 2>&1 | awk '
  /^ *Running / { p=$0; sub(/.*\(/,"",p); sub(/\).*/,"",p); sub(/.*\//,"",p); sub(/-[0-9a-f]+$/,"",p)
                  s=$2" "$3; sub(/ *\(.*/,"",s); t=p"|"s; next }
  /^ *Doc-tests / { t="doc|"$2; next }
  /: test$/ { n=$0; sub(/: test$/,"",n); print t "\t" n }'; }
enum > /tmp/D.txt                       # 기본 feature 전체
cargo test --workspace --no-default-features --locked -- --list 2>&1 | ... > /tmp/H.txt
sort -u -o /tmp/D.txt /tmp/D.txt; sort -u -o /tmp/H.txt /tmp/H.txt
comm -23 /tmp/D.txt /tmp/H.txt | grep '^tasty|' | cut -f2 > /tmp/gui.txt   # gui 게이트된 본체 유닛

cargo test --target x86_64-pc-windows-gnu --bin tasty --no-run --locked   # mingw 링커 필요
win=$(ls -t target/x86_64-pc-windows-gnu/debug/deps/tasty-*.exe | head -1)
while read n; do grep -qaF "$n" "$win" || echo "빈 칸: $n"; done < /tmp/gui.txt
```

**`strings` 로 세지 마라.** 한글 테스트 이름은 비-ASCII 라 `strings` 가 안 잡아,
빈 칸을 **과대**로 센다(실측: `strings` 28 · `grep -aF` 9). 그리고 반대 방향도 봐라 —
`grep -F` 는 부분문자열도 맞히므로, 어떤 이름이 다른 이름의 진부분문자열이면 "있다" 가
거짓 통과한다(실측 0 건).

#### 크로스 빌드 없이 재는 길 — Windows 잡의 **로그**에 물어본다

위 절차는 mingw 링커를 요구한다. 그것 없이 같은 답을 얻는 길이 있다: `check-windows` 의
`cargo test (unit)` 은 실행한 이름을 **한 줄씩 찍으므로**, 그 잡 로그가 곧 "Windows 조합이
실제로 도는 이름의 전수" 다. 크로스 빌드보다 정확하다 — 컴파일되는 것이 아니라 **도는
것**을 세기 때문이다.

```bash
run=$(gh run list --workflow crossplatform-check.yml --limit 1 --json databaseId --jq '.[0].databaseId')
job=$(gh run view "$run" --json jobs --jq '.jobs[]|select(.name=="check-windows").databaseId')
gh api "repos/zilhak/tasty/actions/jobs/$job/logs" \
  | grep -oE " test [A-Za-z0-9_:가-힣]+ \.\.\. (ok|ignored)" \
  | sed -E 's/ test ([^ ]+) \.\.\..*/\1/' | sort -u > /tmp/win.txt
# /tmp/gui.txt(위에서 만든 gui 게이트 본체 유닛)와 차집합
comm -23 /tmp/gui.txt /tmp/win.txt
```

**모듈 경로를 벗기고 비교하지 마라.** 마지막 세그먼트만 남기면 다른 크레이트의 동명
테스트와 충돌해 빈 칸을 **과소**로 센다 — 실측으로 9 건이 8 건이 됐다(사라진 것은
`platform::x11_gdk_window::tests::the_scan_separates_code_from_comments_and_literals`,
같은 이름이 다른 크레이트에 있었다).

### "헤드리스 커버리지" 는 두 가지를 섞어 부른다

위 표의 통합 테스트 줄은 **헤드리스 잡이 유일 채널이다** 까지만 말한다. 그런데 그 타깃들이
거기 있는 이유는 둘이고, **둘은 서로 다른 것을 뜻한다.**

- **헤드리스 고유** — 그 타깃이 `CARGO_BIN_EXE_tasty` 로 자기 바이너리를 띄운다.
  `--no-default-features` 로 빌드된 그 바이너리는 곧 headless 데몬이라, 이 타깃은
  `src/boot.rs` 의 `run_headless` 진입점과 그 아래 IPC · attach · PTY 경로를 **실제로**
  돈다. 이 판정은 기본 조합에서 재현되지 않는다 — 같은 테스트를 돌려도 재는 바이너리가
  다른 코드다. 실례로 `tests/e2e_single_instance_guard.rs` 의 자동 실행은 헤드리스
  조합에서만 일어난다.
- **통합 타깃이라 여기 있는 것** — 레포 파일을 읽어 정합을 보는 가드류와, 프로세스도
  파일도 안 쓰는 순수 로직·파싱. 판정이 조합과 무관해서 gui 빌드로 돌려도 같은 답이
  나온다. 이것들에게 헤드리스 잡은 **헤드리스라서가 아니라 통합 타깃을 자동으로 도는
  유일한 잡이라서** 유일 채널이다. 실례로 `tests/design_token_adherence.rs` 의 자동
  실행도 헤드리스 조합에서만 일어난다 — 앞 항목과 채널은 같고 **이유가 다르다.**

**섞어 부르면 잘못된 추론이 선다.** "헤드리스 잡을 줄이면 헤드리스 커버리지가 준다" 는
뒤쪽에는 성립하지 않는다 — 뒤쪽은 **기본 조합 잡이 통합 타깃을 돌게 하는 것만으로도**
덮인다. 반대로 앞쪽은 어떤 기본 조합 잡으로도 못 덮는다. 그러니 헤드리스 잡의 범위를
논할 때는 두 몫을 갈라서 세야 한다.

어느 쪽인지 가르는 법 — 수를 적지 말고 그 자리에서 세라(타깃이 늘면 바뀌는 값이다,
[ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md)):

```bash
# 헤드리스 고유 — 자기 바이너리를 띄우는 타깃. 공용 하네스가 대신 띄우는 경우가 있어
# `CARGO_BIN_EXE` 만 보면 놓친다(`tests/common/mod.rs` 가 spawn_diag 로 띄운다).
grep -rl -e CARGO_BIN_EXE -e spawn_diag -e 'mod common' tests/ crates/*/tests/

# 그 잡이 실제로 무엇을 돌렸나 — 잡 로그가 정본이다
gh run view --job <check-headless 잡 id> --log \
  | grep -E 'Running tests/|test result:'
```

### 조합에서 사라지는 이유는 대개 **파일 위치**다 (실측)

같은 파일이라도 조합에 따라 채널이 갈리는데, 그 갈림의 원인이 대부분 그 테스트 자신에게
있지 않다. 루트 bin 타깃의 유닛 테스트를 두 조합의 `-- --list` 로 갈라 보면(main
`d7dc4079` 실측) 기본 2039 / 헤드리스 1094 이고, **기본 조합에만 있는 949** 의 내역은:

| 헤드리스에서 사라지는 것을 무엇이 설명하나 | 수 | 비율 |
|---|---|---|
| **다른 파일의 `#[cfg(feature = "gui")] mod …;` 선언 아래에 있다** (위치 상속) | **909** | 95.8% |
| 같은 파일 안의 인라인 `#[cfg(all(test, feature = "gui"))] mod tests { … }` | 11 | 1.2% |
| 개별 `#[test]` 에 직접 붙은 `#[cfg(feature = "gui")]` | 29 | 3.1% |

상위 기여: `adapters::ui` 463 · `view` 177 · `gfx` 31 · `app::attach_client` 30.
그 909 를 물려주는 `mod` 선언 자체는 많지 않다 — `mod x;` 선언 **바로 앞 줄**에 gui cfg 가
붙은 것을 세면 70 이다(main `a3da2fed` 실측). 재는 명령:

```bash
find src -name '*.rs' -exec awk '
  /^[[:space:]]*(pub([(][^)]*[)])?[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/ \
    { if (prev ~ /#\[cfg\(.*feature[[:space:]]*=[[:space:]]*"gui"/) n++ }
  { prev = $0 }
  END { print n+0 }' {} \; | awk '{s+=$1} END {print s}'
```

앞서 이 자리에는 **67** 이 적혀 있었다. 그 값이 틀렸다는 뜻이 아니다 — **어느 도구로 낸
값인지 이 문서가 적어 두지 않아 위 70 과 가릴 수단이 없다**(커밋이 움직인 것인지 세는
대상이 다른 것인지 재현할 방법이 없다). 같은 절의 909 · 11 · 29 는 도구와 모수 커밋
(`d7dc4079`)을 함께 적어 두었고, **한 문서 안의 그 불균일이 이 한 자리를 못 믿게 만든다.**

**세 행의 근거 강도가 다르다.** 아래 둘은 소스에서 게이트를 직접 찾아 붙인 **양성 귀속**이고
(40 건 전부가 셋 중 하나로 분류됐다 — 미분류 0), 위 909 는 **충분조건이지 유일 원인이 아니다**
— 그중 몇이 자기 항목에도 cfg 를 달고 있는지는 재지 않았다(모듈 게이트 하나로 이미 사라지므로
채널 판정에는 영향이 없지만, "cfg 를 떼면 살아난다" 를 이 수로 추론하면 틀린다).

**분류 자체가 텍스트 근사라는 한계도 함께 남긴다.** 게이트는 그 파일에도 부모에도 없이
**조부모의 다른 파일**에 있을 수 있고(`#[path]` 재지정·`cfg_attr`·매크로 생성 모듈도 같다),
그래서 줄 단위 grep 은 **양성만** 말할 수 있고 "없다" 는 말할 수 없다. 위 표는 조합별
`cargo test -- --list` 차집합(949)을 **모수로 고정한 뒤** 그 안에서 원인을 찾은 것이라,
총량은 실행이 정하고 분류만 텍스트가 한다 — 분류가 틀려도 949 는 안 움직인다.

**귀결이 둘이다.**

- **리팩터가 조합 노출을 바꾼다.** 파일을 게이트된 모듈 밖으로 `git mv` 하면 본문과 cfg 를
  한 줄도 안 고쳐도 그 파일의 테스트가 양 조합으로 늘고, 반대로 옮겨 넣으면 한 조합에서
  사라진다. 코드 리뷰에서 "이동뿐" 으로 보이는 변경이 채널을 바꾼다.
- **이동만이 아니라 생성도 그렇다.** 위 909 는 "게이트된 파일 수" 가 아니라 **"게이트된
  루트 아래에 있는 테스트 수"** 다 — `mod` 선언 하나가 서브트리 전체에 게이트를 물려주므로,
  그 아래에 파일을 **새로 만들기만 해도** 그 수가 조용히 는다. **가드를 어디에 둘지는 이
  축에서 먼저 결정한다**: 조합 대조가 목적인 가드를 `adapters::ui` 나 `view` 아래에 만들면
  그 가드는 태어날 때부터 한 조합에서만 돈다.
- **"텍스트 스캔 가드는 cfg 에 면역" 에는 선행 조건이 있다.** 런타임에 `.rs` 를 읽는
  가드는 컴파일된 심볼을 참조하지 않아 cfg 소거에 강하지만, 그건 **그 가드 파일 자체가
  게이트된 모듈 아래에 있지 않을 때** 이야기다. 위 909 가 그 조건이 얼마나 자주 깨지는지의
  값이다 — 스캔 로직이 통째로 컴파일에서 빠지면 스캔 대상이 디스크에 있어도 아무 일도
  일어나지 않는다.

**소스를 런타임에 스캔하는 드리프트 가드에게 "컴파일만 자동" 은 0 이다** — 스캔 로직이
컴파일돼도 실행되지 않으면 아무것도 보지 않는다. `tests/*.rs` 에 있는 가드는 이제
`check-headless` 에서 돌지만, 그 잡은 `paths-ignore` 로 **문서·site 만 담은 push 에서는
발사되지 않는다.** 레포 전체를 훑는 문서 가드는 하필 그 push 에서 위반을 가장 잘
들이므로, 그 구멍은 남아 있다. 그리고 그 잡은 러너 한 대에 묶여 있어(위 §러너 참고)
채널의 존재가 곧 즉시성은 아니다.

**두 방향 모두 틀릴 수 있다.** 통합 테스트에 "CI 가 강제한다" 를 붙이면 사실보다 강하고,
lib 유닛 테스트에서 그 서술을 지우면 사실보다 약하다. 어느 쪽이든 다음 사람의 판단을
망친다 — 채널을 서술할 때 "없다" 는 "있다" 만큼 확인이 필요하다.

### 조건부 allow 도 조합별로 린트 채널을 지운다

파일 위치가 한 축이라면 **조건부 `allow` 는 다른 축**이다. `#![cfg_attr(not(feature =
"gui"), allow(dead_code, unused_imports))]` 같은 attribute 는 그 조합(headless)에서
해당 lint 를 **끈다** — 그 조합에서만 도는 자동 잡(`check-headless`)이 그 lint 를
영영 못 본다. deny 로 승격된 lint 라도 마찬가지다: `allow` 가 deny 를 이긴다.

**crate-level 하나가 조합 전체의 채널을 지운다(실측).** `src/main.rs` 최상단의
`#![cfg_attr(not(feature = "gui"), allow(dead_code))]` 를 임시로 걷고 headless
`cargo check` 를 돌리면 그동안 숨어 있던 dead code 가 다수 error 로 터진다(`enum
Strategy` · `const PAPLAY_SOUND`/`APLAY_SOUND` · `static STRATEGY` 등). 즉 그
attribute 는 no-op 가 아니라 **headless 의 dead_code 채널을 crate 전역으로 삭제**하고
있다 — [R123](../adr/0142-channel-claims-are-written-against-the-working-tree.md) 의
② 형(잡은 돌지만 술어가 못 봐서 초록이 오도)의 전형이다.

**같은 allow 가 중첩되면 자식 제거는 채널을 복원하지 못한다.** inner attribute 는
자손 모듈로 전파되므로, `main.rs`(crate) → `adapters/ipc.rs`(모듈) → `adapters/ipc/
handler.rs`(자식) 처럼 같은 조건부 allow 가 겹쳐 있으면 자식 하나를 떼도 상위가 여전히
그 트리를 덮는다. 자식 allow 제거는 "채널을 되살린 것" 처럼 보이지만 실제로는 중복
제거(no-op)일 뿐이다 — 채널을 되살리려면 **가장 바깥의 allow** 를 걷어야 한다. 그래서
조건부 allow 를 지울 때는 그 자리가 실제로 무엇을 침묵시키는지(가장 바깥인지, 이미 상위가
덮는 중복인지)를 [R136](../adr/0155-global-state-race-prescription-by-parameterization.md)
positive control(일부러 미사용 항목을 심어 그 조합의 잡이 잡는지)로 먼저 확인한다.

**census 는 목록으로만 남기고 일괄로 걷지 않는다.** 레포에는 조건부 `cfg_attr(…, allow(…))`
가 여러 곳에 있고 **자리마다 근거가 다르다** — 플랫폼 분기(`not(all(macos, gui))` 등) ·
`not(debug_assertions)`(release 에서만 dead) · `not(feature = "gui")`(headless 미배선) ·
역방향 `feature = "gui"`(headless 전용 코드). 근거가 살아 있는 자리를 기계적으로 걷으면
다른 조합에서 거짓 경고가 난다. 그래서 걷을 자리는 위 R136 으로 하나씩 판정한다.

## 사람이 돌리는 것 (자동 채널 없음)

| 검사 | 명령 | 누가 언제 |
|---|---|---|
| 전체 스위트 | `cargo test --workspace --locked` | 병합 후 main 에서 conductor 1회. `test.yml` 의 `test-linux-x64` 잡을 수동 실행해도 같다. **그것이 자동 채널 위로 새로 사는 것은 아래에서 잰 대로 1 건이다** |
| Linux x64 gui 컴파일 | — | **더 이상 여기 없다.** `check-headless` 의 `cargo test (linux, gui, unit)` 스텝이 main push 마다 본다 |
| 기본 조합 clippy (Linux) | `cargo clippy --workspace --all-targets --locked` | 각 작업 lane. CI 에서 이 조합을 보는 것은 Windows 잡뿐이다 |
| dist 산출물 빌드 | `scripts/build-*.sh` | `build-check.yml` 수동 실행 |

### 남은 것은 둘이고, 둘 다 **디스플레이를 요구한다** (2026-09-05 실측)

Linux gui 유닛 스텝이 붙은 뒤 모수를 다시 잡았다 — 술어가 바뀌면 모수도 다시 잡는다.
세는 방법은 두 조합의 명부를 실제로 뽑아 차분하는 것이다(위 "조합 격자의 빈 칸" 의 절차).

    기본 feature 통합 타깃 96 / headless 통합 타깃 95      차 = gui_tests 하나
    기본 feature 통합 항목 723 / headless 통합 항목 691
      → gui_tests(33) 를 빼면 **기본에만 있는 항목 0**, headless 에만 있는 것 1

즉 **통합 테스트에는 조합 사각이 없다.** 남은 칸은 정확히 둘이다:

1. **`multi_window_owner_routing` 1 건** — `check-headless` 가 이름으로 `--skip` 한다
   (사유는 그 워크플로 주석). 이 하나가 "전체 스위트를 자동으로 올리면 새로 사는 것" 의
   전부다. **Xvfb 아래에서는 통과한다**(실측 2.77s) — 디스플레이 없이는 기본 feature
   e2e 하네스가 0.10s 만에 죽는다. 즉 이 칸은 *배선 불가*가 아니라 *디스플레이 비용*이다.
2. **`tests/gui_tests.rs` 33 건** — 33 개 전부 `#[ignore]` 라 조합을 바꿔도 안 돈다.

두 번째가 왜 "성질" 인지는 실제로 돌려 봐야 갈린다. 돌려 봤고, 막는 것이 셋이었다:

- **부모의 `TASTY_SURFACE_ID` 상속** — 자식이 help 만 찍고 죽는다. 하네스 결함이었고
  고쳤다(`tests/gui_common/mod.rs`). 이것만 남으면 이 스위트는 *지시받은 대로 돌린
  사람에게 100% 실패*한다 — 이 저장소의 에이전트는 전부 tasty 안에서 돈다.
- **`TASTY_HOME` 을 격리하지 않는다** — 그대로 돌리면 개발자의 **살아 있는 세션**을
  띄워 몬다(실측: `workspace_count: 7, tab_count: 25` 가 그대로 보였다. 격리하면 1/1).
  e2e 하네스는 격리하고 이쪽은 안 한다.
- **나머지** — 위 둘을 치우고 전용 Xvfb 에서 돌리면 **일부는 통과한다.** 다만 **몇 개가
  통과하는지에 답이 없다**: 한 프로세스로 돌리면 2, 테스트마다 프로세스를 가르면 5,
  다른 계기로는 11 이 나온다. 공유 인스턴스 `Mutex` 의 poison 전파(한 panic 이 뒤 30 건을
  무관한 이유로 죽인다)와 테스트 간 상태 의존이 수를 **서로 반대 방향으로** 흔든다.
  그 축은 이 문서의 몫이 아니라 그 스위트의 건강 문제다.
  다만 **왜 대부분이 안 도는지는 갈렸다**: 33 건을 "OS 전역 입력(`enigo`)을 쓰는가" 로
  가르면 그쪽 26 건이 **26/26 실패**하고 통과가 0 이다. 프로세스 안 IPC 주입만 쓰는 쪽은
  5 통과 2 실패다. 창 관리자를 띄워도 같았던 이유가 그것이다 — `enigo` 는 "그 순간 OS
  포커스를 가진 무엇" 에 넣으므로 WM 유무가 아니라 가상 디스플레이에 그 포커스가
  성립하는가가 문제다.

그래서 이 칸은 **디스플레이만 주면 풀리는 종류가 아니다.** "N 통과" 를 커버리지로 인용하지
마라 — 계기(한 프로세스인가 갈랐는가)와 디스플레이(전용인가 공유인가)를 같이 적어야 뜻이
생긴다.

#### 이 칸은 세 층이고, 셋째에는 **단일 값이 없다**

한 덩어리로 세면 칸의 크기가 열 배로 부푼다. 층마다 답의 **종류**가 다르다:

| 층 | 값 |
|---|---|
| 디스플레이가 사는 것 | **1** — `multi_window_owner_routing`. `#[ignore]` 가 아닌데 창이 없어 못 돌았다 |
| 디스플레이가 **못 사는** 것 | **33** — `gui_tests` 전부가 `#[ignore]` 라, 디스플레이가 있어도 평범한 `cargo test` 는 한 건도 안 돈다. 이쪽이 요구하는 것은 디스플레이가 아니라 **플래그**다 |
| `-- --ignored` 를 줘도 나오는 수 | **단일 값이 없다** — 계기마다 다르고 서로 반대 방향으로 흔들린다 |

**앞의 둘은 "얼마인가" 에 답이 있고 셋째는 답이 없다.** 뭉쳐서 세면 셋째가 앞의 둘과 같은
종류의 수처럼 보인다. 값이 없다는 것을 값 자리에 적는 것이 정직한 칸이다.

셋째 칸의 수를 인용할 때는 **반드시 계기를 함께** 적는다. 그 규율은
`crates/tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs` 가 집행하는데,
**층마다 테스트를 따로 둔다** — 한 테스트 안의 세 단정으로 두면 앞이 죽는 순간 뒤가 아예
안 돌아서 한 번에 하나씩만 판정된다:

| 층 | 집행하는 테스트 |
|---|---|
| 디스플레이가 사는 것 | `the_gui_layer_a_display_revives_is_exactly_the_one_named_test` |
| 디스플레이가 못 사는 것 | `the_gui_suite_needs_a_flag_not_a_display` |
| `--ignored` 를 줘도 나오는 수 | `the_gui_ignored_layer_has_no_single_value` |

셋째는 수를 박지 않는다 — 박으면 그 수가 곧 낡고, 낡은 수는 없는 수보다
나쁘다([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md)).
대신 **"단일 값이 없다" 는 단정 자체**를 지킨다: gui 스위트의 통과 수를 적은 **절**은 그
절이나 그 하위 절에 그 단정을 함께 담아야 한다. 범위가 파일이 아니라 절인 이유는, 파일로
물으면 한 문서 안의 무관한 두 문장이 서로를 위반으로 만들기 때문이다(실측으로 밟았다).

재는 명령:

```bash
env -u DISPLAY -u WAYLAND_DISPLAY TASTY_HOME=/tmp/gtiso \
  xvfb-run -a --server-args="-screen 0 1920x1080x24" \
  cargo test --workspace --locked --test gui_tests -- --ignored --test-threads=1
```

**`TASTY_HOME` 격리를 빼지 마라** — 빼면 그 명령이 네 실제 세션을 몬다.

**그리고 이 명령은 부하 실험이다.** 33 건을 갈라 돌리면 인스턴스를 33 개 띄우고 약
**40 GB** 를 쓴다(격리 홈 하나가 1.2 GB — `target/debug/builtin-plugins` 가 정확히 그
크기이고, host 가 부팅할 때 그것을 `<TASTY_HOME>/plugins/` 로 전량 복사한다). self-hosted
러너와 개발 박스를 여러 lane 이 나눠 쓰므로, 이런 측정은 **다른 회차가 도는 중에 돌리면
그 회차가 커밋이 아니라 이 측정을 잰다.** 실측으로 그 형태가 났다 — 같은 창에서 헤드리스
전량의 실패 바이너리 수가 2 에서 12 로 뛰었고, 원인은 커밋이 아니었다.

### 자동 채널이 없는 것이 **결함이 아닌** 갈래

위 표는 "기계가 판정할 수 있는데 아직 자동으로 안 도는 것" 이다. 그것과 섞으면 안 되는
갈래가 하나 더 있다 — **결론이 사람·에이전트의 판단인 절차.** `docs/ai-verification/` 의
시각 검증·DPI 배율·IME 조합이 그것이고, 이쪽은 명령의 종료 코드가 답을 주지 않으므로
"채널이 없다" 가 결함이 아니라 **성질**이다. 자동화할 수 있는 것은 그 절차의 *입력*
(스크린샷을 찍는 것, 배율을 거는 것)까지이고 판정은 아니다. 그래서 이 두 갈래는 모수를
함께 세지 않는다 — 섞으면 "자동 채널 없음" 의 개수가 고칠 수 있는 것보다 커 보인다.

**다만 그 갈래는 문서 단위로 갈리지 않는다.** 문서 단위로 세면 다섯 문서가 전부
"판정이 사람" 쪽으로 넘어가는데, 절차 단위로 가르면 그렇지 않다 — 실측 59 절차 중
사람의 판정이 답인 것은 9 이고, 23 은 판정이 아니라 **측정을 유효하게 만드는 전제**
(Xvfb Xauthority · 측정 전 바이너리 최신 확인 · 저장한 PID 로만 정리)다. 갈래표와
세는 규칙은 [ai-verification/index](../ai-verification/index.md) "이 문서군의 절차 중
무엇이 자동화 대상인가" 가 정본이고, 그중 기계가 볼 수 있는 것과 없는 것의 경계도
거기 있다. 이 표("사람이 돌리는 것")가 갖는 것은 **`gui_tests` 에 자동 채널이 없다**는
쪽이고, 절차 문서가 무엇을 지시하는가는 그쪽이 갖는다.

### `script-gates.yml` — 배선한 날의 상태

이 워크플로는 **배선했다는 것과 초록이라는 것을 갈라 적어야 하는 실례**다.
배선 시점에 두 스크립트를 작업 트리에서 직접 돌린 결과는 `rc=0`(둘 다)이다. 다만
그 직전까지 `check-intent-discipline.sh` 는 **위반 50 건으로 오래 빨갰다** — 채널이
없어 아무도 안 봤고, 그 사이 문서 셋(`docs/design/flows/action-dispatch.md` ·
[ADR-0037](../adr/0037-complexity-gate.md) · `docs/architecture/invariants/index.md`)
은 그것을 살아 있는 게이트로 인용하고 있었다.

**빨간 채로 배선하지 않았다.** 50 을 먼저 갈랐고, 36 이 술어의 오탐이었다 —
주석·문자열을 코드로 셈(2) · 테스트 본문을 위반으로 셈(22) · 이름만 같은 다른 타입의
메서드(6) · 질의 API 를 변이로 셈(1) · 사유 주석이 다음 줄에 있어 못 봄(2) ·
면제 경로가 트리 재조직을 안 따라감(3). 술어를 고쳐 36 이 사라졌고, 남은 14 에
사유를 적었다. 근거는 `scripts/check-intent-discipline.sh` 머리말에 있다.

**둘째 스크립트는 상한 래칫이다 — 리포트로 두면 안 되는 이유가 있다.**
`check-allow-reason.sh` 는 원래 건수와 무관하게 `exit 0` 했다. 그 상태로 CI 스텝에
넣으면 **잔여를 안은 채 영원히 초록인 칸**이 하나 생긴다. 채널은 도는데 술어가
아무것도 안 보는 형태이고, 초록이 뜨니 아무도 다시 안 본다.

잔여가 0 이 아니라 hard-fail 도 답이 아니다(그 자리에서 main 이 빨개진다). 그래서
[전선 가드](../../src/source_guards/length_constant_frontier.rs)와 같은 형태를
썼다 — **상한을 박고 세 방향을 다 본다**: 늘면 실패, **줄어도 실패**(상한을 같이
내리라는 뜻), 스캐너가 깨져도 실패. 셋째 방향이 핵심이다. 상한이 실제 건수보다
크면 그 차이만큼 새 위반을 조용히 받아주므로, **남는 여유가 곧 안 보는 구간**이다.
상한은 한 방향으로만 돈다 — 올리려면 그 한 줄을 고쳐야 하고, 그것이 리뷰에 보인다.

**면제 경로는 이제 썩지 않는다** — 목록의 경로가 실재하지 않으면 스크립트가 `exit 2`
로 죽는다. 예전에는 없는 경로가 조용히 무시돼 다섯이 죽어 있었다.

### 사유 열의 진위는 어디까지 기계가 보는가

면제·예외 표에는 거의 항상 **사유 열**이 붙는다(`// intent-exempt: <사유>` ·
`#[allow(...)] // reason:` · CLI-gap 표의 "대신 이걸 쓰라"). 게이트가 보는 것은
보통 **사유가 있는가**까지다. 그래서 다음이 성립한다:

> 사유의 존재는 기계가 본다. 사유의 진위는 통째로는 아무도 안 본다.
> **다만 사유가 좌표·명령·수를 들면 그 조각은 볼 수 있다.**

뒷문장이 중요하다 — 셋째 절이 없으면 "어차피 못 본다" 로 읽히고, 그건 틀렸다.
`tests/cli_method_table_parity.rs` 가 CLI-gap 표의 사유가 *"대신 이걸 쓰라"* 며 든
명령이 실재하는지 대조했더니 **실재 결함 둘**이 나왔다. 그 행들은 읽는 사람을 없는
명령으로 보내면서 동시에 "그러니 이 메서드는 면제해도 된다" 는 결론을 지탱하고
있었다. 사유가 참인지를 통째로 물으면 답이 없지만, 사유가 **든 좌표**만 물으면
답이 있다.

**집행 규칙: 사유 형식을 정할 때 검사 가능한 조각을 일부러 넣게 만든다.** 자유
서술만 받으면 검사할 것이 아무것도 안 남는다. `check-intent-discipline.sh` 는 그래서
사유 안에서 두 조각을 읽는다:

| 조각 | 무엇을 주장하는가 | 무엇으로 거짓이 되는가 |
|---|---|---|
| `[결과사용]` | 큐를 우회하는 이유가 "응답이 필요해서" 다 | 그 자리가 호출 결과를 버리는 문장이면 거짓 |
| `[부재 <파일> <정규식>]` | "아직 그 변형이 없어서" 우회한다 | 그 정규식이 그 파일에 나타나면 전제가 사라진 것 |

모르는 `[...]` 태그는 통과가 아니라 실패다 — 오타 하나로 검사가 조용히 꺼지면
**검사가 있다는 사실 자체가 거짓**이 된다.

**그 전에 술어가 무엇을 세는지부터 봐야 한다.** 사유의 진위를 논하기 전에, 검사가 그
억제를 **보기는 하는가**와 그 사유를 **알아보기는 하는가**가 먼저다. `check-allow-reason.sh`
는 세 곳에서 좁았고 셋 다 대상이 아니라 표기를 세고 있었다(2026-09-05 실측).

| 좁았던 곳 | 무엇을 놓쳤나 |
|---|---|
| 형태가 `#[allow(` 뿐 | `#[cfg_attr(<조건>, allow(...))]` **60 자리**가 통째로 감사 밖 |
| 마커가 영문 전용 | 이 레포가 쓰는 한글 `이유:` **41 자리**가 "근거 없음" 으로 계산 |
| 창이 직전 한 줄 | 사유는 여러 줄 주석 블록이라 블록의 **마지막 줄**만 보였다 |

셋을 고치자 감사 대상이 319 → 379 로 늘고 잔여가 234 → 231 이 됐다. **두 수는 같은 것을
센 값이 아니다** — 위반이 줄어서가 아니라 두 변화가 상쇄된 결과다.

★ **조건부 억제가 더 위험한 쪽인데 안 보이고 있었다.** 무조건 억제는 한 자리를 끄지만
조건부 억제는 **어떤 조합에서만** 끈다 — 다른 조합에서 살아 있으니 안전해 보이는데,
그 조합에서 무엇이 꺼졌는지는 아무도 안 본다. 같은 절의
["조건부 allow 도 조합별로 린트 채널을 지운다"](#조건부-allow-도-조합별로-린트-채널을-지운다)
가 그 형태를 이미 적어 두고 있었고, 정작 그것을 세는 게이트가 그 형태를 못 봤다.

**그 60 을 조합별로 실측해 갈랐다.** 자리를 전부 주석 처리하고 조합마다 진단을 baseline 과
차분한 뒤 가장 가까운 선행 억제로 귀속시켰다(linux gui debug · +all-targets · headless ·
headless+all-targets · release · `x86_64-pc-windows-gnu`). 결과는 **8 자리가 어느 조합에서도
한 건도 안 막았고**(지웠다), 39 자리는 **이미 사유를 산문으로 갖고 있었다**(마커만 없었다).
새로 지어낸 사유는 0 이다. macOS 는 크로스 체크가 `libsqlite3-sys` 빌드에서 멈춰
(리눅스 `cc` 가 `-arch` 를 모른다) `target_os = "macos"` 조건의 자리들은 **미측정**이다 —
리눅스에서 안 막는다는 사실은 그 자리들에 대해 아무것도 증명하지 않는다.

**태그를 못 붙이는 사유가 남는다.** "이 함수가 `on_close` 훅이라" · "처리 핸들러
본문의 cascade 라" 같은 주장은 *둘러싼 함수의 역할*에 대한 것이고, 그걸 확인하려면
줄 스캔이 아니라 호출 그래프를 따라가야 한다. 그 추적에 기대면 가드의 정확도가
추적의 정확도로 내려앉는다(같은 이유로 [전선 가드](../../src/source_guards/length_constant_frontier.rs)도
쓰임 기반 술어를 버렸다). **그 사유들은 검사되지 않는다 — 그건 한계지 통과가 아니다.**

**전체 스위트를 자동화하지 않는 이유**는 `test.yml` 헤더에 있다 — 실측 274.5s 중
222.4s 가 GUI 인스턴스를 띄우는 11개라 러너 GPU 가용성에 따라 그대로 flaky 가 된다.
e2e 하네스가 헤드리스로 뜨게 되면 그 비용이 사라지고 자동화가 훨씬 싸진다.

## 로컬 훅이 앞당겨 주는 것

훅은 **옵트인**이다(`git config core.hooksPath .githooks` 1회) — 설치하지 않아도
커밋·push 는 된다. 그래서 훅은 "게이트" 가 아니라 CI 게이트의 **빠른 피드백**으로
읽는다. 상세는 [git-hooks](git-hooks.md).

| 훅 | 검사 | CI 에도 있는가 |
|---|---|---|
| pre-commit | `cargo fmt --check` | ✅ `format-check.yml` |
| pre-commit | mod/use 선언 순서 · `egui::Window` 직접 사용 · `println!`/`dbg!` | ❌ 훅에만 있다 |
| pre-commit | plugin 산출물이 바뀌었는데 매니페스트 `version` 이 그대로 (P.1) | ✅ `plugin-version-check.yml` — **같은 스크립트를 부른다**. 훅은 index 를 `main` 과의 merge-base 와 비교하고(amend·rebase 에 안 흔들리게), CI 는 밀어넣은 범위의 두 끝점을 비교한다 |
| pre-commit | 주석 없는 `let _ =` (C.6) | 부분 — 전수판 `tests/let_underscore_documented.rs` 가 훅의 상위집합이고, 그 전수판이 `check-headless` 에서 자동 실행된다(기본 조합 잡은 `--lib --bins` 라 못 본다). **자동 잡의 clippy 는 `let_underscore_must_use`(warn)로 그 자리를 표면화하지만 이 규칙을 집행하지는 않는다** — 주석을 못 읽어 사유가 달린 정상 코드까지 세는 명부이고, `-D warnings` 가 없어 빌드도 막지 않는다([error-handling](error-handling.md)) |
| pre-push | `cargo clippy --workspace --all-targets -- -D clippy::correctness` | 부분 — Windows 잡의 clippy 는 `--locked` 를 쓰고 correctness deny 를 걸지 않는다 |
| pre-push | `cargo check --workspace --all-targets` | 부분 — CI 는 `--all-targets` 없이 macOS 에서 본다 |
| pre-push | `cargo check --no-default-features` | ✅ `crossplatform-check.yml` |
| pre-push | `cargo test -p tasty-doc-guards` | ✅ `doc-guards.yml` — **같은 크레이트를 부른다**. 훅은 push 하는 머신에서만 돌아 worker 머신엔 이 채널이 없다(아래 R142) |

**"훅에만 있다" 는 줄이 실제로 새는지는 재봐야 안다.** 훅은 우회 가능하고(`--no-verify`)
설치도 옵트인이라 그 줄은 원리적으로 샐 수 있는데, 그것이 *샜는가* 는 별개 물음이다.
main 에 들어온 추가 라인을 훅과 같은 술어로 다시 훑으면 그 자리에서 답이 나온다.

```bash
# 창의 양 끝(원하는 두 커밋). 훅의 면제 경로를 그대로 적용한다.
base=<older>; tip=<newer>
git diff --name-only $base $tip -- '*.rs' | while IFS= read -r f; do
  case "$f" in src/main.rs|src/boot/cli_routing.rs|crates/tasty-cli/*|\
               crates/tasty-tui-simulator/*|site/*|*build.rs|\
               crates/tasty-doc-guards/src/bin/*) continue ;; esac
  git diff -U0 $base $tip -- "$f" | grep -E '^\+' | grep -v '^+++' \
    | grep -E '\b(println|eprintln|dbg)!|egui::Window::' | sed "s|^|$f: |"
done
```

면제를 걸기 전 수도 함께 세라 — 그것이 0 이면 새지 않은 것이 아니라 **계측기가 0 만 내는
형태**다.

즉 **훅에만 있는 검사가 셋**이다(mod/use 순서 · `egui::Window` · `println!`/`dbg!`).
훅을 설치하지 않은 체크아웃이나 `--no-verify` 커밋은 그 셋을 통과한다 — 이것들은 diff
기반이라 CI 로 옮기려면 "무엇을 신규로 볼 것인가" 를 다시 정의해야 해서 지금은 훅에
남아 있다. `let _ =` 만 성격이 다르다: 전수판이 이미 있고 diff 기반이 아니므로, 위
"전체 스위트" 에 자동 채널이 생기면 그 순간 함께 자동화된다.

## 이 문서와 레포가 어긋나지 않게 하는 것

문서가 "CI 가 잡아 준다" 고 적어 두고 실제로는 아무것도 돌지 않는 상태가 이 저장소에서
열여덟 자리에 쌓여 있었다. 컴파일도 통과하고, 틀렸다는 사실은 워크플로 파일을 직접
열어야만 보인다 — 그래서 리뷰로는 걸러지지 않는다.

`crates/tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs` 가 그 형태를 막는다(이 가드 자신도 통합
테스트라 `doc-guards.yml` 과 `check-headless` 두 잡이 돌린다 — 위 규칙이 자기에게도 그대로
적용된다). 문서를 문서로 검사하지 않고 **워크플로에서 자동 트리거를 가진 잡을 읽는다.**
네 축이 있다.

- **명령을 인용한 형태** — **기본 조합**의 자동 잡이 전체 스위트를 돌리는지 보고,
  돌리지 않으면 그것을 강제 장치로 서술한 자리를 전부 짚는다. 문서가 인용하는
  `cargo test --workspace` 는 기본 조합의 명령이라, 헤드리스 잡이 전체 스위트를 돌리는
  것과 섞어 보면 이 축이 통째로 잠잠해진다.
- **명령을 적지 않는 형태** — 자동 잡이 `--test` 로 **이름을 지목한** 통합 테스트 목록을
  워크플로에서 읽어, 그 밖의 `tests/*.rs` 를 집행 장치로 부르는 서술을 짚는다. 좁히지
  않은 자동 잡이 하나라도 있으면 이 축은 스스로 잠잠해진다.
- **반대 방향** — 자동 잡이 lib 유닛 테스트를 돌리는 동안, `src/` 안의 유닛 테스트를 두고
  부재를 적은 서술을 짚는다(사실보다 약하다). 이 전제도 상수가 아니라 워크플로에서 읽는다.
- **조합** — 통합 테스트를 지목하면서 자동 채널의 부재를 적었는데 그 테스트가 실제로는
  도는 자리를 짚는다. 판정 단위는 **그 테스트가 자동으로 도는 조합의 수**다: 0 이면 부재
  서술이 참, 1 이면 어느 조합인지 함께 적어야 참, 2 면 어떻게 적어도 거짓이다. 조합별로
  빌드되는지(`required-features`), 그 호출이 통합 타깃을 만드는지, `--skip` 이 그 타깃을
  통째로 걷어내는지를 함께 본다.

부재를 함께 적은 문장(`수동 전용` 등)은 정당한 서술로 통과시키므로 등록 절차가 없다.
조합이 하나뿐인 채널은 그 조합을 함께 적어야 통과한다(`check-headless 잡에서만` 등).
목록을 가드 안에 복사해 두지 않고 워크플로에서 런타임에 읽으므로, 전체 스위트가 자동
채널에 올라가거나 `--test` 열거·`--skip` 이 바뀌는 날 이 가드는 스스로 따라간다 — 그때
문서를 손으로 다시 훑지 않아도 된다.

## 파생 문서는 채널을 다시 쓰지 않는다

이번 스윕에서 실제로 어긋나 있던 것은 **이 문서가 아니라 채널을 따로 서술한 파생
문서들**이었다(복잡도 게이트의 두 축을 한 문장에 묶어 "자동 차단" 이라 적은 자리들).
정본 하나를 고쳐도 파생이 자기 문장을 들고 있으면 다시 어긋난다.

그래서 규칙은 **다시 서술하지 말고 여기를 링크한다** 이다. 서술이 꼭 필요하면 그 문장이
**실행/컴파일**과 **축 단위 실효성** 둘 다에서 이 문서와 같은 말을 하는지 확인한다.

## 관련

- [git-hooks](git-hooks.md) — 훅 각 검사의 내용과 설치
- [clippy-policy](clippy-policy.md) · [complexity-gate](complexity-gate.md) — lint 정책
- [release-runners](release-runners.md) — self-hosted 러너 구성
