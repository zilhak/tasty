# Plugin Marketplace 평가

이 문서는 Tasty plugin marketplace (registry / install-by-id / trust / install flow)
도입 여부와 형태를 평가한 결정 근거다. 0.7.x 까지는 현 상태 유지 (`tasty plugin install
<path>` + 수동 git clone) 가 결론이며, 본문은 그 trade-off 의 *현재 상태* 와 *재검토
trigger* 를 한 곳에 기록해 미래에 trigger 발동 시 어떤 비용을 감수했는지를 참조할 수
있게 한다.

본문은 *현재 상태만* 기술한다. 비용 추정은 *측정 없는 추정* 임을 본문 내에서 표기한다.
외부 URL 은 적지 않는다 (`Sigstore`, `cargo`, `npm`, `VS Code`, `Homebrew` 등 명칭만).

본 평가는 `plugin-sandbox-evaluation.md` 의 어조 + TL;DR / 비교표 / 재검토 trigger 3
섹션 구조를 재사용한다 — 단독 열람 (§0 + §6 + §8 셋만 읽고) yes/no/maybe 판단 가능.

## 0. TL;DR

- **0.7.x 까지 marketplace 도입 보류.** 외부 plugin 0 개 시점에서 marketplace 인프라는
  *음수 가치* (유지 비용 > 사용 가치). 현 `tasty plugin install <path>` + 수동 git clone
  으로 충분.
- 재검토 trigger 4 항목 (§8 와 `plugin-ecosystem.md §2` 동기화).
- trigger 발동 시 도입 순서 (가설): ① **read-only index** (Git tap 형식의 단일 manifest
  목록) → ② **install-by-id** (`tasty plugin install <id>` 가 index 에서 fetch) →
  ③ **publisher verification** (반자동, GitHub OAuth 류) → ④ **signature** (minisign →
  Sigstore keyless).
- **WASM sandbox 도입은 marketplace 와 연동** — `plugin-sandbox-evaluation.md §2.4` 의
  4번째 trigger (marketplace 도입 = 비-trusted plugin 일상화). marketplace 와 OS-level
  sandbox 가 *동시 도입* 되어야 의미가 있다.
- POC (구현 spike) 는 수행하지 않는다. spike 비용 추정만 남긴다.

> Marketplace 의 가치는 *발견 가능성* + *설치 편의성* + *신뢰성* 의 묶음이다. 외부
> plugin 0 개 시점에서는 발견 비용도 신뢰 비용도 0 에 가깝다. 인프라 비용만 선부담하는
> 음수 가치.

## 1. 현재 한계 (factual)

### 1.1 동봉 plugin 7 개, 외부 plugin 0 개

`plugin-ecosystem.md §2` 기준 동봉 plugin 7 개 (markdown 별도 카운트):

```
tasty-plugin-claude
tasty-plugin-clipboard-history
tasty-plugin-codex
tasty-plugin-explorer
tasty-plugin-git-viewer
tasty-plugin-html
tasty-plugin-image
```

`crates/` 디렉토리 기준으로는 `tasty-plugin-markdown` 까지 포함하여 8 개. 본 문서는
`plugin-ecosystem.md` 의 카운트와 동기화하여 7 개로 표기한다.

외부 plugin (third-party 작성) 은 **0 개**. 적대적 plugin scenario 가 *현실화되지 않음*.
즉 marketplace 의 *발견* / *신뢰* 두 가치 모두 *지금* 필요한 정도는 낮다.

### 1.2 현 install 경로 — `tasty plugin install <path>`

정의 위치: `src/app/plugin_glue/lifecycle.rs:58` (`plugin_install`).

동작:

1. `Manifest::load(&src_path)` + `validate_bin_extras` 검증
2. `plugin_root().join(&manifest.id)` 가 이미 존재하면 reject
3. `copy_dir_recursive(src_path → dest)` (재귀 fs 복사)
4. `mgr.packages = discover()` 재스캔
5. `mgr.command_registry.register_plugin(&manifest)` + `recompute_extensions()`
6. `i18n::register_namespace(&manifest.id, &lang_dir)`
7. `mgr.config.set_granted(&manifest.id, manifest.permissions.clone())` —
   **매니페스트의 모든 권한을 자동 grant** (사용자 확인 없음)
8. `mgr.config.save()` → `~/.tasty/plugins.toml` atomic write
9. disabled 목록에 없으면 자동 enable — `mgr.enable(&manifest.id)`
10. `CoreEvent::PluginRegistryChanged { Installed { version } }` 발화

