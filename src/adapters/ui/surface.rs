//! Surface kind 별 egui 렌더링. terminal 은 GPU 렌더링이라 여기에 없고, 나머지
//! builtin surface (empty / image / markdown) 의 view 만 모음.

pub mod empty;
pub mod image;
pub mod markdown;
