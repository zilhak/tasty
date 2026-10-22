//! 원격 Explorer의 서버 원본과 클라이언트 패널 수명을 연결한다.
//! 사용자 탐색은 서버 descriptor를 바꾸지 않으므로 그 경로로 생존 여부를 판단하지 않는다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tasty_remote::connection::ConnectionEpoch;

use crate::model::{ExplorerPanel, Surface};

struct Source {
    remote: u32,
    root: PathBuf,
    connection: ConnectionEpoch,
}

#[derive(Default)]
pub(crate) struct MirrorExplorers(HashMap<u32, Source>);

impl MirrorExplorers {
    /// 같은 서버 원본·연결이면 패널 전체를 그대로 둔다. 실제 원본 변경이나 새 연결만 새 탭을 만든다.
    pub(crate) fn sync(
        &mut self,
        local: u32,
        remote: u32,
        root: &Path,
        connection: ConnectionEpoch,
        surfaces: &mut HashMap<u32, Box<dyn Surface>>,
    ) {
        let same_source = self.0.get(&local).is_some_and(|old| {
            old.remote == remote && old.root == root && old.connection.same(&connection)
        });
        let same_kind = surfaces
            .get(&local)
            .is_some_and(|surface| surface.as_any().is::<ExplorerPanel>());
        if same_source && same_kind {
            return;
        }
        self.0.insert(
            local,
            Source {
                remote,
                root: root.to_owned(),
                connection,
            },
        );
        surfaces.insert(local, Box::new(ExplorerPanel::new(local, root.to_owned())));
    }

    pub(crate) fn remove(&mut self, local: u32) {
        self.0.remove(&local);
    }
}
