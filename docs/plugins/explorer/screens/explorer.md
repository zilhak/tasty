# Explorer surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: 플러그인 UI DSL 렌더 (plugin-rendered) — design-system 의 explorer 디자인이 있으면 그 출처, 없으면 플러그인 측.

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 안에 열리는 파일 트리 surface. surface 배치는 work-area, 여기선 explorer 내용.

## 트리거

`explorer` surface 가 열릴 때(탭/분할 생성, 파일 탐색 진입).

## UI 요소 인벤토리

- **파일 트리** — 디렉토리/파일 목록, 펼침/접힘.
- **현재 경로** 표시 + 상위 이동(go_up)·새로고침(refresh).
- 폰트는 설정 페이지의 override 를 따름.

## 상태별 시각

- 빈 디렉토리 / 권한 없음 / 로딩 등은 플러그인 렌더 상태로 표현.

## 시각 소스

플러그인이 직접 그리므로(plugin-rendered) 픽셀 출처는 플러그인 UI. design-system 에 explorer 컴포넌트가 vendor 되면 그쪽 링크로 교체.
</content>
