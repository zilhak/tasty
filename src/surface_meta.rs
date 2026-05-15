use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

/// File-based per-surface metadata store.
/// On Windows, stored in `%TEMP%\tasty-surfaces\<surface_id>\meta.json`.
pub struct SurfaceMetaStore;

impl SurfaceMetaStore {
    fn meta_dir(surface_id: u32) -> PathBuf {
        let base = std::env::temp_dir().join("tasty-surfaces");
        base.join(surface_id.to_string())
    }

    fn meta_path(surface_id: u32) -> PathBuf {
        Self::meta_dir(surface_id).join("meta.json")
    }

    pub fn ensure_created(surface_id: u32) -> io::Result<()> {
        let dir = Self::meta_dir(surface_id);
        fs::create_dir_all(&dir)?;
        let path = Self::meta_path(surface_id);
        if !path.exists() {
            fs::write(&path, "{}")?;
        }
        Ok(())
    }

    pub fn remove(surface_id: u32) -> io::Result<()> {
        let dir = Self::meta_dir(surface_id);
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn read_all(surface_id: u32) -> HashMap<String, String> {
        let path = Self::meta_path(surface_id);
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn write_all(surface_id: u32, data: &HashMap<String, String>) -> io::Result<()> {
        Self::ensure_created(surface_id)?;
        let path = Self::meta_path(surface_id);
        let json = serde_json::to_string_pretty(data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&path, json)
    }

    pub fn set(surface_id: u32, key: &str, value: &str) -> io::Result<()> {
        let mut data = Self::read_all(surface_id);
        data.insert(key.to_string(), value.to_string());
        Self::write_all(surface_id, &data)
    }

    pub fn get(surface_id: u32, key: &str) -> Option<String> {
        Self::read_all(surface_id).get(key).cloned()
    }

    pub fn unset(surface_id: u32, key: &str) -> io::Result<()> {
        let mut data = Self::read_all(surface_id);
        data.remove(key);
        Self::write_all(surface_id, &data)
    }

    pub fn list(surface_id: u32) -> HashMap<String, String> {
        Self::read_all(surface_id)
    }

    /// Find the first surface ID whose meta contains key=value.
    /// Scans all surface meta directories.
    pub fn find_by_value(key: &str, value: &str) -> Option<u32> {
        let base = std::env::temp_dir().join("tasty-surfaces");
        let entries = fs::read_dir(&base).ok()?;
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(sid) = name.parse::<u32>() {
                    if Self::get(sid, key).as_deref() == Some(value) {
                        return Some(sid);
                    }
                }
            }
        }
        None
    }
}
