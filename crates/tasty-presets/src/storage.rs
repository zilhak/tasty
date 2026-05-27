//! Preset 디스크 저장소.
//!
//! 위치: `~/.tasty/presets/{workspace,tab,pane}/<name>.toml`
//! 파일명이 정본 — `preset.name` 과 파일명이 다르면 파일명을 우선.
//! 같은 kind 내 이름 중복 금지. `unique_name` 으로 자동 -N suffix.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{LayoutPreset, PanePreset, PresetKind, TabPreset, WorkspacePreset};

#[derive(Debug, Error)]
pub enum PresetError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Toml(#[from] toml::ser::Error),
    #[error("deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("invalid preset name: {0}")]
    InvalidName(String),
    #[error("preset already exists: kind={0:?} name={1}")]
    AlreadyExists(PresetKind, String),
    #[error("preset not found: kind={0:?} name={1}")]
    NotFound(PresetKind, String),
    #[error("home directory unavailable")]
    HomeUnavailable,
}

pub type PresetResult<T> = Result<T, PresetError>;

#[derive(Debug, Default, Clone)]
pub struct PresetStore {
    root: Option<PathBuf>,
    workspaces: BTreeMap<String, WorkspacePreset>,
    tabs: BTreeMap<String, TabPreset>,
    panes: BTreeMap<String, PanePreset>,
}

impl PresetStore {
    /// `~/.tasty/presets/` 에서 모든 preset 을 로드. 디렉토리 없으면 빈 store.
    pub fn load_default() -> Self {
        let root = match tasty_utils::path::tasty_home() {
            Some(home) => home.join("presets"),
            None => {
                tracing::warn!("tasty_home unavailable; preset store starts empty");
                return Self::default();
            }
        };
        let mut s = Self {
            root: Some(root.clone()),
            ..Default::default()
        };
        s.workspaces = scan_workspaces(&root.join("workspace"));
        s.tabs = scan_tabs(&root.join("tab"));
        s.panes = scan_panes(&root.join("pane"));
        s
    }

    /// 테스트용 — 임의 디렉토리에 store 를 연다.
    pub fn load_from(root: PathBuf) -> Self {
        let mut s = Self {
            root: Some(root.clone()),
            ..Default::default()
        };
        s.workspaces = scan_workspaces(&root.join("workspace"));
        s.tabs = scan_tabs(&root.join("tab"));
        s.panes = scan_panes(&root.join("pane"));
        s
    }

    fn root(&self) -> PresetResult<&Path> {
        self.root.as_deref().ok_or(PresetError::HomeUnavailable)
    }

    fn kind_dir(&self, kind: PresetKind) -> PresetResult<PathBuf> {
        Ok(self.root()?.join(kind.as_str()))
    }

    // ── list/get ───────────────────────────────────────────────────────

    pub fn list(&self, kind: PresetKind) -> Vec<String> {
        match kind {
            PresetKind::Workspace => self.workspaces.keys().cloned().collect(),
            PresetKind::Tab => self.tabs.keys().cloned().collect(),
            PresetKind::Pane => self.panes.keys().cloned().collect(),
        }
    }

    pub fn get_workspace(&self, name: &str) -> Option<&WorkspacePreset> {
        self.workspaces.get(name)
    }
    pub fn get_tab(&self, name: &str) -> Option<&TabPreset> {
        self.tabs.get(name)
    }
    pub fn get_pane(&self, name: &str) -> Option<&PanePreset> {
        self.panes.get(name)
    }

    // ── save (실패 if 충돌) ─────────────────────────────────────────────

    pub fn save_workspace(&mut self, preset: WorkspacePreset) -> PresetResult<()> {
        self.save_inner(preset, false)
    }
    pub fn save_tab(&mut self, preset: TabPreset) -> PresetResult<()> {
        self.save_inner(preset, false)
    }
    pub fn save_pane(&mut self, preset: PanePreset) -> PresetResult<()> {
        self.save_inner(preset, false)
    }

