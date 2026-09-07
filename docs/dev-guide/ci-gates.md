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

| 검사 | 명령 | 채널 | 트리거 | 등급 |
|---|---|---|---|---|
| 포맷 | `cargo fmt --check` (+ `site/` · `crates/tasty-plugin-sdk-wasm/` 매니페스트 각각) | `format-check.yml` (ubuntu-latest) | main push · PR · 수동 | [실측] |
| SemVer 가드 | `cargo test --locked --no-default-features --test api_baseline_0_7 --test changelog_unreleased --test cli_naming_count_drift` | `test.yml` 의 `semver-guards` (self-hosted Linux X64) | main push · 수동 | [실측] |
| macOS 컴파일 + 단위테스트 | `cargo check --workspace --locked` · `cargo test --workspace --lib --bins --locked --no-fail-fast` | `crossplatform-check.yml` 의 `check-macos` (self-hosted macOS) | main push · PR · 수동 | [실측] |
| Windows lint + 단위테스트 | `cargo clippy --workspace --all-targets --locked` · `cargo test --workspace --lib --bins --locked --no-fail-fast` | `crossplatform-check.yml` (self-hosted Windows) | main push · PR · 수동 | [실측] |
| headless 컴파일 · **전체 스위트** · lint | `cargo check --workspace --no-default-features --locked` · `cargo test --workspace --no-default-features --locked --no-fail-fast -- --skip <1 건>` · `cargo clippy --workspace --all-targets --no-default-features --locked` | `crossplatform-check.yml` 의 `check-headless` (self-hosted Linux X64) | main push · PR · 수동 | [실측] |
| **not-debug(release) 컴파일 · gui** | `cargo check --workspace --release --locked` | `crossplatform-check.yml` 의 `check-release` (self-hosted Linux X64) | main push · PR · 수동 | [실측] |
| 문서 가드 | `cargo test -p tasty-doc-guards --locked --no-fail-fast` | `doc-guards.yml` (ubuntu-latest) | main push · PR · 수동 — **경로 필터 없음**([ADR-0138](../adr/0138-doc-guards-live-in-a-dependency-free-crate.md)) | [실측] |
| 파일 SLOC | `bash scripts/check-file-size.sh` | `complexity-check.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 | [실측] |
| 동결 총합 래칫 | `bash scripts/check-frozen-sum-ratchet.sh` | `complexity-check.yml` (self-hosted Linux X64, 같은 잡) | main push(문서·site 제외) · PR · 수동 | [실측] |
| Intent 규율 | `bash scripts/check-intent-discipline.sh` — **`mask-source` 판정기를 먼저 짓는다** | `script-gates.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 | [실측] |
| 사유 없는 `#[allow]` (**상한 래칫**, 판정기 `mask-source` 선행) | `bash scripts/check-allow-reason.sh` | `script-gates.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 | [실측] |
| 공용 순회를 안 거치는 직접 `read_dir` (**상한 래칫**, 판정기 `mask-source` 선행) | `bash scripts/check-shared-walk-ratchet.sh` | `script-gates.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 | [실측] |
| plugin 버전 bump | `bash scripts/check-plugin-version-bump.sh --range <before> <after>` | `plugin-version-check.yml` (self-hosted Linux X64) | main push · PR — **둘 다 `crates/**` 가 바뀐 경우** · 수동. ★ 판정 대상이 plugin 디렉토리가 아니라 **워크스페이스 내부 의존 폐포**이고 그 안에서 **출하되는 내용**만 세기 때문에([ADR-0166](../adr/0166-the-plugin-version-gate-judges-the-artifact-not-the-directory.md)) 경로 필터가 `crates/**` 다 — `tasty-utils`·`tasty-shm` 처럼 이름이 `tasty-plugin-` 으로 시작하지 않는 크레이트가 바뀌어도 plugin 산출물이 달라진다. 잡이 출하 판정기(`strip-cfg-test`)를 먼저 빌드한다. ★ **모수**: 이 채널은 **push 된 범위**를 본다. lane 의 pre-commit 은 **staged** 를 본다. 둘은 다른 물음에 답한다 — lane 이 자기 통과를 전체 통과로 읽으면 안 된다. 통합 회차가 `--range <직전 push> HEAD` 로 다시 잰다(아래 "등급" 절) | [실측] |
| 공급망 | `cargo deny check` | `supply-chain-check.yml` | main push(`paths: Cargo.lock · deny.toml`) · PR · 매주 월 09:00 UTC · 수동 | [실측] |
| 사이트 생성 — 가이드 링크 · `ORDER` 누락 | `cargo run --release --manifest-path site/Cargo.toml -- --strict` | `pages.yml` 의 `build` (ubuntu-latest) | main push — `site/**` · `Cargo.toml` · 랜딩 아이콘 · 그 워크플로가 바뀐 경우만 · 수동 | 등급 미정 |

### 로컬에서 이 게이트들을 돌리기 전에 — **판정기부터**

위 표에서 "판정기 `mask-source` 선행" 이 붙은 행들은 그 바이너리가 **없거나 낡으면 거짓
빨강을 낸다.** 그리고 **그 빨강은 회귀와 모양이 같다** — 값이 상한보다 크고 실패 메시지도
같다. 세어지는 것이 늘어난 이유가 코드가 아니라 **세는 사본**인데, 줄만 보면 안 갈린다.

판정기가 없으면 원문에서 세게 되어 **문자열·주석 안의 형태까지 세어진다.** 그 방향은 늘
늘리는 쪽이고, 이 래칫들은 **여유가 0** 이라 곧바로 빨갛다.

```bash
cargo build -p tasty-doc-guards --bin mask-source
./target/debug/mask-source --check-fresh .   # rc=0 이어야 그 아래 값이 유효하다
```

★ **평범한 `cargo build` 는 이 바이너리를 짓지 않는다.** 그리고 `resolve_judge` 는 **낡은
판정기도 없는 것으로 다룬다** — 신선도를 mtime 이 아니라 **내용 지문**으로 묻기 때문이다
(`--check-fresh`). 그래서 재sync 후 `cargo build` 만 한 트리, 새 체크아웃, 임시 worktree 는
전부 이 조건에 걸린다.

⇒ **값이 갑자기 상한을 넘으면 먼저 "판정기가 있나" 를 물어라.** rc≠0 이면 그 값은
**판정 불가**지 빨강이 아니다.

☆ 이 성질은 값을 **두 지점에서 재는** 절차(base 를 임시 worktree 로 떼어 같은 판정기로 재고
차분을 내는 것)에 그대로 붙는다. 다만 그 절차에서는 조건이 **자동으로 검증된다** — 판정기
소스가 두 지점에서 다르면 지문이 어긋나 값이 아예 안 나오고 판정 불가로 떨어진다. 즉 그
방법에는 **조용히 틀린 값이 나오는 경로가 없다.**

### 등급 — 이 표의 각 행이 무엇까지 말하는가

프로젝트 규칙이 요구하는 구분("배선돼 있다는 것과 초록이라는 것은 다르다")을 행마다 붙인
것이다. 등급은 셋이고, **셋째는 칸에 안 찍는다.**

★★ **한 등급은 한 물음에만 답한다.** 여기 물음이 셋이다:

| 물음 | 성질 | 등급 |
|---|---|---|
| ① 배선됐나 | 작업 트리 사실 — 안 낡는다 | **[배선]** |
| ② 초록인 것을 본 적 있나 | 과거 사실, 단조 — 안 낡는다 | **[실측]** |
| ③ **지금** 초록인가 | 현재 상태 — push 마다 바뀐다 | **[미측정]** ← 칸 밖 |

★ **③으로 ①이나 ②를 정당화하지 마라.** 그 형태의 결함이 실제로 났다 — 본 표가
`check-headless` 를 [배선]이라 적는 동안 debug/release 행렬의 같은 잡 칸은 [실측]이었다.
**같은 이름이 두 물음에 답하면 값이 두 개 나온다.** (같은 대상이 두 이름을 갖는 형태의
반대가 아니라 **같은 부류**다 — 어느 쪽이든 이름과 물음이 1:1 이 아닌 것이 원인이다.)

- **[배선]** — `.github/workflows/` 의 작업 트리 파일이 그 조합을 배선했다. 그뿐이다.
  초록인지는 이 등급이 말하지 않는다.
- **[실측]** — 그 채널이 **실제로 초록인 것을 본 적이 있다.** ★ **과거형 사실이지 현재
  상태가 아니다.** 관측은 언제나 **어느 한 회차의 값**이고, 이 등급이 말하는 것은 "그때
  초록이었다" 뿐이다. 그래서 낡지 않는다(한 번 참이면 계속 참이다) — 그리고 같은 이유로
  **지금 초록이라는 근거로 쓸 수 없다.** 지금이 궁금하면 아래 [미측정] 의 명령으로 그
  자리에서 재라. 출처는 둘이다:
  - 2026-09-06 회차에서 conductor 가 확인한 여섯 워크플로
    (Test · Format · Complexity · Script Gates · Plugin Version · Doc Guards).
  - 공급망: 같은 날 `7695667a9` 에서 두 워크스페이스 크레이트에 `license` 필드를 넣어
    통과한 것을 conductor 가 확인했다(그 전에는 `error[unlicensed]` 로 빨갰다).
  - `crossplatform-check` 의 네 잡: run 34062607769 (`7ee6b0678`, 2026-09-06T21:58:01Z) 에서
    `check-macos` · `check-headless` · `check-windows` · `check-release` 가 **넷 다 success**
    인 것을 conductor 가 **잡 줄로** 읽었다.

  ★ **출처로 쓸 수 있는 값은 잡 줄(또는 스텝 줄)로 읽은 것이지 워크플로 줄이 아니다.**
  워크플로 결론은 잡 하나만 빨개도 빨강이라 [실측]을 **과소**로 만든다 — `crossplatform-check`
  가 정확히 그 형태다(잡 넷). 같은 함정이 아래 [미측정] 에도 있어서 정의 머리에 올려 적는다.

  ★ **입도는 그 행의 주어를 따른다.** 이 표의 행은 **잡**을 주어로 하므로(행이 그 잡의
  명령을 적는다) 잡 결론이 맞는 입도다. 주어가 **스텝**인 칸 — 아래 debug/release 행렬의
  `check-headless` 의 gui 스텝 같은 것 — 은 스텝 결론으로 읽어야 한다. 잡이 초록이어도
  그 안의 스텝이 `skipped` 일 수 있고, 그때 그 칸은 그 회차에 존재하지 않는다.
  **두 입도를 섞지 마라** — 섞으면 한쪽은 과소, 다른 쪽은 과대가 된다.
- **[미측정]** — 그 행의 잡이 **빨간 동안**의 등급이다. 잡이 빨간 동안 그 잡이 배선한
  커버리지는 실패가 아니라 **안 본 것**이다. 이것만 칸에 안 찍는 이유는 커밋마다 바뀌는
  값이기 때문이다([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md)) —
  찍으면 그날로 낡는다. 대신 **읽는 사람이 그 자리에서 판정한다**:

  ```bash
  gh run list --limit 10
  gh run view <run-id> --json jobs --jq '.jobs[] | "\(.conclusion) \(.name)"'
  ```

  잡 단위로 읽어야 한다 — 워크플로 결론은 잡 하나만 빨개도 빨강이라, 잡이 여럿인
  `crossplatform-check` 에서는 나머지가 초록인 것이 안 보인다.
- **등급 미정** — 초록을 본 적이 없고 배선만으로 판단하기도 곤란한 행. 모른다고 적는 것이
  [배선]이라 적는 것보다 정직하다.

