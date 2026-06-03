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