    pub fn save_workspace_overwrite(&mut self, preset: WorkspacePreset) -> PresetResult<()> {
        self.save_inner(preset, true)
    }
    pub fn save_tab_overwrite(&mut self, preset: TabPreset) -> PresetResult<()> {
        self.save_inner(preset, true)
    }
    pub fn save_pane_overwrite(&mut self, preset: PanePreset) -> PresetResult<()> {
        self.save_inner(preset, true)
    }

    fn save_inner<P: LayoutPreset + PresetKindAccess>(
        &mut self,
        mut preset: P,
        overwrite: bool,
    ) -> PresetResult<()> {
        let kind = P::KIND;
        let name = preset.name().to_string();
        validate_name(&name)?;
        if !overwrite && self.contains_inner(kind, &name) {
            return Err(PresetError::AlreadyExists(kind, name));
        }
        // 파일명 = preset 이름 (정본 일치)
        preset.set_name(name.clone());
        let path = self.kind_dir(kind)?.join(format!("{name}.toml"));
        let serialized = toml::to_string_pretty(preset.serialize_ref())?;
        atomic_write(&path, serialized.as_bytes())?;
        P::insert_into_store(self, name, preset);
        Ok(())
    }

    fn contains_inner(&self, kind: PresetKind, name: &str) -> bool {
        match kind {
            PresetKind::Workspace => self.workspaces.contains_key(name),
            PresetKind::Tab => self.tabs.contains_key(name),
            PresetKind::Pane => self.panes.contains_key(name),
        }
    }

    // ── delete / rename ────────────────────────────────────────────────

    pub fn delete(&mut self, kind: PresetKind, name: &str) -> PresetResult<()> {
        if !self.contains_inner(kind, name) {
            return Err(PresetError::NotFound(kind, name.into()));
        }
        let path = self.kind_dir(kind)?.join(format!("{name}.toml"));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        match kind {
            PresetKind::Workspace => {
                self.workspaces.remove(name);
            }
            PresetKind::Tab => {
                self.tabs.remove(name);
            }
            PresetKind::Pane => {
                self.panes.remove(name);
            }
        }
        Ok(())
    }

    pub fn rename(&mut self, kind: PresetKind, from: &str, to: &str) -> PresetResult<()> {
        validate_name(to)?;
        if !self.contains_inner(kind, from) {
            return Err(PresetError::NotFound(kind, from.into()));
        }
        if from == to {
            return Ok(());
        }
        if self.contains_inner(kind, to) {
            return Err(PresetError::AlreadyExists(kind, to.into()));
        }
        let dir = self.kind_dir(kind)?;
        let from_path = dir.join(format!("{from}.toml"));
        let to_path = dir.join(format!("{to}.toml"));

        match kind {
            PresetKind::Workspace => {
                let mut preset = self.workspaces.remove(from).unwrap();
                preset.set_name(to.into());
                let serialized = toml::to_string_pretty(&preset)?;
                atomic_write(&to_path, serialized.as_bytes())?;
                let _ = std::fs::remove_file(&from_path);
                self.workspaces.insert(to.into(), preset);
            }
            PresetKind::Tab => {
                let mut preset = self.tabs.remove(from).unwrap();
                preset.set_name(to.into());
                let serialized = toml::to_string_pretty(&preset)?;
                atomic_write(&to_path, serialized.as_bytes())?;
                let _ = std::fs::remove_file(&from_path);
                self.tabs.insert(to.into(), preset);
            }
            PresetKind::Pane => {
                let mut preset = self.panes.remove(from).unwrap();
                preset.set_name(to.into());
                let serialized = toml::to_string_pretty(&preset)?;
                atomic_write(&to_path, serialized.as_bytes())?;
                let _ = std::fs::remove_file(&from_path);
                self.panes.insert(to.into(), preset);
            }
        }
        Ok(())
    }

    // ── unique_name (충돌 시 -N suffix) ──────────────────────────────────

    pub fn unique_name(&self, kind: PresetKind, base: &str) -> String {
        let sanitized = sanitize_name(base);
        let base = if sanitized.is_empty() {
            kind.as_str().to_string()
        } else {
            sanitized
        };
        if !self.contains_inner(kind, &base) {
            return base;
        }
        for n in 2u32..10_000 {
            let cand = format!("{base}-{n}");
            if !self.contains_inner(kind, &cand) {
                return cand;
            }
        }
        // 극단적 fallback
        format!("{base}-{}", std::process::id())
    }
}

