# Tasty

<img src="assets/icons/tasty-melon.svg" alt="Tasty logo" width="96" height="96" />

> **Tasty** is a cross-platform, GPU-accelerated terminal emulator purpose-built for AI coding agents. It provides multi-agent orchestration, headless operation, and a focus-independent IPC/CLI surface across Windows, macOS, and Linux. (Detailed docs below are in Korean — start at [`docs/index.md`](docs/index.md).)

크로스 플랫폼 GPU 가속 터미널 에뮬레이터. AI 코딩 에이전트에 특화된 멀티에이전트 오케스트레이션 + 헤드리스 운용 + focus-independent IPC/CLI 동작 표면을 제공한다.

[![Version](https://img.shields.io/badge/version-0.9.10-blue)](CHANGELOG.md)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](#라이선스)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)](docs/installation.md)
[![Workspace](https://img.shields.io/badge/workspace-43%20crates-orange)](crates/)

WezTerm / Alacritty 같은 GPU 가속 터미널이 사람의 타이핑 경험 자체에 집중한다면, Tasty 는 그 위에 "AI 에이전트가 직접 조작할 수 있는 터미널"이라는 좌표를 더한다 — 모든 동작 표면이 사람의 키보드/마우스뿐 아니라 IPC/CLI 로도 동일하게 열려 있다.

## 정체성 — 사용자 행동과 에이전트 행동의 분리

Tasty 의 모든 API 는 **사용자 행동**(키보드/마우스/OS 네이티브 입력)과 **에이전트 행동**(IPC 메서드/CLI 서브커맨드)을 엄격히 분리한다. 에이전트 행동의 부수효과는 사용자의 포커스·히스토리·선택 상태에 닿지 않는다 — 사용자 입력을 *재현*하는 기능(키 주입/포커스 강제 전환 등)은 release 빌드의 IPC/CLI 표면에 존재하지 않는다(debug 격리). 자세한 원칙: [`CLAUDE.md`](CLAUDE.md).

## 핵심 가치

- **크로스 플랫폼** — Windows / macOS / Linux 모두 네이티브 (winit + wgpu).
- **GPU 가속 렌더** — 셀 기반 셰이더, 10+ surface 환경에서도 prepare/draw 안정.
- **Hexagonal 아키텍처** — model + ports + adapters + view + host_api 분리, 43-crate workspace.
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

- **여러 AI 에이전트를 하나의 터미널에서 오케스트레이션한다** — task DAG + barrier / semaphore / lease / reduce / rate-limit 협업 primitive 로 병렬 작업을 조율([`docs/features/agent-collaboration/index.md`](docs/features/agent-collaboration/index.md))
- **GUI 없이도 완전히 동작한다** — CLI/IPC 만으로 surface 를 만들고 끄고 입출력까지 다룰 수 있어 CI/서버 환경에 그대로 올라간다(`--headless`, [`docs/features/headless-pty/index.md`](docs/features/headless-pty/index.md))
- **키보드만으로 화면을 선택·복사한다** — vi 스타일 카피 모드(hjkl 이동·visual 선택·검색)와 GPU 커서 시각화([`docs/features/clipboard/index.md`](docs/features/clipboard/index.md))
- **배포용 설치 파일을 한 번에 뽑는다** — `cargo build --profile dist` + Justfile 로 DMG / MSI / AppImage 를 자동 빌드
- **플러그인으로 기능을 직접 확장한다** — 매니페스트 스키마 + 권한 시스템을 갖춘 SDK 제공([`docs/features/plugin-system/index.md`](docs/features/plugin-system/index.md))
- **에이전트끼리 정보를 공유한다** — Blackboard / Plan / Cache 로 여러 에이전트가 같은 작업 컨텍스트를 주고받음([`docs/features/agent-collaboration/index.md`](docs/features/agent-collaboration/index.md))
- **셸 명령 단위로 출력을 정확히 짚어낸다** — shell prompt 경계를 인식해 "이 명령의 출력"만 골라 캡처([`docs/features/terminal-output/index.md`](docs/features/terminal-output/index.md))
- **터미널 출력을 실시간으로 감시해 후속 작업을 건다** — PTY 출력 라인을 파싱해 memory/file sink 로 자동 팬아웃([`docs/features/terminal-output/index.md`](docs/features/terminal-output/index.md))
- **에이전트 토큰 사용량을 재고 한도에서 자동으로 막는다** — 측정/집계 + cost cap 초과 시 자동 차단([`docs/features/telemetry/index.md`](docs/features/telemetry/index.md))
- **테마를 내 취향대로 바꾼다** — 4px 그리드/14px 폰트 상한 기반 사용자 정의 테마 시스템([`docs/features/themes/index.md`](docs/features/themes/index.md))
- **여러 자식 Claude 를 동시에 굴리고 끝나는 대로 알림을 받는다** — spawn/tell 은 즉시 반환하고, idle/추가입력필요/종료 시점마다 호출자에게 완료 알림이 자동으로 온다([`docs/plugins/claude/index.md`](docs/plugins/claude/index.md))

## 문서

- 인덱스: [`docs/index.md`](docs/index.md)
- 사용자 가이드: [`docs/installation.md`](docs/installation.md), [`docs/features/`](docs/features/index.md)
- 에이전트 가이드: [`docs/reference/`](docs/reference/index.md) (api / event-catalog / output-parsers / environments / plan.schema.json)
- 개발 가이드: [`docs/dev-guide/`](docs/dev-guide/)
- 안정성 정책: [`docs/dev-guide/api-conventions.md`](docs/dev-guide/api-conventions.md) 의 "안정성 정책" 절

## 아키텍처

Hexagonal 아키텍처(model + ports + adapters + view + host_api 분리)의 43-crate workspace. 자세한 구조: [`docs/architecture/`](docs/architecture/).

## 라이선스

MIT — [`LICENSES/`](LICENSES/). Third-party 의존성 라이선스 모음: [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
