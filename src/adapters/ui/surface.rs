//! Surface kind 별 egui 렌더링. terminal 은 GPU 렌더링이라 여기에 없고, 나머지
//! builtin surface (empty / explorer / image / markdown) 의 view 와, webview-kind
//! surface 의 host chrome (webview_chrome) 을 모음.

pub mod empty;
pub mod explorer;
pub mod image;
pub mod markdown;
pub mod webview_chrome;
