use super::{
    ProfileError,
    fs::{Dir, Namespace, Object},
    process::Identity,
};
use std::{
    ffi::OsStr,
    io::{Read, Write},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};
pub(super) const NAME: &str = "owner.json";
const RECORD_BYTES: u64 = 65_536;
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    version: u32,
    kind: String,
    root_path: Vec<u8>,
    parent: Object,
    root: Object,
    pub owner: Identity,
}
impl Record {
    pub fn new(parent: &Namespace, dir: &Dir, path: PathBuf, owner: Identity) -> Self {
        Self {
            version: 1,
            kind: "tasty-private-native-profile".into(),
            root_path: path.as_os_str().as_bytes().to_vec(),
            parent: parent.dir.id,
            root: dir.id,
            owner,
        }
    }
    pub fn write(&self, dir: &Dir) -> Result<(), ProfileError> {
        let bytes = serde_json::to_vec(self).map_err(|_| ProfileError::Invalid)?;
        if bytes.len() as u64 > RECORD_BYTES {
            return Err(ProfileError::Invalid);
        }
        let mut file = dir.create_file(OsStr::new(NAME))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        dir.file.sync_all()?;
        Ok(())
    }
    pub fn read(dir: &Dir) -> Result<Self, ProfileError> {
        let f = dir.open_file(OsStr::new(NAME))?;
        let mut bytes = Vec::new();
        f.take(RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > RECORD_BYTES {
            return Err(ProfileError::Ownership);
        }
        serde_json::from_slice(&bytes).map_err(|_| ProfileError::Ownership)
    }
    pub fn validate(&self, parent: &Namespace, dir: &Dir, path: &Path) -> Result<(), ProfileError> {
        parent.validate()?;
        dir.private()?;
        let valid_name = path.file_name().is_some_and(|name| {
            let bytes = name.as_bytes();
            bytes.len() == 34
                && bytes.starts_with(b"p-")
                && bytes[2..].iter().all(u8::is_ascii_hexdigit)
        });
        if !valid_name
            || self.kind != "tasty-private-native-profile"
            || self.version != 1
            || self.root_path != path.as_os_str().as_bytes()
            || self.root != dir.id
            || self.parent != parent.dir.id
            || self.owner.uid != super::fs::uid()
            || !self.owner.valid()
        {
            return Err(ProfileError::Ownership);
        }
        Ok(())
    }
}