CLI 표면 (`docs/agent-guide/plugins.md:417`):

```
tasty plugin install <path>     # 디렉터리 → plugins/ 복사 + 매니페스트 권한 자동 grant
tasty plugin remove <id>        # graceful shutdown + 디렉터리 삭제
tasty plugin enable <id>
tasty plugin disable <id>
```

→ marketplace 가 추가하는 것은 *path → id 매핑* + *fetch* + *권한 grant prompt* 의 3
요소이며, 그 외 흐름 (검증 / 복사 / 등록 / enable) 은 그대로 재사용 가능.

### 1.3 신뢰 모델 현황

`plugin-ecosystem.md §3` 기준:

- **매니페스트 `permissions[]` + 사용자 grant + IPC method_meta 게이트**.
- 추가 sandbox (seccomp / AppContainer) · 서명 · marketplace 검토는 **0.7 까지 미지원**.

권한 모델의 *한계* (`plugin-permissions.md` 「한계」 절과 동일):

- 모든 권한 게이트는 *IPC method 단위* — plugin 이 자기 프로세스에서 `std::fs::*` /
  `std::net::*` / `Command::new` 직접 호출은 OS process privilege.
- 매니페스트의 `permissions` 는 *호스트 API 호출 권한* 이지 *OS 자원 시스템 권한* 이
  아니다.

`plugin-permissions.md` 권한 토큰 일람:

- 단순 토큰 (20+): `surface.read/write`, `fs.read/write`, `terminal.spawn/write/read`,
  `process.spawn`, `network`, `notification`, `ui.popup`, `agent`, `telemetry`, …
- scope 토큰 (4): `ipc.invoke:<prefix>`, `ext:<plugin_id>`,
  `file_handler.extend:<detector_id>`, `file_handler.handle:<detector_id>`

### 1.4 plugin id 형식 / api_version

plugin id 형식 (`crates/tasty-plugin-manifest/src/validators.rs:3` 의
`is_valid_plugin_id`):

- 비어 있지 않음
- 문자 집합 `[A-Za-z0-9._-]` 만 사용 (**대문자 허용, `_` 허용**)
- `.` 한 개 이상 포함 (**segment 수 제한 없음**)

**권장 컨벤션은 reverse-DNS (`com.<org>.<plugin>`)** 이나 강제 규칙은 아님. 예:
`com.tasty.image`, `com.tasty.clipboard`, `com.example.explorer`. validator 자체는 위
3 조건만 강제하며, "소문자만 / 1~3 segment" 같은 표현은 컨벤션이지 코드 강제가 아니다.

api_version / manifest version:

- `HOST_API_VERSION = "1"` (`crates/tasty-plugin-manifest/src/types.rs:11`)
- `MANIFEST_VERSION = 1` (같은 파일)
- 호환성 규칙: `plugin-ecosystem.md §4` (메이저 매치 강제).

### 1.5 marketplace trigger 명문 (기존 doc)

`plugin-ecosystem.md §2` (배포 채널):

- > 0.7까지 로컬 디렉터리 path install + 동봉 builtin만. marketplace는 0.7 이후 RFC.
- 재검토 trigger:
  - 첫 외부 plugin 출시
  - 수동 설치/업데이트 불편 사례 반복 보고
  - 외부 plugin 5+개 자생

`plugin-sandbox-evaluation.md §2.4`:

- > **신규**: marketplace 도입 (비-trusted plugin 일상화) — `plugin-ecosystem.md §2`
  marketplace trigger 와 연동.

본 문서 §8 는 위 두 doc 의 trigger 와 **충돌하지 않는 합집합** 으로 마감한다.

### 1.6 알려진 marketplace 인스턴스 (비교 대상)

| 명칭 | 형식 | 운영 주체 | 비고 |
|------|-----|----------|------|
| crates.io | source crate registry | Rust Foundation | binary plugin 부적합 |
| npm registry | tarball + dependency graph | npm Inc. | JavaScript 생태계 |
| VS Code Marketplace | VSIX bundle | Microsoft | publisher account + 자동 lint |
| Open VSX | VSIX (open alternative) | Eclipse Foundation | VS Code marketplace 대체 |
| Homebrew tap | git URL → formula | 커뮤니티 | 현재 tasty 의 manual clone 모델과 유사 |

§6 에서 5 시스템 X 8 항목 비교표로 확장한다.

## 2. Registry / Index 옵션 평가

### 2.1 후보

