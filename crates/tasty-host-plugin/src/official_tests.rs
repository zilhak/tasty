use super::*;

#[test]
fn an_unknown_reserved_id_is_refused() {
    let err = admit_plugin_id("com.tasty.fake").expect_err("unknown com.tasty id must be refused");
    assert_eq!(err.id, "com.tasty.fake");
    assert!(
        err.to_string().contains("reserved com.tasty. prefix"),
        "{err}"
    );
}

#[test]
fn every_official_id_is_admitted() {
    assert_eq!(OFFICIAL_PLUGIN_IDS.len(), 8);
    for id in OFFICIAL_PLUGIN_IDS {
        assert_eq!(admit_plugin_id(id), Ok(()), "{id}");
    }
    // 번들하지 않는 공식 플러그인도 목록에 있다.
    assert!(OFFICIAL_PLUGIN_IDS.contains(&"com.tasty.agent-stream"));
}

/// 지운 시범 플러그인의 id는 공식 목록에 없어 예약 id로 거부된다. 남은 설치 폴더는 확인 필요 탭으로 간다.
#[test]
fn the_removed_demo_plugin_id_is_refused() {
    let id = "com.tasty.mesh-demo";
    assert_eq!(
        admit_plugin_id(id),
        Err(ReservedIdError { id: id.to_string() })
    );
}

#[test]
fn ids_outside_the_reserved_prefix_are_admitted() {
    for id in [
        "com.example.x",
        "com.tastyfoo.x",
        "org.tasty.x",
        "com.tasty",
        "io.github.user.tasty",
    ] {
        assert_eq!(admit_plugin_id(id), Ok(()), "{id}");
    }
}

#[test]
fn bundled_ids_of_this_build_are_official() {
    assert!(!crate::builtin::BUILTINS.is_empty());
    for spec in crate::builtin::BUILTINS {
        assert!(
            OFFICIAL_PLUGIN_IDS.contains(&spec.id),
            "bundled '{}' is missing from OFFICIAL_PLUGIN_IDS",
            spec.id
        );
    }
}

/// 번들 플러그인 매니페스트(공식 플러그인 픽스처, workspace에 매니페스트 크레이트가 있으면 그것도)의 id는 모두 공식 목록에 있어야 설치된다.
#[test]
fn repository_plugin_manifests_are_official() {
    let dirs = tasty_test_support::bundled_plugin_manifest_dirs();
    for dir in &dirs {
        let manifest = tasty_plugin_manifest::Manifest::load(dir).expect("manifest");
        assert_eq!(admit_plugin_id(&manifest.id), Ok(()), "{}", dir.display());
    }
    assert_eq!(dirs.len(), 8, "bundled plugin manifests: {dirs:?}");
}

#[test]
fn a_bundle_folder_must_declare_its_own_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = |id: &str| {
        format!(
            "manifest_version = 1\nid = \"{id}\"\nname = \"x\"\nversion = \"0.1.0\"\napi_version = \"1\"\n[entry]\ntype = \"process\"\ncommand = \"x\"\n"
        )
    };
    std::fs::write(
        dir.path().join("tasty-plugin.toml"),
        manifest("com.tasty.fake"),
    )
    .unwrap();
    let reason = bundle_identity_mismatch("com.tasty.markdown", dir.path())
        .expect("mismatch must be reported");
    assert!(reason.contains("com.tasty.fake"), "{reason}");
    std::fs::write(
        dir.path().join("tasty-plugin.toml"),
        manifest("com.tasty.markdown"),
    )
    .unwrap();
    assert_eq!(
        bundle_identity_mismatch("com.tasty.markdown", dir.path()),
        None
    );
}
