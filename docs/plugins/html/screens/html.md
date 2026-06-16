# HTML surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: 네이티브 WebView 오버레이 — 콘텐츠는 OS WebView 가 그린다(디자인 토큰 무관).

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 위치에 WebView 오버레이로 그려지는 HTML surface.

## 트리거

HTML 파일 열기 또는 `html` surface 생성(`--url`).

## UI 요소 인벤토리

- **WebView 콘텐츠** — URL/파일의 웹 렌더. 타일 rect 에 오버레이로 정렬.
- 탭 표시명은 파일명/URL.

## 상태별 시각

- 로딩/로드 실패는 WebView 자체 표현. surface 가 트리에선 `RemoteSurface` marker.

## 시각 소스

콘텐츠는 OS 네이티브 WebView 가 렌더하므로 design-system 토큰이 적용되지 않는다(웹 페이지 자체 스타일). 타일 정렬/경계만 작업영역 레이아웃을 따른다.
</content>
