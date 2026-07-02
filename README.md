# Tasty

> **Tasty** is a cross-platform, GPU-accelerated terminal emulator purpose-built for AI coding agents. It provides multi-agent orchestration, headless operation, and a focus-independent IPC/CLI surface across Windows, macOS, and Linux. (Detailed docs below are in Korean — start at [`docs/index.md`](docs/index.md).)

크로스 플랫폼 GPU 가속 터미널 에뮬레이터. AI 코딩 에이전트에 특화된 멀티에이전트 오케스트레이션 + 헤드리스 운용 + focus-independent IPC/CLI 동작 표면을 제공한다.

[![Version](https://img.shields.io/badge/version-0.7.0-blue)](CHANGELOG.md)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](#라이선스)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)](docs/installation.md)
[![Workspace](https://img.shields.io/badge/workspace-34%20crates-orange)](crates/)

## 핵심 가치

- **크로스 플랫폼** — Windows / macOS / Linux 모두 네이티브 (winit + wgpu).
- **GPU 가속 렌더** — 셀 기반 셰이더, 10+ surface 환경에서도 prepare/draw 안정.
- **Hexagonal 아키텍처** — model + ports + adapters + view + host_api 분리, 34-crate workspace.
- **AI 에이전트 first-class** — IPC 와 CLI 의 모든 동작 표면이 focus-independent ID 기반. 사용자 행동과 에이전트 행동이 완전히 분리됨 (debug 격리).

## 설치

자세한 절차: [`docs/installation.md`](docs/installation.md).

```bash
# 소스 빌드 (모든 플랫폼 공통)
git clone https://github.com/zilhak/tasty.git
cd tasty
cargo build --release
./target/release/tasty

# 배포 산출물 (DMG / MSI / AppImage) — GitHub Releases 참조
```

## 핵심 기능

- **멀티에이전트 오케스트레이션** — `agent.task_*` DAG + `barrier` / `semaphore` / `lease` / `task_reduce` / `rate_limit` primitive
- **헤드리스 운용** — GUI 없이 CLI/IPC 만으로 surface 생성·종료·I/O (`--headless`)
- **vi copy mode** — 키바인딩 기반 영역 선택/복사, GPU 커서 시각화
- **dist 자동 빌드** — `cargo build --profile dist` + Justfile 래퍼 (DMG / MSI / AppImage)
- **Plugin SDK** — `tasty-plugin-sdk` 크레이트 + 매니페스트 스키마 + 권한 시스템 (`api_version = 1`)
- **공유 컨텍스트** — Blackboard / Plan / Cache (`memory.*` 위 래퍼)
- **OSC 133 command indexing** — shell prompt 단위 출력 캡처
- **출력 옵저버** — PTY 라인 → 빌트인 파서 → memory / file sink 팬아웃
- **텔레메트리 + cost cap** — `telemetry.*` 측정/집계 + 토큰 cap 시 자동 차단
- **테마 시스템** — 4px 그리드, 14px 폰트 상한, 사용자 정의 가능
- **wait blocking IPC** — 자식 Claude 의 idle 상태까지 대기

## 아키텍처

```
src/
├─ model/        — 도메인 모델 (surface / pane / workspace 트리, 상태 머신)
├─ ports/        — 의존성 역전 인터페이스 (PtySpawner, ClipboardProvider 등)
├─ adapters/     — 외부 시스템 어댑터 (ipc, clipboard, plugin, production/test)
├─ view/         — egui UI + 커스텀 GPU 셰이더
├─ host_api/     — IPC handler + CLI command 라우터
└─ engine/       — surface registry + layout persistence

crates/          — 34 개 도메인 크레이트 (tasty-agent / tasty-ipc / tasty-memory / ...)
```

자세한 구조: [`docs/architecture/`](docs/architecture/).

## 정체성 — 사용자 행동과 에이전트 행동의 분리

Tasty 의 모든 API 는 두 표면을 엄격히 분리한다.

- **사용자 행동** = 키보드 단축키 / 마우스 / OS 네이티브 입력. 사용자 자신이 만든다.
- **에이전트 행동** = IPC 메서드 / CLI 서브커맨드. AI 에이전트가 자기 작업을 수행하기 위해 호출한다.

에이전트 행동의 부수효과는 사용자의 포커스·히스토리·선택 상태에 닿지 않는다. 사용자 입력을 *재현* 하는 기능 (키 주입 / 포커스 강제 전환 등) 은 release 빌드의 IPC/CLI 표면에 존재하지 않는다 (debug 격리). 자세한 원칙: [`CLAUDE.md`](CLAUDE.md).

## 문서

- 인덱스: [`docs/index.md`](docs/index.md)
- 사용자 가이드: [`docs/installation.md`](docs/installation.md), [`docs/features.md`](docs/features.md)
- 에이전트 가이드: [`docs/agent-guide/`](docs/agent-guide/) (api-reference / blackboard / plan / cache / telemetry / agent / output-parsers)
- 개발 가이드: [`docs/dev-guide/`](docs/dev-guide/)
- 안정성 정책: [`docs/dev-guide/api-conventions.md`](docs/dev-guide/api-conventions.md) 의 "안정성 정책" 절

## 라이선스

MIT — [`LICENSES/`](LICENSES/). Third-party 의존성 라이선스 모음: [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
