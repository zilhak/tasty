# ADR-0025: 기획 단계 도구 3분할 (Figma=기획 / Claude design=디자인 / claude code=구현) — 실험적

- **Status**: Experimental
- **Date**: 2026-06-27
- **Tags**: workflow, figma, claude-design, planning, design-parity, gallery-first, experimental

## Context

tasty 의 화면·컴포넌트를 만드는 단계에서 어떤 도구가 무엇을 담당하는지가 불명확했다. 초기에는 "Figma 에서 디자인하고 코드로 옮긴다" 정도의 막연한 그림이었고, Cover 페이지의 SoT 다이어그램에도 `Source code / Claude design / Figma` 3 노드를 나란히 두기만 했다.

실제로 작업해 보니 세 도구의 강점이 갈린다.

- **Figma**: 와이어프레임·IA·플로우·주석 등 *구조적 기획*에 강하다. 반면 AI 에이전트(claude)가 Figma 안에서 픽셀 단위 시각 판단을 하기는 약하다 — 좌표를 계산해 노드를 박을 수는 있어도, "이게 보기 좋은가"를 Figma 캔버스 위에서 판단하기 어렵다.
- **Claude design (claude.ai Artifacts)**: 실제로 렌더되는 HTML/CSS/JS 시안을 즉시 만든다. 색·간격·인터랙션이 살아있는 *고충실도 비주얼*을 빠르게 탐색할 수 있다.
- **claude code (로컬)**: Rust/egui 구현과 토큰 정합, gallery-first 카탈로그를 담당한다.

핵심 미지수는 **"Figma 단계가 실제로 도움이 되는가"** 다. 기획을 Figma 에서 하는 것이 Claude design 시안 + 코드 구현 사이에서 정말 가치를 더하는지, 아니면 한 번 그려두고 아무도 다시 안 보는 산출물이 되는지 아직 확신이 없다.

## Decision

화면·컴포넌트 제작을 **3 단계로 분할하고, 단방향으로 흐르게 한다. 단 이 분할 전체를 "실험적(Experimental)" 상태로 둔다** — 특히 Figma 기획 단계의 실효성이 검증되기 전까지는 확정 정책이 아니다.

| 단계 | 도구 | 담당 | 충실도 |
|------|------|------|--------|
| 기획 | **Figma** | 와이어프레임, IA/플로우, 주석·설명, 토큰·컴포넌트 카탈로그 미러 | 저~중 (구조) |
| 디자인 | **Claude design** (claude.ai Artifacts, HTML/CSS) | 실제 비주얼 시안 — 색·간격·인터랙션 살아있는 고충실도 | 고 (픽셀) |
| 구현 | **claude code** (로컬) | egui 구현 + 토큰 정합 + gallery specimen | 실제 코드 |

흐름:

```
Figma(기획) ──▶ Claude design(시안) ──▶ claude code(구현) ──▶ gallery specimen
                                                      │
   토큰 / 기존 컴포넌트  ◀──  코드가 SoT  ────────────┘   (code → Figma 미러)
```

이는 기존 gallery-first 원칙([ADR-0020](0020-gallery-complete-component-source.md))과 충돌하지 않는다. "Figma 에서 컴포넌트를 디자인한다"를 "Figma 에서 **기획**하고 Claude design 에서 **디자인**한다"로 한 칸 쪼갠 것이다. 신규 컴포넌트의 SoT 는 여전히 gallery → code 이고, 이 분할은 그 *앞단(기획·시안)*을 어떤 도구로 채울지를 정한 것뿐이다.

## Consequences

- **얻은 것**: 각 도구가 강점만 맡는다. Figma 로 구조를 빠르게 잡고, Claude design 으로 실제 보이는 시안을 만들고, code 로 정합 구현한다. AI 에이전트가 Figma 안에서 시각 판단을 강요받지 않는다.
- **잃은 것 / 위험**:
  - **Figma 단계가 죽은 산출물이 될 위험** — 이 ADR 이 실험적인 핵심 이유. 기획을 Figma 에 그려도 실제로는 Claude design 시안에서 바로 구현으로 가버리면 Figma 는 한 번 쓰고 버려진다.
  - **HTML→egui 전사 갭** — Claude design 은 flexbox, 구현은 egui. [`design-parity-notes.md`](../design/systems/design-parity-notes.md) 의 구조 전사 원칙이 그대로 적용된다 (다만 둘 다 코드라 Figma 시안보다 오히려 가까운 경우가 많다).
- **운영 비용 / 유지 부담**:
  - **토큰 SoT 는 끝까지 코드(`Theme`)** — Claude design 시안도 색·치수는 Catppuccin Mocha 토큰 안에서만 골라야 한다. 시안 작성 시 토큰 팔레트를 입력으로 먼저 준다.
  - **Claude design 산출물은 휘발성** — Artifact 는 세션 종료 시 사라진다. 확정 시안은 HTML 을 `docs/design/` 또는 `.claude-workspace/` 에 저장하거나, 스크린샷을 Figma Screens 페이지에 "확정 시안 아카이브"로 박아 보존한다.

## Alternatives Considered

- **Figma 에서 고충실도까지 (2단: Figma → code)**: AI 에이전트가 Figma 캔버스에서 픽셀 판단을 못 해 시안 품질이 안 나온다. 좌표 계산으로 박는 한계.
- **Claude design 만 + Figma 제거 (2단: Claude design → code)**: 와이어프레임·IA·플로우 같은 구조적 기획을 담을 곳이 없어진다. 다만 *Figma 가 실제로 안 쓰이면 이 안으로 수렴하는 게 맞다* — 그래서 본 ADR 이 실험적이다.
- **전부 코드 우선 (기획·디자인 없이 code 에서 바로)**: 탐색 비용이 비싸고(매번 빌드), 대안 비교가 어렵다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- **Figma 기획 단계가 실제로 거의 참조되지 않는 것이 확인되면** → Figma 단계를 제거하고 `Claude design → code` 2 단으로 축소(위 Alternative 2 안으로 supersede).
- 반대로 Figma 기획이 분명히 가치를 보이면 → Status 를 `Accepted` 로 승격하고 휘발성·토큰 정합 규칙을 정식 dev-guide 로 문서화.
- 토큰 정합(Theme ↔ 시안) 이 반복적으로 깨지면 → 시안 작성 입력 규약을 강화하거나 자동 토큰 주입 도구를 검토.
- HTML→egui 전사 갭이 실제 구현 비용으로 누적되면 → Claude design 시안 출력 형식(egui 친화 구조)을 재정의.

## References

- [ADR-0020: 갤러리는 본체 UI 컴포넌트의 완전한 단일 출처 — gallery-first](0020-gallery-complete-component-source.md)
- [ADR-0018: Claude Design 세션 자격증명은 평문으로 저장한다](0018-claude-design-auth-at-rest-plaintext.md)
- [`docs/design/systems/design-parity-notes.md`](../design/systems/design-parity-notes.md) — 구조 전사 원칙
- [`docs/dev-guide/gallery-first.md`](../dev-guide/gallery-first.md)
- Figma 기획 파일: https://www.figma.com/design/ct3uPefwY2uk6i1i9wYpkU/Untitled (7 페이지: Cover / Flows & IA / Wireframes / Foundations / Components / Screens / Archive)