★ **등급이 답하지 않는 물음이 하나 더 있다: 그 채널이 무엇을 모수로 재는가.**
`plugin-version-check.yml` 이 그 실물이다. 이 채널은 [실측]이 맞지만, 그것이 재는 범위는
**직전 push 지점부터 지금까지**이고 lane 이 로컬에서 돌리는 `--staged` 검사가 재는 범위는
**그 커밋 하나**다. 두 물음이 다르다 — 앞은 "발행된 값과 지금 내용이 짝이 맞나", 뒤는
"내 커밋이 버전을 올렸나". 그래서 **모든 lane 이 자기 기준으로 통과하고도 통합 지점에서
빨개질 수 있다.** 실측으로 그 형태가 났다(한 크레이트 변경이 세 번들 plugin 의 워크스페이스
의존 폐포 안이라 산출물이 달라진 경우 —
[ADR-0166](../adr/0166-the-plugin-version-gate-judges-the-artifact-not-the-directory.md)).
누구의 잘못도 아니다. **그 판정은 병합하는 쪽이 `--range <직전 push> HEAD` 로 한다.**
등급을 올려도 이 어긋남은 안 사라진다 — 등급의 축이 아니라 **모수의 축**이기 때문이다.

**판정(2026-09-06): 두 모수를 다 둔다.** 한쪽을 없애면 각각 이렇게 깨진다 —
`--staged` 를 없애면 **push 전에 답할 수 있는 채널이 사라지고**(발행 판정은 push 지점을
아는 쪽만 할 수 있다), `--range` 를 없애면 **분할 착지에서 같은 버전 아래 두 산출물**이
남는다. 비대칭이 선택을 정한다: `--staged` 의 과잉 요구는 **버전이 부푸는 것**으로 끝나고,
`--range` 의 부재는 **재시작 전에는 영영 반영 안 되는 조용한 실패**다.

실측이 그 과잉을 값으로 보여 준다 — 한 lane 의 커밋에서 `--staged` 는 위반 3, 같은
내용에 대해 `--range <직전 push> HEAD` 는 통과였다. **둘째 bump 는 산출물이 요구한 것이
아니라 모수가 요구한 것**이다.

그래서 고친 것은 모수가 아니라 **메시지**다(R504 — 판별식이 메시지 밖의 지식을 요구하면
안 된다). `--staged` 로 걸렸을 때 스크립트가 이제 스스로 말한다: 자기가 본 범위가
"이 커밋 하나" 라는 것, 발행이 묻는 범위는 다르다는 것, 그리고 처방 둘 —
**(가) 앞 bump 커밋에 합치거나 (나) 한 번 더 올리고 최종 값은 병합하는 쪽이 정한다**.

그리고 이건 **일회성이 아니다.** 같은 형태가 하루 안에 두 번 났고(통합 회차의 게이트
빨강 하나, 그리고 다른 lane 이 자기 검사로 스스로 다시 잡은 것 하나), 둘 다 원인이 같다 —
`tasty-ui-widgets` 처럼 여러 번들 plugin 이 링크하는 워크스페이스 크레이트를 고치면
그 폐포 안의 plugin 산출물이 전부 달라진다. 그러니 **그 크레이트를 건드리는 회차마다
나온다고 보고 통합 지점에서 `--range` 로 다시 재는 것**이 정상 절차다.

★ **모수의 둘째 형태 — 그 채널에서 조건이 아예 안 생기는 가드.**
`crates/tasty-doc-guards/tests/build_cache_markers_are_name_prunable.rs` 가 실물이다. 이
가드는 레포를 순회해 빌드 캐시 표식이 이름 제외 밖에 있는지 본다. 그런데 그 표식을 만드는
디렉토리는 **문서화된 e2e 절차를 로컬에서 돌린 사람에게만** 생긴다 — CI 는 그 절차를 안
돌아서 그 디렉토리가 없다. 그래서 **이 가드는 CI 에서 영원히 초록이다.** 빨강은 절차를
따른 사람의 기계에서만 난다.

