//! 테스트 전용 — **실제로 기동에 성공하는** 가장 작은 plugin.
//!
//! 인증 한 줄을 보내고 소켓이 닫힐 때까지 읽기만 한다. 그래서 `PluginManager` 가 띄우면
//! `is_running` 이 참이 되고, 요청에는 답하지 않는다. bash 의 `/dev/tcp` 를 쓰므로 unix 에서만
//! 쓴다.
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
