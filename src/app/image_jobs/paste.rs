//! 로컬 터미널 이미지 붙여넣기. PNG 파일 저장은 worker 가 하고, 경로 입력은 완료 시 원 대상을
//! 다시 확인한 App 이 한다. 원격 mirror 의 이미지 붙여넣기는 `image_upload` 가 따로 맡는다.

use crate::runtime::engine_access::EngineMut;
use crate::runtime::engine_session::EngineId;
use crate::runtime::surface_binding::SurfaceBinding;

/// 붙여넣기 시점에 고정한 저장 요청. engine 이 App 에 넘기기 전까지 들고 있는 값이다.
#[derive(Clone, Debug)]
pub(crate) struct PendingImageSave {
    pub(crate) target: SurfaceBinding,
    /// 실패·포화 안내를 표시할 View. 경로 입력 여부에는 쓰지 않는다.
    pub(crate) origin_view: std::sync::Weak<()>,
    pub(crate) bracketed: bool,
    pub(crate) file_name: String,
    pub(crate) png_bytes: Vec<u8>,
}

/// worker 가 저장을 마친 결과. 경로를 전달하지 않고 버리면 임시 디렉터리와 파일이 지워진다.
pub(crate) struct SavedPaste {
    pub(crate) engine: EngineId,
    pub(crate) request: PasteRequest,
    pub(crate) file: Result<SavedFile, String>,
}

/// 저장 요청에서 바이트를 뺀 나머지. 완료 때 대상 확인과 안내에 쓴다.
pub(crate) struct PasteRequest {
    pub(crate) target: SurfaceBinding,
    pub(crate) origin_view: std::sync::Weak<()>,
    pub(crate) bracketed: bool,
}

/// 요청마다 따로 만든 임시 디렉터리 안의 PNG. 같은 파일명의 붙여넣기가 서로 덮어쓰지 않는다.
pub(crate) struct SavedFile {
    directory: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl SavedFile {
    #[cfg(test)]
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// worker 에서 실행한다. 디렉터리 생성과 파일 쓰기는 GUI 호출 스택에서 하지 않는다.
pub(crate) fn save(file_name: &str, png_bytes: &[u8]) -> Result<SavedFile, String> {
    let directory = tempfile::Builder::new()
        .prefix("tasty-clipboard-")
        .tempdir()
        .map_err(|error| format!("clipboard image directory creation failed: {error}"))?;
    let path = directory.path().join(file_name);
    std::fs::write(&path, png_bytes)
        .map_err(|error| format!("clipboard image save failed: {error}"))?;
    Ok(SavedFile { directory, path })
}

/// 경로 입력 결과.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PasteDelivery {
    /// 원 터미널에 경로를 입력했다. 받는 셸이 나중에 읽으므로 파일을 남긴다.
    Delivered,
    /// 원 터미널이 닫혔거나 같은 번호의 새 터미널로 바뀌었다. 파일은 회수했다.
    TargetRetired,
    /// 다른 client 가 그 터미널을 점유하고 있다. 파일은 회수했다.
    HardOccupied,
}

/// 원 대상의 세대와 현재 점유를 다시 확인한 뒤에만 경로를 입력한다.
pub(crate) fn deliver(
    engine: &mut EngineMut<'_>,
    request: &PasteRequest,
    file: SavedFile,
) -> PasteDelivery {
    let surface = request.target.surface_id();
    if !request.target.current(&engine.as_ref()) {
        return PasteDelivery::TargetRetired;
    }
    if engine.live.occupancy.is_hard_occupied(surface) {
        return PasteDelivery::HardOccupied;
    }
    let Some(terminal) = engine.runtime.terminals.get_mut(surface) else {
        return PasteDelivery::TargetRetired;
    };
    let mut bytes = Vec::new();
    if request.bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(file.path.to_string_lossy().as_bytes());
    if request.bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    terminal.send_bytes(&bytes);
    // 받는 셸은 이 입력 뒤에 파일을 읽는다. 운영체제의 임시 폴더 정리 때까지 남긴다.
    drop(file.directory.keep());
    PasteDelivery::Delivered
}
