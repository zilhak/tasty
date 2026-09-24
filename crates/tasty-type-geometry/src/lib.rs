#![forbid(unsafe_code)]

//! 픽셀 길이·사각형·분할 방향을 정의한다. 다른 tasty-* 크레이트에 의존하지 않는다.
//! LogicalPx/LogicalRect와 PhysicalPx/PhysicalRect를 구분하고 변환할 때 배율을 명시한다.
//! Theme나 Workspace 같은 도메인 타입은 상위 크레이트에 둔다.

// 이유: 테스트에서 사용하지 않는 반환값을 버리는 것은 허용한다. 제품 코드의 검사는 유지한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

pub mod direction;
pub mod length;
pub mod rect;