- (a) **단일 호스트 JSON index** (예: `plugins.tasty.dev/index.json`) — manifest 목록 +
  tarball URL 을 한 JSON 파일에 모음. CLI 가 단일 URL fetch + parse.
- (b) **Git-based registry (Homebrew tap 형식)** — plugin id 별 manifest 파일을 git
  repo 에 commit. CLI 는 git URL 추론 + clone (또는 raw fetch).
- (c) **분산 (DNS-TXT / IPFS)** — plugin id 의 reverse-DNS 가 실제 DNS 와 매핑되거나
  content-addressed 저장소에서 fetch.
- (d) **federation** (multiple registry, 사용자 추가) — 사용자가 신뢰한 registry 만
  사용. 사내 mirror / fork 생태계 지원.

### 2.2 비교

| 후보 | 운영비 | 외부 plugin 발견 | publisher 추적 | 검열 저항 | 0.7 이후 신규 부담 |
|------|--------|-----------------|---------------|----------|-------------------|
| (a) 단일 JSON index | 中 (인프라 1 인 운영) | 쉬움 (검색·필터 UI 추가) | 호스트 책임 (account DB) | X | 中 |
| (b) Git tap | 0 (git host 무료) | 中 (git URL 알아야) | PR author = publisher | X | 작음 |
| (c) DNS / IPFS | 0 ~ 大 | 어려움 (제 3 자 색인 필요) | X (anonymous) | O | 大 (외부 의존성) |
| (d) federation | 中 | 中 (registry 별 검색) | registry 별 책임 | 부분 | 충돌 해결 정책 추가 |

FFI 부담은 모두 0 (host-side 만 추가). 비용 차이는 *운영* 과 *외부 의존성* 에서 발생.

### 2.3 권고

**(b) Git tap → (a) 단일 JSON index 로 점진 전환** 을 권고한다.

- (b) 는 외부 plugin **5+ 자생** trigger 까지 *비용 0* 으로 충분. git host 가 이미 모든
  publisher 의 source-of-truth.
- (a) 는 외부 plugin **20+ 자생** 시점에 검색 / 필터 UI 가 필요해지면 도입. (b) 의
  manifest 를 색인하여 단일 JSON 으로 정기 생성하는 가벼운 변환만 추가 가능.
- (c) 는 *검열 저항* 이 Tasty 에 필요한 가치가 아니므로 도입 보류 — 외부 의존성 비용이
  가치를 초과.
- (d) 는 *사내 mirror 요구 1 건 보고 시* 재검토. 사내 환경 (외부 registry 접근 차단)
  에서는 즉시 필요해질 수 있다. NHN / 금융권 / 정부 등 사내망 환경의 시나리오.

### 2.4 plugin id ↔ source URL 매핑 (Git tap 의 경우)

(b) 가 1 차 도입 형태라면 매핑 규칙은 reverse-DNS 컨벤션을 *Git tap 의 추론 규칙* 으로
재해석할 수 있다. 예 (제안만, 강제 X):

- `com.<github-user>.<plugin>` → `https://github.com/<github-user>/tasty-plugin-<plugin>`
- `com.<github-org>.<plugin>` → 같은 host 에 `<github-org>` 경유

이 규칙은 1.4 의 validator 가 강제하지 않으므로, Git tap 도입 시점에 *별도 메커니즘*
(예: tap index 의 매핑표) 또는 *컨벤션 강제* 양자택일이 필요하다. 본 평가 시점에서는
*추론 규칙 도입 시점에 결정* 으로 미룬다.

## 3. 설치 flow

### 3.1 목표 flow — `tasty plugin install <id>`

현재 (§1.2) 의 `tasty plugin install <path>` 가 *path → id* 인 반면, marketplace
도입 후의 목표 flow 는 *id 만으로 설치*:

```
tasty plugin install <id>
  ↓
1. index fetch          # registry 에서 <id> manifest 조회
2. tarball / git clone  # registry 가 알려준 source URL
3. dependency 해결       # 1.0 에는 plugin → plugin 의존성 미지원 (skip)
4. 권한 grant prompt    # 사용자에게 manifest.permissions 표시 + confirm
                         # — 현재 auto-grant (§1.2 7 단계) 와 다른 점
5. 기존 plugin_install() # 검증 + copy + register + enable (lifecycle.rs:58 재사용)
```

핵심 변화는 **1·2·4** 의 3 단계 추가. 5 의 기존 흐름은 그대로 재사용.

### 3.2 권한 grant prompt 의 형식

