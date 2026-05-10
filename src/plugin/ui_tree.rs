//! UI tree DSL는 `tasty-plugin-protocol` 크레이트로 이동했다.
//! 호스트 코드 호환을 위해 thin re-export만 남긴다.

pub use tasty_plugin_protocol::ui_tree::{
    ButtonStyle, LabelStyle, SelectionMode, SplitDir, TreeNode, UiEvent, UiNode,
};