// ── 직렬화/insert dispatch trait (kind 분기 회피) ────────────────────────

trait PresetKindAccess: LayoutPreset {
    fn serialize_ref(&self) -> &Self {
        self
    }
    fn insert_into_store(store: &mut PresetStore, name: String, preset: Self);
}

impl PresetKindAccess for WorkspacePreset {
    fn insert_into_store(store: &mut PresetStore, name: String, preset: Self) {
        store.workspaces.insert(name, preset);
    }
}
impl PresetKindAccess for TabPreset {
    fn insert_into_store(store: &mut PresetStore, name: String, preset: Self) {
        store.tabs.insert(name, preset);
    }
}
impl PresetKindAccess for PanePreset {
    fn insert_into_store(store: &mut PresetStore, name: String, preset: Self) {
        store.panes.insert(name, preset);
    }
}

// ── 디스크 스캔 ──────────────────────────────────────────────────────────

fn scan_workspaces(dir: &Path) -> BTreeMap<String, WorkspacePreset> {
    scan_dir::<WorkspacePreset>(dir)
}
fn scan_tabs(dir: &Path) -> BTreeMap<String, TabPreset> {
    scan_dir::<TabPreset>(dir)
}
fn scan_panes(dir: &Path) -> BTreeMap<String, PanePreset> {
    scan_dir::<PanePreset>(dir)
}

fn scan_dir<P: LayoutPreset>(dir: &Path) -> BTreeMap<String, P> {
    let mut out = BTreeMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %path.display(), "preset read failed: {e}");
                continue;
            }
        };
        let mut preset: P = match toml::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(path = %path.display(), "preset parse failed: {e}");
                continue;
            }
        };
        // 파일명 = 정본
        preset.set_name(stem.clone());
        out.insert(stem, preset);
    }
    out
}

// ── helpers ─────────────────────────────────────────────────────────────

fn validate_name(name: &str) -> PresetResult<()> {
    if name.is_empty() {
        return Err(PresetError::InvalidName("(empty)".into()));
    }
    if name.len() > 100 {
        return Err(PresetError::InvalidName(name.into()));
    }
    // 경로 구분/숨김 파일/제어 문자 금지
    for ch in name.chars() {
        if ch == '/' || ch == '\\' || ch == '\0' || ch.is_control() {
            return Err(PresetError::InvalidName(name.into()));
        }
    }
    if name.starts_with('.') {
        return Err(PresetError::InvalidName(name.into()));
    }
    Ok(())
}