현재 동작 (auto-grant) 을 **install 시점 항상 prompt** 로 전환:

- TUI / GUI / CLI `--yes` 3 분기.
- CLI 단독 실행 (예: e2e, CI) 에서는 `--yes` 명시 시 auto-grant. 미명시 시 reject.
- GUI 동시 실행 중이면 popup (PopupDef 패턴) 로 fallback.

이는 *비-trusted plugin 일상화* (sandbox §2.4 의 4번째 trigger) 에 정합하는 변경이며,
auto-grant 를 *유지하면서 marketplace 만 도입* 하는 시나리오는 **거부** 한다. 둘은 묶음.

### 3.3 의존성 해결

1.0 의 hook (`ext:<plugin_id>`) 모델은 *의존성이 아니라 hook* — host 가 lazy 평가하므로
target plugin 이 부재해도 동작은 가능 (효과만 사라짐).

marketplace 도입 시 *진짜 의존성* (plugin A 의 동작에 plugin B 가 필요) 도입 여부는:

- 권고: **보류**. 1.0 의 hook 모델 유지. marketplace 의 install 단계에서 `ext:<id>` 의
  target 부재 시 warning + manual install 안내만.
- 추가 가치 대비 schema 복잡도 (semver tree 해결, 충돌, 순환) 가 크다.

### 3.4 update / uninstall

- **update**: `tasty plugin update <id>` 신설.
  - 현재는 `tasty plugin upgrade-builtins` 만 존재 (host 와 함께 배포되는 builtin
    재설치). marketplace 출처 plugin 은 *별도 경로* 가 필요.
  - 동작: version 비교 → 새 tarball 받음 → 기존 폴더 atomic swap → snapshot/restore
    체인 (이미 hot reload 메커니즘 일부 존재, `plugin-development.md` 5절).
  - 권한 재-grant 정책: manifest.permissions 가 *추가* 된 경우 prompt 재발. *동일 또는
    감소* 시 silent.
- **uninstall**: 현 `tasty plugin remove <id>` 그대로 재사용. marketplace 별도 변경 없음.

## 4. Trust model

trust 는 4 layer 의 합 — *publisher* (누가 publish 했나) · *signature* (무결성) · *content
review* (publish 전 검사) · *revocation* (사후 차단). 각각 옵션을 평가한다.

### 4.1 publisher verification

| 옵션 | 동작 | 비용 |
|------|-----|------|
| A1: GH OAuth | registry 가 GitHub 계정 보유자에게만 publish 허용 | 中 (OAuth flow + DB) |
| A2: 이메일 verification + manual review (Homebrew 식) | 사람이 PR review | 大 (운영 인력) |
| A3: 무검증 | anyone publish | 0 (트러스트 비용 그대로 사용자에게 전가) |

권고: **A1**. Tasty 가 이미 git 생태계 의존 (release 채널, builtin plugin host).
`publisher = com.{github-user}.<plugin>` 컨벤션과 정합.

### 4.2 signature

| 옵션 | 동작 | 비용 |
|------|-----|------|
| B1: minisign / signify | 단순 ed25519, host 의 public key 1 개 embed | 작음 |
| B2: Sigstore keyless | OIDC token 으로 단기 서명 + Rekor transparency log | 中 (sigstore-rs 의존, 인프라) |
| B3: 서명 없음 | TLS + checksum 만 (registry 가 source-of-truth) | 0 |

권고: **B3 (초기) → B1 (외부 plugin 5+) → B2 (외부 plugin 20+ 또는 보안 이슈 1 건)**.
B2 의 keyless 모델은 key 분실 위험 0 이라는 운영상의 이점이 크지만, sigstore 자체에
의존성이 생긴다 — Tasty 가 single-org 도구 단계에서는 과투자.

### 4.3 content review

| 옵션 | 동작 | 비용 |
|------|-----|------|
| C1: 자동 manifest validation | `validate_bin_extras` (이미 존재) + 추가 schema lint | 작음 |
| C2: 자동 권한 risk score | `fs.write + network + process.spawn → high` 식 매핑 | 작음 |
| C3: 수동 review (VS Code 식) | 사람이 patchnote / binary 검토 | 大 |

권고: **C1 + C2**. C3 는 운영 비용이 marketplace 가치를 초과 — 외부 plugin 20+ 자생
시점에 재검토. C2 는 install 시점 권한 grant prompt 와 같은 표면에 색상화로 표시.

### 4.4 revocation

