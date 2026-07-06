# ADR (Architecture Decision Record) 인덱스

아키텍처/정책 결정의 *근거·대안·재검토 조건* 을 기록한다. design/ 문서가 "지금 어떻게 동작하나" 를 기술한다면, ADR 은 "왜 그렇게 결정했나" 를 기술한다.

- 신규 작성: [`template.md`](template.md) 양식을 따른다. 파일명 `XXXX-<slug>.md`, 번호는 0001 부터 4 자리.
- **Accepted 후에는 Status 만 갱신한다.** 본문 변경이 필요하면 새 ADR 로 Supersede 한다. (예외: References 섹션의 깨진 링크 위생 수정 — [`template.md`](template.md) 의 "작성 규칙" 참조.)
- **외부(비-git) 위치 문서 참조 금지** + 필요한 근거는 `docs/` 로 재구성해 참조 — 상세·예외는 [`template.md`](template.md) 의 "작성 규칙" 참조.
- 커밋 형식: [`dev-guide/commit-convention.md`](../dev-guide/commit-convention.md) 의 "ADR 커밋" 항목.

## 목록

| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0001 | [시스템 트레이 — 전 OS best-effort 지원](0001-system-tray-best-effort.md) | Accepted | 2026-06-17 | system-tray, platform, background, cross-platform, windows, macos, linux |
| 0002 | [VTE 파싱을 입력 스레드 밖 파서 스레드로 분리](0002-vte-parsing-off-input-thread.md) | Accepted | 2026-06-15 | performance, terminal, threading, input-latency, vte |
| 0003 | [네이티브 데코 대신 CSD(Client-Side Decorations) 채택](0003-client-side-decorations.md) | Accepted | 2026-06-15 | window, csd, titlebar, cross-platform, winit, macos, windows, linux |
| 0004 | [IPC transport = 127.0.0.1 loopback TCP (동적 포트)](0004-ipc-transport-tcp.md) | Accepted | 2026-06-16 | ipc, transport, tcp, loopback, security, trust-boundary, cross-platform |
| 0005 | [memory secret 영역은 "안전 보관소" 가 아니다](0005-memory-secret-not-a-vault.md) | Accepted | 2026-06-16 | memory, secret, security, encryption, plugin, trust-boundary |
| 0006 | [문서 분류체계 — 동작 우선(behavior-first), 화면 종속](0006-docs-taxonomy-behavior-first.md) | Accepted | 2026-06-16 | docs, taxonomy, headless, screen-spec, design-system, behavior-first |
| 0007 | [attach 는 원격을 대상으로 한다 (로컬 self-attach 는 debug 격리)](0007-attach-targets-remote.md) | Accepted | 2026-06-17 | attach, remote, debug-isolation, cli, user-agent-separation, security |
| 0008 | [인라인 그래픽 프로토콜(Sixel / Kitty / iTerm)은 보류](0008-inline-graphics-protocols-deferred.md) | Deferred | 2026-06-17 | terminal, graphics, sixel, kitty, image, vte, scope, deferred |
| 0009 | [Plugin sandbox 보류 — OS-level opt-in 우선](0009-plugin-sandbox-deferred.md) | Deferred | 2026-06-17 | plugin, sandbox, security, wasm, seccomp, trust-boundary, deferred |
| 0010 | [Plugin marketplace 보류 — 로컬 path install 유지](0010-plugin-marketplace-deferred.md) | Deferred | 2026-06-17 | plugin, marketplace, registry, trust, distribution, deferred |
| 0011 | [XTWINOPS 창 조작·창 상태 질의는 미지원 (크기/타이틀 스택만 응답)](0011-xtwinops-window-ops-unsupported.md) | Accepted | 2026-06-18 | terminal, xtwinops, vte, window, user-agent-separation, security, scope |
| 0012 | [tmux control mode(DCS) 및 DECRQSS 는 미지원](0012-tmux-dcs-decrqss-unsupported.md) | Deferred | 2026-06-18 | terminal, vte, dcs, tmux, decrqss, scope, deferred |
| 0013 | [레거시·니치 입력 사설 모드는 미지원](0013-niche-input-private-modes-unsupported.md) | Deferred | 2026-06-18 | terminal, vte, dec-private-mode, mouse, input, scope, deferred |
| 0014 | [폰트 ligature 는 보류 (현재 미지원, 추후 지원 계획)](0014-font-ligatures-deferred.md) | Deferred | 2026-06-18 | font, ligatures, appearance, settings, rendering, cell-grid, scope, deferred |
| 0015 | [원격 접속 프로필 = 범용 typed 레지스트리, attach 는 소비자](0015-remote-profiles-typed-registry.md) | Superseded by 0032 | 2026-06-19 | remote, profile, registry, attach, ssh, smb, extensibility, plugin, ubiquitous-language |
| 0016 | [Passkey 저장소 — path 수렴 · 파일권한 위임 · 참조 모델](0016-passkey-store-path-convergence.md) | Accepted | 2026-06-19 | passkey, secret, security, file-permission, trust-boundary, remote-profile |
| 0017 | [Windows 절전(suspend/resume) 후 PTY 헬스 복구는 Windows 전용](0017-windows-suspend-resume-pty-recovery.md) | Accepted | 2026-06-21 | pty, conpty, suspend, resume, power-management, windows, platform, lifecycle, terminal, cross-platform |
| 0018 | [Claude Design 세션 자격증명은 평문으로 저장한다](0018-claude-design-auth-at-rest-plaintext.md) | Accepted | 2026-06-22 | claude-design, plugin, secret, security, encryption, auth, trust-boundary |
| 0019 | [마우스 버튼/드래그 리포팅 — 트래킹 앱에 전면 위임, 로컬 선택 우회는 보류](0019-mouse-button-reporting-app-delegation.md) | Accepted | 2026-06-24 | terminal, vte, mouse, mouse-reporting, sgr, input, selection, scope |
| 0020 | [갤러리는 본체 UI 컴포넌트의 완전한 단일 출처 — cut 금지, gallery-first](0020-gallery-complete-component-source.md) | Accepted | 2026-06-24 | gallery, design-parity, demo-main, component-catalog, workflow, ui |
| 0021 | [자체 업데이트 확인 기능(update-check) 전면 제거](0021-remove-update-check-feature.md) | Accepted | 2026-06-25 | update, auto-update, scope, distribution, removal, maintenance, cli, plugin |
| 0022 | [Shift+우클릭 modifier 우회 + 트래킹 안내 toast](0022-shift-rightclick-context-menu-bypass.md) | Accepted | 2026-06-25 | terminal, mouse, mouse-reporting, context-menu, modifier, discoverability, ux |
| 0023 | [Shift+좌클릭 드래그 = 마우스 리포팅 우회 로컬 텍스트 선택 + 안내 toast 범용화](0023-shift-leftclick-selection-bypass.md) | Accepted | 2026-06-26 | terminal, mouse, mouse-reporting, selection, modifier, clipboard, discoverability, ux |
| 0024 | [Banner — Modal/Popup/Toast 에 이은 4번째 오버레이 개념(별도 매니저)](0024-banner-fourth-overlay-concept.md) | Accepted | 2026-06-26 | ui, overlay, banner, popup, toast, ubiquitous-language, user-agent-separation |
| 0025 | [기획 단계 도구 3분할 (Figma=기획 / Claude design=디자인 / claude code=구현)](0025-planning-tool-split-experimental.md) | Experimental | 2026-06-27 | workflow, figma, claude-design, planning, design-parity, gallery-first, experimental |
| 0026 | [클립보드 히스토리 백엔드 제거 + 뷰어는 plugin 직접-read](0026-clipboard-history-removal-plugin-direct-read.md) | Accepted | 2026-06-28 | clipboard, plugin, removal, scope, sandbox, user-agent-separation, semver, breaking, adr-0009 |
| 0027 | [Figma 기획 파일의 SoT·네이밍 규약과 파생 인덱스 (anti-drift)](0027-figma-planning-sot-naming-derived-index.md) | Accepted | 2026-06-28 | figma, planning, naming-convention, source-of-truth, anti-drift, sigma, spellbook, workflow, adr-0025 |
| 0028 | [Plugin egui mesh 렌더 채널 — plugin tessellate, host 합성 (out-of-process)](0028-plugin-egui-mesh-render-channel.md) | Accepted | 2026-06-29 | plugin, render-channel, egui, epaint, mesh, ipc, shared-memory, surface-kind, bundled-only, adr-0008, adr-0009 |
| 0029 | [워크스페이스 카테고리 — active 는 전역 인덱스 단일 진실 소스 유지](0029-workspace-category-global-index.md) | Accepted | 2026-06-29 | workspace, workspace-category, sidebar, indexing, focus |
| 0030 | [image surface 는 mesh-only(비트맵=egui 텍스처) — ADR-0028 image 하이브리드 조항 개정](0030-image-egui-mesh-bitmap-texture.md) | Accepted | 2026-07-01 | plugin, render-channel, egui, epaint, mesh, image, surface-kind, bitmap, texture, host-rendered-removal, adr-0028 |
| 0031 | [Lua 스크립트의 tasty 접근은 고정 호스트 API 표면으로만 — state 직접 접근 불가 + 워커 스레드 격리](0031-lua-host-api-only-worker-isolated.md) | Proposed | 2026-07-01 | lua, scripting, host-api, worker-thread, snapshot, command-queue, capability-boundary, sandbox, init-lua-removal, observe-only, adr-0009, adr-0028 |
| 0032 | [원격 프로필을 ssh(연결) / tasty-attach(attach) 2-레이어로 분리](0032-remote-attach-two-layer-split.md) | Accepted | 2026-07-01 | remote, profile, attach, ssh, two-layer, ref, port-file, cli |
| 0033 | [UI 색은 semantic role 접근자로만 — primitive 필드 직접 접근 전면 금지(위젯 포함)](0033-ui-color-semantic-role-only.md) | Accepted | 2026-07-03 | design-tokens, color, semantic, primitive, theme, ui-widgets, guard, enforcement, adr-0020 |
| 0034 | [터미널 PTY 셸을 호스트(tasty) 수명에 결박한다](0034-terminal-shell-host-lifetime-binding.md) | Accepted | 2026-07-04 | process-lifetime, reaper, job-object, pty, terminal, windows, conpty, orphan, cross-platform, adr-0009 |
| 0035 | [modifier-hint 오버레이 — 눌린 조합으로 섹션 좁힘 + Shift 단독 표시 지연 1.2초](0035-modifier-hint-combo-narrowing-and-shift-delay.md) | Accepted | 2026-07-05 | modifier-hint, overlay, keybindings, combo, subset, reveal-delay, shift, design-token, accessibility, debug-ipc, adr-0020 |
| 0036 | [플러그인 아이콘은 빌드타임 SVG 베이크 + `tasty-icons` 단일 소스로 그린다](0036-plugin-icon-buildtime-bake-tasty-icons-single-source.md) | Accepted | 2026-07-05 | plugin, icons, tasty-icons, single-source, build-time-bake, svg, vector, egui-mesh, design-parity, i18n, adr-0020, adr-0028, adr-0030 |
| 0037 | [복잡도 게이트 — clippy cognitive(deny) + tokei 파일 SLOC, baseline 은 위치 단위 동결](0037-complexity-gate.md) | Accepted | 2026-07-06 | lint, complexity, ci, quality-gate, clippy, cognitive-complexity, tokei, file-size, maintainability, clippy-policy, ratchet |
| 0038 | [modifier-hint 빈 조합 섹션은 "바인딩 없음" 플레이스홀더로 표시한다](0038-modifier-hint-empty-combo-placeholder.md) | Accepted | 2026-07-06 | modifier-hint, overlay, keybindings, empty-state, placeholder, design-token, i18n, accessibility, debug-ipc, adr-0035, adr-0020 |
| 0039 | [Surface highlight 는 producer 중립 공유 primitive](0039-surface-highlight-shared-primitive.md) | Accepted | 2026-07-07 | surface-highlight, notification, ipc, cli, state, focus-independence |
