//! 원격 workspace를 로컬 mirror 트리로 표시하고 입력을 원격으로 전달한다.
//! 수신 이벤트와 주기 확인에서 출력을 적용한다. 창 없는 parked engine도 적용·정리에 포함한다.
//! 자동 연결 매핑은 auto_attach가 관리한다. docs/dev-guide/attach-behavior.md 참조.

mod agent_origin;
pub(crate) mod attempts;
mod bulk;
mod connection;
mod dispatch;
mod forward;
#[cfg(test)]
mod navigation_tests;
mod output;
pub(crate) mod pending;
mod projection;
mod survivors;
#[cfg(test)]
mod tests;
mod wire;

use survivors::merge_survivor_mapping;

use crate::runtime::engine_access::EngineMut;
use dispatch::{AttachSource, Outcome, dispatch_attach};
use tasty_remote::client_session::*;
use tasty_remote::transport::*;

use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use winit::event_loop::EventLoopProxy;

use crate::ipc::client::StreamConnection;
use tasty_terminal::Terminal;

use crate::AppEvent;
use crate::app::App;
use crate::app::window_access::{EngineScanMut, engines_mut};
use crate::ipc::stream::{self, STREAM_PROTO, StreamControl, StreamTag, StructuralOp};
use crate::model::{
    DeferredPlugin, EmptySurface, ExplorerPanel, Pane, PaneNode, SplitDirection, Surface,
    SurfaceLayout, Tab, TerminalSurface, Workspace,
};
use crate::runtime::engine_session::EngineId;
use crate::view::ui::View as _;

/// 번들 git-viewer 매니페스트의 ID와 일치해야 한다.
const GIT_VIEWER_PLUGIN_ID: &str = "com.tasty.git-viewer";
const GIT_VIEWER_QUERY_RESULT_EVENT: &str = "git_viewer.query_result";

/// 서버의 is_attach_content_allowed와 같은 kind·소유자 쌍만 로컬 문서로 만든다.
const MARKDOWN_MIRROR_KIND: &str = "markdown";
const MARKDOWN_PLUGIN_ID: &str = "com.tasty.markdown";
const MARKDOWN_MIRROR_CONTENT_RESULT_EVENT: &str = "markdown_mirror.content_result";
const MARKDOWN_MIRROR_CHANGED_EVENT: &str = "markdown_mirror.changed";

/// mesh leaf를 AttachMeshSurface로 만들 때 필요한 표시 정보.
#[derive(Debug, Clone)]
struct MirrorMeshInfo {
    kind: String,
    plugin_id: String,
    display_name: String,
}

fn attach_wake(proxy: &EventLoopProxy<AppEvent>) -> Arc<dyn Fn() + Send + Sync> {
    let proxy = proxy.clone();
    Arc::new(move || {
        if let Err(error) = proxy.send_event(AppEvent::AttachClientData) {
            tracing::debug!("remote wake after event loop closed: {error}");
        }
    })
}

fn bind_mirror_input(
    mirror: &mut Terminal,
    remote_id: u32,
    frame_tx: &SharedFrameSender,
    reconnect: bool,
) {
    let sender = frame_tx.clone();
    mirror.bind_external_input(
        Arc::new(move |bytes| {
            const MAX_BODY: usize = stream::MAX_FRAME_LEN as usize - 4;
            for part in bytes.chunks(MAX_BODY) {
                if sender
                    .send(OutFrame {
                        tag: StreamTag::Data,
                        payload: stream::encode_mux(remote_id, part),
                    })
                    .is_err()
                {
                    // A partial paste is an explicit retired-connection loss, never a replay candidate.
                    return Err(std::sync::mpsc::SendError(bytes));
                }
            }
            Ok(())
        }),
        reconnect,
    );
}

use connection::*;

use forward::*;

pub(super) use output::find_parked_with_workspace;
use output::*;

use projection::*;

use wire::*;

use bulk::*;

pub(crate) use bulk::{BULK_REJECT_PREFIX, upload_file_over_bulk};
pub(crate) use forward::RemoteTarget;