이건 "가드는 있는데 돌릴 채널이 없다"(아래 [사람이 돌리는 것](#사람이-돌리는-것-자동-채널-없음))와 다르다. 거기는 가드가 있고 돌릴
채널이 없다. 여기는 **채널이 있고 그 채널에서 트리거 조건이 절대 안 생긴다.** 그리고
**등급이 아니다** — 등급 축은 "무엇이 지키는가" 하나이고, 이건 그 가드가 **무엇을 모수로
훑었는가**의 문제다. 위 plugin-version 과 같은 부류다(R476/R497).

판정: **"CI 에서 영원히 초록" 은 그 축에서 초록이 아니라 미측정이다.** 빈 모수를 훑은
초록과 실제로 판정한 초록이 같은 줄로 보이기 때문이다.

⇒ 집행: **모수가 환경마다 달라질 수 있는 가드는 자기가 훑은 수를 노출한다.** 위 가드는
이미 그렇게 한다(`MIN_DIRS_WALKED` 하한 + 실패문이 순회 수를 찍는다). 그 수가 없으면
"안전하다" 와 "애초에 못 봤다" 가 구별되지 않는다.

⇒ 그리고 여기서 나오는 일반형: **"doc-guards 초록" 은 기계마다 모수가 다르다.** 여러
사람이 같은 타깃의 수를 세다 값이 어긋나면 **그 차이를 자기 커밋에 귀속시키지 마라** —
먼저 그 가드의 모수가 기계에 의존하는지 물어라. 실측으로 그 형태가 났다(한 워크트리에서만
84/1, 원인은 그 기계에만 있던 9.7 GB 짜리 e2e 캐시 디렉토리였다).

★ **처방을 낼 때 그 처방을 잴 수 있는 채널의 이름을 함께 적는다.** 채널이 없으면
**"없다" 고 적는다** — 있는 척하는 단정보다 낫다. 이 문서가 채널의 정본이므로 그 이름은
여기서 고른다.

실측(2026-09-06)이 이 규율을 값으로 만들었다. Windows 잡만 빨간 결함이 있었고, 원인은
스캐너가 경로 구분자를 플랫폼 것 그대로 낸 것이었다. **그 처방은 Linux 에서 검증할 수
없다** — 고치기 전에도 Linux 는 `/` 를 내므로 세 leg(full · unit · headless)은 처방 전에도
초록이었고 처방 뒤에도 초록이다. 증거는 `check-windows` 하나뿐이었고 그것이 답했다.

⇒ 만약 "검증했다" 를 적으려고 **Linux 에서 도는 단정**을 하나 붙였으면 그 초록은 처방
전후로 똑같았을 것이다. 검증했다고 적힌 채 아무것도 안 잰 상태가 남는다. 그러니
**잴 수 없는 자리에 계기를 달지 마라** — 그 자리에는 계기 대신 "이 축의 채널은
`<잡 이름>` 하나다" 를 적는다.

★ **한 규칙에 채널이 둘 다 붙지는 않는다 — 축을 잘라서 절반만 붙인다.** `CLAUDE.md` 의
"문서 갱신 (필수)" 는 사용자에게 보이는 동작(메뉴 · 단축키 · 설정 키 · CLI 명령 · 설치
절차)이 바뀌면 가이드도 같은 커밋에서 갱신하라고 요구한다. 그 다섯을 하나씩 재 보면
**같은 규칙 안에서 채널이 갈린다.**

| 축 | 두 쪽이 같은 어휘를 쓰는가 | 채널 |
|---|---|---|
| CLI 명령 | 쓴다 — 양쪽 다 `tasty <명령>` | `crates/tasty-doc-guards/tests/every_cli_command_is_classified_in_the_guide.rs` |
| 설치 절차 · **산출물 파일명** | 쓴다 — 버전 자리만 `${VERSION}` ↔ `{ver}` | `crates/tasty-doc-guards/tests/every_released_artifact_is_named_in_the_guide.rs` |
| 설치 절차 · **설치 사실**(위치 · 의존) | 쓴다 — 셋으로 갈린다(아래) | `crates/tasty-doc-guards/tests/install_facts_match_the_packaging_sources.rs` |
| 설치 절차 · **절차 본문**(순서 · 명령) | 안 쓴다 — 원본이 배포판 관례라 저장소에 없다 | **없다** |
| 훅 이벤트 이름 | 쓴다 — 사용자가 `--event <이름>` 으로 직접 친다 | `crates/tasty-doc-guards/tests/every_hook_event_name_is_in_the_guide.rs` |
| 설정 키 · **절 이름** | 쓴다 — 양쪽 다 `[general]` 같은 식별자 | `crates/tasty-doc-guards/tests/settings_and_menu_names_reach_the_guide.rs` |
| 메뉴 · **컨텍스트 메뉴 항목** | 쓴다 — 가이드가 `lang/ko.toml` 값을 글자 그대로 인용한다 | 같은 파일 |
| 설정 키 · **필드 이름** | 안 쓴다 — 아래 | **없다** |
| 메뉴 · **macOS 애플리케이션 메뉴** | 안 쓴다 — 아래 | **없다** |
| 단축키 | 안 쓴다 — 설정은 `alt+up`, 가이드는 `Alt+↑` 에 산문 등가("`Alt` 도 됨")까지 | **없다** |

단축키 축을 재 봤다(실측 2026-09-06): `preset_tasty()` 가 내는 (필드, 조합) 55 쌍 중 47 이
가이드에 그대로 있고, 나머지 8 은 전부 **표기 변형**이었다 — 가이드가 틀린 것이 아니라
같은 것을 다르게 적는다. 여기에 문자열 대조 가드를 놓으면 가장 싼 초록화 경로가 **가이드를
설정 파일 어휘로 고쳐 쓰는 것**이 된다. 그건 보호 대상을 깎는다.

**설정 필드 축과 macOS 앱 메뉴 축도 재고 접었다**(실측 2026-09-06).

★★ **설치 절차 축은 앞 판정을 뒤집었다**(재측정 2026-09-07). 2026-09-06 에 절차 본문을 "소스가 WiX 선언과 스크립트 내부 변수라 공통 어휘가 없다" 로 접었는데, 다시 재니 **셋으로 갈렸다** — 앞 판정은 셋째 하나를 보고 전체를 덮은 것이었다.

- **어휘가 그대로 같다** — `/usr/bin/tasty`(rpm 의 `dest`) · `libvulkan1`(deb 의 `recommends`) ·
  `vulkan-loader`(rpm 의 requires 키). 셋 다 가이드에 글자 그대로 있다.
- **한 홉 변환으로 같다** — Windows 경로는 `wix/main.wxs` 의 `Name=` 사슬(`tasty` · `bin` ·
  `tasty.exe`)을 이어야 나온다. 같은 파일 안에서도 형식마다 문법이 다르다: deb 는 배열형
  `["target/release/tasty", "usr/bin/", "755"]`(목적지가 **디렉토리**)이고 rpm 은 표형
  `dest = "/usr/bin/tasty"`(전체 경로)다. 하나만 보면 다른 하나가 바뀌어도 조용하다.
- **소스에 없다** — glibc 하한(`GLIBC_2.39`)은 저장소 문자열 0 이고 **빌드 러너 이미지**가
  정한다. 절차 본문(설치 순서 · `sudo apt remove tasty`)도 원본이 배포판 관례다.

⇒ 이 축이 단축키 축과 갈린 지점: 설치 경로에는 **두 어휘가 없다.** `/usr/bin/tasty` 는
하나뿐이고 어긋나면 그냥 틀린 것이라, 가장 싼 초록화가 가이드를 **참값으로** 고치는 것이다.
단축키는 설정 어휘와 읽는 표기가 일부러 다르고, 거기서는 같은 초록화가 가이드를 기계
어휘로 끌어내린다.

★★ **셋째 갈래("소스에 없다")는 종착역이 아니다** (2026-09-07, 원칙 2.3 에서 확인). 위
세 갈래는 *가이드↔소스* 축에서 만든 것인데, **소스 안에서만 성립하는 규칙**에 그대로 대면
셋째로 잘못 떨어지는 것이 있다. 그때 한 번 더 물을 것이 있다 —
`crates/tasty-doc-guards/tests/debug_handlers_live_in_cfg_declared_modules.rs` 가 debug
격리에서 먼저 쓴 갈림이다:

- **의미 물음** — "이 자리가 그 규칙을 어기는가". 사람이 판정한다.
- **배치 물음** — "그 자리가 **갈래와 사유와 함께 명부에 적혀 있는가**". 판정된다.

원칙 2.3 의 "활성 상태 의존 금지" 가 그 형태였다. 의미 물음은 정말로 안 갈린다 —
`"active": i == state.active_workspace`(보고)와 `engine.workspaces[state.active_workspace]`
(대상 선택)가 **같은 식별자**를 쓴다. 그런데 배치 물음은 갈리고, 그것이
`crates/tasty-doc-guards/tests/agent_facing_reads_of_active_state_are_classified.rs` 다.

⇒ 그 전환의 값은 **모수**에 있었다. 저장소 전체로 세면 922 출현이라 "값싸게는 못 만든다"
가 나온다. 규칙이 말하는 경로(IPC 핸들러와 그것이 부르는 도메인 cascade)로 좁히고 주석·
문자열·`#[cfg(test)]` 를 걷어내면 **33** 이다. 마지막 단계가 특히 크다 — 눈으로 읽으면
테스트 헬퍼 다섯이 위반처럼 보인다(`expect` 를 쓰고 id 해석이 없다).
**모수를 갈라 보기 전에 "채널이 없다" 를 적지 않는다.**


- **설정 필드** — 코드에 *사용자가 고르는 값*과 *내부 영속 슬롯*을 가르는 표시가 없다.
  `theme_base`(테마 색 덤프) · `sidebar_width`(드래그 결과) · `macos_fda_notice_shown`
  (다시 보지 않기 플래그)이 사용자 항목과 같은 구조체에 나란히 있다. 이름으로 가르려고
  `lang/ko.toml` 의 `_label` 접미사를 써 보면 119 중 **91** 이 가이드에 있고 남는 23 은
  대부분 누락이 아니라 어휘 차이다(가이드 "페인 분할 (좌우 / 상하)" ↔ `lang` "페인 수직
  분할" · "페인 수평 분할"). 대조를 놓으면 그 23 을 고발하고, 가장 싼 초록화는 가이드를
  코드 어휘로 고쳐 쓰는 것이 된다.
- **macOS 애플리케이션 메뉴** — 11 항목 중 가이드에 4. 없는 7 중 셋은 제목이 **형식
  문자열**(`{} 정보`)이라 문자열 대조가 원리적으로 못 맞히고, 나머지는 `make_std_item` 으로
  만드는 **OS 표준 항목**(가리기 · 모두 보기 등)이라 가이드가 안 적는 것이 옳다. 남는 모수는
  한 자리이고 그것도 macOS 에만 있다(Windows·Linux 는 등록 자리가 0 이라 거기 초록은
  "잴 것이 없다" 다).

⇒ 반대로 **컨텍스트 메뉴 축은 실측 23/23 이 이미 맞아 있었다.** 가이드가 그 이름들을
`lang/ko.toml` 값 그대로 인용한다 — 같은 "메뉴" 라는 낱말 아래 두 축의 답이 정반대다.

★ **이 기준을 재기 전에 예측으로 걸어 봤다 — 세 축에서 셋 다 맞았다**(2026-09-06,
훅 이벤트 · 원격 프로필 플래그 · 테마 파일 키). 사후설명이 아니라는 증거다. 다만 **한 축은
답이 맞고 이유가 틀렸다**: "가이드의 그 장이 파일 고치기 장인가" 로 예측했는데, 원격 장은
파일 고치기 장이 아니라 **CLI 장**이었고 어휘가 공유된 진짜 이유는 그 문자열을 사용자가
**타이핑한다**는 것이었다. 그래서 술어를 이렇게 적는다 — **개념에 사용자 노출 표기가
있는가**(치거나 눈으로 대조하는 문자열인가), 장의 성격이 아니다.

★★ **모수를 소스의 내부 이름에서 뽑지 마라 — 거짓 미스가 난다.** 같은 회차에 두 번 밟았다:
`remote_tasty` 로 재면 가이드 적중 **0/4** 인데 사용자가 실제로 치는 표기 `--remote-tasty`
로 재면 **4/4** 이고, `ThemeColors` 구조체 필드로 재면 **30/47** 인데 디스크 형태
(`ansi_black` → `[ansi] black`)로 재면 전부 일치다. 내부 이름으로 재는 가드는 **잘 쓴
문서를 고발하고**, 그때 가장 싼 초록화는 문서를 내부 이름으로 고쳐 쓰는 것이 된다.

⇒ 그래서 판정은 "기계적으로 잴 수 있는가" 가 아니라 **"두 쪽이 같은 어휘를 쓰는가"** 이고,
그 다음이 **"빨개졌을 때 가장 싼 초록화 경로가 보호 대상을 깎는가"** 다. 둘 중 하나라도
걸리면 계기 대신 이 표에 **"없다"** 를 적는다.

★★ **두 조건이 갈라지는 자리가 하나 있다 — 앞것이 안 서는데 뒷것만으로 놓은 가드다.**
`crates/tasty-doc-guards/tests/one_concept_one_word_on_the_user_facing_surface.rs` 는 사용자
표면이 한 개념을 두 낱말로 부르는 것을 막는다(`페인` · `윈도우` · `ウィンドウ`). 여기서
앞 조건은 **안 선다** — 위 표의 축들은 사용자가 그 문자열을 *치기* 때문에 어휘가 공유됐는데,
이 낱말들은 사용자가 **읽기만 한다.** 그런데도 놓은 것은 뒷 조건이 확실하기 때문이다:
빨개졌을 때 통과 경로가 셋인데(정본으로 교체 · 명부에 자리와 근거 등록 · 문장 삭제)
제일 싼 것이 **정본으로 교체**이고 그것이 곧 의도한 수정이다.

⇒ 그래서 두 조건은 **and 가 아니다.** 앞것은 "그 가드가 무엇을 대조할 수 있는가" 를 묻고
뒷것은 "빨개졌을 때 무엇이 일어나는가" 를 묻는다. 앞것이 서면 대조 대상이 저절로 정해지지만,
안 서더라도 **한 쪽 안의 일관성**처럼 대조 대상이 다른 방식으로 정해지는 축이 있다.
그때 판정을 지는 것은 뒷것 하나다.

★ **빨강을 귀속하기 전에 절차가 하나 있다.** 한 조합에서만 난 빨강은 직전 회차에
귀속하기 전에 **같은 커밋으로 그 잡을 재실행해 대조군을 만든다** — 재실행이 빨강이면
결정론, 초록이면 그 회차에 귀속되지 않는다. 언제 재실행을 쓰고 언제 안 쓰는지(자리가 이미
지목된 빨강 · 회차가 그 자리를 실제로 건드린 빨강은 쓰지 않는다), 그리고 재실행이 초록이어도
사건이 안 닫히는 이유는 [ADR-0188](../adr/0188-a-red-is-not-attributed-until-a-rerun-control-answers.md).
그 잡이 **지금** 무엇인지는 값이 아니라 아래 `gh` 명령으로 잰다.

★ **모수의 셋째 형태 — 빨강을 진단할 때 좁히는 축.** 한 조합에서만 나는 실패를 진단할
때 사람은 자동으로 **"이번에 무엇이 바뀌었나"** 로 좁힌다. 그 좁힘은 자주 틀린다. 실측
(2026-09-06): macOS 잡의 `--lib --bins` 스텝이 빨개졌고, 여덟 사람이 그 회차가 바꾼
`src/` 파일 14 개로 좁혔다. 좁힘 자체는 정확했다 — 그런데 **터진 자리를 그 회차는 한 번도
안 건드렸다**(`git log <before>..<after> -- <그 파일>` = 0 커밋).

바뀐 것은 그 파일이 아니라 **같은 바이너리에서 함께 도는 형제의 수**였다(같은 조합의
`--bin tasty` 가 2329 → 2341). 형제는 프로세스 전역을 공유한다 — 환경변수, 전역 락,
스레드, fd, 포트. 그래서 **좁히는 축은 "무엇이 바뀌었나" 가 아니라 "무엇이 같은
프로세스에서 함께 도는가" 다.**

같은 자리에서 부하 가설도 수로 갈렸다. 부하가 원인이면 **형제도 함께 느려져야 한다**:

    회차 73  2329 passed 0 failed  10.55s
    회차 74  2341 passed 1 failed  30.07s   ← 예산 30s 정각

형제 2341 개는 예전 속도로 끝났고 늘어난 19.5 s 는 **걸린 하나의 예산 소진분**이다.
⇒ 러너가 느려진 것이 아니다. **가설이 요구하는 값이 안 나오면 그 가설은 반증된 것이다.**

그리고 이 진단이 한 줄로 갈린 것은 **판별 메시지가 갈래를 나눠 뒀기 때문**이다 — 그
단정은 "레지스트리에서 사라졌다" 와 "종료 신호가 안 왔다" 를 다른 문구로 냈고, 나온 문구가
후자라 자리가 즉시 좁혀졌다. 실패 메시지에 값을 싣는 것이 그 자리에서 값을 낸다.

★ **등급은 "무엇이 지키는가" 한 축만 잰다.** 그 가드가 **옳은 곳을 지목하는가**는 다른
축이고, 여기 칸으로 안 들어간다. 빨강이 났는데 그 이유가 흔든 자리를 안 가리키면, 그
이유가 시키는 처방을 따를수록 보호가 사라질 수 있다 — 그런 가드도 이 표에서는 여전히
[가드]다. 실제로 이 저장소에 그 형태가 있었다: `ci_channel_claims_match_workflows` 의
실패 메시지가 **판정 범위와 "주어를 단정하지 않는다"** 를 말하지 않아, 저자가 문장을 고칠지
인용을 옮길지 못 정한 채 멈췄다. 등급을 올려도 그 결함은 안 사라진다. **처방은 등급이
아니라 메시지에 있다.**

★ **한 잡의 빨강이 다른 사람의 측정을 지운다.** 실측(2026-09-06): `check-headless` 의 앞
스텝이 죽자 뒤의 gui 스텝 둘과 디스크 진단이 통째로 **skipped** 됐고, 그 회차에 그 조합은
존재하지 않았는데 로그에는 실패로도 안 남았다 — 줄 자체가 없어서 조용하다.

★ **이 관측은 [미측정]의 정의 그 자체이지, 표의 등급을 내리는 근거가 아니다.** 앞선 판에서
이 문단은 "그래서 `crossplatform-check` 의 네 행이 [배선]에 머문다" 로 이어졌는데, 그것은
**현재형 판정으로 과거형 등급을 눌러 쓴 것**이었다. 잡이 빨간 동안 그 커버리지가 미측정으로
떨어지는 것은 맞다 — 다만 그때 답이 바뀌는 물음은 "**지금** 초록인가"([미측정])이지
"초록인 것을 본 적이 있는가"([실측])가 아니다. 등급이 내려가는 것이 아니라 **등급이
답하는 물음이 다른 것**이다. 스텝이 건너뛰어지지 않게 하는 쪽의 처방은 아래
[그 잡이 초록인가](#그-잡이-초록인가-그리고-그-결과가-읽히는가) 절에 있다.


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
| 필터 없는 채널을 가진 것 | 23 | `crates/tasty-doc-guards/tests/` — `doc-guards.yml` 은 경로 필터가 없다 |
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

### 처방을 낼 때는 그 처방을 재는 채널의 이름을 함께 적는다

어떤 성질은 **한 플랫폼에서만 관측된다.** 그 성질의 처방을 다른 플랫폼에서 도는 단정으로
"검증했다" 고 적으면, 그 단정은 처방 전후로 똑같이 초록이다 — 검증했다고 적히고 실제로는
아무것도 재지 않은 상태가 남는다.

실례가 경로 구분자다. `Path::strip_prefix` 는 그 플랫폼의 구분자를 남기고, 그 결과를
문자열로 펴서 소스에 박힌 `/` 리터럴과 맞추면 Windows 에서만 어긋난다. 어긋남은 예외가
아니라 **조용한 0** 이라 조회가 전부 빗나간 채 "위반 0" 이 나온다. Linux·macOS 는 고치기
전에도 `/` 를 내므로 세 게이트(full·unit·headless)가 처방 전후로 똑같이 초록이었고,
답한 채널은 `check-windows` 하나였다.

그래서 두 가지를 나눠 적는다.

- **성질을 재는 채널** — 구분자 정규화가 살아 있는지 묻는 단정은
  `crates/tasty-doc-guards/src/source_text.rs` 의 유닛 테스트에 있고, 그것을 **실행하는
  채널은 `check-windows` 하나다**(`--lib --bins`). 그 단정 자리에 채널 이름을 함께
  적어 둔다 — 안 적으면 다음 사람이 Linux 의 초록을 증거로 읽는다.
- **형태를 재는 채널** — 잴 수 없는 성질은 잴 수 있는 형태로 옮긴다.
  `src/source_guards/repo_relative_paths.rs` 는 구분자를 재지 않고, 레포 상대 경로를
  문자열로 펴는 자리가 공용 정규화(`source_text::repo_relative`)를 지나는지를 본다.
  형태는 소스에 있으니 **어느 플랫폼에서든 같은 답**이 나오고, 그래서 이 가드는 유닛
  타깃이 도는 모든 조합에서 채널을 갖는다.

채널이 없으면 **"없다" 고 적는다.** 있는 척하는 단정보다 낫다 —
[ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md) 가 수에
대해 말한 것과 같은 이유로, 검증 주장도 그 출처를 잃으면 낡은 채로 읽힌다.

### 같은 파일이라도 **컴파일 채널과 실행 채널이 다르다** — 결론에 둘을 갈라 적는다

`crates/tasty-doc-guards/tests/*` 가 그 형태다.

- **컴파일 축은 채널이 있다.** Windows 잡의 `cargo clippy --workspace --all-targets` 가 이
  크레이트의 통합 테스트를 타깃으로 잡는다. 플랫폼 API 를 분기 없이 쓰면 거기서 죽는다 —
  실제로 `std::os::unix::fs::symlink` 를 분기 없이 쓴 시험이 `E0433` 으로 잡혔다.
- **실행 축은 채널이 없다.** 이 통합 테스트를 **실행하는** 자동 잡은 `doc-guards.yml`
  하나이고 `ubuntu-latest` 다. Windows 잡의 테스트 명령은 `--lib --bins` 라 여기 안 닿는다.

그래서 "이 크레이트는 Windows 에서 안 돈다" 도 "돈다" 도 둘 다 반쪽이다. 결론을 쓸 때
**어느 축인지 말한다.**

푸시 전에 컴파일 축을 직접 재는 명령:

```bash
cargo check -p tasty-doc-guards --all-targets --target x86_64-pc-windows-msvc
```

★ **타깃이 안 깔렸으면 그 빨강은 코드 결함이 아니라 미측정이다.** `std` 를 못 찾는 실패와
소스의 실패를 같은 칸에 적지 마라(`rustup target add x86_64-pc-windows-msvc`).

실행 축에 남는 미측정을 좁히려면 그 크레이트 안에서 **플랫폼이 답을 가를 수 있는 자리**를
세면 된다. 경로 구분자 축은 그렇게 셌고 지금 0 이다 — 판정에 쓰이는 레포 상대 경로는 전부
공용 정규화나 손 정규화를 거치거나 성분 하나짜리 이름이고, 나머지 평탄화는 진단 문자열이라
구분자가 **찍히는 글자만** 바꾼다. 세는 술어는
[`repo_relative_paths`](../../src/source_guards/repo_relative_paths.rs) 가 들고 있고, 그
가드는 bin 이라 Windows 의 `--bins` 로도 돈다. 줄 축은 `str::lines()` 가 후행 `\r` 를
떼므로 그 함수를 쓰는 자리는 CRLF 에 안 흔들린다 — `split('\n')` 을 쓰는 자리만 따로 본다.

### macOS 유닛 테스트의 비용은 이 잡의 시간이 아니다

`check-macos` 는 오래 `cargo check` 하나뿐이었고, 그래서 **macOS 로 게이트된 유닛 테스트는
컴파일만 되고 아무도 안 돌렸다.** 지금은 같은 잡에 `--lib --bins` 스텝이 붙어 있다.

**비용을 그 잡의 시간으로 재면 틀린다.** 잡들은 병렬이고 워크플로 벽시계는 **최댓값**이다.
그러므로 이 스텝이 사람을 기다리게 하는 시간은 **macOS 잡이 임계경로 잡을 넘는지**로만
정해진다 — 넘지 않는 동안은 **0** 이다. 수를 여기 적지 않는다
([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md)) — 잡 시간은
커밋마다 바뀐다. 적을 것은 관계와 **재는 법**이다:

```bash
# 회차 하나의 잡별 시간 — 최댓값이 임계경로다
gh run list --workflow=crossplatform-check.yml --limit 5 --json databaseId,conclusion
gh api repos/<owner>/<repo>/actions/runs/<run-id>/jobs \
  --jq '.jobs[] | "\(.name) \(.started_at) \(.completed_at)"'
# 한 잡 안의 스텝별 시간
gh api repos/<owner>/<repo>/actions/jobs/<job-id> \
  --jq '.steps[] | "\(.name) \(.started_at) \(.completed_at)"'
```

★ **러너를 새로 점유하지 않고 잰다.** 이 두 수는 과거 실행에 이미 들어 있다 —
`workflow_dispatch` 로 새로 돌리면 재는 행위 자체가 그 비용을 한 번 치른다.

★ **이 판단이 뒤집히는 조건**(재검토 트리거): self-hosted macOS 러너는 **한 대**다.
`concurrency` + `cancel-in-progress` 가 **같은 ref** 의 연속 push 를 취소해 주므로 지금은
직렬화가 안 일어난다. 그러나 **서로 다른 ref 둘이 동시에 밀리면** 그 취소가 안 걸려 macOS
잡이 직렬로 쌓이고, 그때 macOS 가 새 임계경로가 될 수 있다. 레인이 각자 push 하기 시작하면
그 조건이 성립한다 — 그때 위 명령으로 다시 재고 이 스텝을 유지할지 판단한다.

### macOS 잡의 fd 예산 — 여유가 남아 있는지 단정한다

같은 잡의 `fd budget` 스텝이 `ulimit -n` · `ulimit -Hn` ·
`sysctl -n kern.maxfilesperproc kern.maxfiles` 를 찍고, **soft 상한이 4096 미만이면
실패한다.**

왜 이 자리에 단정이 있나: 그 다음 스텝의 바이너리는 `test_state()` 를 쓰는 시험마다
실제 PTY 와 자식 셸을 띄운다([unit-test-isolation](unit-test-isolation.md) §8). 여유가
사라지면 증상은 여기가 아니라 **아래 유닛 테스트 스텝이 EMFILE 로 깨지는 것**으로 나타나고,
그 빨강은 원인을 안 말한다. 이 단정은 그때 원인 자리에서 먼저 터지라고 있다.

그리고 그 여유는 **우리가 정한 것이 아니라 러너 이미지가 준 것**이다. 이 레포는 워크플로
어디에서도 `ulimit` 을 설정하지 않는다(그 스텝도 읽기만 한다). 그래서 이미지가 바뀌면
여유는 아무 커밋 없이 사라질 수 있다 — 단정이 없으면 그 변화를 아무도 안 본다.

#### 측정값 (2026-09-06 · run 33994212447 · commit `5d00e2641`)

    macOS self-hosted 러너   soft 10240 · hard unlimited
                             kern.maxfilesperproc 122880 · kern.maxfiles 245760
    Linux 최고 fd(실측)      966 (기본 병렬도) · 1157 (`--test-threads 3`)

⇒ 약 9 배 여유. **이 값 때문에 fd 축이 닫혔다.**

★ **두 Linux 수는 방향이 뒤집혀 있다 — 그리고 그 방향은 아직 안 풀렸다.** 병렬도를
줄였는데(`--test-threads 3`) 최고 fd 가 **올라갔다**(966 → 1157).
[unit-test-isolation](unit-test-isolation.md) §8 을 곧이곧대로 읽으면 반대가 나온다 —
거기 적힌 것은 "spawn 총수는 병렬도에 안 움직이고, 바뀌는 것은 동시에 살아 있는 수뿐"
이다. 그 읽기가 맞다면 스레드를 줄일 때 최고치는 내려가야 한다.

확인한 것 둘: 회수 경로 `sweep_idle_ptys` 의 프로덕션 호출자는 **headless 부팅 루프
(`src/boot.rs`) 하나뿐**이고 테스트 바이너리는 그 루프에 안 들어간다. 그리고 idle TTL
기본값은 **5 분**이라 런 길이보다 길 수 있다. 즉 런 도중 회수가 그 경로로는 안 일어난다.
그래도 그것만으로 **오르는** 방향은 안 나온다 — 회수가 전혀 없으면 최고치는 병렬도와
무관하게 같아야 하지 총수보다 커지지 않는다.

⇒ 그러니 **"병렬도를 낮춰 fd 를 아낀다" 를 이 수로 정당화하지 마라.** 실측이 반대
방향이고, 왜 그런지는 아직 안 쟀다. 미측정이지 "효과 없음" 이 아니다.

값을 여기 적는 이유: CI 로그는 90 일 뒤 사라지고, 사라지는 곳에만 있는 값은 다음 사람에게
없는 값이다. 대신 **측정일과 측정 대상을 함께** 박는다 — 이 수는 커밋이 아니라 러너
이미지와 스위트 크기를 따라가므로, 날짜 없이 적으면 낡은 줄 모르고 근거로 쓰인다
([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md)).

#### 다시 재는 법

```bash
# 러너 쪽 상한 — 잡 로그에서
gh api repos/<owner>/<repo>/actions/jobs/<job-id>/logs | grep -A 4 'fd budget'

# 우리 쪽 최고 fd — 테스트 바이너리를 직접 띄우고 /proc 를 표본한다
cargo test --bin tasty --no-run          # 바이너리 경로를 찍는다
# 그 경로를 백그라운드로 띄우고, 도는 동안 `ls /proc/<pid>/fd | wc -l` 의 최댓값을 잡는다
```

단정이 터지면 하한을 내리지 마라 — 재야 할 것은 그 시점의 **실제 최고 fd** 다. 위 두 수를
다시 재서 여유가 정말 남아 있으면 그때 하한을 정하고, 이 절의 측정값과 날짜를 함께 고친다.

#### 안 쟀다 — macOS 쪽 **최고 fd**, 그리고 재려면 무엇이 필요한가

위 여유 판정은 **러너 상한 대 Linux 최고치**의 비교다. 같은 스위트가 macOS 에서 실제로 몇
개까지 여는지는 **안 쟀다.** 안 쟀다와 잴 채널이 없다는 다르므로, 무엇이 있어야 재지는지를
적어 둔다.

- **계기** — macOS 엔 `/proc` 이 없어 위 Linux 절차를 그대로 못 쓴다. 테스트 바이너리를
  백그라운드로 띄우고 도는 동안 `lsof -p <pid> | wc -l` 을 표본하는 스텝이 그 자리를 대신한다.
- **값을 남길 자리** — 그 스텝의 출력은 잡 로그에만 남고 90 일 뒤 사라진다. 위 "측정값" 절처럼
  **커밋되는 자리로 옮기는 절차**가 함께 있어야 한다.

계기만 있고 옮길 자리가 없으면 다음 사람은 다시 안 재진 상태에서 시작한다 — 그래서 둘이 한 쌍이다.

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
| macOS + gui | `check-macos`(컴파일 + `--lib --bins` 유닛) **[실측]** | — |
| Windows + gui | `check-windows`(컴파일 + 유닛) **[실측]** | — |
| Linux + headless | `check-headless`(컴파일 + 전체) **[실측]** | — |
| **Linux + gui** | `check-headless` 의 gui 스텝(컴파일 + `--lib --bins` 유닛) **[실측]** | `check-release`(컴파일) **[실측]** |

★ **등급의 출처**(위 "등급" 절의 정의 그대로 — 과거형 사실이다): 성공 회차 둘의 잡·스텝
결론을 직접 읽었다. 거기서 `cargo test (linux, gui, unit)` 은 **success** 였고, 그것이
**Linux + gui + debug 칸이 배선을 넘어 실측으로 올라간 근거**다. macOS 칸도 같은 방식으로
올랐다 — 그 스텝이 처음 도는 회차에서 `cargo test (macos, gui, unit)` 이 **success** 였다.
(과거값이라 값으로 적는다. **지금 초록인가**는 현재형이므로 아래 명령으로 그 자리에서 재라.)

★ **판정은 잡이 아니라 스텝까지 본다.** 잡이 초록이어도 그 안의 스텝이 `skipped` 일 수 있고,
그때 그 칸은 그 회차에 존재하지 않는다 — 실제로 이 저장소에서 그 형태가 났다(아래
"그 잡이 초록인가" 절). 그러니 [실측]으로 올릴 때 근거는 **그 스텝의 `conclusion`** 이지
잡의 결론이 아니다. 재는 법은
위 [비용 절](#macos-유닛-테스트의-비용은-이-잡의-시간이-아니다)의 `gh api` 두 줄과 같고,
`.steps[]` 의 `conclusion` 을 보면 된다.

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

   그 `--skip` 의 사유는 **논증이 아니라 실측이다**(2026-09-06). headless 조합에서 그
   타깃만 돌리면 제품 자신의 진단(`-32017`)이 `window.create` 가 이 조합에서 dispatch
   arm 째로 빠졌다고 말한다. 항목별 사유는 `src/source_guards/headless_app_layer_coverage.rs`
   의 명부가 들고 소스 가드가 지킨다.

   ★ 그리고 이 칸 옆에 오래 있던 미측정 하나가 닫혔다 — **이 러너에 `xvfb-run` 이 있다**
   (2026-09-06 · run 33994212447 · commit `5d00e2641`). 그 회차에 관측용 gui/Xvfb 스텝이
   **처음으로 실제 실행**됐고 그 테스트가 통과했다. 그 전 세 회차는 앞 스텝의 실패·취소로
   `skipped` 라 물음이 던져지지도 않았다. 그래서 그 스텝의 승격 조건 둘 중 앞엣것
   (러너에 `xvfb-run` 이 있는가)은 충족됐고, 남은 것은 **연속 N 회 초록**이다.
   N 은 아직 정하지 않았다 — 초록 한 번으로 정하면 그 수는 관측이 아니라 선호가 된다.
2. **`tests/gui_tests.rs` 33 건** — 33 개 전부 `#[ignore]` 라 조합을 바꿔도 안 돈다.

두 번째가 왜 "성질" 인지는 실제로 돌려 봐야 갈린다. 돌려 봤고, 막는 것이 셋이었다:

- **부모의 `TASTY_SURFACE_ID` 상속** — 자식이 help 만 찍고 죽는다. 하네스 결함이었고
  고쳤다(`tests/gui_common/mod.rs`). 이것만 남으면 이 스위트는 *지시받은 대로 돌린
  사람에게 100% 실패*한다 — 이 저장소의 에이전트는 전부 tasty 안에서 돈다.
- **`TASTY_HOME` 을 격리하지 않는다** — 그대로 돌리면 개발자의 **살아 있는 세션**을
  띄워 몬다(실측: `workspace_count: 7, tab_count: 25` 가 그대로 보였다. 격리하면 1/1).
  e2e 하네스는 격리하고 이쪽은 안 한다.
- **나머지** — 위 둘을 치우고 전용 Xvfb 에서 돌리면 **일부는 통과한다.** 다만 **몇 개가
  통과하는지에 답이 없다.** 그 값이 왜 없는지는 2026-09-07 에 한 번 갈렸다.

  ★ **먼저, 한 프로세스로 잰 값 둘은 사건 수가 아니었다.** "한 프로세스로 돌리면 2 ·
  다른 계기로는 11" 은 **뮤텍스를 센 수**다 — 공유 인스턴스 락이 `.lock().unwrap()` 이라
  한 건이 단정에서 죽으면 뒤가 전부 `PoisonError` 로 죽었다. 그때의 "31 실패" 는
  **진짜 1 + 전파 30** 이다. 증거는 차분이다: 그 한 건을 `--skip` 해도 수가 안 줄고
  **다음 한 건이 그 자리를 차지한다.** 사건이 줄을 서 있던 것이지 사건이 많았던 게 아니다.
  ⇒ 그 둘을 커버리지로 인용하지 마라. **살아남는 값은 프로세스를 가른 계기의 5** 다
  (각자 자기 프로세스라 전파가 없다).

  ★ **그 전파(B)는 이제 걷혔다.** `tests/gui_common/mod.rs` 의 공유 인스턴스 접근자가
  `.lock().unwrap_or_else(|poisoned| poisoned.into_inner())` 로 오염에서 복구한다.
  **그런데도 수는 아직 단일 값이 아니다** — 증폭기가 둘이고 배타였는데 남은 하나가 산다:

  | 증폭기 | 무엇을 만드는가 | 상태 |
  |---|---|---|
  | **B** 뮤텍스 오염 연쇄 | spawn 성공 뒤 본문 패닉이면 **가짜 실패** N 개 | 걷혔다 |
  | **A** `get_or_init` 재시도 | spawn 자체가 패닉하면 다음 테스트가 **실제로 재spawn**(프로세스 N 개) | **아직 산다** |

  A 를 막는 장치는 형제 하네스에 있다 — `tests/common/mod.rs` 의 `SHARED_SPAWN_FAILED`
  래치. `tests/gui_common/mod.rs` 에는 **없고**, 그 파일의 초기화 클로저 주석이 그 사실을
  스스로 적어 두고 있다("이 클로저는 panic 하면 `OnceLock` 을 미초기화로 남긴다").
  ⇒ **"단일 값이 없다" 는 여전히 참인데 근거가 B 에서 A 로 옮겨갔다.** 참인 채로 근거가
  바뀐 것이라 문장은 살리고 근거를 갈았다. A 는 가짜 실패가 아니라 실제 재spawn 을 만들어서,
  한 프로세스로 잰 값은 여전히 사건 수가 아니다.

  ☆ 아래 33 은 이 정정의 대상이 **아니다.** 그것은 사건 수가 아니라 `--ignored --list` 의
  항목 수라 지금도 맞다. **수를 지우지 말고 무엇을 센 수였는지로 바꾼다** — 33 은 틀린 수가
  아니라 다른 것을 센 수다.
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

**셋째 칸의 값을 하나 더 쟀다 (2026-09-07).** 이 칸의 규칙이 "계기를 함께 적어라" 이므로
계기부터 적는다.

    전용 Xvfb(공유 디스플레이 아님) · 한 프로세스 · `--test-threads=1` · `TASTY_HOME` 격리
    컴파일은 벽시계에서 뺐다(`--no-run` 으로 먼저 지었다)
    → **3 통과 / 30 실패** · 110.05 s · 격리 홈 최대 1155 MB(스위트당 부팅 **1 회**)

★ 이 값의 계보에 둘을 함께 적는다. **적어야 이 수가 다음 사람에게 뜻이 있다.**

1. **잰 트리가 지금 main 이 아니다.** 잰 뒤에 `cf9ca89e7` 이 이 스위트를 고쳤다
   (`UiState` 에 `active_tab` 을 더하고 탭 전환을 sleep 대신 관측으로 바꿨다).
   즉 이 3/30 은 **그 변경 이전** 값이다. 다시 재면 달라질 수 있다.
2. **위 문단의 2 / 5 / 11 과 모수가 또 다르다.** 이 넷을 시계열로 읽으면 안 된다 —
   추세가 아니라 **계기가 다른 네 개의 값**이다.

☆ 그리고 이 값은 위의 "enigo 쪽 26 건이 26/26 실패" 와 어긋나지 않는다. 이번에도 키보드
쪽 통과는 **0** 이고, 통과한 셋은 전부 프로세스 안 IPC 주입 쪽이다(그 7 중 3 통과 4 실패).

★★ **그 0 의 뜻을 고쳐 적는다 — "다 빨갛다" 가 아니라 "못 쟀다" 다.** 값은 그대로 0 이다.

이 스위트에서 키 자극의 **양성 대조**가 될 수 있는 시험은 정확히 셋뿐이다 —
`test_notification_panel_{toggle,close_escape,speed}`. 그 셋만 관측 채널이 키보드 경로에
옳게 배정돼 있다: 키보드 경로가 `popups` 의 "notifications" 를 **직접 토글**하고
(`src/adapters/ui/input/shortcuts/keybinding.rs:239`) 그 상태가 **지속**되며
(`src/adapters/ipc/handler/debug_state.rs:35` 가 그대로 노출한다), 한 프레임 플래그가 아니다.

나머지는 대조가 못 된다. `state.settings_open` 을 관측하는 일곱은 **채널이 어긋나 있다** —
그 필드는 사이드바 **클릭** 경로(`src/adapters/ui/draw.rs:33`·`:61`, `r.settings_clicked`)만
세우고, `dispatch_pending_modal_opens`(`src/view/main/redraw.rs:206-208`)가 같은 프레임에
false 로 되돌리며 `AppEvent::OpenSettings` 로 바꾼다. 키보드 경로는 그 이벤트를 직접 보내고
**그 필드를 아예 안 건드린다.** 즉 `Ctrl+,` 를 눌러 `settings_open == true` 를 보는 일은
계기와 무관하게 **원리적으로 없다.** 그리고 남은 열여섯은 조합이 낡아(위 표) 대조가 못 된다.

⇒ **그 셋이 이 회차에 전원 음성이었다.** 양성 대조 전체가 음성이면 그 축은 빨간 것이 아니라
**미측정**이다. 그러므로 "키보드 통과 0" 을 *키보드 단축키가 동작하지 않는다* 로 읽지 마라 —
이 계기가 **키 자극을 창에 넣었는지 자체가 안 밝혀졌다**는 뜻이다.

☆ **왜 못 넣었는가는 이제 가설이 아니다 — 갈렸다.** `GuiTestInstance::focus()` 에 Linux
분기가 없었다. WM 없는 Xvfb 에서 그 창이 X 포커스를 갖는지 아무도 관리하지 않고, `enigo` 는
*그 순간 OS 포커스를 가진 무엇*에 넣는다. 같은 커밋·같은 계기·같은 세 이름·같은 묶음에서
두 팔로 갈랐다:

    포커스 분기 있음   3 passed / 0 failed
    포커스 분기 없음   0 passed / 3 failed — 셋 다 `notification panel open` 타임아웃,
                       `notification_panel_open: false`

부하 셋(5.53 / 7.77 / 10.85)에서 양성 팔이 재현됐다. 그래서 이 값은 계기 값이 아니라 기제
값이다. 분기는 `xdotool search --pid` → 넓이 최대 창 → `xdotool windowfocus` 다
(`windowactivate` 는 WM 에 요청하는 것이라 WM 없는 디스플레이에서 안 선다).

#### 도착 카나리아 — **키 자극을 재는 회차는 이 셋을 먼저 읽는다**

위에서 갈린 대로, 이 스위트에서 키 자극의 **도착**을 증언할 수 있는 시험은 그 셋뿐이다.
그러므로 규율이 하나 선다: **키 자극 판정을 담은 회차는 그 셋을 먼저 돌리고, 셋이 음성이면
그 회차의 나머지 판정을 전부 버린다.** 빨강이 아니라 **미측정**으로 적는다.
섞어 두면 사후에만 읽히고, 앞에 세우면 사전에 읽힌다.

★ **그리고 카나리아는 "몇 건이 돌았는가" 를 함께 찍어야 한다.** 이름 필터로 부르므로 이름이
바뀌거나 필터가 안 맞으면 `0 passed; 0 failed` 에 **rc=0** 이 나온다 — **안 돈 것이 다 통과한
것과 같은 줄을 만든다.** 카나리아는 그 회차를 통과시키는 관문이라, 그 자리에서 조용해지면
뒤의 판정 전부가 근거 없이 살아난다. 그래서 판정은 rc 가 아니라 **`3 passed` 라는 수**로 한다:
셋이 아니면 초록도 빨강도 아니고 **판정 불가**다.

**그 규율을 켜고 다시 쟀다 (2026-09-07).** 계기는 위와 같고 포커스 분기만 더했다.

    카나리아 3 통과 → **6 통과 / 27 실패** · 100.41 s

☆ 그러므로 앞 문단의 3/30 은 **같은 스위트의 더 낡은 계기 값**이다. 두 수를 시계열로 읽지
마라 — 바뀐 것은 스위트가 아니라 **키가 창에 닿았는가**다. 초록 6 은 마우스 3 과 위 카나리아
3 이고, 키보드 쪽 초록은 그 셋뿐이다.

#### 셋업을 고치면 초록이 되는가 — **아니다. 스위트가 나빠진다**

열여섯이 공유하는 `Ctrl+Shift+N` 셋업을 preset 의 `alt+n` 으로 맞추고 같은 계기로 재발사한
값(2026-09-07):

    **3 통과 / 30 실패** · 115.81 s — 위 6/27 보다 **나쁘다**
    그리고 본 발사 안의 **카나리아 셋이 전원 음성**이 됐다(같은 발사 직전 단독 실행은 3 통과)

⇒ 카나리아 규율대로 그 회차의 개별 판정은 버린다. 다만 **왜** 죽었는지가 값이다. 실패 시점의
`workspace_count` 를 시험 순서대로 늘어놓으면 갈린다:

    셋업 낡은 채   1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1
    셋업 고친 뒤   2 3 4 5 5 5 5 5 5 5 5 5 5 5 5 5 5 5 5 5 5 5 5

셋업은 **실제로 살아난다**(1 에 붙박이던 수가 오른다). 살아나자 **뒷정리가 완료되는 경로가
없다**는 것이 드러난다. `press_alt(Key::Unicode('W'))` 열일곱 자리 전부에
`wait_for_ui("ws closed" …)` 가 붙어 있는데, 뒷정리에 **닿은** 시험은 거기서 죽고, 대부분은
본문 중간에서 죽어 **뒷정리에 닿지도 못한 채** 워크스페이스를 남긴다. 공유 인스턴스가
오염되고, 다섯쯤에서 카나리아 차례가 오면 그것도 죽는다 — **키가 죽은 게 아니라 상태가 죽는다.**

⇒ 그러므로 이 수리는 **단계로 자를 수 없다.** 셋업만 고치면 스위트가 나빠진 채 남는다.
  셋업과 뒷정리를 **같은 걸음**에 고쳐야 한다. 그래서 그 셋업 수정은 착지시키지 않았다.
☆ 위 문단의 **"상태가 죽는다"** 는 그때의 읽기였고, 뒤에 반증됐다 — 아래 절 참조. 값(수의 나열)은
  그대로 두고 해석만 정정한다.
☆ 닫힌 자리: `press_alt` 가 Shift 를 안 보내던 것과 `press_alt_shift` 헬퍼 부재는 해소됐다.

#### 누적이 5 에서 멎는 이유 — **누적이 아니었다. 화살표가 반대다**

위 나열(`2 3 4 5 5 5 …`)을 "쌓인 워크스페이스가 뭔가를 막는다" 로 읽었다. **틀렸다.**
스위트 밖에서 키 없이 `workspace.create` 만 반복하면 열셋까지 아무 문제 없이 늘고, 단축키
게이트는 내내 열려 있다. 즉 **워크스페이스 수는 게이트와 인과가 없다.**

수가 5 에서 얼어붙는 것은 원인이 아니라 **자국**이다. 각 시험이 워크스페이스를 단축키로 만들고
단축키로 닫으므로, 단축키가 죽으면 **만들지도 닫지도 못해** 수가 안 움직인다. 같은 데이터,
반대 방향.

★ **진짜 기제 — 한 시험이 남긴 popup 포커스가 그 뒤 전부를 막는다.**
단축키 경로는 `keyboard_overlay_open` 이 참이면 **테이블 전체**에 진입하지 않는다
(`src/view/main/keyboard.rs`). 그 술어의 한 항이 `popups.has_focused()` 다. 그리고 이 스위트는
**한 인스턴스를 전부가 공유한다.** 그래서 어떤 시험이 popup 을 포커스한 채로 끝나면 —
그 상태를 요구하는 시험이 실제로 있다 — 뒤따르는 모든 시험의 단축키가 죽고, 그 안에 도착
카나리아도 들어간다. **카나리아 음성이 곧 "자극이 안 닿았다" 는 아니다**: 여기서는 자극이
닿았고 게이트가 닫혀 있었다. 그 둘을 가르려면 `ui.state` 가 **어느 항이 참인지**를 항마다
한 칸씩 내야 한다 — 참인 것만 나열하면 "거짓이라 빠졌다" 와 "보고가 그 항을 모른다" 가 같은
모양이 된다.

⇒ 공유 인스턴스를 쓰는 스위트에 붙는 일반형: **시험이 남기는 것은 데이터만이 아니라 게이트다.**
  뒷정리가 자기 시험이 세운 게이트에 걸리면, 그 시험 하나가 뒤의 전부를 판정 불가로 만든다.

#### alt 조합은 "못 잰다" 가 아니었다 — **셋업 조합이 낡았던 것이다**

이 자리에는 원래 "이 스위트로는 alt 를 원리적으로 못 잰다" 고 적혀 있었다. **그 결론을
철회한다.** 값은 그대로 두고 뜻만 바꾼다 — 값이 틀린 것이 아니라 값의 원인을 잘못
지목했다.

값(그대로 유효): `press_alt` 를 쓰는 시험이 **열여섯**이고, **그 열여섯 전부가 첫 alt
자극 앞에 ctrl 자극을 갖는다**(첫 자극이 alt 인 시험 **0 건**). 이름이 alt 를 가리키는
`test_workspace_switch_alt_number` 조차 Alt+1 에 닿기 전 `Ctrl+Shift+N` 셋업에서 죽는다.

바뀐 것은 **그 죽음의 원인**이다. 단축키 층은 ctrl 도 alt 도 살아 있다 — 직접 자극으로
갈렸다(`ctrl+shift+w` 는 pane 을 2→1 로 먹고, `alt+t` 는 tab 을 1→2 로 늘리며, 수정자
상태도 양쪽 다 정확하다). 죽은 것은 층이 아니라 **시험이 누르는 조합**이다:
`crates/tasty-settings/src/keybindings/presets.rs` 의 기본 preset 에 `ctrl+shift+n` 이
**아예 없다**(새 워크스페이스는 `alt+n` 이다).

⇒ 그러므로 열여섯의 빨강은 **ctrl 의 색이 아니라 낡은 기대값의 색**이다. 인용 금지는
그대로지만 이유가 더 좁아졌다 — 열여섯의 빨강을 alt 의 증거로도, ctrl 의 증거로도
인용하지 마라. 그리고 alt 축을 재는 전제는 "첫 자극이 alt 인 새 시험" 이 아니라
**셋업 조합을 preset 에 맞추는 것**이다. 그건 이 스위트 안에서 된다.

#### 그래서 33 건을 다시 갈랐다 — **조합이 낡은 것 / 조합은 멀쩡한데 죽은 것**

위 원인이 갈리면 **6 통과 / 27 실패**(도착 보장 회차)를 원인별로 나눌 수 있다. 소스 전수로 각 시험이 누르는
조합을 뽑아 기본 preset(`preset_tasty` = `Default`)에 조인했다. 규칙 기반 조합(quick-switch
의 `<수정자> + 슬롯키`)도 preset 쪽에 펼쳐 넣었다.

| 갈래 | 수 | 도착 보장 회차의 결과 |
|---|---|---|
| 키 자극이 있고 **preset 밖 조합을 하나라도** 누른다 | **16** | 16 실패 — 전부 `ws created` 에서 |
| 키 자극이 있고 **누르는 조합이 전부 preset 안** | **10** | **3 통과**(카나리아) · 7 실패 |
| 키 자극이 없다(마우스 전용) | **7** | 3 통과 · 4 실패 |

⇒ 27 실패 = **16**(조합 낡음) + **7**(관측 채널 오배정) + **4**(마우스). 33 = 26 + 7.
그리고 6 통과 = 마우스 3 + 카나리아 3.

**이 갈래가 답하는 것과 답하지 않는 것을 나눠 적는다.**

- 답하는 것: 조합을 preset 에 맞춰 고쳤을 때 살아날 수 있는 건수의 **상한이 16** 이다.
  16 을 넘을 수는 없다 — 나머지 열넷은 애초에 낡은 조합을 안 누른다.
- 답하지 않는 것: **하한은 미측정이다.** 고쳐서 다시 돌리기 전에는 열여섯 중 몇이
  실제로 살아나는지에 값이 없다. "상한 16" 을 "16 건이 살아난다" 로 읽지 마라.
- 그리고 **열하나(7 + 4)는 조합 수정으로 설명되지 않는다.** 일곱은 위에서 갈린 관측 채널
  오배정이고(계기와 무관하게 빨갛다), 넷은 키와 무관한 마우스 단정이다.
- ★ 그리고 **상한 16 이 곧 수리 계획이 되지는 않는다.** 셋업만 고치면 스위트가 나빠진다는
  것을 위 절에서 쟀다 — 상한은 조합의 수이지 걸음의 수가 아니다.

★ **열여섯은 열여섯 개의 증거가 아니다.** 그 열여섯 **전부**가 `ctrl+shift+n` 을 셋업으로
쓴다 — 첫 자극에서 죽으므로 뒤의 단정은 한 번도 평가되지 않는다. 즉 이 빨강 열여섯은
**한 자리의 죽음이 열여섯 번 세어진 것**이고, 서로 독립 증거가 아니다. 같은 원인이 N 번
세어지면 **N 이 증거의 강도로 읽힌다** — 그 오독을 여기서 막는다. 수리도 열여섯 곳이
아니라 한 곳(셋업 헬퍼)에서 시작한다.

☆ **부재만이 낡음의 형태가 아니다.** `ctrl+shift+t` 는 preset 에 **있다.** 다만
`restore_closed`(닫은 항목 복원)에 묶여 있고, 그걸 누르는 시험
(`test_new_tab_ctrl_shift_t` 등 다섯)은 **새 탭**을 기대한다. 이쪽은 자극이 무시되는
것이 아니라 **다른 동작이 조용히 일어난다.** "preset 에 있는가" 만으로 고치면 이 다섯은
그대로 남는다. 위 열여섯 안에 이미 들어 있어 수는 안 바뀌지만 **수리의 모양이 다르다.**

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

**넷째는 값의 층이 아니라 방향의 층이다.** 위 셋은 전부 소스와 이 문서만 읽어서, 워크플로
쪽에서 누가 `-- --ignored` 를 넣으면 셋 다 통과하는데 둘째 층의 서술은 거짓이 된다.
`the_gui_suite_channel_claim_points_the_same_way_as_the_workflows` 가 그 자리를 막는다 —
자동 잡이 `--ignored` 를 넘기는가와 이 문서가 부재를 적고 있는가가 **같은 방향인가**만
묻는다. 채널을 넣기로 하면 문서가 부재 표지를 걷어야 하고, 안 넣기로 하면 그 표지가 그
결정을 지키는 자리가 된다. 수동 전용 잡은 안 본다 — 물음이 "자동 채널" 이기 때문이다.

#### ★ 스텝은 앞 스텝이 죽으면 **안 돈다** — 배선돼 있는데 채널이 없는 회차

잡 단위로 읽는 것만으로는 부족하다. **한 잡 안에서도** 앞 스텝이 실패하면 뒤 스텝은
`skipped` 가 되고, 그 회차에 그 스텝이 배선한 조합은 **존재하지 않는다.** 그런데 로그에는
실패로도 안 남는다 — 줄 자체가 없다.

실측(run 33982090607, `check-headless`):

    success  cargo check (headless)
    failure  cargo test (headless)          ← 여기서 죽고
    skipped  cargo clippy (headless)
    skipped  cargo test (linux, gui, unit)
    skipped  cargo test (linux, gui, e2e)
    skipped  disk (diagnostic)

`continue-on-error` 로는 이것을 못 막는다. 그 플래그는 **자기 실패**를 무해하게 만들 뿐,
앞 스텝의 실패로 건너뛰어지는 것은 막지 못한다. 건너뛰지 않게 하려면 `if: !cancelled()`
가 필요하다.

★ 그래서 **없음을 두 갈래로 갈라라**: "그 스텝이 안 돌았다" 와 "돌았는데 결과가 없다"
는 다른 판정이다. 위 회차에서 `command -v xvfb-run` 줄이 없는 것은 러너에 `xvfb-run` 이
없다는 증거가 **아니다** — 그 줄이 있는 스텝이 안 돌았을 뿐이고, 그 물음은 여전히
미측정이다. 진단 줄은 **빨간 회차에서 가장 필요한데**, 건너뛰면 정확히 그때만 없다.

##### 그 진단은 세 회차 내리 **0 회** 였다 (과거값) — 그리고 네 번째에 답이 나왔다

두 시제를 갈라 적는다. **과거값은 값으로 적어도 낡지 않는다**; 현재형은 명령으로만 적는다
([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md)).

- **과거값**: 그 스텝이 존재한 **첫 세 회차**에서는 `command -v xvfb-run` 의 결과 줄이 한 번도
  안 나왔다 — 셋 다 `skipped` 였다(앞의 `cargo test (headless)` 가 실패하거나 취소돼서다).
  그보다 앞선 회차들에는 그 스텝이 **아예 없었다**(스텝 목록에 줄이 없다). 즉 그때의 "0 회" 는
  두 가지가 합쳐진 값이었다: 스텝이 없던 구간 + 있었지만 건너뛰어진 구간.
- **현재형 물음**: *이 러너에 `xvfb-run` 이 있는가.* — **닫혔다: 있다.** run 33994212447
  (2026-09-06 · commit `5d00e2641`)에서 그 스텝이 **처음으로 실제 실행**됐다(위 "남은 것은
  둘이고" 절의 같은 관측이다 — 한 물음에 두 자리가 다른 답을 들지 않게 여기서도 같은 값을 적는다).
- ★ 그래서 이 절이 남기는 교훈은 뒤집히지 않았다: 세 번의 `skipped` 는 "없다" 의 증거가
  **아니었다.** 물음이 안 던져졌던 것뿐이고, 던지자 답이 나왔다. 그때 세 번을 근거로 "없다" 라고
  적었더라면 **그 줄은 지금 틀린 채로 남아 있었을 것이다.**
- **처방이 이미 있고, 아직 안 돌았다**: `if: !cancelled()` 가 그 건너뜀을 막는다. 그러나
  그 수정이 main 에서 한 번 돌기 전까지 위 0 은 안 움직인다. **"다음 회차에 답이 나온다" 는
  예측은 세 번 빗나갔다** — 예측을 반복해 적는 대신, 답이 나오는 **조건**을 적는다.

재는 법(러너를 새로 점유하지 않는다 — 과거 실행에 이미 들어 있다):

```bash
gh api repos/<owner>/<repo>/actions/runs/<run-id>/jobs \
  --jq '.jobs[] | select(.name=="check-headless") | .id'
gh api repos/<owner>/<repo>/actions/jobs/<job-id> \
  --jq '.steps[] | "\(.conclusion) \(.name)"'
```

★ 그 목록에서 **줄이 아예 없는 것**과 `skipped` 는 다르다. 앞은 그 회차의 워크플로에 그
스텝이 없었다는 뜻이고, 뒤는 있었는데 안 돌았다는 뜻이다. 둘을 섞으면 "0 회" 의 원인을
하나로 착각한다.

#### 세 층 중 하나만 **워크플로 파서에 기댄다** — 그 파서는 이제 고정돼 있다

층 2·3 은 소스(`tests/gui_tests.rs`)와 문서만 읽어서, 워크플로를 어떻게 읽든 답이
안 바뀐다. **층 1 만** 워크플로에서 "무엇이 자동으로 도는가" 를 뽑아 쓴다. 그 추출의
잡 분할 규칙(2 칸 들여쓰기 = 잡 헤더)을 3 칸으로 깨뜨리는 변이를 쏘면 잡 절반과 호출
하나가 사라지는데(`bodies 16→8`, `invocations 6→5`), 그 가드의 테스트는 하나도 안
죽는다. 사라진 호출이 마침 층 1 이 지목하는 것이 아니었을 뿐이다.

그래서 **층 1 에 대해서만** 문장을 약하게 쓴다 — "가드가 본다" 가 아니라 **"가드가
본다고 되어 있다"**. 초록은 "덮였다" 와 "안 덮여서 볼 수 없다" 둘 다와 양립하므로,
잡 분할을 고정하는 단정이 서기 전까지 그 초록은 층 1 이 옳다는 증거가 아니다.
**그 단정이 지금 섰다.** 같은 변이를 패키지 전체(`cargo test -p tasty-doc-guards`)에 쏘면
넷이 죽는다 — 잡 분할·접힌 스칼라·잡 수 하한, 그리고 필터 뒤 명부 래칫. 그래서 층 1 의
문장도 다시 "가드가 본다" 로 쓴다.

★ **모수를 옮겨 적지 않는다.** 그 가드 파일 **하나만** 돌리면 같은 변이에서 여전히 전부
초록이다(실측 2026-09-06: 그 파일 52 초록 / 패키지 4 실패). 파서가 고정된 것은 **패키지
모수에서**다. 위 문단의 표를 지우지 않고 남기는 이유가 그것이다 — "무엇이 왜 초록인가" 가
이 층의 실제 성질이고, 그것을 지우면 다음 사람은 파일 하나를 돌려 보고 고정됐다고 읽는다.

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

### 무엇을 돌릴지 고를 때 — **"무엇을 고쳤나" 가 아니라 "고친 것을 무엇이 보나"**

앞쪽으로 물으면 **자기 패키지가 답으로 나오고, 그건 거의 항상 틀린다.** 고친 것이 셸
스크립트여도 그것을 검사하는 가드는 `tests/*.rs` 에 있고, 고친 것이 가드여도 그 가드를
검사하는 것은 또 다른 패키지에 있다. **인구가 반대 방향이다** — 게이트는 레포를 보고,
가드들은 게이트를 본다.

검증의 단위는 **(패키지 × 타깃 × 필터)** 다. `-p <크레이트>` 는 루트 패키지의 통합 타깃을
안 돌리고, `--bin tasty` 는 `tests/*.rs` 를 **아예 안 짓는다**. 이름 필터를 걸었으면 타깃이
맞아도 안 돈 것이다. 그래서 "돌렸다" 가 아니라 **"어느 패키지의 어느 타깃을 필터 없이
돌렸다"** 로 적는다.

찾는 방법은 셋이고, **셋이 서로 다른 것을 잡는다.** 하나만 하면 나머지를 놓친다.

1. **이름으로 읽는 타깃** — 그 가드 소스에 파일 이름이 문자열로 나온다.
   `grep -rn '<고친 파일 이름>' --include='*.rs' tests/ crates/*/tests/ crates/*/src/`
2. **그 디렉토리를 아는 타깃** — 이름은 없고 디렉토리 이름이 나온다.
   `grep -ln '<고친 디렉토리>/' tests/*.rs`
3. **성질로 모수를 잡는 타깃** — ★ **이 갈래가 가장 안 보인다.** 파일 이름도 디렉토리
   이름도 그 가드 소스에 **안 나온다.** 확장자나 shebang 으로 레포를 훑어 인구를 만들기
   때문이다. `tests/no_early_exit_consumer_in_shell_pipes.rs` 가 그 실물이다 — `scripts/`
   라는 문자열을 한 번도 안 쓰고, 모수도 `scripts/` 만이 아니다(`Justfile` 과 워크플로의
   `run:` 블록까지 셸 담는 자리로 센다). 1·2 로 물으면 **안 나온다.**
   `grep -ln '"\.<확장자>"\|shebang' tests/*.rs crates/*/tests/*.rs`

**넷째가 하나 더 있다 — 문서를 고쳤을 때다.** 새로 적은 경로·명령은 소스 가드가 붙잡는다
(`cited_coordinates_exist` 는 인용된 경로가 실재하는지, `cited_just_recipes_exist` 는 인용된
`just <recipe>` 가 `Justfile` 에 있는지). **문서만 고쳤는데 소스 가드가 빨개지는 경로**라
위 셋 어느 쪽으로 물어도 안 나온다.

★ **발견 명령도 명부처럼 낡는다.** 위 명령들이 내는 수는 **그 명령의 모수**이지 "그 파일을
보는 가드의 수" 가 아니다. 같은 명령이 트리마다 다른 수를 낸다. 그러니 수를 낼 때는 **그
수가 무엇의 모수인지**를 같이 적고, 그 수를 다른 물음의 답으로 옮기지 않는다.

☆ 그리고 발견 술어에도 **코드/주석 축**이 든다. 위 1 번은 `//!` 머리말에서 채널 정본으로
그 문서를 **인용만** 하는 타깃까지 센다 — 파일을 읽는 것과 언급하는 것은 다르다. 안 가르면
인구가 부풀고, 부푼 인구는 "다 돌려라" 로 이어져 실행 예산을 태운다.

★★ **발견의 모수와 편집의 모수는 다르다.** 위 명령들이 답하는 것은 "무엇이 이 파일을 보는가"
이지 "무엇을 고쳐야 하는가" 가 아니다. 이름으로 찾으면 **그 이름이 이미 참인 자리**까지
같이 나오고, 그 자리를 고치면 맞는 것을 틀리게 만든다(실측: 한 이름이 122 자리에서 나왔는데
실제 편집 대상은 18 이었다 — 나머지 104 는 그 이름이 참인 자리였다). 발견 목록은 **돌릴
것**을 고르는 데 쓰고, **고칠 것**은 자리마다 다시 판정한다.

★★★ **그 네 갈래에 무엇을 먹이는가 — 여기서 실제로 빨강이 샌다.** 갈래를 다 알아도 입력이
좁으면 소용없다. 자연스러운 입력은 "이 작업이 무엇에 관한가" 인데, 그건 **새로 만든 파일**로
좁아진다. 옳은 입력은 그게 아니라 **편집이 닿은 전부**다:

```bash
git diff --name-only <base>..HEAD    # ← 발견의 입력은 이것이다
```

실측 사례 하나. clap 도움말을 번역하는 작업에서 새 파일 `help_i18n.rs` 에 대해 갈래 1 을
물었고 답을 얻었다. 그런데 같은 작업이 `crates/tasty-cli/src/lib.rs` 에도 설명 주석 두 줄을
넣었고, **그 파일은 `no_hardcoded_ui_strings` 의 clap 도움말 스캔 뿌리 셋 중 하나**였다.
그 술어는 주석이 clap 항목에 붙었는지 안 보고 `#[cfg(test)]` 밖 `///` 의 CJK 를 전부 문다.
갈래 1 을 **`lib.rs` 에 대해** 물었으면 1 초 만에 나왔을 것이다 — 술어가 약한 것이 아니라
입력이 좁았다. 작업의 주제가 아니라 **변경집합**을 먹여라.

### 동결 중에 잴 수 있는 것 — 낡은 바이너리가 현재 트리를 판정한다

컴파일이 금지된 구간에서도 **파일을 런타임에 읽는 가드**는 잴 수 있다. 그 가드의 입력은
링크된 코드가 아니라 파일시스템이라, `src/` 를 고쳐 낡아진 바이너리라도 **지금 트리에 대한
답은 정확하다.** `cargo` 를 부르지 말고 산출물을 직접 실행한다 — 그러면 컴파일 경로가 아예
없어서 "혹시 다시 빌드되나" 를 걱정할 필요가 없다.

```bash
b=$(ls -t target/debug/deps/<타깃>-* | grep -v '\.d$' | head -1)
out=$("$b" 2>&1); rc=$?
```

첫 줄의 파이프는 **rc 를 재지 않는 자리**라서 괜찮다. 재는 자리(둘째 줄)는 파이프가 없다 —
이 둘을 섞어 `"$b" 2>&1 | head -3` 처럼 쓰면 `pipefail` 아래에서 앞 단이 SIGPIPE 로 죽어
**rc 가 그 죽음을 가리킨다.** 값을 먼저 변수에 받고, 자르는 것은 그다음이다.

**전제가 둘이고, 둘 다 확인해야 한다.**

1. **그 판정자 자신의 링크 입력이 바이너리보다 새것이 아닐 것.** 이때 "테스트 소스 mtime"
   으로 물으면 틀린다 — 하네스가 `mod common;` 으로 딸려 오기 때문이다. 빌드가 답을 이미
   적어 뒀다: `target/debug/deps/<타깃>-<해시>.d` 가 실제 링크 입력 전부를 준다.

   ★ **`.d` 는 링크를 답하지 판정 대상을 답하지 않는다.** 레포를 런타임에 순회하는 가드는
   자기가 무는 파일을 링크하지 않는다 — `let_underscore_documented.d` 는 세 줄인데 그
   시험이 실제로 무는 것은 워크스페이스 전역이다. 그러니 `.d` 로 "이 크레이트를 보는
   타깃" 을 세면 0 이 나온다. 여기서 `.d` 를 쓰는 이유는 **그 하나**다: *이 바이너리가
   지어진 뒤에 그 재료가 바뀌었나.* 다른 물음에 옮기지 마라.

   ☆ 그리고 `.d` 자신도 철자다. 세 가지가 붙는다 — ① 경로가 `mod` 가 쓴 철자 그대로라
   `tests/a/../b/mod.rs` 같은 형태가 섞인다(정규화 전후로 수가 달라진다). ② `target/debug/*.d`
   는 절대경로인데 `target/debug/deps/*.d` 는 상대경로다. ③ `deps/` 에는 한 타깃의 낡은
   해시본이 쌓여서, 이름으로 접지 않으면 타깃 수가 부풀어 나온다.
2. **카탈로그를 `include_str!` 로 굽지 않을 것.** 구우면 낡은 바이너리는 **옛 카탈로그**를
   판정한다 — rc 는 초록인데 지금 트리 이야기가 아니다. 그리고 이 갈래는 **철자로 안
   갈린다**: 어떤 가드는 `include_str!` 이라는 문자열을 주석에서만 쓴다. 열어서 그것이
   코드인지 봐야 한다.

전제가 깨지면 그 타깃은 통과가 아니라 **미측정**이다. 그렇게 적어라.

### 갈래가 여럿일 때 게이트를 어떻게 보고하는가 — **rc 가 이미 답하는 경우가 있다**

**먼저 물어야 할 것은 delta 가 아니라 이것이다: 그 게이트에서 `rc` 가 값을 고정하는가.**

상한 래칫이 **양방향**이면(늘어도 실패, 줄어도 실패 — 상한을 같이 내리라는 뜻이다) 그리고
**여유가 0 이면**(건수 == 상한), `rc=0` 은 곧 `값 == 상한` 이다. 상한은 소스에 박힌 커밋된
상수이므로, **상한을 안 건드린 갈래가 낸 `rc=0` 은 정의상 기여분 0 이다.** 그런 게이트에서는
게이트를 흉내 내는 바늘을 따로 만들 것 없이 **부르면 된다** — 보고할 것은 `rc` 와
**상한을 건드렸는가** 둘뿐이다.

극성은 게이트 이름이 아니라 **술어에 붙는다.** 남이 말해 준 극성을 그대로 받지 말고 그
스크립트의 종료 분기를 읽어라 — 두 방향이 다 `exit 1` 인지 한 방향만인지는 파일 하나를
읽으면 끝난다.

**바늘이 필요한 자리는 그 조건이 깨지는 곳이다:**

- **여유가 있는 게이트** — `rc=0` 이 값의 범위만 말하고 값을 안 고정한다.
- **상한이 움직인 경우** — 위반을 하나 없앤 갈래는 규칙대로 상한을 같이 내린다. 그러면 양쪽
  `rc=0` 인데 값이 옮겨간 것이고, 다른 갈래가 하나 늘린 채 합쳐지면 **합친 트리만 넘친다.**
  `gate-delta.sh` 는 그래서 상한을 base 와 HEAD 양쪽에서 읽어 움직였는지 먼저 찍는다.
- **값을 안 찍는 게이트** — 순수 통과/실패라 delta 를 만들 수 없다. 그 사실을 `0` 으로 적지
  않는다("못 잼" 과 "0" 은 다른 값이다).

아래는 그 바늘이 필요한 자리를 위한 것이다.

그 자리에서는 **각 갈래가 초록인데 합친 트리만 넘치는** 형태가 생긴다. 여유가 있는
게이트에서 두 갈래가 각각 하나씩 늘리면 양쪽 다 초록이고 합친 값만 상한을 넘는다. 상한이
움직인 경우도 같다 — 값을 하나 줄이며 상한을 같이 내린 갈래와 값을 하나 늘린 갈래가 합쳐지면,
둘 다 자기 base 에서 초록인데 합친 트리는 **낮아진 상한에 늘어난 값**을 얹는다. 그리고 두
경우 모두 **갈래들이 같은 값을 보고할 수 있다** — 값은 그 갈래의 base 에서의 절대치라,
base 가 다르면 갈래끼리 비교가 성립을 안 한다. 합치는 쪽은 그 보고들로 합친 결과를
예측할 수 없다. (CLAUDE.md 의 plugin 분할 착지 함정과 같은 부류다 — 거기서도 각자
규칙대로 했는데 합류 지점만 어긋난다.)

**delta 는 base 와 무관하게 더해진다.** 그래서 보고 형식은 셋이 아니라 넷이다:

- **rc** — 통과했는가.
- **값** — 지금 몇 건인가(그 트리에서만 의미가 있다).
- **delta** — 내 base 대비 몇 건 움직였는가. 합치는 쪽이 Σdelta 로 합친 값을 **돌리기
  전에 예측**한다. 예측과 실측이 어긋나면 그 어긋남 자체가 신호다 — 예측하지 못한
  상호작용이 있었다는 뜻이라, 이 형식은 보고 편의가 아니라 **판정을 하나 더 만든다**.
- **어느 상태에서 뽑았는가** — tip 과 dirty. `check-shared-walk-ratchet.sh` 의 좌변이
  `git ls-files` 라 **미추적 파일을 안 센다**. 커밋 전에 돌린 `rc=0` 은 거짓 초록일 수
  있고, 그 초록이 믿을 만한 이유는 조심이 아니라 **상태**다.

재는 것은 `scripts/gate-delta.sh <base-rev> [게이트...]` 다. base 를 분리
워크트리로 꺼내 **양쪽 트리에서 게이트를 실제로 돌리고 값을 뺀다.**

**delta 에는 계기를 밝힌다 — diff 로 낸 것은 확정이 아니라 하한이다.** 아래 두 형태가
같은 `0` 을 내고, 그 `0` 이 뜻하는 것이 다르다.

- **재측정(확정)** — base 를 임시 워크트리로 떼어 **값을 만드는 술어를 그대로 다시 돌리고**
  뺀다. `gate-delta.sh` 가 하는 일이다.
- **diff(하한)** — 변경 줄에서 바늘을 센다. 빠르지만 **아래 이유로 조용히 0 을 낸다.**

**diff 로 낸 수를 확정 delta 로 쓰지 않는 이유**는 diff 의 바늘과 게이트의 바늘이 다른
것을 세기 때문이고, 그 어긋남은 **양방향**이다.

- **더 찾음** — 사유 주석을 *붙여서* 넣은 `#[allow]` 은 게이트에 0 인데 diff 엔 1 이다.
  문자열·주석 안의 억제 형태도 diff 엔 보이는데 게이트는 마스킹으로 지운다.
- **덜 찾음** — 사유 *주석만* 지우면 게이트는 늘어나는데 억제 줄은 안 바뀌어 diff 는
  0 이다. 반대로 사유만 더하면 게이트는 줄어드는데 diff 는 역시 0 이다 — 래칫이 양방향이라
  양쪽으로 다 샌다. 그리고 좌변이 파일 집합인 게이트에서는 **파일을 옮기기만 해도** 값이
  움직이는데 순수 이동은 변경 줄에 안 나타난다.

또 하나 — **좌변이 파일 목록이면 delta 는 그 갈래 안에서만 참이다.** 게이트의 바늘이
줄 단위면 delta 는 갈래끼리 더해진다. 그런데 `check-shared-walk-ratchet.sh` 의 좌변은
`git ls-files` 로 고른 **파일 집합**이라, `read_dir(` 를 한 줄도 안 건드리고 **파일을 그
집합 안팎으로 옮기기만 해도** 값이 움직인다(순수 이동은 변경 줄에 안 나타난다). 두 갈래가
**같은 파일을** 각각 고쳐 합쳐지면 Σdelta 와 실측이 어긋날 수 있다 — 그 게이트에서
어긋남을 보면 "예측 못 한 상호작용" 이라고 적기 전에 **같은 파일을 둘이 만졌는가**를 먼저
물어라. 어긋남이 신호가 되는 것은 delta 가 가산일 때뿐이다.

☆ 이 계기는 base 와 HEAD **두 끝점**만 본다. 넣었다 뺀 것(상쇄된 `0`)과 안 건드린 `0` 을
구분하지 않는다 — 병합 예측에는 그 구분이 필요 없다(누적 diff 는 최종 상태만 본다).
그 구분이 필요한 물음은 **갈래의 이력**이고, 다른 계기로 재야 한다. 두 물음에 한 계기를
쓰지 않는다.

★ 그리고 뺄셈에는 조건이 하나 있다 — **양쪽이 같은 술어로 물어야 한다.** 이 계열
게이트의 판정기는 `target/` 에 사는 빌드 산출물이라 **새 워크트리에는 없고**, 없으면
게이트는 실패하지 않고 원문 세기로 **폴백한다**(더 많이 세는 방향). 그때 두 트리의 값
줄은 생김새가 똑같은데 답한 물음이 다르고, 그대로 빼면 **없는 기여분이 보고된다**.
그래서 `gate-delta.sh` 는 양쪽의 폴백 여부를 먼저 대조하고 갈리면 **판정 불가로
실패한다** — 통과가 아니다. 판정기를 넘기는 법은 그 실패 메시지가 알려준다.

이름이 `check-` 로 시작하지 않는 것은 의도다. `ls scripts/check-*.sh` 가 게이트를
발견하는 술어이고 그 출력을 그대로 루프에 먹이는 것이 규율인데, 이것은 게이트가 아니라
측정 도구다(인자 없이 부르면 판정이 아니라 `exit 2`). 그 glob 에 넣으면 남의 루프에서
**빨간 게이트로 보인다** — 발견 술어에 게이트 아닌 것을 넣지 않는다.

**재측정법에는 조용히 틀릴 경로가 없다 — 그게 diff 법과의 진짜 차이다.** 판정기를
환경변수로 **지목해도** `resolve_judge` 는 `--check-fresh` 를 돌린다(실측: 판정기 소스를
한 줄 건드리면 `rc=1` 이 되고, 경로를 명시한 게이트도 그 판정기를 거부하고 폴백한다).
그러니 **base 에서 값이 나왔다는 사실 자체가 "그 판정기가 base 의 판정기 소스와 맞는다" 의
증명**이다. 안 맞으면 값이 안 나오고 판정 불가로 떨어진다. 같은 이유로, 새 체크아웃에서
평소의 값이 나온 것 자체가 판정기가 주입됐다는 **양성 대조**다 — 안 들었으면 원문 세기의
더 큰 값이 나온다.

**잴 수 있는 게이트는 값을 `… : <수>건 (상한 <수>)` 로 찍는 것뿐이다.** 그 밖은 못 잰다고
말하고 실패한다 — 값을 지어내지 않는다. 그리고 판정기를 `scripts/lib/judge-bin.sh` 의
`resolve_judge` 로 찾지 않는 게이트(자기 트리의 `target/` 만 직접 보는 것)는 환경변수를 안
읽어 **새 워크트리에서 그냥 죽는다.** 그때도 값이 없으니 못 잰다고 나오지만, 그런 게이트에
대해서는 재측정법의 위 보증이 서지 않는다는 뜻이기도 하다.

이 스크립트는 게이트의 **본 판정 경로를 건드리지 않는다.** 여유 0 인 래칫이라 판정을
만지는 것 자체가 위험이고, 부르기만 하면 그 위험이 없다.

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
