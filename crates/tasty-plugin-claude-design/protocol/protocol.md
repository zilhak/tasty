# Claude Design 동시성 lock 프로토콜 (v1)

claude.ai/design 은 **한 프로젝트에 동시에 한 turn 만** 허용한다(초과 시 "Your other
tab is working on a request"). busy 여부를 값싸게 물을 API 가 없으므로, **공유 저장소
(디자인 프로젝트 파일)를 상태 채널** 삼아 advisory lock + 작업 히스토리를 운용한다.
디자인 프로젝트를 구동하는 AI 에이전트는 이 규칙을 세우고 따라야 한다.

설치 절차는 `tasty design protocol --bootstrap`.

## 폴더 / 파일명

```
design-tasks/<YYYYMMDD-hhmm>-<slug>.<STATE>.md
```
- `STATE` ∈ `WORKING` | `DONE` | `FAILED` | `NEEDS-INPUT`.
- 선두 시각 = 시간순 정렬 + stale 판정 기준.
- 상태를 **파일명에 인코딩** → `list_files` 한 번(내용 read 불필요, 값쌈)으로 busy 판정.
- 요청 1건 = 파일 1개. 토글 lock 하나를 돌려쓰지 않고 쌓아서 히스토리로 남긴다.

## 파일 내용 (최소 스키마)

```
---
request: <원 요청 파일 경로, 예 uploads/<slug>.md>
started: <ISO8601>
state: WORKING|DONE|FAILED|NEEDS-INPUT
finished: <ISO8601 | 비움>
---
## 열린 결정 답 (있으면)
## 만든 것 (DONE 시) — 수정/추가한 파일·컴포넌트 + 한 줄 요약   ← 구동측이 회수하는 reply
## 실패 사유 (FAILED) / 필요한 결정 (NEEDS-INPUT)
```

## designer (claude.ai/design) 규율 — 매 turn

1. **첫 액션**: `design-tasks/<ts>-<slug>.WORKING.md` 를 만든다(디자인 작업보다 **먼저**).
2. 디자인 작업(파일/컴포넌트 수정) 수행.
3. **마지막 액션**: 결과 요약을 그 파일에 써넣고 **`.DONE.md` 로 rename**(실패 `.FAILED`,
   결정 필요 `.NEEDS-INPUT`). rename 은 **반드시 turn 의 맨 끝** — 중간에 바꾸면 아직 안
   끝났는데 lock 이 풀린다.

## claude code (구동측) 규율

1. **발사 전**: `list_files(design-tasks/)` → 비-stale `.WORKING` 존재하면 발사 금지(대기).
2. **발사**: `tasty design chat` 으로 요청(요청 원문은 `uploads/` 등에 선업로드).
3. **완료 폴링**: 대응 `<ts>-<slug>` 파일이 `.DONE`(/`.FAILED`/`.NEEDS-INPUT`)로 바뀔
   때까지 `list_files` 폴링. 브라우저 turn-end 신호에 **의존하지 않는다**.
4. **회수**: `.DONE` 파일을 read → `## 만든 것` 을 reply 로 사용.

## TTL — "즉시 폐기"가 아니라 "재확인 체크포인트"

`WORKING_TTL = 10분`. `.WORKING` 나이 > TTL 인 lock 을 만나면:
1. **turn liveness 재확인**(`tasty design turn-status`) — 아직 응답 생성/작업 중인가?
2. **살아있으면 → +5분 연장** 하고 계속 대기(다음 만료 때 재확인, 반복).
3. **죽었으면**(turn 종료·크래시인데 `.DONE` rename 안 됨) → stale: `.WORKING`→`.FAILED`
   강등 후 발사.

> TTL 은 결과를 기다리는 "대기시간"이 아니라 *lock 이 죽었는지 재확인하는 주기*다.
> 10분으로 짧게 둬도 ②의 연장 로직이 정상적으로 오래 걸리는 turn 을 지켜준다.

## backstop

designer 가 파일을 아예 안 만든 경우(잊음/에러) 대비 — 구동측 runner 는
`"other tab is working"` 배너를 **감지해 `busy` 로 즉시 보고**(timeout 까지 헛대기 X).
= 파일 lock(1차) + 배너 감지(2차 안전망) 이중화.
