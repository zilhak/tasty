# Markdown surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: host-rendered (egui) — `design-system/` 의 마크다운 surface 디자인(있으면), vendor 예정.

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 안에 열리는 마크다운 렌더 surface. host 가 egui 로 그린다.

## 트리거

마크다운 파일 열기 또는 `markdown` surface 생성/전환.

## UI 요소 인벤토리

- **렌더된 마크다운 본문** — 제목/문단/목록/코드블록/링크 등.
- 탭 표시명은 파일명.

## 상태별 시각

- 파일 없음/로드 실패 등은 surface 내 표시.

## 시각 소스

host-rendered 이므로 픽셀은 host 렌더 + 테마 토큰. design-system 에 마크다운 디자인이 vendor 되면 링크로 교체.
</content>
