# ADR-0057: claude-design 플러그인 전면 제거

- **Status**: Accepted
- **Date**: 2026-07-30
- **Tags**: claude-design, plugin, removal, scope, maintenance, cli, playwright, external-project, adr-0018, adr-0025

## Context

`claude-design` 플러그인(`com.tasty.claude-design`, 당시 `tasty-plugin-claude-design` 크레이트)은 `claude.ai/design` 캔버스를 off-screen headful Playwright 브라우저로 자동화하는 번들 plugin이었다. 구성:

- **CLI**: `tasty design *` 서브커맨드 11종 — `login`/`logout`/`import-session`/`status`/`projects`/`detect`/`probe`/`chat`/`chat-status`/`turn-status`/`protocol`. 각각 plugin 자신의 IPC 네임스페이스(`design.login`~`design.protocol`)에 매핑됐다.
- **런타임**: Node.js + system Playwright + Chromium 감지·기동 로직(`detect.rs` 452줄, `runner.js` 457줄), Chrome/Firefox 로컬 세션 import(`chrome_import.rs` 299줄, `firefox_import.rs` 307줄), 세션 저장(`auth.rs`, ADR-0018 평문 저장 결정의 대상).
- **워크플로 통합**: [`design-change-workflow.md`](../dev-guide/design-change-workflow.md)의 "A 경로"가 `tasty design chat`을 이용한 자동 지시 + `design-tasks/` 파일 기반 동시성 lock 프로토콜(`tasty design protocol`)을 전제로 했다.
- **패키징**: builtin 등록(`crates/tasty-host-plugin/src/builtin.rs::BUILTINS`, 9종 중 1), deb/rpm asset 목록, `wix/main.wxs`의 Windows MSI Directory/Component 6개.

이 플러그인은 tasty 본체 안에서 제대로 동작할 수 없는 구조라는 판단 아래(headful 브라우저 자동화·세션 관리·Cloudflare 우회 등 본체의 관심사와 이질적인 운영 부담) 별도 프로젝트로 분리하기로 결정했다(사용자 직접 지시).

## Decision

**claude-design 플러그인을 tasty 본체에서 전면 제거한다.** 크레이트 전체, builtin 등록, 패키징(deb/rpm assets, `profile.dev.package` 오버라이드, `wix/main.wxs` Directory/Component/ComponentRef), `tasty design *` CLI 서브커맨드 11종과 그 IPC(`design.*`)를 모두 삭제한다. 워크플로 문서의 "자동 지시"·"동시성 lock 프로토콜" 서술은 실행 불가능한 절차가 됐으므로 제거하고, A경로의 "지시" 단계는 항상 "사용자가 Claude design을 열어 직접 지시"로 고정한다.

**워크플로 개념 자체(Figma=기획 / Claude design=디자인 / claude code=구현 3분할, [ADR-0025](0025-planning-tool-split-experimental.md))는 유지한다** — 무효화된 것은 그 중 "자동화"라는 실행 수단뿐이지, 3분할 워크플로라는 결정 자체가 아니다. 요청문서 §0~§8 구성, 상태 라이프사이클, DesignSync 기반 직접 접근(read/write) 경로도 그대로 유지된다.

## Consequences

- **얻은 것**: 번들 plugin 1개 제거(9→8), `tasty design *` CLI 11종 제거로 CLI 표면 축소, tasty 본체가 system Playwright/Node/Chromium 런타임 유무를 더 이상 신경 쓸 필요 없음(감지·기동 로직 자체가 사라짐), Windows MSI/deb/rpm 패키징에서 headful 브라우저 자동화 관련 산출물이 사라짐.
- **잃은 것 (BREAK)**: `tasty design *` CLI 서브커맨드 전체와 그 IPC(`design.*`)가 사라진다 — 대체/alias 없음. `auto_wait` 체이닝의 유일한 실사용 소비자였던 `design.chat`이 사라져, 현재 번들 8종 plugin 중 이 메커니즘의 실사용 소비자가 없어졌다(스키마 자체는 외부/서드파티 plugin을 위해 유지, [`api-conventions.md`](../dev-guide/api-conventions.md) "auto_wait chain" 절 참고). claude.ai/design 캔버스 자동화(로그인 세션 관리, Chrome/Firefox 세션 import, 동시성 lock 기반 chat 자동 전송)라는 기능 자체가 tasty에서 완전히 빠진다 — 사용자는 이제 Claude design을 직접 열어 요청문서를 제출해야 한다.
- **운영 비용 / 유지 부담**: 0.x SemVer 정책상([`api-conventions.md`](../dev-guide/api-conventions.md) "안정성 정책") 직접 제거이며 deprecation 유예 없이 즉시 제거했다 — 사용자 직접 지시에 의한 스코프 조정이라 CHANGELOG `(BREAK)` 표기로 충분하다고 판단. [ADR-0018](0018-claude-design-auth-at-rest-plaintext.md)(세션 자격증명 평문 저장 결정)은 그 결정 대상 자체가 사라졌으므로 본 ADR로 Supersede한다.

