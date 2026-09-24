//! Unix 시험용 플러그인. bash /dev/tcp로 인증한 뒤 요청을 읽기만 하고 응답하지 않는다.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// 가짜 plugin 의 id — 매니페스트의 `id` 와 같다.
pub(crate) const ID: &str = "com.example.test";

pub(crate) const MANIFEST: &str = r#"
manifest_version = 1
id = "com.example.test"
name = "Test"
version = "1.0.0"
api_version = "1"
[entry]
type = "process"
command = "fake.sh"
"#;

const ENTRY: &str = r#"#!/bin/bash
exec 3<>/dev/tcp/127.0.0.1/"$TASTY_HOST_IPC_PORT"
printf '{"plugin_id":"%s","token":"%s"}\n' "$TASTY_PLUGIN_ID" "$TASTY_PLUGIN_TOKEN" >&3
exec cat <&3 >/dev/null
"#;

/// `dir` 에 매니페스트와 실행 파일을 쓴다.
pub(crate) fn write(dir: &Path) {
    std::fs::create_dir_all(dir).expect("plugin dir");
    std::fs::write(dir.join("tasty-plugin.toml"), MANIFEST).expect("manifest");
    let entry = dir.join("fake.sh");
    std::fs::write(&entry, ENTRY).expect("entry");
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// `dir` 에 설치된 가짜 plugin 의 package.
pub(crate) fn package(dir: &Path) -> tasty_plugin_manifest::PluginPackage {
    tasty_plugin_manifest::PluginPackage {
        dir: dir.to_path_buf(),
        manifest: toml::from_str(MANIFEST).expect("fixture manifest"),
    }
}