fn sanitize_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            out.push(ch);
        } else if ch == ' ' {
            out.push('-');
        }
        // 그 외는 drop
    }
    while out.starts_with('.') {
        out.remove(0);
    }
    out
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ── tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PresetPane, PresetPaneNode, PresetSurface, PresetSurfaceLayout, PresetTab};
    use tempfile::tempdir;

    fn ws(name: &str) -> WorkspacePreset {
        WorkspacePreset {
            name: name.into(),
            subtitle: String::new(),
            description: String::new(),
            layout: PresetPaneNode::Leaf {
                pane: PresetPane {
                    tabs: vec![PresetTab {
                        explicit_name: None,
                        layout: PresetSurfaceLayout::Leaf {
                            surface: PresetSurface {
                                kind: "terminal".into(),
                                cwd: None,
                                startup_command: None,
                                params: serde_json::Value::Null,
                            },
                        },
                    }],
                    active_tab: 0,
                },
            },
        }
    }

    #[test]
    fn save_then_load_roundtrip() {
        let tmp = tempdir().unwrap();
        let mut s = PresetStore::load_from(tmp.path().into());
        s.save_workspace(ws("dev")).unwrap();
        let s2 = PresetStore::load_from(tmp.path().into());
        let got = s2.get_workspace("dev").unwrap();
        assert_eq!(got.name, "dev");
    }

    #[test]
    fn save_rejects_duplicate_name() {
        let tmp = tempdir().unwrap();
        let mut s = PresetStore::load_from(tmp.path().into());
        s.save_workspace(ws("dev")).unwrap();
        let err = s.save_workspace(ws("dev")).unwrap_err();
        assert!(matches!(err, PresetError::AlreadyExists(_, _)));
    }

    #[test]
    fn save_overwrite_replaces() {
        let tmp = tempdir().unwrap();
        let mut s = PresetStore::load_from(tmp.path().into());
        let mut a = ws("dev");
        a.subtitle = "v1".into();
        s.save_workspace(a).unwrap();
        let mut b = ws("dev");
        b.subtitle = "v2".into();
        s.save_workspace_overwrite(b).unwrap();
        assert_eq!(s.get_workspace("dev").unwrap().subtitle, "v2");
    }

    #[test]
    fn delete_removes_file_and_cache() {
        let tmp = tempdir().unwrap();
        let mut s = PresetStore::load_from(tmp.path().into());
        s.save_workspace(ws("dev")).unwrap();
        s.delete(PresetKind::Workspace, "dev").unwrap();
        assert!(s.get_workspace("dev").is_none());
        let f = tmp.path().join("workspace/dev.toml");
        assert!(!f.exists());
    }

    #[test]
    fn rename_moves_file_and_updates_cache() {
        let tmp = tempdir().unwrap();
        let mut s = PresetStore::load_from(tmp.path().into());
        s.save_workspace(ws("dev")).unwrap();
        s.rename(PresetKind::Workspace, "dev", "main").unwrap();
        assert!(s.get_workspace("dev").is_none());
        assert!(s.get_workspace("main").is_some());
        assert!(!tmp.path().join("workspace/dev.toml").exists());
        assert!(tmp.path().join("workspace/main.toml").exists());
        // 내부 name 도 변경
        assert_eq!(s.get_workspace("main").unwrap().name, "main");
    }

    #[test]
    fn invalid_name_rejected() {
        let tmp = tempdir().unwrap();
        let mut s = PresetStore::load_from(tmp.path().into());
        for bad in ["", "a/b", "..", ".hidden", "x\0y"] {
            let mut p = ws("placeholder");
            p.name = bad.into();
            let r = s.save_workspace(p);
            assert!(r.is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn unique_name_appends_suffix() {
        let tmp = tempdir().unwrap();
        let mut s = PresetStore::load_from(tmp.path().into());
        assert_eq!(s.unique_name(PresetKind::Workspace, "dev"), "dev");
        s.save_workspace(ws("dev")).unwrap();
        assert_eq!(s.unique_name(PresetKind::Workspace, "dev"), "dev-2");
        s.save_workspace(ws("dev-2")).unwrap();
        assert_eq!(s.unique_name(PresetKind::Workspace, "dev"), "dev-3");
    }

    #[test]
    fn unique_name_sanitizes_input() {
        let tmp = tempdir().unwrap();
        let s = PresetStore::load_from(tmp.path().into());
        assert_eq!(
            s.unique_name(PresetKind::Workspace, "my workspace"),
            "my-workspace"
        );
        assert_eq!(s.unique_name(PresetKind::Workspace, ""), "workspace");
        assert_eq!(s.unique_name(PresetKind::Workspace, "../etc/x"), "etcx");
    }

    #[test]
    fn load_skips_garbage_files() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("workspace")).unwrap();
        // garbage toml
        std::fs::write(
            tmp.path().join("workspace/bad.toml"),
            b"this is not toml [[[",
        )
        .unwrap();
        // valid one
        std::fs::write(
            tmp.path().join("workspace/good.toml"),
            toml::to_string(&ws("good")).unwrap(),
        )
        .unwrap();
        // wrong extension
        std::fs::write(tmp.path().join("workspace/skip.txt"), b"hi").unwrap();

        let s = PresetStore::load_from(tmp.path().into());
        assert_eq!(s.list(PresetKind::Workspace), vec!["good".to_string()]);
    }

    #[test]
    fn missing_directory_is_empty_store() {
        let tmp = tempdir().unwrap();
        let s = PresetStore::load_from(tmp.path().into());
        assert!(s.list(PresetKind::Workspace).is_empty());
        assert!(s.list(PresetKind::Tab).is_empty());
        assert!(s.list(PresetKind::Pane).is_empty());
    }
}