## Alternatives Considered

- **플러그인 유지·개선**: headful Playwright 자동화·Cloudflare 우회·세션 관리는 tasty 본체의 관심사(터미널 에뮬레이션·GPU 렌더링·plugin 호스팅)와 이질적이라 유지 비용이 계속 본체에 얹힌다. 별도 프로젝트로 분리하면 그 프로젝트의 릴리스 주기·의존성 관리를 tasty 릴리스와 독립시킬 수 있어 기각.
- **별도 프로젝트로 분리하되 tasty 저장소 내 stub(빈 CLI 안내 메시지 등) 유지**: `tasty design *` 호출 시 "별도 프로젝트를 쓰라"는 안내만 남기는 안. 실제 기능이 없는 죽은 CLI 표면을 유지하는 것은 사용자에게 혼란만 더하고(무엇이 동작하고 무엇이 안내인지 구분 필요), 매니페스트·builtin 등록·패키징 코드는 그대로 남아 유지 비용 절감 효과가 없어 기각.
- **deprecation 기간(한 minor 이상 경고 후 제거)을 둔다**: 0.x 정책의 원칙이지만, 이번 제거는 버그/보안 수정이 아니라 사용자가 직접 지시한 아키텍처 스코프 조정이다. 플러그인이 이미 별도 프로젝트로 이전될 예정이라 tasty 안에 경고만 내보내는 반쪽짜리 버전을 유지하는 이득이 낮아 기각 — 즉시 제거하고 CHANGELOG `(BREAK)` 로 명시하는 쪽을 택했다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR을 재검토한다.

- claude.ai/design 자동화가 별도 프로젝트에서 안정화되어, tasty 본체로 재통합할 명분(예: tasty의 plugin 배포 채널이 성숙해 외부 프로젝트를 번들에 다시 편입하는 비용이 낮아짐)이 생길 때.
- 워크플로의 "자동 지시" 단계(A경로)에 대한 실사용 수요가 반복 제기되고, 그 자동화를 tasty 본체가 아닌 순수 외부 plugin(로컬 path install)으로 재도입할 수 있을 때.

## References

- 제거 커밋: `dc1fbfff`(크레이트/builtin/패키징 전면 제거) · `13ec01a9`(번들 plugin 개수/목록 참조 문서 동기화, 9→8) · `1189d719`(워크플로 문서 자동 지시 서술 제거).
- [ADR-0018: Claude Design 세션 자격증명은 평문으로 저장한다](0018-claude-design-auth-at-rest-plaintext.md) — 본 ADR로 Superseded(대상 자체가 제거됨).
- [ADR-0025: 기획 단계 도구 3분할](0025-planning-tool-split-experimental.md) — 워크플로 개념의 근거, 본 제거로 영향받지 않음(자동화 실행 수단만 무효화).
- [`dev-guide/design-change-workflow.md`](../dev-guide/design-change-workflow.md) — 현재 운영 절차(자동 지시 서술 제거 반영됨).
- [`dev-guide/api-conventions.md`](../dev-guide/api-conventions.md) "auto_wait chain" · "안정성 정책" 절.
- [`dev-guide/plugin-packaging.md`](../dev-guide/plugin-packaging.md) — 번들 plugin SoT(8종).