| 옵션 | 동작 | 비용 |
|------|-----|------|
| D1: opt-in online check | `tasty plugin update --check-revocation` 으로 registry 의 revocation 상태 확인 | 작음 |
| D2: 매번 fetch | 모든 plugin start 시 online check (privacy 침해) | 中 |
| D3: revocation 없음 | 사용자가 직접 발견 + manual remove | 0 |

권고: **D1**. D2 는 *privacy 침해* (실행마다 외부 fetch) + *오프라인 동작 불가* 의 두
비용. D1 의 opt-in CLI 가 정합. host 가 *자동* 으로 fetch 하지 않는다 (포커스 독립성
원칙과 같은 결: 명시적 사용자 / 에이전트 호출 시에만).

### 4.5 비교표 (기존 marketplace 와 대조)

| Layer | crates.io | npm | VS Code Marketplace | **권고 (Tasty)** |
|-------|----------|-----|--------------------|-----------------|
| publisher | crates account | npm account | MS Publisher account | GH OAuth (A1) |
| signature | crates v3 (선택) | sigstore (선택) | code-signed VSIX (선택) | B3 → B1 → B2 점진 |
| content review | 자동 lint | 자동 audit (`npm audit`) | 자동 + 수동 review (paid) | C1 + C2 |
| revocation | yank | unpublish (24h 제한) | unpublish | D1 (opt-in) |

VS Code 의 수동 review 는 *유료 publisher account* + Microsoft 운영 인력이 받쳐주는
모델 — Tasty 의 single-org / 무료 publisher 모델로는 재현 불가. crates.io / npm 의
*자동만* 모델이 현실적 참고점.

## 5. 보안 — 악성 plugin 방어

권한 모델 (§1.3) 의 한계: plugin 이 자기 프로세스에서 직접 fs / network 접근 = OS
권한. 즉 marketplace 도입 = **비-trusted plugin 일상화** → `plugin-sandbox-evaluation.md
§2.4` 의 4번째 trigger 발동.

### 5.1 권한 토큰 risk score

- 동작: manifest.permissions 를 install 시점 색상화 (low / mid / high).
- 매핑 (제안): `fs.write` + `network` + `process.spawn` 동시 보유 → high. `surface.read`
  단독 → low.
- 비용: 토큰 → risk weight 테이블 (LOC 작음).
- 가치: 사용자에게 *시각적 warning*. C2 (§4.3 자동 risk score) 와 동일 메커니즘을
  install 시점에도 노출.

### 5.2 OS-level sandbox 강제 (sandbox-evaluation §3 연동)

- marketplace 출처 plugin 은 `sandbox = "os-strict"` **강제** (auto-grant 와 마찬가지로
  사용자 선택권 없이 묶음).
- 의존: `plugin-sandbox-evaluation.md §3` 의 opt-in sandbox 가 *먼저* 도입되어야 함.
  marketplace 도입 = sandbox §3 도입의 *동반 trigger*.
- 비용: marketplace 출처 추적 (`source = "marketplace" | "local"`) + spawn 시 source
  검사 → sandbox profile 자동 선택. LOC 작음.

### 5.3 WASM sandbox

- 옵션: 모든 marketplace plugin 을 wasm32 target 으로 강제.
- 비용: 외부 plugin 작성자가 wasm32 cross-compile 강제 → 진입 장벽 大. `plugin-sandbox-
  evaluation.md §2.3` 의 FFI 부담 / debug 인프라 / FS preopen 등 비용 그대로.
- 권고: marketplace 1 차 도입 시점에는 *옵션 entry type* 으로만 (`sandbox = "wasm"`
  매니페스트 토글). 강제는 외부 plugin 자생 + 보안 이슈 누적 후로.

### 5.4 OS-level 검역 (별도 trigger)

크로스 플랫폼 원칙 #4 의 관점:

- **macOS**: marketplace 에서 받은 unsigned binary 는 quarantine attribute
  (`com.apple.quarantine`) 로 Gatekeeper 가 차단할 수 있다.
- **Windows**: SmartScreen 이 알 수 없는 publisher 의 .exe 차단.
- **Linux**: 영향 없음.

marketplace 출처 binary 의 OS-level 검역 대응 (code signing infrastructure, Apple
Developer ID / Authenticode 인증서 등) 은 **별도 trigger** — marketplace 의 기본
도입 (§8 1~6 단계) 과 분리. 도입 시점에서 publisher (§4.1 A1) 가 *각자의 code
signing key* 를 제출하는 모델 vs *Tasty 가 통합 서명* 하는 모델 양자택일이 필요.
