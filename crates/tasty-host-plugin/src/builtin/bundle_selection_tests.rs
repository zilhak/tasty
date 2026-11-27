use super::*;

/// 번들 목록의 첫 플러그인. 모든 번들 플러그인은 스테이징한 산출물로 쓴다.
fn spec() -> &'static BuiltinSpec {
    BUILTINS.first().expect("번들 플러그인이 없다")
}

/// 실행 파일 위 workspace에 같은 id의 크레이트 원본과 더 새 빌드 산출물을 둔다.
/// 번들 선택은 이 파일들을 읽지 않아야 한다.
fn write_workspace_leftovers(workspace: &Path, exe_dir: &Path, id: &str) {
    let source = workspace.join("crates").join(id);
    std::fs::create_dir_all(source.join("lang")).unwrap();
    std::fs::create_dir_all(exe_dir).unwrap();
    std::fs::write(source.join("tasty-plugin.toml"), "edited manifest").unwrap();
    std::fs::write(source.join("tasty-plugin.toml.sig"), "old signature").unwrap();
    std::fs::write(source.join("lang/en.toml"), "edited language").unwrap();
    std::fs::write(exe_dir.join(id), "new binary").unwrap();
}

/// 스테이징한 파일을 원본보다 옛 시각으로 만든다.
fn stamp_epoch(path: &Path) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
}

fn workspace_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let workspace = tempfile::tempdir().unwrap();
    let exe_dir = workspace.path().join("target/profile");
    write_workspace_leftovers(workspace.path(), &exe_dir, spec().id);
    let bundle = exe_dir.join("builtin-plugins");
    (workspace, exe_dir, bundle)
}

/// 어느 프로필도 스테이징한 산출물을 workspace의 더 새 파일로 덮지 않는다.
#[test]
fn bundle_selection_keeps_the_staged_artifacts() {
    let (_workspace, exe_dir, bundle) = workspace_fixture();
    let staged = bundle.join(spec().id);
    std::fs::create_dir_all(staged.join("lang")).unwrap();
    let files = [
        ("tasty-plugin.toml", "released manifest"),
        ("tasty-plugin.toml.sig", "paired signature"),
        ("lang/en.toml", "staged language"),
        (spec().id, "released binary"),
    ];
    for (path, content) in files {
        std::fs::write(staged.join(path), content).unwrap();
        stamp_epoch(&staged.join(path));
    }

    assert_eq!(bundle_root_from_exe_dir(&exe_dir), Some(bundle));
    for (path, content) in files {
        assert_eq!(
            std::fs::read_to_string(staged.join(path)).unwrap(),
            content,
            "{path}"
        );
    }
}

/// 스테이징하지 않았으면 workspace에 크레이트와 빌드가 있어도 번들을 만들지 않는다.
#[test]
fn bundle_selection_does_not_build_a_bundle_from_workspace_crates() {
    let (_workspace, exe_dir, bundle) = workspace_fixture();
    assert_eq!(bundle_root_from_exe_dir(&exe_dir), None);
    assert!(!bundle.exists());
}

#[test]
fn bundle_selection_portable_bundle_precedes_the_staged_bundle() {
    let (_workspace, exe_dir, bundle) = workspace_fixture();
    std::fs::create_dir_all(&bundle).unwrap();
    let portable = exe_dir.join("plugins");
    std::fs::create_dir(&portable).unwrap();
    assert_eq!(bundle_root_from_exe_dir(&exe_dir), Some(portable));
}
