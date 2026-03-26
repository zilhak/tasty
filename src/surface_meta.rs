use std::collections::HashMap;
use std::fs;
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

    pub fn ensure_created(surface_id: u32) {
        let dir = Self::meta_dir(surface_id);
        let _ = fs::create_dir_all(&dir);
        let path = Self::meta_path(surface_id);
        if !path.exists() {
            let _ = fs::write(&path, "{}");
        }
    }

    pub fn remove(surface_id: u32) {
        let dir = Self::meta_dir(surface_id);
        let _ = fs::remove_dir_all(&dir);
    }

    fn read_all(surface_id: u32) -> HashMap<String, String> {
        let path = Self::meta_path(surface_id);
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn write_all(surface_id: u32, data: &HashMap<String, String>) {
        Self::ensure_created(surface_id);
        let path = Self::meta_path(surface_id);
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = fs::write(&path, json);
        }
    }

    pub fn set(surface_id: u32, key: &str, value: &str) {
        let mut data = Self::read_all(surface_id);
        data.insert(key.to_string(), value.to_string());
        Self::write_all(surface_id, &data);
    }

    pub fn get(surface_id: u32, key: &str) -> Option<String> {
        Self::read_all(surface_id).get(key).cloned()
    }

    pub fn unset(surface_id: u32, key: &str) {
        let mut data = Self::read_all(surface_id);
        data.remove(key);
        Self::write_all(surface_id, &data);
    }

    pub fn list(surface_id: u32) -> HashMap<String, String> {
        Self::read_all(surface_id)
    }
}
