# 평가 / POC 인덱스

본 디렉토리의 문서는 *현재 결정의 재료* 다. ADR (`docs/adr/`) 이 결정의 *결과* 라면, 본 문서들은 그 *근거 / 대안 비교* 의 원본을 보존한다. 시점성 (snapshot) 문서이므로 본문은 작성 시점 기준으로 읽는다 — 재검토 trigger 충족 여부 점검 시 일괄 조회 용도.

| 문서 | 작성 시점 | 결론 | 재검토 trigger |
|------|----------|------|---------------|
| [plugin-marketplace.md](plugin-marketplace.md) | 0.7.x | 보류 | 외부 plugin 생태계 성장 등 — 해당 문서 §8 |
| [plugin-sandbox.md](plugin-sandbox.md) | 0.7.x | 보류 (0.7 현 상태 유지) | 해당 문서 §2.4 |
| [wasm-poc.md](wasm-poc.md) | Phase J.C | 정식 도입 권고 | — (sandbox §2.4 trigger 의 실측 입력) |
| [refactoring-status.md](refactoring-status.md) | 해당 문서 헤더 | 진행 중 (잔여 개선 로드맵) | — |
| [library-separation/](library-separation/) (6 관점) | 2025 | (역사) 분리 계획 시점 분석 보존본 | 신규 crate 분리 검토 시 framework 재사용 |

- library-separation 의 *현재 구조* (현황 매트릭스 / execution-plan / workspace-design) 는 [`../architecture/library-separation/index.md`](../architecture/library-separation/index.md) 에 있다.
