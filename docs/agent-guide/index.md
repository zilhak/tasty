# Tasty 사용 가이드 — AI 에이전트용

Tasty를 **사용하는** AI 에이전트를 위한 가이드. IPC/CLI로 Tasty를 조작하는 방법을 다룬다.

> 이 프로젝트를 **개발하는** AI 에이전트를 위한 가이드는 `docs/dev-guide/`를 참조.

## 환경별 가이드

| 환경 | 문서 |
|------|------|
| Linux | [linux.md](linux.md) |
| Windows | (예정) |
| macOS | (예정) |

## API 레퍼런스

IPC/CLI 전체 메서드는 [api-reference.md](api-reference.md) 참조.

## 주제별 가이드

| 주제 | 문서 |
|------|------|
| 클립보드 히스토리 | [clipboard.md](clipboard.md) |
| 공유 컨텍스트 — Blackboard (`memory.bb_*`) | [blackboard.md](blackboard.md) |
| 공유 컨텍스트 — Plan (`memory.plan_*`) | [plan.md](plan.md) |
| 공유 컨텍스트 — Cache (`memory.cache_*`) | [cache.md](cache.md) |
| Plugin 시스템 | [plugins.md](plugins.md) |
| Event 카탈로그 (Event Bus 1.0 wire) | [event-catalog.md](event-catalog.md) |
| 터미널 출력 구조화 (parse_since_mark / commands / observer) | [output.md](output.md) |
| 출력 파서 카탈로그 (`tasty-output`) | [output-parsers.md](output-parsers.md) |
| 휴먼 핸드오프 (approval / diff surface) | [approval.md](approval.md) |
| 텔레메트리 (관측 / 비용 / 이상 탐지 / 세션 요약) | [telemetry.md](telemetry.md) |
| 다중 에이전트 협업 (task DAG / barrier / semaphore / lease / reducer / rate-limit) | [agent.md](agent.md) |
| 권한 / capability_elevation / audit log | [capabilities.md](capabilities.md) |
| 레이아웃 프리셋 (`preset.*`) | [../features.md#레이아웃-프리셋-layout-presets](../features.md) |
